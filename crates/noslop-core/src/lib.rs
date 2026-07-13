//! `noslop-core` — the orchestrator. Wires the five stages together, owns the
//! parse cache and config loading, and post-processes findings (config
//! severities, ignore paths, suppressions) before handing a [`Report`] back to
//! the CLI. The pipeline is strictly one-directional; core is the only place
//! that knows about every stage.

mod cache;
mod config;

pub use config::Config;

use cache::Cache;
use noslop_graph::{FileFacts, Finding, Suppression};
use noslop_report::Report;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Options controlling a single scan.
#[derive(Default)]
pub struct ScanOptions {
    /// Repository root to scan (will be canonicalized).
    pub root: PathBuf,
    /// Whether to read/write the on-disk parse cache.
    pub use_cache: bool,
    /// Worker thread count; `None` uses rayon's default (num CPUs).
    pub threads: Option<usize>,
    /// Force duplication detection on regardless of config (the `dupes` command).
    pub force_duplication: bool,
}

/// The result of a scan: the report plus presentation metadata.
pub struct ScanOutcome {
    pub report: Report,
    /// Per-file facts from extraction — needed by `noslop fix`.
    pub facts: Vec<FileFacts>,
    /// The resolved graph — needed by the `graph` views. Free to carry: it is
    /// already built during the scan.
    pub graph: noslop_graph::Graph,
    pub elapsed_ms: u128,
    pub warm_cache: bool,
}

/// Run the full pipeline and produce a report.
pub fn scan(opts: &ScanOptions) -> anyhow::Result<ScanOutcome> {
    let start = Instant::now();
    let root = std::fs::canonicalize(&opts.root)
        .map_err(|e| anyhow::anyhow!("cannot access root '{}': {e}", opts.root.display()))?;

    if let Some(n) = opts.threads {
        // Best-effort: a global pool may already exist; ignore that error.
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global();
    }

    let mut config = Config::load(&root);
    if opts.force_duplication {
        config.enable_duplication();
    }

    // Stage 1 — discover.
    let discover_opts = config.discover_options();
    let mut workspace = noslop_discover::discover_with(&root, &discover_opts);

    // Stage 2 — extract (parallel, cached).
    let cache = if opts.use_cache {
        Cache::load(&root)
    } else {
        Cache::default()
    };
    let facts = extract_all(&workspace, &cache);
    if opts.use_cache {
        Cache::save(&root, &facts);
    }

    // Fact-derived implicit entry points (decorated routes/tasks/commands).
    noslop_discover::augment_entries_with_facts(&mut workspace, &facts);

    // Stage 3 — resolve + graph build.
    let graph = noslop_resolve::build_graph(&workspace, &facts);

    // Stage 4 — analysis passes.
    let mut raw = noslop_passes::run_all(&graph, &facts, &workspace, config.analysis());

    // Duplication re-tokenizes the repo, so it runs only when requested and its
    // tokens never touch the cached facts.
    if config.analysis().duplication.enabled {
        let inputs = token_inputs(&workspace);
        raw.extend(noslop_passes::duplication::run(
            &inputs,
            &config.analysis().duplication,
        ));
    }

    // Post-process: apply config severities, ignore paths, suppressions.
    let (findings, suppressed_count) = postprocess(raw, &facts, &config);

    // Stage 5 — assemble the report.
    let report = Report::build(
        &graph,
        &workspace,
        &facts,
        findings,
        suppressed_count,
        env!("CARGO_PKG_VERSION"),
    );

    Ok(ScanOutcome {
        report,
        facts,
        graph,
        elapsed_ms: start.elapsed().as_millis(),
        warm_cache: opts.use_cache && cache.was_loaded(),
    })
}

/// The configured `fail-on` threshold (used by the CLI's `audit`/exit codes).
pub fn fail_on(root: &Path) -> noslop_graph::Severity {
    Config::load(root).fail_on
}

