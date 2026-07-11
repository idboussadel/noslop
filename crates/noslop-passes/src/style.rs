//! Styling liveness (Slice 1): `unused-css-token`, `broken-css-reference`,
//! `unused-css-class`. Cross-file, mirroring the dead-export model: a design
//! token or class is live iff referenced *anywhere* (CSS `var()`, a `className`
//! attribute, or a `var(--x)` inside a TS string). References can only miss, so
//! these findings never invent a use — class liveness caps at Medium because
//! dynamic class construction is common.

use noslop_graph::{Confidence, FileFacts, Finding, RuleId, Severity, StyleConfig};
use std::collections::HashSet;

pub fn run(facts: &[FileFacts], cfg: &StyleConfig) -> Vec<Finding> {
    if !cfg.enabled {
        return Vec::new();
    }

    // Global reference sets, pooled across CSS and TS/JS files.
    let mut used_tokens: HashSet<&str> = HashSet::new();
    let mut declared_tokens: HashSet<&str> = HashSet::new();
    let mut used_classes: HashSet<&str> = HashSet::new();
    // `var(--x)` embedded in a string literal (CSS-in-JS / inline styles).
    let mut string_token_refs: HashSet<String> = HashSet::new();

    for f in facts {
        for t in &f.style.declared_tokens {
            declared_tokens.insert(t.name.as_str());
        }
        for r in &f.style.var_refs {
            used_tokens.insert(r.as_str());
        }
        for r in &f.style.class_refs {
            used_classes.insert(r.as_str());
        }
        for s in &f.string_literals {
            collect_var_refs(s, &mut string_token_refs);
        }
    }
    let token_is_used = |name: &str| used_tokens.contains(name) || string_token_refs.contains(name);

    let mut findings = Vec::new();
    for f in facts {
        for t in &f.style.declared_tokens {
            if !token_is_used(&t.name) {
                findings.push(Finding {
                    rule: RuleId::UnusedCssToken,
                    severity: Severity::Warn,
                    confidence: Confidence::High,
                    symbol: None,
                    file: f.path.clone(),
                    span: t.span,
                    message: format!(
                        "Custom property '{}' is declared but never referenced.",
                        t.name
                    ),
                    reason: "no var() reference anywhere in the repo".to_string(),
                });
            }
        }
        for r in &f.style.var_refs {
            if !declared_tokens.contains(r.as_str()) {
                findings.push(Finding {
                    rule: RuleId::BrokenCssReference,
                    severity: Severity::Warn,
                    confidence: Confidence::High,
                    symbol: None,
                    file: f.path.clone(),
                    span: noslop_graph::Span::new(1, 1),
                    message: format!("var({}) references an undeclared custom property.", r),
                    reason: "no matching custom-property declaration".to_string(),
                });
            }
        }
        for c in &f.style.class_selectors {
            if !used_classes.contains(c.name.as_str()) {
                findings.push(Finding {
                    rule: RuleId::UnusedCssClass,
                    severity: Severity::Warn,
                    // Dynamic classnames are common, so never High.
                    confidence: Confidence::Medium,
                    symbol: None,
                    file: f.path.clone(),
                    span: c.span,
                    message: format!("Class selector '.{}' is never used in markup.", c.name),
                    reason: "no className/class reference to this selector".to_string(),
                });
            }
        }
    }
    findings
}

/// Pull every `--name` out of `var(--name[, fallback])` occurrences in a string.
fn collect_var_refs(s: &str, out: &mut HashSet<String>) {
    let mut rest = s;
    while let Some(pos) = rest.find("var(") {
        rest = &rest[pos + 4..];
        let end = rest.find([',', ')']).unwrap_or(rest.len());
        let name = rest[..end].trim();
        if name.starts_with("--") {
            out.insert(name.to_string());
        }
    }
}
