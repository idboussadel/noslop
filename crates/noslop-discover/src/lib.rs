//! `noslop-discover` — stage 1. Walk the repo, build the package registry, and
//! detect entry points via the plugin engine (ARCHITECTURE.md §5).
//!
//! Produces a [`Workspace`]: packages (scan roots with their resolution
//! context), the source-file list, and the entry-point / test-file sets that
//! seed reachability. All of this is plain data the later stages read.

mod builtin_plugins;
mod manifest;
mod plugin_def;
mod registry;

pub use plugin_def::{DetectRule, GlobRule, PluginDef};
pub use registry::{
    is_route_decorator, plugin_language, trigger_dep, DiscoverOptions, PluginRegistry,
};

use ignore::WalkBuilder;
use noslop_graph::{
    DiscoveredFile, FileFacts, FileRole, Language, ManifestKind, Marker, Package, Workspace,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Walk `root` (absolute) and produce the workspace model.
pub fn discover(root: &Path) -> Workspace {
    discover_with(root, &DiscoverOptions::default())
}

/// Like [`discover`], with user refinements from `noslop.toml`.
pub fn discover_with(root: &Path, opts: &DiscoverOptions) -> Workspace {
    let registry = PluginRegistry::load(root, opts);
    discover_inner(root, opts, &registry)
}

fn discover_inner(root: &Path, opts: &DiscoverOptions, registry: &PluginRegistry) -> Workspace {
    let mut raw_manifests: Vec<RawManifest> = Vec::new();
    let mut files: Vec<DiscoveredFile> = Vec::new();

    for entry in WalkBuilder::new(root).hidden(false).build().flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let abs = entry.path();
        let Ok(rel) = abs.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_path_buf();
        let name = abs.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if let Some(kind) = manifest_kind(name) {
            raw_manifests.push(RawManifest {
                kind,
                dir: rel.parent().unwrap_or(Path::new("")).to_path_buf(),
                abs: abs.to_path_buf(),
            });
            continue;
        }
        if let Some(lang) = abs
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Language::from_extension)
        {
            let role = FileRole::classify(&rel, lang);
            files.push(DiscoveredFile {
                rel_path: rel,
                abs_path: abs.to_path_buf(),
                language: lang,
                package: String::new(), // assigned below
                role,
            });
        }
    }

    let mut packages = build_packages(root, &raw_manifests);
    assign_files(&mut packages, &mut files);
    finalize_languages(&mut packages, &files);
    detect_plugins(root, &mut packages, registry);

    let (entry_points, test_files) = compute_entries(root, &packages, &files, registry, opts);

    Workspace {
        root: root.to_path_buf(),
        packages,
        files,
        entry_points,
        test_files,
    }
}

/// Augment the entry-point set with fact-derived implicit roots: a file whose
/// decorators match an active plugin's route/task/command markers is live even
/// if nothing imports it (ARCHITECTURE.md §5.3). Called after extraction.
pub fn augment_entries_with_facts(ws: &mut Workspace, facts: &[FileFacts]) {
    let pkg_of: HashMap<&Path, &str> = ws
        .files
        .iter()
        .map(|f| (f.rel_path.as_path(), f.package.as_str()))
        .collect();

    let mut new_entries = Vec::new();
    for f in facts {
        let Some(pkg) = pkg_of.get(f.path.as_path()) else {
            continue;
        };
        let has_route = f.markers.iter().any(|m| match m {
            Marker::Decorator { dotted, .. } => package_route_decorator(ws, pkg, dotted),
            _ => false,
        });
        if has_route {
            new_entries.push(f.path.clone());
        }
    }
    ws.entry_points.extend(new_entries);
}

fn package_route_decorator(ws: &Workspace, pkg_id: &str, dotted: &str) -> bool {
    let Some(pkg) = ws.packages.iter().find(|p| p.id == pkg_id) else {
        return false;
    };
    let last = dotted.rsplit('.').next().unwrap_or(dotted);
    pkg.route_decorators.iter().any(|d| d == last)
}

#[cfg(test)]
mod integration {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn fastapi_route_module_becomes_entry_after_facts() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mixed");
        let root = root.canonicalize().unwrap();
        let mut ws = discover(&root);
        let facts: Vec<FileFacts> = ws
            .files
            .iter()
            .map(|f| {
                let bytes = std::fs::read(&f.abs_path).unwrap_or_default();
                let source = String::from_utf8_lossy(&bytes);
                noslop_extract::extract(f.rel_path.clone(), &source, 0, f.language)
            })
            .collect();
        augment_entries_with_facts(&mut ws, &facts);
        let pkg = ws
            .packages
            .iter()
            .find(|p| p.id == "acme-api")
            .expect("acme-api package");
        assert!(
            pkg.plugins.contains(&"fastapi".to_string()),
            "fastapi plugin should activate; deps={:?} plugins={:?}",
            pkg.dependencies,
            pkg.plugins
        );
        assert!(
            ws.entry_points
                .contains(&PathBuf::from("apps/api/app/routes.py")),
            "route decorators must seed entry points; route_decorators={:?}",
            pkg.route_decorators
        );
    }
}

