//! Symbol-level dead code: `unused-enum-member`, `unused-class-member`,
//! `unused-parameter` (matching Fallow's symbol-level surface; `unused-type` is
//! emitted from the exports pass).
//!
//! Enum members and private class members are judged against the repo-wide
//! member-access index ([`Graph::accessed_members`]): a member declaration never
//! contributes to that index, so the check is self-reference-free and can only
//! miss a use (suppressing a finding), never invent one. Parameters are decided
//! locally during extraction — a `Parameter` symbol is only emitted when it is
//! already known to be unused — so here they map straight to a finding.

use crate::confidence::dead_confidence;
use crate::reach::Reach;
use noslop_graph::{Confidence, Finding, Graph, RuleId, Severity, SymbolKind};

pub fn run(graph: &Graph, reach: &Reach) -> Vec<Finding> {
    let mut findings = Vec::new();

    for sym in &graph.symbols {
        let file = graph.file(sym.file);
        // Only judge live source files: dead files are covered by `unused-file`,
        // and configs/decls/inits are not our concern. Tests routinely carry
        // intentionally-unused params and helpers, so skip them.
        if !file.role.is_source() || file.is_test || !reach.reachable[file.id] {
            continue;
        }
        // An `@public` symbol is intentional API even at member/param level.
        if sym.visibility == Some(noslop_graph::Visibility::Public) {
            continue;
        }

        let (rule, kind_word) = match sym.kind {
            SymbolKind::EnumMember if !graph.member_is_accessed(&sym.name) => {
                (RuleId::UnusedEnumMember, "Enum member")
            }
            SymbolKind::Method | SymbolKind::Field if !graph.member_is_accessed(&sym.name) => {
                (RuleId::UnusedClassMember, "Private member")
            }
            SymbolKind::Parameter => (RuleId::UnusedParameter, "Parameter"),
            _ => continue,
        };

        let qualified = match &sym.parent {
            Some(parent) => format!("{parent}.{}", sym.name),
            None => sym.name.clone(),
        };
        let (message, reason, confidence) = match rule {
            RuleId::UnusedParameter => (
                format!("{kind_word} '{qualified}' is never used in its body."),
                "no reference to the parameter in the function body".to_string(),
                // Local and syntactic — as safe as `unused-import`.
                Confidence::High,
            ),
            _ => (
                format!("{kind_word} '{qualified}' is never accessed."),
                "no member access to this name anywhere in the repo".to_string(),
                dead_confidence(graph, sym.file, Some(&sym.name)),
            ),
        };

        findings.push(Finding {
            rule,
            severity: Severity::Warn,
            confidence,
            symbol: Some(sym.id.clone()),
            file: file.path.clone(),
            span: sym.span,
            message,
            reason,
        });
    }

    findings
}
