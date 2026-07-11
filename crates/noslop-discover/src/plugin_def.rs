//! Declarative plugin schema — framework conventions as data (ARCHITECTURE.md §5.3).

use noslop_graph::{Language, Package};
use serde::Deserialize;
use std::path::Path;

/// One framework/tool plugin loaded from TOML.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PluginDef {
    pub name: String,
    /// Single-language shorthand (`language = "python"`).
    #[serde(default)]
    pub language: Option<String>,
    /// Multi-language filter (`languages = ["typescript", "javascript"]`).
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub detect: DetectRule,
    #[serde(default)]
    pub entry_points: Vec<GlobRule>,
    /// Tool-loaded configs — treated as entry roots (never `unused-file`).
    #[serde(default)]
    pub config_patterns: Vec<GlobRule>,
    #[serde(default)]
    pub test_patterns: Vec<GlobRule>,
    /// Decorator tail names that mark a handler file as implicitly live.
    #[serde(default)]
    pub route_decorators: Vec<String>,
    /// Declared deps used only via CLI/config — exempt from `unused-dependency`.
    #[serde(default)]
    pub tooling_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct GlobRule {
    pub glob: String,
}

/// When a plugin activates for a package.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct DetectRule {
    /// Always contribute globs (used by `_fallback`).
    #[serde(default)]
    pub always: bool,
    pub dependency: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub file_exists: Option<String>,
    #[serde(default)]
    pub files_exist: Vec<String>,
    #[serde(default)]
    pub any: Vec<DetectRule>,
    #[serde(default)]
    pub all: Vec<DetectRule>,
}

impl PluginDef {
    pub fn parse(name: &str, text: &str) -> Result<Self, toml::de::Error> {
        let mut def: Self = toml::from_str(text)?;
        if def.name.is_empty() {
            def.name = name.to_string();
        }
        Ok(def)
    }

    pub fn matches_language(&self, lang: Language) -> bool {
        let tags: Vec<&str> = if !self.languages.is_empty() {
            self.languages.iter().map(String::as_str).collect()
        } else if let Some(l) = &self.language {
            vec![l.as_str()]
        } else {
            return true;
        };
        tags.iter().any(|tag| language_tag_matches(tag, lang))
    }

    pub fn entry_globs(&self) -> impl Iterator<Item = &str> {
        self.entry_points
            .iter()
            .chain(self.config_patterns.iter())
            .map(|r| r.glob.as_str())
    }

    pub fn test_globs(&self) -> impl Iterator<Item = &str> {
        self.test_patterns.iter().map(|r| r.glob.as_str())
    }

    pub fn trigger_dependency(&self) -> Option<&str> {
        self.detect.trigger_dependency()
    }
}

impl DetectRule {
    pub fn matches(&self, pkg: &Package, abs_root: &Path) -> bool {
        if self.always {
            return true;
        }
        if !self.any.is_empty() {
            return self.any.iter().any(|c| c.matches(pkg, abs_root));
        }
        if !self.all.is_empty() {
            return self.all.iter().all(|c| c.matches(pkg, abs_root));
        }
        let mut saw = false;
        if let Some(dep) = &self.dependency {
            saw = true;
            if dependency_matches(dep, &pkg.dependencies) {
                return true;
            }
        }
        for dep in &self.dependencies {
            saw = true;
            if dependency_matches(dep, &pkg.dependencies) {
                return true;
            }
        }
        if let Some(file) = &self.file_exists {
            saw = true;
            if abs_root.join(file).is_file() {
                return true;
            }
        }
        for file in &self.files_exist {
            saw = true;
            if abs_root.join(file).is_file() {
                return true;
            }
        }
        !saw
    }

    fn trigger_dependency(&self) -> Option<&str> {
        self.dependency
            .as_deref()
            .or_else(|| self.dependencies.first().map(String::as_str))
    }
}

/// Trailing `/` on a dependency means prefix match (Fallow convention).
fn dependency_matches(dep: &str, deps: &std::collections::HashSet<String>) -> bool {
    if dep.ends_with('/') {
        deps.iter().any(|d| d.starts_with(dep))
    } else {
        deps.contains(dep)
    }
}

fn language_tag_matches(tag: &str, lang: Language) -> bool {
    match tag {
        "python" => lang.is_python(),
        "typescript" | "javascript" | "tsx" | "js" => lang.is_js_family(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noslop_graph::ManifestKind;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn pkg(deps: &[&str]) -> Package {
        Package {
            id: "t".into(),
            name: "t".into(),
            root: PathBuf::new(),
            language: Language::TypeScript,
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
    fn detect_dependency_matches() {
        let rule = DetectRule {
            dependency: Some("next".into()),
            ..Default::default()
        };
        assert!(rule.matches(&pkg(&["next"]), Path::new(".")));
        assert!(!rule.matches(&pkg(&[]), Path::new(".")));
    }

    #[test]
    fn detect_any_combinator() {
        let rule = DetectRule {
            any: vec![
                DetectRule {
                    dependency: Some("vitest".into()),
                    ..Default::default()
                },
                DetectRule {
                    file_exists: Some("vitest.config.ts".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let root = std::env::temp_dir();
        assert!(rule.matches(&pkg(&["vitest"]), &root));
    }

    #[test]
    fn detect_dependency_prefix_matches() {
        let rule = DetectRule {
            dependency: Some("@sanity/".into()),
            ..Default::default()
        };
        let mut deps = HashSet::new();
        deps.insert("@sanity/client".into());
        let pkg = Package {
            id: "t".into(),
            name: "t".into(),
            root: PathBuf::new(),
            language: Language::TypeScript,
            manifest_kind: ManifestKind::PackageJson,
            dependencies: deps,
            ts_base_url: None,
            ts_paths: Vec::new(),
            py_roots: Vec::new(),
            plugins: Vec::new(),
            framework_deps: HashSet::new(),
            route_decorators: Vec::new(),
        };
        assert!(rule.matches(&pkg, Path::new(".")));
    }
}