struct RawManifest {
    kind: ManifestKind,
    dir: PathBuf,
    abs: PathBuf,
}

fn manifest_kind(name: &str) -> Option<ManifestKind> {
    match name {
        "package.json" => Some(ManifestKind::PackageJson),
        "pyproject.toml" => Some(ManifestKind::PyProject),
        "setup.py" | "setup.cfg" => Some(ManifestKind::SetupPy),
        // tsconfig is resolution metadata merged into its package, not a package
        // on its own; handled in `build_packages`.
        _ => None,
    }
}

fn build_packages(root: &Path, manifests: &[RawManifest]) -> Vec<Package> {
    let mut by_dir: HashMap<PathBuf, Package> = HashMap::new();

    for m in manifests {
        let entry = by_dir.entry(m.dir.clone()).or_insert_with(|| Package {
            id: String::new(),
            name: String::new(),
            root: m.dir.clone(),
            language: Language::TypeScript,
            manifest_kind: m.kind,
            dependencies: Default::default(),
            ts_base_url: None,
            ts_paths: Vec::new(),
            py_roots: Vec::new(),
            plugins: Vec::new(),
            framework_deps: Default::default(),
            route_decorators: Vec::new(),
        });

        let text = std::fs::read_to_string(&m.abs).unwrap_or_default();
        match m.kind {
            ManifestKind::PackageJson => {
                let pj = manifest::parse_package_json(&text);
                entry.name = pj.name.clone().unwrap_or_default();
                entry.dependencies.extend(pj.dependencies);
                entry.language = Language::TypeScript;
                entry.manifest_kind = ManifestKind::PackageJson;
            }
            ManifestKind::PyProject => {
                let pp = manifest::parse_pyproject(&text, root.join(&m.dir).as_path());
                if entry.name.is_empty() {
                    entry.name = pp.name.clone().unwrap_or_default();
                }
                entry.dependencies.extend(pp.dependencies);
                entry.language = Language::Python;
                entry.manifest_kind = ManifestKind::PyProject;
                entry.py_roots = python_roots(&m.dir, pp.src_layout);
            }
            ManifestKind::SetupPy => {
                if entry.manifest_kind != ManifestKind::PyProject {
                    entry.language = Language::Python;
                    entry.manifest_kind = ManifestKind::SetupPy;
                    entry.py_roots = python_roots(&m.dir, root.join(&m.dir).join("src").is_dir());
                }
            }
            ManifestKind::Implicit => {}
        }
    }

    // Merge tsconfig.json settings into the JS package sharing its directory.
    merge_tsconfigs(root, &mut by_dir);

    // Guarantee a root package so files outside any manifest still belong somewhere.
    by_dir.entry(PathBuf::new()).or_insert_with(|| Package {
        id: String::new(),
        name: String::new(),
        root: PathBuf::new(),
        language: Language::TypeScript,
        manifest_kind: ManifestKind::Implicit,
        dependencies: Default::default(),
        ts_base_url: None,
        ts_paths: Vec::new(),
        py_roots: vec![PathBuf::new()],
        plugins: Vec::new(),
        framework_deps: Default::default(),
        route_decorators: Vec::new(),
    });

    let mut packages: Vec<Package> = by_dir.into_values().collect();
    for p in &mut packages {
        // Stable id: the manifest name if present, else the root path (or "." for
        // the repo root package).
        p.id = if !p.name.is_empty() {
            p.name.clone()
        } else if p.root.as_os_str().is_empty() {
            ".".to_string()
        } else {
            p.root.display().to_string()
        };
        if p.py_roots.is_empty() {
            p.py_roots = python_roots(&p.root, root.join(&p.root).join("src").is_dir());
        }
    }
    packages.sort_by(|a, b| a.root.cmp(&b.root));
    packages
}

fn merge_tsconfigs(root: &Path, by_dir: &mut HashMap<PathBuf, Package>) {
    let dirs: Vec<PathBuf> = by_dir.keys().cloned().collect();
    for dir in dirs {
        let tsconfig = root.join(&dir).join("tsconfig.json");
        if !tsconfig.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&tsconfig).unwrap_or_default();
        let ts = manifest::parse_tsconfig(&text);
        if let Some(pkg) = by_dir.get_mut(&dir) {
            pkg.ts_base_url = ts.base_url.map(|b| dir.join(b));
            pkg.ts_paths = ts.paths;
        }
    }
}

