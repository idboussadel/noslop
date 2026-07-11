//! Plugin registry — loads built-in TOML, user paths, and repo conventions.

use crate::builtin_plugins::builtin_sources;
use crate::plugin_def::PluginDef;
use globset::{Glob, GlobSet, GlobSetBuilder};
use noslop_graph::{Language, Package};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// User-supplied discovery refinements (from `noslop.toml`).
#[derive(Debug, Clone, Default)]
pub struct DiscoverOptions {
    /// Extra entry-point globs, merged after built-in detection.
    pub entry_points: Vec<String>,
    /// Extra plugin files or directories to load.
    pub plugin_paths: Vec<PathBuf>,
}

/// All plugins available for a scan — built-in plus user-supplied.
#[derive(Debug, Clone)]
pub struct PluginRegistry {
    plugins: Vec<PluginDef>,
}

impl PluginRegistry {
    /// Load built-in plugins plus anything under `root` / `opts.plugin_paths`.
    pub fn load(root: &Path, opts: &DiscoverOptions) -> Self {
        let mut plugins = Vec::new();
        for (stem, text) in builtin_sources() {
            match PluginDef::parse(stem, text) {
                Ok(p) => plugins.push(p),
                Err(e) => panic!("built-in plugin '{stem}' is invalid TOML: {e}"),
            }
        }
        for path in &opts.plugin_paths {
            load_user_path(root, path, &mut plugins);
        }
        discover_repo_plugins(root, &mut plugins);
        dedup_by_name(&mut plugins);
        PluginRegistry { plugins }
    }

    pub fn all(&self) -> &[PluginDef] {
        &self.plugins
    }

    /// Plugins active for this package (language + detect).
    pub fn active_for<'a>(&'a self, pkg: &Package, abs_root: &Path) -> Vec<&'a PluginDef> {
        self.plugins
            .iter()
            .filter(|p| p.matches_language(pkg.language))
            .filter(|p| p.detect.matches(pkg, abs_root))
            .collect()
    }

    /// Universal test globs: `_fallback` plus any active plugin `test_patterns`.
    pub fn test_globs_for(&self, pkg: &Package, abs_root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        for p in self.active_for(pkg, abs_root) {
            out.extend(p.test_globs().map(str::to_string));
        }
        out
    }

    /// Entry globs: fallback + active plugins + per-scan user additions.
    pub fn entry_globs_for(
        &self,
        pkg: &Package,
        abs_root: &Path,
        user_extra: &[String],
    ) -> Vec<String> {
        let mut out = Vec::new();
        for p in self.active_for(pkg, abs_root) {
            out.extend(p.entry_globs().map(str::to_string));
        }
        out.extend(user_extra.iter().cloned());
        out
    }

    pub fn build_globset(patterns: &[String]) -> GlobSet {
        let mut builder = GlobSetBuilder::new();
        for pat in patterns {
            if let Ok(glob) = Glob::new(pat) {
                builder.add(glob);
            }
        }
        builder.build().unwrap_or_else(|_| GlobSet::empty())
    }
}

pub fn trigger_dep(plugin: &PluginDef) -> Option<&str> {
    plugin.trigger_dependency()
}

/// Does any active plugin treat `dotted` decorator as a route/handler marker?
pub fn is_route_decorator(
    registry: &PluginRegistry,
    active_names: &[String],
    dotted: &str,
) -> bool {
    let last = dotted.rsplit('.').next().unwrap_or(dotted);
    registry
        .plugins
        .iter()
        .filter(|p| active_names.iter().any(|n| n == &p.name))
        .any(|p| p.route_decorators.iter().any(|d| d == last))
}

pub fn plugin_language(registry: &PluginRegistry, name: &str) -> Option<Language> {
    let p = registry.plugins.iter().find(|p| p.name == name)?;
    if p.matches_language(Language::Python) && !p.matches_language(Language::TypeScript) {
        Some(Language::Python)
    } else {
        Some(Language::TypeScript)
    }
}

