//! Health scoring and ranked refactor targets.
//!
//! This is report-layer aggregation: it consumes the already post-processed
//! findings plus cached extraction facts. It must stay O(files + findings) and
//! must not re-read source files or peek at ASTs.

use noslop_graph::{ComplexityMetrics, Confidence, FileFacts, Finding, Graph, RuleId, Severity};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FORMULA_VERSION: u32 = 1;
const TARGET_LIMIT: usize = 5;

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub score: f64,
    pub grade: String,
    pub formula_version: u32,
    pub components: Vec<HealthComponent>,
    pub refactor_targets: Vec<RefactorTarget>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthComponent {
    pub name: String,
    pub score: f64,
    pub penalty: f64,
    pub findings: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefactorTarget {
    pub rank: usize,
    pub path: PathBuf,
    pub kind: String,
    pub score: f64,
    pub payoff: f64,
    pub effort: String,
    pub findings: usize,
    pub reasons: Vec<String>,
}

impl HealthReport {
    pub fn compute(graph: &Graph, facts: &[FileFacts], findings: &[Finding]) -> Self {
        let files = graph.files.len().max(1);
        let components = components(findings, files);
        let total_penalty: f64 = components.iter().map(|c| c.penalty).sum();
        let score = round1((100.0 - total_penalty).clamp(0.0, 100.0));
        let grade = grade_for(score).to_string();
        let refactor_targets = refactor_targets(facts, findings);

        HealthReport {
            score,
            grade,
            formula_version: FORMULA_VERSION,
            components,
            refactor_targets,
        }
    }
}

fn components(findings: &[Finding], files: usize) -> Vec<HealthComponent> {
    let mut groups: BTreeMap<&'static str, ComponentAccum> = BTreeMap::new();
    for finding in findings.iter().filter(|f| f.confidence == Confidence::High) {
        let component = component_for(finding.rule);
        let entry = groups.entry(component.name).or_insert(ComponentAccum {
            penalty: 0.0,
            findings: 0,
            cap: component.cap,
        });
        entry.penalty += finding_weight(finding);
        entry.findings += 1;
    }

    let mut out: Vec<HealthComponent> = groups
        .into_iter()
        .map(|(name, acc)| {
            let penalty = round1((acc.penalty * 100.0 / files as f64).min(acc.cap));
            HealthComponent {
                name: name.to_string(),
                score: round1(100.0 - penalty),
                penalty,
                findings: acc.findings,
            }
        })
        .collect();

    if out.is_empty() {
        out.push(HealthComponent {
            name: "cleanliness".to_string(),
            score: 100.0,
            penalty: 0.0,
            findings: 0,
        });
    }

    out.sort_by(|a, b| {
        b.penalty
            .total_cmp(&a.penalty)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

fn refactor_targets(facts: &[FileFacts], findings: &[Finding]) -> Vec<RefactorTarget> {
    let loc_by_path: BTreeMap<&Path, u32> = facts
        .iter()
        .map(|f| (f.path.as_path(), f.metrics.loc))
        .collect();
    let mut by_file: BTreeMap<&Path, Vec<&Finding>> = BTreeMap::new();

    for finding in findings.iter().filter(|f| f.confidence == Confidence::High) {
        if targetable(finding.rule) {
            by_file
                .entry(finding.file.as_path())
                .or_default()
                .push(finding);
        }
    }

    let mut targets: Vec<TargetCandidate> = by_file
        .into_iter()
        .filter_map(|(path, group)| {
            let payoff: f64 = group.iter().map(|f| finding_weight(f)).sum();
            if payoff <= 0.0 {
                return None;
            }
            let loc = loc_by_path.get(path).copied().unwrap_or(0);
            let effort = effort_for(loc);
            let effort_weight = effort_weight(loc);
            let score = round1(payoff / effort_weight);
            Some(TargetCandidate {
                path: path.to_path_buf(),
                kind: kind_for(&group).to_string(),
                score,
                payoff: round1(payoff),
                effort: effort.to_string(),
                findings: group.len(),
                reasons: reasons_for(&group),
            })
        })
        .collect();

    targets.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.payoff.total_cmp(&a.payoff))
            .then_with(|| a.path.cmp(&b.path))
    });

    targets
        .into_iter()
        .take(TARGET_LIMIT)
        .enumerate()
        .map(|(i, t)| RefactorTarget {
            rank: i + 1,
            path: t.path,
            kind: t.kind,
            score: t.score,
            payoff: t.payoff,
            effort: t.effort,
            findings: t.findings,
            reasons: t.reasons,
        })
        .collect()
}

#[derive(Clone, Copy)]
struct Component {
    name: &'static str,
    cap: f64,
}

struct ComponentAccum {
    penalty: f64,
    findings: usize,
    cap: f64,
}

struct TargetCandidate {
    path: PathBuf,
    kind: String,
    score: f64,
    payoff: f64,
    effort: String,
    findings: usize,
    reasons: Vec<String>,
}

fn component_for(rule: RuleId) -> Component {
    let (name, cap) = match rule {
        RuleId::UnusedExport | RuleId::UnusedType => ("api-surface", 12.0),
        RuleId::UnusedFile
        | RuleId::UnusedImport
        | RuleId::UnusedEnumMember
        | RuleId::UnusedClassMember
        | RuleId::UnusedParameter
        | RuleId::UnusedDependency
        | RuleId::OnlyUsedInTests => ("dead-code", 45.0),
        RuleId::ExpectedUnusedButUsed | RuleId::MissingSuppressionReason => ("maintenance", 10.0),
        RuleId::CircularImports => ("cycles", 25.0),
        RuleId::HighComplexity | RuleId::LargeFunction => ("complexity", 25.0),
        RuleId::DuplicateCode => ("duplication", 20.0),
        RuleId::BannedImport
        | RuleId::BannedCall
        | RuleId::BannedEffect
        | RuleId::BoundaryViolation => ("architecture", 25.0),
        RuleId::UnusedCssToken | RuleId::BrokenCssReference | RuleId::UnusedCssClass => {
            ("styling", 15.0)
        }
    };
    Component { name, cap }
}

fn finding_weight(finding: &Finding) -> f64 {
    let base = match finding.rule {
        RuleId::UnusedFile => 8.0,
        RuleId::UnusedExport => 1.0,
        RuleId::UnusedType => 0.75,
        RuleId::UnusedImport => 1.0,
        RuleId::UnusedDependency => 4.0,
        RuleId::UnusedEnumMember | RuleId::UnusedClassMember => 1.5,
        RuleId::UnusedParameter => 1.0,
        RuleId::OnlyUsedInTests => 3.0,
        RuleId::ExpectedUnusedButUsed | RuleId::MissingSuppressionReason => 1.0,
        RuleId::HighComplexity => {
            let base = ComplexityMetrics::decode(&finding.reason)
                .map(|m| 4.0 + m.crap / 30.0)
                .unwrap_or(5.0);
            base
        }
        RuleId::LargeFunction => {
            let loc = function_span_loc(finding);
            4.0 + (loc as f64 / 60.0)
        }
        RuleId::CircularImports => cycle_weight(&finding.message),
        RuleId::DuplicateCode => 5.0,
        RuleId::BannedImport | RuleId::BannedCall | RuleId::BannedEffect => 5.0,
        RuleId::BoundaryViolation => 6.0,
        RuleId::UnusedCssToken | RuleId::UnusedCssClass => 2.0,
        RuleId::BrokenCssReference => 3.0,
    };
    base * severity_weight(finding.severity)
}

fn severity_weight(severity: Severity) -> f64 {
    match severity {
        Severity::Off => 0.0,
        Severity::Warn => 1.0,
        Severity::Error => 1.5,
    }
}

fn cycle_weight(message: &str) -> f64 {
    let files = message
        .strip_prefix("Circular import group (")
        .and_then(|rest| rest.split_once(" files)"))
        .and_then(|(count, _)| count.parse::<f64>().ok())
        .unwrap_or(2.0);
    6.0 + files
}

fn function_span_loc(finding: &Finding) -> u32 {
    finding
        .span
        .end_line
        .saturating_sub(finding.span.start_line)
        + 1
}

fn targetable(rule: RuleId) -> bool {
    !matches!(
        rule,
        RuleId::MissingSuppressionReason | RuleId::ExpectedUnusedButUsed
    )
}

fn kind_for(group: &[&Finding]) -> &'static str {
    let primary = group
        .iter()
        .max_by(|a, b| finding_weight(a).total_cmp(&finding_weight(b)))
        .map(|f| f.rule)
        .unwrap_or(RuleId::UnusedFile);
    match primary {
        RuleId::UnusedFile => "dead file",
        RuleId::UnusedExport | RuleId::UnusedType => "API surface review",
        RuleId::CircularImports => "cycle",
        RuleId::HighComplexity => "complexity hotspot",
        RuleId::LargeFunction => "large function refactor",
        RuleId::DuplicateCode => "duplicate block",
        RuleId::BannedImport
        | RuleId::BannedCall
        | RuleId::BannedEffect
        | RuleId::BoundaryViolation => "architecture violation",
        RuleId::UnusedCssToken | RuleId::BrokenCssReference | RuleId::UnusedCssClass => {
            "style cleanup"
        }
        _ => "dead-code cleanup",
    }
}

