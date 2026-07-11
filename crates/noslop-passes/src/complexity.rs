//! `high-complexity` and `large-function` — cyclomatic/cognitive complexity,
//! CRAP risk, and function length over configured thresholds. Pure over per-
//! function facts plus graph reachability for coverage estimates.

use crate::reach::Reach;
use globset::{Glob, GlobSet, GlobSetBuilder};
use noslop_graph::{
    crap_score, ComplexityConfig, ComplexityMetrics, ComplexityOverride, Confidence, FileFacts,
    FileId, Finding, Graph, RuleId, Severity,
};
use std::path::Path;

struct Thresholds {
    max_cyclomatic: u32,
    max_cognitive: u32,
    max_loc: u32,
    max_crap: f64,
    severity: Severity,
}

pub fn run(
    graph: &Graph,
    reach: &Reach,
    facts: &[FileFacts],
    cfg: &ComplexityConfig,
) -> Vec<Finding> {
    if !cfg.enabled {
        return Vec::new();
    }
    let overrides: Vec<(GlobSet, &ComplexityOverride)> = cfg
        .overrides
        .iter()
        .map(|o| (compile(&o.paths), o))
        .collect();

    let mut findings = Vec::new();
    for f in facts {
        let thresholds = thresholds_for(&f.path, cfg, &overrides);
        let coverage = estimate_coverage(f.file, graph, reach);
        for func in &f.functions {
            let name = match &func.parent {
                Some(p) => format!("{p}.{}", func.name),
                None => func.name.clone(),
            };
            let symbol = Some(format!(
                "{}::{name}@{}",
                f.path.display(),
                func.span.start_line
            ));
            let metrics = ComplexityMetrics {
                cyclomatic: func.cyclomatic,
                cognitive: func.cognitive,
                loc: func.loc,
                crap: crap_score(func.cyclomatic, coverage),
                coverage_pct: coverage,
            };

            let over_cyc = func.cyclomatic > thresholds.max_cyclomatic;
            let over_cog = func.cognitive > thresholds.max_cognitive;
            let over_crap = metrics.crap >= thresholds.max_crap;
            if over_cyc || over_cog || over_crap {
                let exceeded = exceeded_label(over_cyc, over_cog, over_crap);
                findings.push(Finding {
                    rule: RuleId::HighComplexity,
                    severity: thresholds.severity,
                    confidence: Confidence::High,
                    symbol: symbol.clone(),
                    file: f.path.clone(),
                    span: func.span,
                    message: format!(
                        "Function '{name}' exceeds complexity thresholds ({exceeded})"
                    ),
                    reason: metrics.encode(),
                });
            }

            if func.loc > thresholds.max_loc {
                findings.push(Finding {
                    rule: RuleId::LargeFunction,
                    severity: thresholds.severity,
                    confidence: Confidence::High,
                    symbol,
                    file: f.path.clone(),
                    span: func.span,
                    message: format!(
                        "Function '{name}' is {loc} lines (limit {max_loc})",
                        loc = func.loc,
                        max_loc = thresholds.max_loc
                    ),
                    reason: "function length exceeds the configured line limit".to_string(),
                });
            }
        }
    }
    findings
}

fn exceeded_label(over_cyc: bool, over_cog: bool, over_crap: bool) -> &'static str {
    match (over_cyc, over_cog, over_crap) {
        (true, true, _) => "cyclomatic, cognitive",
        (true, false, true) => "cyclomatic, CRAP",
        (false, true, true) => "cognitive, CRAP",
        (true, false, false) => "cyclomatic",
        (false, true, false) => "cognitive",
        (false, false, true) => "CRAP",
        (false, false, false) => "threshold",
    }
}

/// Fallow `static_estimated`: 85% when a test file imports directly, 40% when
/// only transitively test-reachable, else 0%.
fn estimate_coverage(file: FileId, graph: &Graph, reach: &Reach) -> f64 {
    if !reach.test_reachable[file] {
        return 0.0;
    }
    let direct = graph
        .files
        .iter()
        .filter(|f| f.is_test)
        .any(|test| graph.imports[test.id].contains(&file));
    if direct {
        85.0
    } else {
        40.0
    }
}

fn thresholds_for(
    path: &Path,
    cfg: &ComplexityConfig,
    overrides: &[(GlobSet, &ComplexityOverride)],
) -> Thresholds {
    let rel = path.to_string_lossy();
    for (set, o) in overrides {
        if set.is_match(rel.as_ref()) {
            return Thresholds {
                max_cyclomatic: o.max_cyclomatic.unwrap_or(cfg.max_cyclomatic),
                max_cognitive: o.max_cognitive.unwrap_or(cfg.max_cognitive),
                max_loc: o.max_loc.unwrap_or(cfg.max_loc),
                max_crap: o.max_crap.unwrap_or(cfg.max_crap),
                severity: o.severity.unwrap_or(Severity::Warn),
            };
        }
    }
    Thresholds {
        max_cyclomatic: cfg.max_cyclomatic,
        max_cognitive: cfg.max_cognitive,
        max_loc: cfg.max_loc,
        max_crap: cfg.max_crap,
        severity: Severity::Warn,
    }
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