fn dedup_by_name(plugins: &mut Vec<PluginDef>) {
    let mut seen = HashSet::new();
    plugins.retain(|p| seen.insert(p.name.clone()));
}

fn discover_repo_plugins(root: &Path, out: &mut Vec<PluginDef>) {
    let dir = root.join(".noslop/plugins");
    if dir.is_dir() {
        load_dir(&dir, out);
    }
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(s) = name.to_str() else { continue };
            if (s.starts_with("noslop-plugin-") && (s.ends_with(".toml") || s.ends_with(".json")))
                || s == "noslop-plugin.toml"
            {
                load_user_file(&entry.path(), out);
            }
        }
    }
}

fn load_user_path(root: &Path, path: &Path, out: &mut Vec<PluginDef>) {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    if abs.is_dir() {
        load_dir(&abs, out);
    } else if abs.is_file() {
        load_user_file(&abs, out);
    }
}

fn load_dir(dir: &Path, out: &mut Vec<PluginDef>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            load_user_file(&path, out);
        }
    }
}

fn load_user_file(path: &Path, out: &mut Vec<PluginDef>) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin");
    if let Ok(p) = PluginDef::parse(stem, &text) {
        out.push(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noslop_graph::ManifestKind;
    use std::collections::HashSet;

    fn pkg(deps: &[&str], lang: Language) -> Package {
        Package {
            id: "t".into(),
            name: "t".into(),
            root: PathBuf::new(),
            language: lang,
            manifest_kind: ManifestKind::PackageJson,
            dependencies: deps.iter().map(|d| (*d).to_string()).collect(),
            ts_base_url: None,
            ts_paths: Vec::new(),
            py_roots: Vec::new(),
            plugins: Vec::new(),
            framework_deps: HashSet::new(),
            route_decorators: Vec::new(),
        }
    }

    #[test]
    fn fastapi_plugin_has_route_decorators() {
        let reg = PluginRegistry::load(Path::new("."), &DiscoverOptions::default());
        let fastapi = reg.all().iter().find(|p| p.name == "fastapi").unwrap();
        assert!(
            !fastapi.route_decorators.is_empty(),
            "got {:?}",
            fastapi.route_decorators
        );
    }

    #[test]
    fn all_builtin_plugins_parse() {
        let reg = PluginRegistry::load(Path::new("."), &DiscoverOptions::default());
        assert!(
            reg.all().len() >= 120,
            "expected Fallow parity plugin count, got {}",
            reg.all().len()
        );
        assert!(reg.all().iter().any(|p| p.name == "nextjs"));
        assert!(reg.all().iter().any(|p| p.name == "tailwind"));
        assert!(reg.all().iter().any(|p| p.name == "gunicorn"));
    }

    #[test]
    fn nextjs_activates_on_dependency() {
        let reg = PluginRegistry::load(Path::new("."), &DiscoverOptions::default());
        let next = reg
            .all()
            .iter()
            .find(|p| p.name == "nextjs")
            .expect("nextjs plugin");
        assert!(next
            .detect
            .matches(&pkg(&["next"], Language::TypeScript), Path::new(".")));
        let globs: Vec<_> = next.entry_globs().collect();
        assert!(globs.iter().any(|g| g.contains("proxy")));
    }

    #[test]
    fn fallback_always_active() {
        let reg = PluginRegistry::load(Path::new("."), &DiscoverOptions::default());
        let fb = reg.all().iter().find(|p| p.name == "_fallback").unwrap();
        assert!(fb.detect.always);
        let tests: Vec<_> = fb.test_globs().collect();
        assert!(tests.iter().any(|g| g.contains("e2e-spec")));
    }

    #[test]
    fn user_entry_globs_merge() {
        let reg = PluginRegistry::load(Path::new("."), &DiscoverOptions::default());
        let globs = reg.entry_globs_for(
            &pkg(&[], Language::TypeScript),
            Path::new("."),
            &["custom/**".into()],
        );
        assert!(globs.iter().any(|g| g == "custom/**"));
        assert!(globs.iter().any(|g| g.contains("index.")));
    }
}
