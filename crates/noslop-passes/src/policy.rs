//! Declarative policy — `banned-import`, `banned-call`, `banned-effect`, and
//! `boundary-violation`. Pure over `(graph, facts, policy)`. Every rule maps onto
//! a fixed [`RuleId`]; the pack's own rule id travels in the finding `reason`.
//!
//! All four are structural (an import edge / call site either exists or it does
//! not), so findings are High confidence — which is exactly why policy is the
//! safest place to let a project fail CI on `error`.

use globset::{Glob, GlobSet, GlobSetBuilder};
use noslop_graph::{
    Confidence, FileFacts, Finding, Graph, Layer, PolicyConfig, PolicyKind, PolicyRule, RuleId,
    Severity, Span,
};
use std::path::Path;

/// Effect name → the import specifiers / call callees that constitute it. Kept
/// as data next to the pass (like the plugin registry) so `banned-effect` is just
/// sugar over the banned-import/call matcher.
static EFFECTS: &[(&str, &[&str])] = &[
    (
        "network",
        &[
            "fetch",
            "axios",
            "axios.*",
            "http",
            "https",
            "http.request",
            "https.request",
            "node:http",
            "node:https",
            "requests",
            "requests.*",
            "urllib",
            "urllib.*",
            "httpx",
            "httpx.*",
            "aiohttp",
            "aiohttp.*",
        ],
    ),
    (
        "process",
        &[
            "child_process",
            "child_process.*",
            "node:child_process",
            "subprocess",
            "subprocess.*",
            "os.system",
            "os.popen",
        ],
    ),
    (
        "fs",
        &[
            "fs",
            "fs.*",
            "node:fs",
            "shutil",
            "shutil.*",
            "os.remove",
            "os.rmdir",
        ],
    ),
];

pub fn run(graph: &Graph, facts: &[FileFacts], policy: &PolicyConfig) -> Vec<Finding> {
    if policy.is_empty() {
        return Vec::new();
    }
    let mut findings = Vec::new();
    let compiled: Vec<CompiledRule> = policy.rules.iter().map(CompiledRule::new).collect();
    let layers = CompiledLayers::new(&policy.layers);

    for f in facts {
        // A file's imports and calls are matched against every in-scope rule.
        for rule in &compiled {
            if !rule.applies_to(&f.path) {
                continue;
            }
            match rule.rule.kind {
                PolicyKind::BannedImport => {
                    for imp in &f.imports {
                        if rule.matches(&imp.specifier) {
                            findings.push(rule.finding(
                                RuleId::BannedImport,
                                &f.path,
                                imp.span,
                                &format!("import of '{}'", imp.specifier),
                            ));
                        }
                    }
                }
                PolicyKind::BannedCall => {
                    for call in &f.calls {
                        if rule.matches(&call.callee) {
                            findings.push(rule.finding(
                                RuleId::BannedCall,
                                &f.path,
                                call.span,
                                &format!("call to '{}'", call.callee),
                            ));
                        }
                    }
                }
                PolicyKind::BannedEffect => {
                    for imp in &f.imports {
                        if rule.matches(&imp.specifier) {
                            findings.push(rule.finding(
                                RuleId::BannedEffect,
                                &f.path,
                                imp.span,
                                &format!("import of '{}'", imp.specifier),
                            ));
                        }
                    }
                    for call in &f.calls {
                        if rule.matches(&call.callee) {
                            findings.push(rule.finding(
                                RuleId::BannedEffect,
                                &f.path,
                                call.span,
                                &format!("call to '{}'", call.callee),
                            ));
                        }
                    }
                }
            }
        }
    }

    findings.extend(boundary_findings(graph, &layers));
    findings
}

/// A rule with its pattern/path globs precompiled.
struct CompiledRule<'a> {
    rule: &'a PolicyRule,
    patterns: GlobSet,
    paths: Option<GlobSet>,
}

impl<'a> CompiledRule<'a> {
    fn new(rule: &'a PolicyRule) -> Self {
        // banned-effect stores effect *names*; expand them to concrete patterns.
        let patterns: Vec<String> = if rule.kind == PolicyKind::BannedEffect {
            rule.patterns
                .iter()
                .flat_map(|name| effect_patterns(name))
                .map(|s| s.to_string())
                .collect()
        } else {
            rule.patterns.clone()
        };
        CompiledRule {
            rule,
            patterns: compile(&patterns),
            paths: if rule.paths.is_empty() {
                None
            } else {
                Some(compile(&rule.paths))
            },
        }
    }

    fn applies_to(&self, path: &Path) -> bool {
        match &self.paths {
            Some(set) => set.is_match(path.to_string_lossy().as_ref()),
            None => true,
        }
    }

    fn matches(&self, subject: &str) -> bool {
        self.patterns.is_match(subject)
    }

    fn finding(&self, rule_id: RuleId, path: &Path, span: Span, what: &str) -> Finding {
        let hint = self
            .rule
            .hint
            .as_ref()
            .map(|h| format!(" — {h}"))
            .unwrap_or_default();
        Finding {
            rule: rule_id,
            severity: self.rule.severity,
            confidence: Confidence::High,
            symbol: None,
            file: path.to_path_buf(),
            span,
            message: format!("Policy '{}' forbids this {what}{hint}.", self.rule.id),
            reason: format!("matched policy rule '{}'", self.rule.id),
        }
    }
}

fn effect_patterns(name: &str) -> &'static [&'static str] {
    EFFECTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, p)| *p)
        .unwrap_or(&[])
}

/// Layers with compiled path globs, in declaration order (first match wins).
struct CompiledLayers<'a> {
    layers: Vec<(GlobSet, &'a Layer)>,
}

impl<'a> CompiledLayers<'a> {
    fn new(layers: &'a [Layer]) -> Self {
        CompiledLayers {
            layers: layers.iter().map(|l| (compile(&l.paths), l)).collect(),
        }
    }

    fn layer_of(&self, path: &Path) -> Option<&Layer> {
        let s = path.to_string_lossy();
        self.layers
            .iter()
            .find(|(set, _)| set.is_match(s.as_ref()))
            .map(|(_, l)| *l)
    }
}

/// A file may import its own layer plus the layers in its `allow` list; any other
/// cross-layer import is a violation.
fn boundary_findings(graph: &Graph, layers: &CompiledLayers) -> Vec<Finding> {
    if layers.layers.is_empty() {
        return Vec::new();
    }
    let mut findings = Vec::new();
    for from in &graph.files {
        let Some(from_layer) = layers.layer_of(&from.path) else {
            continue;
        };
        for &to_id in &graph.imports[from.id] {
            let to = graph.file(to_id);
            let Some(to_layer) = layers.layer_of(&to.path) else {
                continue;
            };
            if to_layer.name == from_layer.name || from_layer.allow.contains(&to_layer.name) {
                continue;
            }
            findings.push(Finding {
                rule: RuleId::BoundaryViolation,
                severity: Severity::Error,
                confidence: Confidence::High,
                symbol: None,
                file: from.path.clone(),
                span: Span::new(1, 1),
                message: format!(
                    "Layer '{}' must not import layer '{}' ({} → {}).",
                    from_layer.name,
                    to_layer.name,
                    from.path.display(),
                    to.path.display()
                ),
                reason: format!(
                    "'{}' is not in the allow-list of layer '{}'",
                    to_layer.name, from_layer.name
                ),
            });
        }
    }
    findings
}

fn compile(patterns: &[String]) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        if let Ok(g) = Glob::new(p) {
            b.add(g);
        }
    }
    b.build().unwrap_or_else(|_| GlobSet::empty())
}
