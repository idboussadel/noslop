//! Headline metrics derived from the graph and findings.

use noslop_graph::{Finding, Graph, RuleId};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub files: usize,
    pub dead_files: usize,
    /// Percentage of files that are dead, rounded to one decimal.
    pub dead_file_pct: f64,
    pub dead_exports: usize,
    pub unused_types: usize,
    pub unused_imports: usize,
    pub unused_enum_members: usize,
    pub unused_class_members: usize,
    pub unused_parameters: usize,
    pub cycles: usize,
    pub unused_dependencies: usize,
    pub only_used_in_tests: usize,
    pub high_complexity: usize,
    pub large_functions: usize,
    pub duplicate_code: usize,
    /// `null` until token totals are wired through the report.
    pub duplication_pct: Option<f64>,
    /// `null` until the health pass ships.
    pub grade: Option<String>,
}

impl Metrics {
    pub fn compute(graph: &Graph, findings: &[Finding]) -> Self {
        Self::for_files(graph.files.len(), findings)
    }

    /// Headline counts for a subset of findings (e.g. one monorepo workspace).
    pub fn for_files(files: usize, findings: &[Finding]) -> Self {
        let count = |rule: RuleId| findings.iter().filter(|f| f.rule == rule).count();
        let dead_files = count(RuleId::UnusedFile);
        let dead_file_pct = if files == 0 {
            0.0
        } else {
            round1(dead_files as f64 * 100.0 / files as f64)
        };

        Metrics {
            files,
            dead_files,
            dead_file_pct,
            dead_exports: count(RuleId::UnusedExport),
            unused_types: count(RuleId::UnusedType),
            unused_imports: count(RuleId::UnusedImport),
            unused_enum_members: count(RuleId::UnusedEnumMember),
            unused_class_members: count(RuleId::UnusedClassMember),
            unused_parameters: count(RuleId::UnusedParameter),
            cycles: count(RuleId::CircularImports),
            unused_dependencies: count(RuleId::UnusedDependency),
            only_used_in_tests: count(RuleId::OnlyUsedInTests),
            high_complexity: count(RuleId::HighComplexity),
            large_functions: count(RuleId::LargeFunction),
            duplicate_code: count(RuleId::DuplicateCode),
            duplication_pct: None,
            grade: None,
        }
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