fn reasons_for(group: &[&Finding]) -> Vec<String> {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for finding in group {
        *counts.entry(rule_title(finding.rule)).or_default() += 1;
    }
    let mut reasons: Vec<(&'static str, usize)> = counts.into_iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    reasons
        .into_iter()
        .take(3)
        .map(|(rule, count)| {
            if count == 1 {
                rule.to_string()
            } else {
                format!("{count} {rule}")
            }
        })
        .collect()
}

fn effort_for(loc: u32) -> &'static str {
    match loc {
        0..=120 => "small",
        121..=400 => "medium",
        _ => "large",
    }
}

fn effort_weight(loc: u32) -> f64 {
    1.0 + (loc as f64).sqrt() / 8.0
}

fn grade_for(score: f64) -> &'static str {
    match score {
        s if s >= 90.0 => "A",
        s if s >= 80.0 => "B",
        s if s >= 70.0 => "C",
        s if s >= 60.0 => "D",
        _ => "F",
    }
}

fn rule_title(rule: RuleId) -> &'static str {
    match rule {
        RuleId::UnusedFile => "unused file",
        RuleId::UnusedExport => "unused export",
        RuleId::UnusedType => "unused type",
        RuleId::UnusedImport => "unused import",
        RuleId::UnusedDependency => "unused dependency",
        RuleId::UnusedEnumMember => "unused enum member",
        RuleId::UnusedClassMember => "unused class member",
        RuleId::UnusedParameter => "unused parameter",
        RuleId::ExpectedUnusedButUsed => "stale @expected-unused",
        RuleId::MissingSuppressionReason => "missing suppression reason",
        RuleId::HighComplexity => "high complexity",
        RuleId::LargeFunction => "large function",
        RuleId::BannedImport => "banned import",
        RuleId::BannedCall => "banned call",
        RuleId::BannedEffect => "banned effect",
        RuleId::BoundaryViolation => "boundary violation",
        RuleId::DuplicateCode => "duplicate code",
        RuleId::UnusedCssToken => "unused CSS token",
        RuleId::BrokenCssReference => "broken CSS reference",
        RuleId::UnusedCssClass => "unused CSS class",
        RuleId::CircularImports => "circular import",
        RuleId::OnlyUsedInTests => "only used in tests",
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use noslop_graph::{Language, Span};

    fn finding(rule: RuleId, path: &str) -> Finding {
        Finding {
            rule,
            severity: Severity::Warn,
            confidence: Confidence::High,
            symbol: None,
            file: PathBuf::from(path),
            span: Span::new(1, 1),
            message: "test finding".to_string(),
            reason: "test".to_string(),
        }
    }

    fn facts(path: &str, loc: u32) -> FileFacts {
        FileFacts {
            file: 0,
            path: PathBuf::from(path),
            language: Language::TypeScript,
            content_hash: 0,
            symbols: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
            refs: Vec::new(),
            member_accesses: Vec::new(),
            markers: Vec::new(),
            suppressions: Vec::new(),
            annotations: Vec::new(),
            string_literals: Vec::new(),
            metrics: noslop_graph::FileMetrics { loc },
            functions: Vec::new(),
            calls: Vec::new(),
            style: noslop_graph::StyleFacts::default(),
        }
    }

    #[test]
    fn grade_uses_high_confidence_findings_only() {
        let mut low = finding(RuleId::UnusedFile, "src/noisy.ts");
        low.confidence = Confidence::Low;
        let report = HealthReport::compute(&Graph::default(), &[], &[low]);

        assert_eq!(report.score, 100.0);
        assert_eq!(report.grade, "A");
    }

    #[test]
    fn targets_rank_by_payoff_over_effort_then_path() {
        let findings = vec![
            finding(RuleId::UnusedExport, "src/large.ts"),
            finding(RuleId::UnusedExport, "src/small.ts"),
            finding(RuleId::UnusedExport, "src/small.ts"),
        ];
        let facts = vec![facts("src/large.ts", 900), facts("src/small.ts", 20)];
        let report = HealthReport::compute(&Graph::default(), &facts, &findings);

        assert_eq!(
            report.refactor_targets[0].path,
            PathBuf::from("src/small.ts")
        );
        assert_eq!(report.refactor_targets[0].rank, 1);
    }

    #[test]
    fn unused_exports_are_api_surface_health_not_dead_code() {
        let findings = vec![
            finding(RuleId::UnusedExport, "src/api.ts"),
            finding(RuleId::UnusedFile, "src/orphan.ts"),
        ];
        let report = HealthReport::compute(&Graph::default(), &[], &findings);
        let names: Vec<&str> = report.components.iter().map(|c| c.name.as_str()).collect();

        assert!(names.contains(&"api-surface"));
        assert!(names.contains(&"dead-code"));
    }
}
