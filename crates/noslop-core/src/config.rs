//! `noslop.toml` loading. Zero-config is the default: a missing or partial file
//! yields sane defaults (ARCHITECTURE.md §3/§9). Config only *refines*.

use globset::{Glob, GlobSet, GlobSetBuilder};
use noslop_graph::{
    AnalysisConfig, ComplexityConfig, ComplexityOverride, DuplicationConfig, DuplicationMode,
    Layer, PolicyConfig, PolicyKind, PolicyRule, RuleId, Severity,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Effective configuration for a scan.
pub struct Config {
    /// Per-rule severity overrides (rule name → severity).
    rule_severity: HashMap<String, Severity>,
    /// Compiled ignore-path globs; matching findings are dropped.
    ignore: GlobSet,
    /// The severity threshold `audit` exits non-zero on.
    pub fail_on: Severity,
    /// Pass-facing config for the optional analyses (complexity, policy, …).
    analysis: AnalysisConfig,
    /// Extra entry-point globs merged into plugin detection.
    pub entry_points: Vec<String>,
    /// Extra plugin files or directories to load.
    pub plugin_paths: Vec<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            rule_severity: HashMap::new(),
            ignore: GlobSet::empty(),
            fail_on: Severity::Error,
            analysis: AnalysisConfig::default(),
            entry_points: Vec::new(),
            plugin_paths: Vec::new(),
        }
    }
}

impl Config {
    /// Load `noslop.toml` from the repo root if present; otherwise defaults.
    pub fn load(root: &Path) -> Self {
        let path = root.join("noslop.toml");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Config::default();
        };
        Config::parse(&text, root)
    }

    fn parse(text: &str, root: &Path) -> Self {
        let mut config = Config::default();
        let Ok(value) = text.parse::<toml::Value>() else {
            return config;
        };

        if let Some(rules) = value.get("rules").and_then(|r| r.as_table()) {
            for (rule, setting) in rules {
                // A rule is either `"warn"` or `{ level = "warn", ... }`.
                let level = setting
                    .as_str()
                    .or_else(|| setting.get("level").and_then(|l| l.as_str()));
                if let Some(sev) = level.and_then(Severity::from_name) {
                    config.rule_severity.insert(rule.clone(), sev);
                }
            }
        }

        if let Some(paths) = value
            .get("ignore")
            .and_then(|i| i.get("paths"))
            .and_then(|p| p.as_array())
        {
            let mut builder = GlobSetBuilder::new();
            for pat in paths.iter().filter_map(|p| p.as_str()) {
                if let Ok(glob) = Glob::new(pat) {
                    builder.add(glob);
                }
            }
            config.ignore = builder.build().unwrap_or_else(|_| GlobSet::empty());
        }

        if let Some(fail_on) = value
            .get("audit")
            .and_then(|a| a.get("fail-on"))
            .and_then(|f| f.as_array())
        {
            // `fail-on = ["error"]` — take the lowest listed severity as threshold.
            config.fail_on = fail_on
                .iter()
                .filter_map(|s| s.as_str())
                .filter_map(Severity::from_name)
                .min()
                .unwrap_or(Severity::Error);
        }

        if let Some(cx) = value.get("complexity").and_then(|c| c.as_table()) {
            config.analysis.complexity = parse_complexity(cx);
        }

        if let Some(dup) = value.get("duplication").and_then(|d| d.as_table()) {
            config.analysis.duplication = parse_duplication(dup);
        }

        // `[style]` presence enables styling analysis (may be an empty table).
        if let Some(style) = value.get("style") {
            config.analysis.style.enabled = style
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
        }

        config.analysis.policy = parse_policy(root, &value);

        if let Some(ep) = value.get("entry_points") {
            if let Some(add) = ep.get("add").and_then(|a| a.as_array()) {
                config.entry_points = add
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
            }
        }
        if let Some(plugins) = value.get("plugins") {
            if let Some(paths) = plugins.get("paths").and_then(|p| p.as_array()) {
                config.plugin_paths = paths
                    .iter()
                    .filter_map(|v| v.as_str().map(PathBuf::from))
                    .collect();
            }
        }

        config
    }

    /// Force-enable duplication (the `dupes` subcommand), overriding config.
    pub fn enable_duplication(&mut self) {
        self.analysis.duplication.enabled = true;
    }

    /// Pass-facing config for the optional analyses.
    pub fn analysis(&self) -> &AnalysisConfig {
        &self.analysis
    }

    /// Discovery refinements for the plugin engine.
    pub fn discover_options(&self) -> noslop_discover::DiscoverOptions {
        noslop_discover::DiscoverOptions {
            entry_points: self.entry_points.clone(),
            plugin_paths: self.plugin_paths.clone(),
        }
    }

    /// The configured severity for a rule, or the pass-provided default.
    pub fn severity_for(&self, rule: RuleId, default: Severity) -> Severity {
        self.rule_severity
            .get(rule.as_str())
            .copied()
            .unwrap_or(default)
    }

    /// Is this repo-relative path excluded by an ignore glob?
    pub fn is_ignored(&self, path: &Path) -> bool {
        self.ignore.is_match(path)
    }

    /// The severity at which to demand a `-- reason` on every suppression and
    /// `@expected-unused` tag, if `[rules].require-suppression-reason` is set.
    pub fn require_suppression_reason(&self) -> Option<Severity> {
        self.rule_severity
            .get("require-suppression-reason")
            .copied()
            .filter(|s| *s != Severity::Off)
    }
}