fn python_roots(dir: &Path, src_layout: bool) -> Vec<PathBuf> {
    if src_layout {
        vec![dir.join("src"), dir.to_path_buf()]
    } else {
        vec![dir.to_path_buf()]
    }
}

/// Assign each file to the deepest package whose root is an ancestor.
fn assign_files(packages: &mut [Package], files: &mut [DiscoveredFile]) {
    let mut roots: Vec<(usize, PathBuf)> = packages
        .iter()
        .enumerate()
        .map(|(i, p)| (i, p.root.clone()))
        .collect();
    // Deepest root first so the most specific package wins.
    roots.sort_by_key(|(_, root)| std::cmp::Reverse(root.as_os_str().len()));

    for f in files.iter_mut() {
        for (i, root) in &roots {
            if root.as_os_str().is_empty() || f.rel_path.starts_with(root) {
                f.package = packages[*i].id.clone();
                break;
            }
        }
    }
}

/// For implicit / mixed packages, pick the dominant language by file count.
fn finalize_languages(packages: &mut [Package], files: &[DiscoveredFile]) {
    for pkg in packages.iter_mut() {
        if pkg.manifest_kind != ManifestKind::Implicit {
            continue;
        }
        let (mut py, mut js) = (0u32, 0u32);
        for f in files.iter().filter(|f| f.package == pkg.id) {
            if f.language.is_python() {
                py += 1;
            } else {
                js += 1;
            }
        }
        pkg.language = if py > js {
            Language::Python
        } else {
            Language::TypeScript
        };
    }
}

fn detect_plugins(root: &Path, packages: &mut [Package], registry: &PluginRegistry) {
    for pkg in packages.iter_mut() {
        let abs_root = root.join(&pkg.root);
        let active = registry.active_for(pkg, &abs_root);
        pkg.framework_deps = active
            .iter()
            .filter_map(|p| trigger_dep(p))
            .map(String::from)
            .collect();
        pkg.plugins = active.iter().map(|p| p.name.clone()).collect();
        pkg.route_decorators = active
            .iter()
            .flat_map(|p| p.route_decorators.iter().cloned())
            .collect();
    }
}

/// Match plugin + fallback globs against files to build the entry-point set, and
/// test globs to build the test-file set. Returns (entry_points, test_files),
/// both repo-relative.
fn compute_entries(
    root: &Path,
    packages: &[Package],
    files: &[DiscoveredFile],
    registry: &PluginRegistry,
    opts: &DiscoverOptions,
) -> (
    std::collections::HashSet<PathBuf>,
    std::collections::HashSet<PathBuf>,
) {
    use std::collections::HashSet;
    let mut entries = HashSet::new();
    let mut tests = HashSet::new();

    for pkg in packages {
        let abs_root = root.join(&pkg.root);
        let test_set = PluginRegistry::build_globset(&registry.test_globs_for(pkg, &abs_root));
        let entry_set = PluginRegistry::build_globset(&registry.entry_globs_for(
            pkg,
            &abs_root,
            &opts.entry_points,
        ));

        for f in files.iter().filter(|f| f.package == pkg.id) {
            let rel_to_pkg = f
                .rel_path
                .strip_prefix(&pkg.root)
                .unwrap_or(&f.rel_path)
                .to_string_lossy()
                .replace('\\', "/");
            if test_set.is_match(&rel_to_pkg) {
                tests.insert(f.rel_path.clone());
                entries.insert(f.rel_path.clone()); // tests keep imports alive
            } else if entry_set.is_match(&rel_to_pkg) {
                entries.insert(f.rel_path.clone());
            }
        }

        // Manifest-declared entry targets (package.json main/bin, pyproject scripts).
        for target in manifest_entry_targets(root, pkg) {
            if let Ok(rel) = target.strip_prefix(root) {
                entries.insert(rel.to_path_buf());
            }
        }
    }

    (entries, tests)
}

fn manifest_entry_targets(root: &Path, pkg: &Package) -> Vec<PathBuf> {
    let abs_root = root.join(&pkg.root);
    let is_py = pkg.language.is_python();
    let mut out = Vec::new();

    if is_py {
        let path = abs_root.join("pyproject.toml");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let pp = manifest::parse_pyproject(&text, &abs_root);
            for module in pp.script_modules {
                if let Some(p) = manifest::resolve_entry_target(&abs_root, &module, true) {
                    out.push(p);
                }
            }
        }
    } else {
        let path = abs_root.join("package.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let pj = manifest::parse_package_json(&text);
            for target in pj.entry_targets {
                if let Some(p) = manifest::resolve_entry_target(&abs_root, &target, false) {
                    out.push(p);
                }
            }
        }
    }
    out
}
