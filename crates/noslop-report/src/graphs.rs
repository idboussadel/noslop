//! Build `noslop-viz` view models from the resolved [`Graph`].
//!
//! Projects the internal file-level graph to a coarser import graph: packages at
//! depth 0, directories under each package at depth 1+, with weighted edges and
//! import cycles. All rendering lives in `noslop-viz`.

use noslop_graph::Graph;
use noslop_viz::{Edge, EdgeKind, Node, PackageGraph};
use petgraph::algo::tarjan_scc;
use petgraph::graph::DiGraph;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Knobs for aggregating the file graph into a view graph.
#[derive(Debug, Clone, Copy)]
pub struct ImportGraphOptions {
    /// `0` = one node per package; `1+` = group files by that many directory
    /// segments under each package root (`app`, `src/lib`, …).
    pub depth: usize,
    /// Drop nodes once this cap is reached (stable order: label, then id).
    pub max_nodes: usize,
    /// Drop edges whose aggregated file-edge weight is below this.
    pub min_weight: usize,
}

impl Default for ImportGraphOptions {
    fn default() -> Self {
        ImportGraphOptions {
            depth: 0,
            max_nodes: 40,
            min_weight: 1,
        }
    }
}

/// Aggregate the file-level import graph into a [`PackageGraph`].
pub fn build_package_graph(graph: &Graph) -> PackageGraph {
    build_import_graph(graph, &[], ImportGraphOptions::default())
}

/// Build an import graph with configurable directory depth.
///
/// `package_roots` maps package id → repo-relative root path (from
/// [`crate::ScanRootReport`]). When empty, roots are inferred as the longest
/// common path prefix of each package's files.
pub fn build_import_graph(
    graph: &Graph,
    package_roots: &[(&str, &str)],
    opts: ImportGraphOptions,
) -> PackageGraph {
    let roots = resolve_roots(graph, package_roots);
    let keys = file_node_keys(graph, &roots, opts.depth);
    let mut nodes = collect_nodes(graph, &keys);
    nodes.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.id.cmp(&b.id)));
    if nodes.len() > opts.max_nodes {
        nodes.truncate(opts.max_nodes);
    }
    let index: BTreeMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.clone(), i))
        .collect();

    let mut weights: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (fid, targets) in graph.imports.iter().enumerate() {
        let Some(&src) = index.get(&keys[fid]) else {
            continue;
        };
        for &tid in targets {
            let Some(&dst) = index.get(&keys[tid]) else {
                continue;
            };
            if src != dst {
                *weights.entry((src, dst)).or_insert(0) += 1;
            }
        }
    }
    let edges: Vec<Edge> = weights
        .iter()
        .filter(|(_, &w)| w >= opts.min_weight)
        .map(|(&(from, to), &weight)| Edge {
            from,
            to,
            kind: EdgeKind::Import,
            weight,
        })
        .collect();

    let cycles = package_cycles(nodes.len(), &edges);

    PackageGraph {
        nodes,
        edges,
        cycles,
    }
}

fn resolve_roots(graph: &Graph, package_roots: &[(&str, &str)]) -> BTreeMap<String, PathBuf> {
    let mut roots: BTreeMap<String, PathBuf> = package_roots
        .iter()
        .map(|(id, root)| (id.to_string(), PathBuf::from(root)))
        .collect();
    if !roots.is_empty() {
        return roots;
    }
    let mut paths: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for f in &graph.files {
        paths
            .entry(f.package.clone())
            .or_default()
            .push(f.path.clone());
    }
    for (pkg, paths) in paths {
        roots.insert(pkg, common_prefix(&paths));
    }
    roots
}

fn common_prefix(paths: &[PathBuf]) -> PathBuf {
    if paths.is_empty() {
        return PathBuf::new();
    }
    let mut prefix: Vec<Component<'_>> = paths[0].components().collect();
    for path in &paths[1..] {
        let comps: Vec<_> = path.components().collect();
        let n = prefix
            .iter()
            .zip(comps.iter())
            .take_while(|(a, b)| a == b)
            .count();
        prefix.truncate(n);
        if prefix.is_empty() {
            break;
        }
    }
    prefix.into_iter().collect()
}

fn file_node_keys(graph: &Graph, roots: &BTreeMap<String, PathBuf>, depth: usize) -> Vec<String> {
    graph
        .files
        .iter()
        .map(|f| node_key(f.package.as_str(), &f.path, roots.get(&f.package), depth))
        .collect()
}

fn node_key(package: &str, path: &Path, root: Option<&PathBuf>, depth: usize) -> String {
    if depth == 0 {
        return display_id(package);
    }
    let rel = root
        .and_then(|r| path.strip_prefix(r).ok())
        .unwrap_or(path);
    let segs = path_segments(rel);
    if segs.is_empty() {
        return display_id(package);
    }
    let take = depth.min(segs.len());
    let segment = segs[..take].join("/");
    format!("{}/{}", display_id(package), segment)
}