/// Parse the `[complexity]` table. Presence enables the pass; thresholds fall
/// back to the fallow-parity defaults. Each `[[complexity.override]]` requires a
/// `reason` (an override without one is dropped, like a reasonless suppression).
fn parse_complexity(table: &toml::value::Table) -> ComplexityConfig {
    let mut cfg = ComplexityConfig {
        enabled: true,
        ..ComplexityConfig::default()
    };
    if let Some(v) = table.get("enabled").and_then(|v| v.as_bool()) {
        cfg.enabled = v;
    }
    if let Some(v) = table.get("max-cyclomatic").and_then(|v| v.as_integer()) {
        cfg.max_cyclomatic = v.max(1) as u32;
    }
    if let Some(v) = table.get("max-cognitive").and_then(|v| v.as_integer()) {
        cfg.max_cognitive = v.max(1) as u32;
    }
    if let Some(v) = table
        .get("max-loc")
        .or_else(|| table.get("max-unit-size"))
        .and_then(|v| v.as_integer())
    {
        cfg.max_loc = v.max(1) as u32;
    }
    if let Some(v) = table.get("max-crap").and_then(parse_toml_f64) {
        cfg.max_crap = v.max(1.0);
    }
    if let Some(overrides) = table.get("override").and_then(|o| o.as_array()) {
        for o in overrides {
            let Some(reason) = o.get("reason").and_then(|r| r.as_str()) else {
                continue; // an override must document why
            };
            let paths = o
                .get("paths")
                .and_then(|p| p.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            cfg.overrides.push(ComplexityOverride {
                paths,
                max_cyclomatic: o
                    .get("max-cyclomatic")
                    .and_then(|v| v.as_integer())
                    .map(|v| v as u32),
                max_cognitive: o
                    .get("max-cognitive")
                    .and_then(|v| v.as_integer())
                    .map(|v| v as u32),
                max_loc: o
                    .get("max-loc")
                    .or_else(|| o.get("max-unit-size"))
                    .and_then(|v| v.as_integer())
                    .map(|v| v as u32),
                max_crap: o.get("max-crap").and_then(parse_toml_f64),
                severity: o
                    .get("severity")
                    .and_then(|s| s.as_str())
                    .and_then(Severity::from_name),
                reason: reason.to_string(),
            });
        }
    }
    cfg
}

/// Parse `[duplication]`. Presence enables the pass; fields fall back to the
/// fallow-parity defaults (mode `mild`, min-tokens 30).
fn parse_duplication(table: &toml::value::Table) -> DuplicationConfig {
    let mut cfg = DuplicationConfig {
        enabled: true,
        ..DuplicationConfig::default()
    };
    if let Some(m) = table
        .get("mode")
        .and_then(|v| v.as_str())
        .and_then(DuplicationMode::from_name)
    {
        cfg.mode = m;
    }
    if let Some(v) = table.get("min-tokens").and_then(|v| v.as_integer()) {
        cfg.min_tokens = v.max(1) as u32;
    }
    if let Some(v) = table.get("skip-local").and_then(|v| v.as_bool()) {
        cfg.skip_local = v;
    }
    cfg
}

/// Assemble the policy config from inline `[[policy.rule]]` entries, referenced
/// pack files (`[policy].packs`), and the `[boundaries]` layer model.
fn parse_policy(root: &Path, value: &toml::Value) -> PolicyConfig {
    let mut policy = PolicyConfig::default();

    if let Some(pol) = value.get("policy") {
        // Inline rules.
        if let Some(rules) = pol.get("rule").and_then(|r| r.as_array()) {
            policy.rules.extend(rules.iter().filter_map(parse_rule));
        }
        // Referenced pack files (TOML, with their own `[[rule]]` array).
        if let Some(packs) = pol.get("packs").and_then(|p| p.as_array()) {
            for pack in packs.iter().filter_map(|p| p.as_str()) {
                if let Ok(text) = std::fs::read_to_string(root.join(pack)) {
                    if let Ok(pv) = text.parse::<toml::Value>() {
                        if let Some(rules) = pv.get("rule").and_then(|r| r.as_array()) {
                            policy.rules.extend(rules.iter().filter_map(parse_rule));
                        }
                    }
                }
            }
        }
    }

    if let Some(b) = value.get("boundaries") {
        if let Some(preset) = b.get("preset").and_then(|p| p.as_str()) {
            policy.layers.extend(preset_layers(preset));
        }
        if let Some(layers) = b.get("layer").and_then(|l| l.as_array()) {
            policy.layers.extend(layers.iter().filter_map(parse_layer));
        }
    }

    policy
}

fn parse_rule(value: &toml::Value) -> Option<PolicyRule> {
    let id = value.get("id").and_then(|v| v.as_str())?.to_string();
    let kind = match value.get("kind").and_then(|v| v.as_str())? {
        "banned-import" => PolicyKind::BannedImport,
        "banned-call" => PolicyKind::BannedCall,
        "banned-effect" => PolicyKind::BannedEffect,
        _ => return None,
    };
    // The pattern key differs by kind, matching the JSON/TOML shape teams expect.
    let key = match kind {
        PolicyKind::BannedImport => "specifiers",
        PolicyKind::BannedCall => "callees",
        PolicyKind::BannedEffect => "effects",
    };
    let patterns = str_array(value.get(key));
    Some(PolicyRule {
        id,
        kind,
        patterns,
        paths: str_array(value.get("paths")),
        severity: value
            .get("severity")
            .and_then(|s| s.as_str())
            .and_then(Severity::from_name)
            .unwrap_or(Severity::Warn),
        hint: value.get("hint").and_then(|h| h.as_str()).map(String::from),
    })
}

fn parse_layer(value: &toml::Value) -> Option<Layer> {
    Some(Layer {
        name: value.get("name").and_then(|v| v.as_str())?.to_string(),
        paths: str_array(value.get("paths")),
        allow: str_array(value.get("allow")),
    })
}

fn str_array(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Built-in layer presets (directory-name conventions; a non-matching layout
/// simply assigns no layers and finds nothing — zero-config-safe).
fn preset_layers(name: &str) -> Vec<Layer> {
    let layer = |name: &str, paths: &[&str], allow: &[&str]| Layer {
        name: name.to_string(),
        paths: paths.iter().map(|s| s.to_string()).collect(),
        allow: allow.iter().map(|s| s.to_string()).collect(),
    };
    match name {
        "layered" => vec![
            layer("domain", &["**/domain/**"], &[]),
            layer(
                "application",
                &["**/application/**", "**/app/**"],
                &["domain"],
            ),
            layer(
                "infrastructure",
                &["**/infrastructure/**", "**/infra/**"],
                &["domain", "application"],
            ),
            layer(
                "ui",
                &["**/ui/**", "**/presentation/**", "**/web/**"],
                &["application", "domain"],
            ),
        ],
        "hexagonal" => vec![
            layer("core", &["**/core/**", "**/domain/**"], &[]),
            layer("ports", &["**/ports/**"], &["core"]),
            layer(
                "adapters",
                &["**/adapters/**", "**/infrastructure/**"],
                &["ports", "core"],
            ),
        ],
        "feature-sliced" => {
            // Each slice may import only slices to its right.
            let order = [
                "shared",
                "entities",
                "features",
                "widgets",
                "pages",
                "processes",
                "app",
            ];
            order
                .iter()
                .enumerate()
                .map(|(i, name)| Layer {
                    name: name.to_string(),
                    paths: vec![format!("**/{name}/**")],
                    allow: order[..i].iter().map(|s| s.to_string()).collect(),
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn parse_toml_f64(value: &toml::Value) -> Option<f64> {
    value
        .as_float()
        .or_else(|| value.as_integer().map(|i| i as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn complexity_enabled_without_noslop_toml() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mixed");
        let cfg = Config::load(&root);
        assert!(
            cfg.analysis().complexity.enabled,
            "zero-config scans should report complexity + large functions"
        );
    }

    #[test]
    fn complexity_can_be_disabled_in_toml() {
        let text = "[complexity]\nenabled = false\n";
        let cfg = Config::parse(text, Path::new("."));
        assert!(!cfg.analysis().complexity.enabled);
    }
}