fn extract_all(workspace: &noslop_graph::Workspace, cache: &Cache) -> Vec<FileFacts> {
    workspace
        .files
        .par_iter()
        .enumerate()
        .map(|(id, file)| {
            let bytes = std::fs::read(&file.abs_path).unwrap_or_default();
            let hash = noslop_extract::content_hash(&bytes);
            if let Some(cached) = cache.get(&file.rel_path, hash) {
                return cached;
            }
            let source = String::from_utf8_lossy(&bytes);
            noslop_extract::extract(file.rel_path.clone(), &source, id, file.language)
        })
        .collect()
}

/// Read and tokenize every source file for duplication detection. Parallel; only
/// called when duplication is enabled.
fn token_inputs(workspace: &noslop_graph::Workspace) -> Vec<noslop_passes::FileTokens> {
    workspace
        .files
        .par_iter()
        .map(|file| {
            let bytes = std::fs::read(&file.abs_path).unwrap_or_default();
            let source = String::from_utf8_lossy(&bytes);
            noslop_passes::FileTokens {
                path: file.rel_path.clone(),
                language: file.language,
                tokens: noslop_extract::tokenize(&source, file.language),
            }
        })
        .collect()
}

/// Apply config-driven severity, ignore-path filtering, and inline
/// suppressions. Returns the surviving findings and the suppressed count.
fn postprocess(raw: Vec<Finding>, facts: &[FileFacts], config: &Config) -> (Vec<Finding>, usize) {
    use noslop_graph::Severity;

    let suppressions = index_suppressions(facts);
    let mut suppressed_count = 0;
    let mut out = Vec::with_capacity(raw.len());

    for mut finding in raw {
        finding.severity = config.severity_for(finding.rule, finding.severity);
        if finding.severity == Severity::Off {
            continue;
        }
        if config.is_ignored(&finding.file) {
            continue;
        }
        if is_suppressed(&finding, &suppressions) {
            suppressed_count += 1;
            continue;
        }
        out.push(finding);
    }

    // Policy: every suppression / `@expected-unused` must carry a `-- reason`.
    if let Some(sev) = config.require_suppression_reason() {
        out.extend(missing_reason_findings(facts, sev));
    }

    (out, suppressed_count)
}

/// One `missing-suppression-reason` finding per reasonless suppression or
/// `@expected-unused` annotation, at the configured severity.
fn missing_reason_findings(facts: &[FileFacts], sev: noslop_graph::Severity) -> Vec<Finding> {
    use noslop_graph::{Confidence, RuleId, Span, Visibility};
    let mut out = Vec::new();
    for f in facts {
        let reasonless = f
            .suppressions
            .iter()
            .filter(|s| s.reason.is_none())
            .map(|s| (s.line, "suppression comment"))
            .chain(
                f.annotations
                    .iter()
                    .filter(|a| a.visibility == Visibility::ExpectedUnused && a.reason.is_none())
                    .map(|a| (a.line, "@expected-unused tag")),
            );
        for (line, what) in reasonless {
            out.push(Finding {
                rule: RuleId::MissingSuppressionReason,
                severity: sev,
                confidence: Confidence::High,
                symbol: None,
                file: f.path.clone(),
                span: Span::new(line.max(1), line.max(1)),
                message: format!("This {what} is missing a `-- <reason>`."),
                reason: "require-suppression-reason is enabled".to_string(),
            });
        }
    }
    out
}

fn index_suppressions(facts: &[FileFacts]) -> HashMap<&Path, &Vec<Suppression>> {
    facts
        .iter()
        .filter(|f| !f.suppressions.is_empty())
        .map(|f| (f.path.as_path(), &f.suppressions))
        .collect()
}

fn is_suppressed(finding: &Finding, index: &HashMap<&Path, &Vec<Suppression>>) -> bool {
    let Some(suppressions) = index.get(finding.file.as_path()) else {
        return false;
    };
    suppressions.iter().any(|s| {
        s.rule == finding.rule.as_str()
            && (s.file_scoped
                || (s.line >= finding.span.start_line && s.line <= finding.span.end_line))
    })
}
