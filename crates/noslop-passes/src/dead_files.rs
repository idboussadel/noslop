//! `unused-file` and `only-used-in-tests` — the headline dead-code rules.

use crate::confidence::dead_confidence;
use crate::reach::Reach;
use noslop_graph::{Confidence, FileRole, Finding, Graph, RuleId, Severity, Span};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn run(graph: &Graph, reach: &Reach) -> Vec<Finding> {
    let mut findings = Vec::new();
    // Directories that contain at least one reachable file (at any depth) — the
    // signal that a Python package is live even if its `__init__.py` has no
    // inbound edge, because importing any submodule executes the init.
    let reachable_dirs = reachable_dir_set(graph, reach);

    for file in &graph.files {
        // Entry points and implicitly-used files are alive by definition.
        if file.is_entry || file.is_implicit_used {
            continue;
        }

        // Config and ambient type-declaration files are live via tooling / the
        // compiler, never through an import edge — they are not `unused-file`
        // candidates at all (see FALSE_POSITIVES_AND_FALLOW.md §1).
        match file.role {
            FileRole::Config | FileRole::TypeDecl => continue,
            FileRole::PackageInit => {
                // `pkg/__init__.py` is alive iff its package subtree is reachable.
                if reach.reachable[file.id] {
                    continue;
                }
                let dir = file.path.parent().unwrap_or(Path::new(""));
                if reachable_dirs.contains(dir) {
                    continue;
                }
                // Genuinely-dead package init: report, but never at High — package
                // reachability is a coarser signal than a direct import edge.
                findings.push(Finding {
                    rule: RuleId::UnusedFile,
                    severity: Severity::Warn,
                    confidence: Confidence::Low,
                    symbol: None,
                    file: file.path.clone(),
                    span: Span::new(1, 1),
                    message: format!("Package '{}' has no reachable modules.", dir.display()),
                    reason: "no module in this package is reachable from any entry point"
                        .to_string(),
                });
                continue;
            }
            FileRole::Source => {}
        }

        if !reach.reachable[file.id] {
            let stem = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            findings.push(Finding {
                rule: RuleId::UnusedFile,
                severity: Severity::Warn,
                confidence: dead_confidence(graph, file.id, Some(stem)),
                symbol: None,
                file: file.path.clone(),
                span: Span::new(1, 1),
                message: format!(
                    "File '{}' is not reachable from any entry point.",
                    file.path.display()
                ),
                reason: "no inbound edges from any entry point".to_string(),
            });
        } else if !reach.prod_reachable[file.id] && !file.is_test {
            // Reachable only through test entry points.
            findings.push(Finding {
                rule: RuleId::OnlyUsedInTests,
                severity: Severity::Warn,
                // Test-only reachability is a strong, well-defined signal.
                confidence: Confidence::High,
                symbol: None,
                file: file.path.clone(),
                span: Span::new(1, 1),
                message: format!(
                    "File '{}' is only reachable from test files.",
                    file.path.display()
                ),
                reason: "reachable from test entry points but not from production entry points"
                    .to_string(),
            });
        }
    }

    findings
}

/// Every ancestor directory of every reachable file. Membership answers "is any
/// module in this package reachable?" in O(1), which is how a live `__init__.py`
/// is recognised without modelling package-execution edges in the resolver.
fn reachable_dir_set(graph: &Graph, reach: &Reach) -> HashSet<PathBuf> {
    let mut dirs = HashSet::new();
    for file in &graph.files {
        if !reach.reachable[file.id] {
            continue;
        }
        // A package init doesn't count as evidence for *its own* liveness.
        if file.role == FileRole::PackageInit {
            continue;
        }
        let mut cur = file.path.parent();
        while let Some(dir) = cur {
            if !dirs.insert(dir.to_path_buf()) {
                break; // ancestors already recorded
            }
            cur = dir.parent();
        }
    }
    dirs
}
