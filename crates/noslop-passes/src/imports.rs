//! `unused-import` — an imported name never referenced in its file. Cheap, and
//! with in-file reference collection it is effectively false-positive-free, so
//! findings are always High confidence (ARCHITECTURE.md §8).

use noslop_graph::{Confidence, FileFacts, Finding, Graph, RuleId, Severity};
use std::collections::HashSet;

pub fn run(graph: &Graph, facts: &[FileFacts]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for f in facts {
        // Only analyze files that made it into the graph (skip anything filtered).
        let Some(fid) = graph.file_id(&f.path) else {
            continue;
        };
        // A Python `__init__.py` conventionally re-exports its package surface via
        // `from .x import y` — those bindings are intentionally "unused" locally.
        if graph.file(fid).role == noslop_graph::FileRole::PackageInit {
            continue;
        }
        let refs: HashSet<&str> = f.refs.iter().map(|r| r.name.as_str()).collect();

        for import in &f.imports {
            // Re-export edges have no local binding to reference.
            if import.is_reexport {
                continue;
            }
            for name in &import.names {
                if refs.contains(name.local.as_str()) {
                    continue;
                }
                findings.push(Finding {
                    rule: RuleId::UnusedImport,
                    severity: Severity::Warn,
                    confidence: Confidence::High,
                    symbol: None,
                    file: f.path.clone(),
                    span: import.span,
                    message: format!(
                        "'{}' is imported from '{}' but never used.",
                        name.local, import.specifier
                    ),
                    reason: "imported name has no in-file references".to_string(),
                });
            }
        }
    }

    findings
}
