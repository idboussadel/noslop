//! `unused-export` / `unused-type` — an exported symbol (a `type`/`interface`/
//! `enum` for the latter) that no live file references by name.

use crate::confidence::dead_confidence;
use crate::reach::Reach;
use noslop_graph::{
    Confidence, FileFacts, Finding, Graph, Marker, RuleId, Severity, SymbolKind, Visibility,
    Workspace,
};

pub fn run(graph: &Graph, reach: &Reach, facts: &[FileFacts], ws: &Workspace) -> Vec<Finding> {
    let mut findings = Vec::new();

    for sym in &graph.symbols {
        if !sym.exported {
            continue;
        }
        // Members are judged by the symbol-level passes, not as exports.
        if matches!(
            sym.kind,
            SymbolKind::EnumMember | SymbolKind::Field | SymbolKind::Parameter
        ) {
            continue;
        }
        // `@public` is intentional API — never flag it, in any file.
        if sym.visibility == Some(Visibility::Public) {
            continue;
        }
        let file = graph.file(sym.file);
        let used = graph.export_is_used(sym.file, &sym.name);

        // `@expected-unused` inverts the rule: suppress the unused finding, but if
        // the symbol is now referenced the annotation has gone stale — report that.
        if sym.visibility == Some(Visibility::ExpectedUnused) {
            if used {
                findings.push(Finding {
                    rule: RuleId::ExpectedUnusedButUsed,
                    severity: Severity::Warn,
                    confidence: Confidence::High,
                    symbol: Some(sym.id.clone()),
                    file: file.path.clone(),
                    span: sym.span,
                    message: format!(
                        "'{}' is marked @expected-unused but is now referenced — drop the annotation.",
                        sym.name
                    ),
                    reason: "@expected-unused symbol has inbound references".to_string(),
                });
            }
            continue;
        }

        if !reach.reachable[file.id] {
            continue; // dead file — already covered by `unused-file`
        }
        // Framework route/task/CLI handlers are live via decorators, not imports.
        if is_framework_handler(sym, graph, facts, ws) {
            continue;
        }
        // Exports of an entry point are the package's public API and normally
        // exempt — unless `@internal` explicitly opts back into analysis.
        let internal = sym.visibility == Some(Visibility::Internal);
        if (file.is_entry || file.is_implicit_used) && !internal {
            continue;
        }
        // Non-source files export by convention (`*.config.ts` default, `.d.ts`
        // ambient names, `__init__.py` re-exports). None are "unused exports".
        if !file.role.is_source() {
            continue;
        }
        if used {
            continue;
        }

        // A `type`/`interface`/`enum` gets its own rule (matching Fallow), so it
        // can be triaged and configured separately from value exports.
        let rule = if sym.kind == SymbolKind::Type {
            RuleId::UnusedType
        } else {
            RuleId::UnusedExport
        };
        let confidence = export_confidence(graph, facts, sym);
        let reason = if is_referenced_in_defining_file(facts, file.path.as_path(), &sym.name) {
            "0 inbound reference edges; file reachable; symbol is referenced inside its defining file"
        } else {
            "0 inbound reference edges; file reachable"
        };
        findings.push(Finding {
            rule,
            severity: Severity::Warn,
            confidence,
            symbol: Some(sym.id.clone()),
            file: file.path.clone(),
            span: sym.span,
            message: format!(
                "Exported {} '{}' has no references from any live file.",
                kind_word(sym.kind),
                sym.name
            ),
            reason: reason.to_string(),
        });
    }

    findings
}

fn export_confidence(
    graph: &Graph,
    facts: &[FileFacts],
    sym: &noslop_graph::SymbolNode,
) -> Confidence {
    let base = dead_confidence(graph, sym.file, Some(&sym.name));
    let file = graph.file(sym.file);
    if is_referenced_in_defining_file(facts, file.path.as_path(), &sym.name) {
        return base.min(Confidence::Medium);
    }
    base
}

fn is_referenced_in_defining_file(facts: &[FileFacts], path: &std::path::Path, name: &str) -> bool {
    let Some(facts) = facts.iter().find(|f| f.path == path) else {
        return false;
    };
    // The declaration identifier itself is collected as one ref by the extractors.
    // A second occurrence means the export is local API, not deletion evidence.
    facts.refs.iter().filter(|r| r.name == name).take(2).count() > 1
}

/// Route/task/CLI decorators register handlers at runtime — never unused exports.
fn is_framework_handler(
    sym: &noslop_graph::SymbolNode,
    graph: &Graph,
    facts: &[FileFacts],
    ws: &Workspace,
) -> bool {
    let file = graph.file(sym.file);
    let Some(pkg) = ws.package_for(&file.path) else {
        return false;
    };
    let Some(facts) = facts.iter().find(|f| f.path == file.path) else {
        return false;
    };
    facts.markers.iter().any(|m| match m {
        Marker::Decorator { dotted, on_symbol } if on_symbol == &sym.name => {
            let tail = dotted.rsplit('.').next().unwrap_or(dotted);
            pkg.route_decorators.iter().any(|d| d == tail)
        }
        _ => false,
    })
}

fn kind_word(kind: noslop_graph::SymbolKind) -> &'static str {
    use noslop_graph::SymbolKind::*;
    match kind {
        Function => "function",
        Class => "class",
        Method => "method",
        Variable => "value",
        Type => "type",
        EnumMember => "enum member",
        Field => "field",
        Parameter => "parameter",
    }
}