/// Repo-relative path under a package root as segments: directories, then file stem.
fn path_segments(rel: &Path) -> Vec<String> {
    let mut segs: Vec<String> = Vec::new();
    if let Some(parent) = rel.parent().filter(|p| p.as_os_str() != std::ffi::OsStr::new("")) {
        for c in parent.components() {
            if let Component::Normal(s) = c {
                segs.push(s.to_string_lossy().into_owned());
            }
        }
    }
    if let Some(stem) = rel.file_stem().and_then(|s| s.to_str()) {
        segs.push(stem.to_string());
    }
    segs
}

fn collect_nodes(graph: &Graph, keys: &[String]) -> Vec<Node> {
    let mut files: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for (f, key) in graph.files.iter().zip(keys) {
        let entry = files.entry(key.clone()).or_insert_with(|| (f.package.clone(), 0));
        entry.1 += 1;
    }
    files
        .into_iter()
        .map(|(id, (package, count))| Node {
            label: short_label(&id, &package),
            files: count,
            package,
            id,
        })
        .collect()
}

fn package_cycles(node_count: usize, edges: &[Edge]) -> Vec<Vec<usize>> {
    let mut g: DiGraph<usize, ()> = DiGraph::new();
    let idx: Vec<_> = (0..node_count).map(|i| g.add_node(i)).collect();
    for e in edges {
        g.add_edge(idx[e.from], idx[e.to], ());
    }
    let mut cycles: Vec<Vec<usize>> = tarjan_scc(&g)
        .into_iter()
        .filter(|scc| scc.len() > 1)
        .map(|scc| {
            let mut ids: Vec<usize> = scc.into_iter().map(|n| g[n]).collect();
            ids.sort_unstable();
            ids
        })
        .collect();
    cycles.sort();
    cycles
}

fn display_id(package: &str) -> String {
    if package.is_empty() {
        "root".to_string()
    } else {
        package.to_string()
    }
}

/// Short display label — last path segment, or `package/segment` at depth > 0.
fn short_label(id: &str, package: &str) -> String {
    if id == display_id(package) {
        return Path::new(package)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(package)
            .to_string();
    }
    id.strip_prefix(&format!("{}/", display_id(package)))
        .or_else(|| id.strip_prefix(&format!("{package}/")))
        .unwrap_or(id)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use noslop_graph::{FileNode, FileRole, Language};
    use std::path::PathBuf;

    fn graph(files: &[(&str, &str, &[usize])]) -> Graph {
        let mut g = Graph::default();
        for (i, (pkg, rel, _)) in files.iter().enumerate() {
            g.files.push(FileNode {
                id: i,
                path: PathBuf::from(rel),
                language: Language::TypeScript,
                is_entry: false,
                is_test: false,
                is_implicit_used: false,
                package: pkg.to_string(),
                role: FileRole::Source,
            });
        }
        g.imports = files.iter().map(|(_, _, imps)| imps.to_vec()).collect();
        g
    }

    #[test]
    fn aggregates_file_edges_by_package_and_counts_files() {
        let g = graph(&[
            ("app", "app/f0.ts", &[2]),
            ("app", "app/f1.ts", &[2]),
            ("core", "core/f2.ts", &[3]),
            ("util", "util/f3.ts", &[]),
        ]);
        let view = build_package_graph(&g);

        let labels: Vec<_> = view.nodes.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["app", "core", "util"]);
        assert_eq!(view.nodes[0].files, 2);

        let app_core = view.edges.iter().find(|e| e.from == 0 && e.to == 1).unwrap();
        assert_eq!(app_core.weight, 2);
        assert!(view.cycles.is_empty());
    }

    #[test]
    fn depth_splits_directories_within_a_package() {
        let g = graph(&[
            ("web", "apps/web/src/app/page.tsx", &[1]),
            ("web", "apps/web/src/lib/format.ts", &[]),
        ]);
        let roots = [("web", "apps/web")];
        let view = build_import_graph(
            &g,
            &roots,
            ImportGraphOptions {
                depth: 2,
                ..Default::default()
            },
        );
        assert_eq!(view.nodes.len(), 2);
        assert_eq!(view.edges.len(), 1);
        assert_eq!(view.edges[0].weight, 1);
    }

    #[test]
    fn detects_package_cycle() {
        let g = graph(&[
            ("a", "a/f0.ts", &[1]),
            ("b", "b/f1.ts", &[0]),
        ]);
        let view = build_package_graph(&g);
        assert_eq!(view.cycles, vec![vec![0, 1]]);
    }

    #[test]
    fn intra_package_same_dir_has_no_edge_at_depth_zero() {
        let g = graph(&[("app", "app/a.ts", &[1]), ("app", "app/b.ts", &[])]);
        let view = build_package_graph(&g);
        assert_eq!(view.nodes.len(), 1);
        assert!(view.edges.is_empty());
    }

    #[test]
    fn depth_two_splits_files_in_flat_package_dir() {
        let g = graph(&[
            ("api", "apps/api/app/cycle_a.py", &[1]),
            ("api", "apps/api/app/cycle_b.py", &[0]),
        ]);
        let roots = [("api", "apps/api")];
        let view = build_import_graph(
            &g,
            &roots,
            ImportGraphOptions {
                depth: 2,
                ..Default::default()
            },
        );
        assert_eq!(view.nodes.len(), 2);
        assert_eq!(view.edges.len(), 2);
        assert!(!view.cycles.is_empty());
    }
}
