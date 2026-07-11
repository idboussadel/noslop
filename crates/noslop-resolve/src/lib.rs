//! `noslop-resolve` — stage 3. Resolve every import to an internal file or an
//! external dependency, then build the whole-repo [`Graph`] (ARCHITECTURE.md §7).
//!
//! Resolution matches candidate paths against the *set of discovered files*
//! rather than the filesystem, which keeps it deterministic and I/O-free. The
//! graph is rebuilt from scratch on every run — there is no incremental state to
//! corrupt.

mod path;
mod py;
mod ts;

use noslop_graph::{
    FileFacts, FileNode, FileRole, Graph, ImportKind, Language, Package, SymbolNode, Workspace,
};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The outcome of resolving one import specifier.
pub(crate) enum Resolved {
    /// An internal file within the repo.
    Internal(PathBuf),
    /// An external dependency, keyed by its top-level distribution name.
    External(String),
    /// Could not be resolved (unknown path, non-literal, or outside the repo).
    Unresolved,
}

/// Shared, read-only resolution context.
pub(crate) struct Ctx<'a> {
    /// All discovered repo-relative source paths, for membership checks.
    files: &'a HashSet<PathBuf>,
    packages: &'a [Package],
    /// Union of every Python import root in the repo (monorepo cross-package
    /// imports resolve the same way as in-package ones).
    py_roots: Vec<PathBuf>,
}

impl Ctx<'_> {
    pub(crate) fn file_exists(&self, rel: &Path) -> bool {
        self.files.contains(rel)
    }
}

/// Resolve all imports and build the graph.
pub fn build_graph(ws: &Workspace, facts: &[FileFacts]) -> Graph {
    // Deterministic FileId assignment: sort by path. Ids are stable within a run
    // and never leak into the output contract.
    let mut ordered: Vec<&FileFacts> = facts.iter().collect();
    ordered.sort_by(|a, b| a.path.cmp(&b.path));

    // Role is derived during discovery; index it by path so the graph node keeps it.
    let role_by_path: HashMap<&Path, FileRole> = ws
        .files
        .iter()
        .map(|f| (f.rel_path.as_path(), f.role))
        .collect();

    let mut graph = Graph::default();
    let mut facts_by_path: HashMap<&Path, &FileFacts> = HashMap::new();
    for (id, f) in ordered.iter().enumerate() {
        let pkg = ws
            .package_for(&f.path)
            .map(|p| p.id.clone())
            .unwrap_or_else(|| ".".to_string());
        let role = role_by_path
            .get(f.path.as_path())
            .copied()
            .unwrap_or(FileRole::Source);
        graph.files.push(FileNode {
            id,
            path: f.path.clone(),
            language: f.language,
            is_entry: ws.entry_points.contains(&f.path),
            is_test: ws.test_files.contains(&f.path),
            is_implicit_used: false,
            package: pkg,
            role,
        });
        graph.path_to_file.insert(f.path.clone(), id);
        facts_by_path.insert(f.path.as_path(), f);
    }

    let n = graph.files.len();
    graph.imports = vec![Vec::new(); n];
    graph.imported_by = vec![Vec::new(); n];
    graph.imported_names = vec![HashSet::new(); n];

    let file_set: HashSet<PathBuf> = graph.path_to_file.keys().cloned().collect();
    let ctx = Ctx {
        files: &file_set,
        packages: &ws.packages,
        py_roots: global_py_roots(&ws.packages),
    };

    // Per-file adjacency as sets to dedup, converted to sorted Vecs at the end.
    let mut adjacency: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];

    for id in 0..n {
        let facts = facts_by_path[graph.files[id].path.as_path()];
        collect_symbols(&mut graph, id, facts);

        for s in &facts.string_literals {
            // Tokenize into identifier-like words so the "name appears in a
            // string" dampener is an O(1) membership check.
            for word in s.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                if !word.is_empty() {
                    graph.string_literals.insert(word.to_string());
                }
            }
        }
        // Cross-file member-liveness index: property names accessed anywhere.
        // Declaration names never land here, so it is self-reference-free.
        for m in &facts.member_accesses {
            graph.accessed_members.insert(m.clone());
        }
        if facts.has_unresolvable_dynamism() {
            graph
                .dynamic_packages
                .insert(graph.files[id].package.clone());
        }

        let from_dir = facts
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let pkg = ws.package_for(&facts.path);

        for import in &facts.imports {
            let resolved = match facts.language {
                Language::Python => py::resolve(&ctx, &import.specifier, &facts.path),
                _ => ts::resolve(&ctx, &import.specifier, &from_dir, pkg),
            };
            match resolved {
                Resolved::Internal(target) => {
                    if let Some(&tid) = graph.path_to_file.get(&target) {
                        if tid != id {
                            adjacency[id].insert(tid);
                        }
                        record_used_names(&mut graph, tid, import);
                    }
                }
                Resolved::External(dep) => {
                    graph.external_used.insert(dep);
                }
                Resolved::Unresolved => {
                    // A statically unresolvable bare/relative import in a package
                    // dampens confidence there (handled via dynamic_packages only
                    // for genuine dynamism; plain typos are left alone).
                }
            }

            // A Python submodule import (`from pkg import sub`) should also keep
            // the submodule file reachable, not just the package `__init__`.
            if facts.language == Language::Python {
                py::resolve_submodules(&ctx, import, &facts.path, |target| {
                    if let Some(&tid) = graph.path_to_file.get(&target) {
                        if tid != id {
                            adjacency[id].insert(tid);
                        }
                    }
                });
            }
        }
    }

    for (id, set) in adjacency.into_iter().enumerate() {
        for tid in set {
            graph.imports[id].push(tid);
            graph.imported_by[tid].push(id);
        }
    }
    for list in graph.imported_by.iter_mut() {
        list.sort_unstable();
    }

    graph
}

fn collect_symbols(graph: &mut Graph, file_id: usize, facts: &FileFacts) {
    let path_str = facts.path.to_string_lossy();
    for sym in &facts.symbols {
        // Members carry their owner and declaration line in the id so two members
        // sharing a name (across enums/classes, or overloaded) stay distinct.
        let id = match &sym.parent {
            Some(parent) => format!("{path_str}::{parent}.{}@{}", sym.name, sym.span.start_line),
            None => format!("{path_str}::{}", sym.name),
        };
        graph.symbols.push(SymbolNode {
            id,
            name: sym.name.clone(),
            kind: sym.kind,
            file: file_id,
            span: sym.span,
            exported: sym.exported,
            is_type_only: sym.is_type_only,
            parent: sym.parent.clone(),
            visibility: sym.visibility,
        });
    }
}

/// Record, on the *target* file, which of its exported names an importer uses.
fn record_used_names(graph: &mut Graph, target: usize, import: &noslop_graph::RawImport) {
    let names = &mut graph.imported_names[target];
    if import.is_namespace || matches!(import.kind, ImportKind::Require) {
        // Whole-module binding: we cannot tell which exports are touched, so keep
        // all of them live (the conservative, false-positive-avoiding choice).
        names.insert(Graph::WILDCARD.to_string());
        return;
    }
    for n in &import.names {
        names.insert(n.imported.clone());
    }
}

fn global_py_roots(packages: &[Package]) -> Vec<PathBuf> {
    let mut roots: BTreeSet<PathBuf> = BTreeSet::new();
    for p in packages {
        for r in &p.py_roots {
            roots.insert(r.clone());
        }
    }
    roots.insert(PathBuf::new()); // repo root is always a candidate
    roots.into_iter().collect()
}
