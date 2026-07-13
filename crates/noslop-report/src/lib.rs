//! `noslop-report` — stage 5. The versioned JSON contract (the product for
//! agents), a pretty terminal renderer (the product for humans), and the stable
//! exit-code policy (ARCHITECTURE.md §10). Findings are sorted deterministically
//! here so identical input yields byte-identical output.

mod graphs;
mod health;
mod metrics;
mod pretty;
mod sarif;

pub use graphs::{build_import_graph, build_package_graph, ImportGraphOptions};
pub use health::{HealthComponent, HealthReport, RefactorTarget};
pub use metrics::Metrics;

use noslop_graph::{Confidence, FileFacts, Finding, Graph, RuleId, Severity, Workspace};
use serde::Serialize;

/// The output-contract schema version. A breaking change bumps this and opens a
/// deprecation window (agents and CI couple to it).
pub const SCHEMA_VERSION: u32 = 1;

/// One scan root as reported.
#[derive(Debug, Clone, Serialize)]
pub struct ScanRootReport {
    pub package: String,
    pub root: String,
    pub language: String,
    pub plugins: Vec<String>,
    pub files: usize,
    pub entry_points: usize,
}

/// The complete report — serialized verbatim for `--format json`.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub tool_version: String,
    pub repo: String,
    pub scan_roots: Vec<ScanRootReport>,
    pub metrics: Metrics,
    pub health: HealthReport,
    /// Every finding, at every confidence. Terminal rendering filters this; JSON
    /// never does (agents decide for themselves).
    pub findings: Vec<Finding>,
    pub suppressed_count: usize,
}

impl Report {
    /// Assemble a report from the graph, workspace, and post-processed findings.
    /// `findings` must already have config severities applied and suppressed
    /// entries removed; this method only sorts them into stable order.
    pub fn build(
        graph: &Graph,
        ws: &Workspace,
        facts: &[FileFacts],
        mut findings: Vec<Finding>,
        suppressed_count: usize,
        tool_version: &str,
    ) -> Self {
        sort_findings(&mut findings);
        let health = HealthReport::compute(graph, facts, &findings);
        let mut metrics = Metrics::compute(graph, &findings);
        metrics.grade = Some(health.grade.clone());
        let scan_roots = build_scan_roots(graph, ws);

        Report {
            schema_version: SCHEMA_VERSION,
            tool_version: tool_version.to_string(),
            repo: ".".to_string(),
            scan_roots,
            metrics,
            health,
            findings,
            suppressed_count,
        }
    }

    /// Serialize to pretty, deterministic JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Render the human terminal report. `show_all` reveals Medium/Low findings.
    pub fn to_pretty(&self, show_all: bool, elapsed_ms: u128, warm_cache: bool) -> String {
        pretty::render(self, show_all, elapsed_ms, warm_cache)
    }

    /// SARIF 2.1.0 for GitHub code scanning. Minimal but valid.
    pub fn to_sarif(&self) -> String {
        sarif::render(self)
    }

    /// GitHub Actions workflow annotations (`::warning file=...`).
    pub fn to_github(&self) -> String {
        let mut out = String::new();
        for f in &self.findings {
            let level = if f.severity == Severity::Error {
                "error"
            } else {
                "warning"
            };
            out.push_str(&format!(
                "::{level} file={},line={}::[{}] {}\n",
                f.file.display(),
                f.span.start_line,
                f.rule.as_str(),
                f.message
            ));
        }
        out
    }

    /// A shallow clone keeping only findings whose rule is in `rules` — the basis
    /// for the `dead`/`cycles`/`deps` filtered views.
    pub fn filtered(&self, rules: &[RuleId]) -> Report {
        let mut copy = self.clone();
        copy.findings.retain(|f| rules.contains(&f.rule));
        copy
    }

    /// Keep only findings absent from a baseline key set — the `audit` ratchet.
    /// Returns how many were filtered out as accepted legacy.
    pub fn subtract_baseline(&mut self, baseline: &std::collections::HashSet<String>) -> usize {
        let before = self.findings.len();
        self.findings
            .retain(|f| !baseline.contains(&f.stable_key()));
        before - self.findings.len()
    }

    /// The stable keys of all current findings, for `baseline update`.
    pub fn baseline_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.findings.iter().map(|f| f.stable_key()).collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// Exit code policy (ARCHITECTURE.md §10): `1` when any finding at or above
    /// `fail_on` severity is present, else `0`. Execution errors use `2`, decided
    /// by the CLI, never here — the distinction is what CI depends on.
    pub fn exit_code(&self, fail_on: Severity) -> i32 {
        let hit = self
            .findings
            .iter()
            .any(|f| f.severity >= fail_on && f.severity != Severity::Off);
        if hit {
            1
        } else {
            0
        }
    }

    /// Findings visible in the default terminal view: High confidence only.
    pub fn visible(&self, show_all: bool) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(move |f| show_all || f.confidence == Confidence::High)
    }
}

/// Stable ordering: by file, then start line, then rule name. This single rule
/// is what makes output byte-identical across runs (ARCHITECTURE.md §1).
fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.span.start_line.cmp(&b.span.start_line))
            .then(a.rule.as_str().cmp(b.rule.as_str()))
            .then(a.symbol.cmp(&b.symbol))
    });
}

fn build_scan_roots(graph: &Graph, ws: &Workspace) -> Vec<ScanRootReport> {
    let mut roots: Vec<ScanRootReport> = ws
        .packages
        .iter()
        .map(|pkg| {
            let files = graph.files.iter().filter(|f| f.package == pkg.id).count();
            let entry_points = graph
                .files
                .iter()
                .filter(|f| f.package == pkg.id && (f.is_entry || f.is_implicit_used))
                .count();
            ScanRootReport {
                package: pkg.id.clone(),
                root: if pkg.root.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    pkg.root.display().to_string()
                },
                language: language_str(pkg.language),
                plugins: pkg.plugins.clone(),
                files,
                entry_points,
            }
        })
        // Only report roots that actually contain files.
        .filter(|r| r.files > 0)
        .collect();
    roots.sort_by(|a, b| a.root.cmp(&b.root));
    roots
}

fn language_str(lang: noslop_graph::Language) -> String {
    use noslop_graph::Language::*;
    match lang {
        TypeScript => "typescript",
        Tsx => "typescript",
        JavaScript => "javascript",
        Python => "python",
        Css => "css",
    }
    .to_string()
}
