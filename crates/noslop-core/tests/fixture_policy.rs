//! Policy fixture — banned import/call/effect from a pack file, plus a layered
//! boundary violation, across TS and Python.

use noslop_core::{scan, ScanOptions};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/policy")
}

fn messages(rule: &str) -> Vec<String> {
    let outcome = scan(&ScanOptions {
        root: fixture_root(),
        use_cache: false,
        threads: None,
        ..Default::default()
    })
    .expect("scan should succeed");
    outcome
        .report
        .findings
        .iter()
        .filter(|f| f.rule.as_str() == rule)
        .map(|f| f.message.clone())
        .collect()
}

#[test]
fn banned_import_call_effect_fire() {
    assert!(messages("banned-import")
        .iter()
        .any(|m| m.contains("moment")));
    // Python `subprocess.run` matched by the `subprocess.*` callee glob.
    assert!(messages("banned-call")
        .iter()
        .any(|m| m.contains("subprocess.run")));
    // `fetch` in the domain layer, scoped by the rule's `paths`.
    assert!(messages("banned-effect")
        .iter()
        .any(|m| m.contains("fetch")));
}

#[test]
fn layered_boundary_violation_fires() {
    let v = messages("boundary-violation");
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("domain") && v[0].contains("infrastructure"));
}
