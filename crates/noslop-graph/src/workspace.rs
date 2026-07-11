//! Workspace model — the output of the *discover* stage, consumed by *resolve*.
//!
//! Lives in the IR crate so `resolve` can read it without depending on
//! `discover` (siblings never depend on each other; everything flows through
//! `noslop-graph`). Discovery *logic* lives in `noslop-discover`; these are just
//! the plain data structures it produces.

use crate::ids::{FileRole, Language};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestKind {
    PackageJson,
    PyProject,
    SetupPy,
    /// A directory treated as a package with no manifest (fallback root).
    Implicit,
}

/// A scan root: a package with its own entry points and resolution context
/// (ARCHITECTURE.md §5.2). Graphs are built per root and merged.
#[derive(Debug, Clone)]
pub struct Package {
    /// Stable identity (the manifest `name`, else the root path).
    pub id: String,
    pub name: String,
    /// Repo-relative package root.
    pub root: PathBuf,
    pub language: Language,
    pub manifest_kind: ManifestKind,
    /// Declared dependency names (for `unused-dependency`).
    pub dependencies: HashSet<String>,
    /// tsconfig `baseUrl`, repo-relative, if any.
    pub ts_base_url: Option<PathBuf>,
    /// tsconfig `paths`: alias pattern → target patterns, relative to base_url.
    pub ts_paths: Vec<(String, Vec<String>)>,
    /// Python import roots (repo-relative): the dirs a dotted path resolves under
    /// (flat layout = root, src-layout = root/src).
    pub py_roots: Vec<PathBuf>,
    /// Active framework plugin names (for reporting and implicit-used logic).
    pub plugins: Vec<String>,
    /// Dependencies that activated a plugin (the framework itself, e.g. `next`,
    /// `fastapi`). Used implicitly, so `unused-dependency` must not flag them.
    pub framework_deps: HashSet<String>,
    /// Merged `route_decorators` from all active plugins for this package.
    pub route_decorators: Vec<String>,
}

/// A source file the walker found, with the package it belongs to.
#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    /// Repo-relative path — the stable identity used everywhere downstream.
    pub rel_path: PathBuf,
    pub abs_path: PathBuf,
    pub language: Language,
    pub package: String,
    /// How dead-code rules should treat this file (source vs config/decl/init).
    pub role: FileRole,
}

/// The complete discovery result.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Absolute repository root.
    pub root: PathBuf,
    pub packages: Vec<Package>,
    pub files: Vec<DiscoveredFile>,
    /// Repo-relative paths that are live roots (framework/CLI/fallback entries).
    pub entry_points: HashSet<PathBuf>,
    /// Repo-relative test file paths (entry points, but excluded from
    /// "is this production-dead" — enables `only-used-in-tests`).
    pub test_files: HashSet<PathBuf>,
}

impl Workspace {
    /// Find the package a repo-relative path belongs to (longest-root match).
    pub fn package_for(&self, rel_path: &std::path::Path) -> Option<&Package> {
        self.packages
            .iter()
            .filter(|p| rel_path.starts_with(&p.root) || p.root.as_os_str().is_empty())
            .max_by_key(|p| p.root.as_os_str().len())
    }
}
