//! The resolved graph — the single IR every analysis pass queries.
//!
//! The whole graph is rebuilt from cached [`crate::facts::FileFacts`] on every
//! run; build time is milliseconds and there is never an incremental-graph to
//! corrupt (ARCHITECTURE.md §7.2). Reachability and cycle detection run over the
//! file-level import graph; symbol-level liveness is answered from the
//! precomputed `imported_names` index.

use crate::facts::SymbolKind;
use crate::ids::{FileId, FileRole, Language, Span};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A file node: identity plus the flags plugins and passes set on it.
#[derive(Debug, Clone)]
pub struct FileNode {
    pub id: FileId,
    /// Repo-relative path — the stable identity used in output and symbol ids.
    pub path: PathBuf,
    pub language: Language,
    /// Reachable "live" root: a framework/CLI entry point or fallback root.
    pub is_entry: bool,
    /// A test file — an entry point that keeps imports alive but is excluded
    /// from "is this production-dead" (enables the `only-used-in-tests` rule).
    pub is_test: bool,
    /// Some plugin marked this file implicitly used (e.g. a Django settings
    /// module referenced only by string), independent of import edges.
    pub is_implicit_used: bool,
    /// The package (scan root) this file belongs to.
    pub package: String,
    /// Source / config / type-decl / package-init — gates which dead-code rules
    /// apply (see [`FileRole`]).
    pub role: FileRole,
}

/// A symbol node: a definition plus enough context to attribute a finding.
#[derive(Debug, Clone)]
pub struct SymbolNode {
    /// Stable id: `<repo-relative-path>::<dotted symbol path>`.
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub file: FileId,
    pub span: Span,
    pub exported: bool,
    pub is_type_only: bool,
    /// Enclosing type/class name for members (enum members, class members).
    pub parent: Option<String>,
    /// Declared visibility (`@public`/`@internal`/`@expected-unused`), if any.
    pub visibility: Option<crate::facts::Visibility>,
}

/// The edge relations the graph records. Only `Imports` participates in
/// reachability/cycle analysis in v1; the rest are carried for future passes
/// and richer reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Imports,
    ReExports,
    PluginEdge,
}

/// The resolved repository graph.
#[derive(Debug, Default)]
pub struct Graph {
    pub files: Vec<FileNode>,
    pub symbols: Vec<SymbolNode>,

    /// `path_to_file[path]` → `FileId`. Repo-relative paths.
    pub path_to_file: HashMap<PathBuf, FileId>,

    /// File-level import adjacency: `imports[a]` are the internal files `a`
    /// imports. This is the graph reachability and cycle passes traverse.
    pub imports: Vec<Vec<FileId>>,
    /// Reverse of `imports`, precomputed for blast-radius / reverse BFS.
    pub imported_by: Vec<Vec<FileId>>,

    /// For each file, the set of names other files import *from* it (by name).
    /// Drives `unused-export`. A namespace/default import of a file records the
    /// sentinel [`Graph::WILDCARD`] meaning "all exports of this file are used".
    pub imported_names: Vec<HashSet<String>>,

    /// Top-level external module/package names some import resolved to
    /// (`react`, `fastapi`). Drives `unused-dependency`.
    pub external_used: HashSet<String>,

    /// Files that contain unresolvable dynamism, grouped by package — every
    /// `unused-*` finding in these packages is confidence-capped.
    pub dynamic_packages: HashSet<String>,

    /// Every string literal token in the repo, lowercased-free, used for the
    /// cheap "name appears in a string" confidence dampener.
    pub string_literals: HashSet<String>,

    /// Every property name *accessed* anywhere in the repo (`obj.x`, `self._y`,
    /// `E.Red`, `{ x } = obj`). A member (enum member, private class member) is
    /// live iff its name is in here. Declaration sites contribute nothing — a
    /// method/enum-member declaration name is not a member access — so this is
    /// self-reference-free and cross-file safe (it can only miss a use, which
    /// suppresses a finding, never invents one).
    pub accessed_members: HashSet<String>,
}

impl Graph {
    /// Sentinel stored in `imported_names` to mean "a namespace/default import
    /// bound the whole module, so treat all its exports as used".
    pub const WILDCARD: &'static str = "*";

    pub fn file(&self, id: FileId) -> &FileNode {
        &self.files[id]
    }

    pub fn file_id(&self, path: &Path) -> Option<FileId> {
        self.path_to_file.get(path).copied()
    }

    /// Is `name`, exported by file `id`, imported by any other file?
    pub fn export_is_used(&self, id: FileId, name: &str) -> bool {
        let names = &self.imported_names[id];
        names.contains(Self::WILDCARD) || names.contains(name)
    }

    /// True if the package has unresolvable dynamism that should cap confidence.
    pub fn package_is_dynamic(&self, package: &str) -> bool {
        self.dynamic_packages.contains(package)
    }

    /// Is a member/enum-member `name` accessed anywhere in the repo?
    pub fn member_is_accessed(&self, name: &str) -> bool {
        self.accessed_members.contains(name)
    }
}
