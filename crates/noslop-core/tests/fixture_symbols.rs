//! Symbol-level dead-code fixture — proves `unused-type`, `unused-enum-member`,
//! `unused-class-member`, and `unused-parameter` across TS and Python, and that
//! the *live* siblings of each dead symbol are correctly spared.

use noslop_core::{scan, ScanOptions};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/symbols")
}

/// The `name`s (last dotted segment of the symbol id) flagged by a rule.
fn flagged(rule: &str) -> Vec<String> {
    let outcome = scan(&ScanOptions {
        root: fixture_root(),
        use_cache: false,
        threads: None,
        ..Default::default()
    })
    .expect("scan should succeed");
    let mut out: Vec<String> = outcome
        .report
        .findings
        .iter()
        .filter(|f| f.rule.as_str() == rule)
        .filter_map(|f| f.symbol.clone())
        .collect();
    out.sort();
    out
}

fn contains(list: &[String], needle: &str) -> bool {
    list.iter().any(|s| s.contains(needle))
}

#[test]
fn unused_types_split_from_exports() {
    let types = flagged("unused-type");
    assert!(contains(&types, "DeadType"), "got {types:?}");
    assert!(!contains(&types, "UsedType"), "UsedType is used: {types:?}");
    // A type must never appear under unused-export (it has its own rule now).
    assert!(!contains(&flagged("unused-export"), "Type"));
}

#[test]
fn unused_enum_members_spare_the_used_one() {
    let members = flagged("unused-enum-member");
    assert!(contains(&members, "Green"), "got {members:?}");
    assert!(contains(&members, "Blue"), "got {members:?}");
    assert!(!contains(&members, "Red"), "Red is used: {members:?}");
}

#[test]
fn unused_class_members_ts_and_python() {
    let members = flagged("unused-class-member");
    // TS: private field + private method, never accessed.
    assert!(contains(&members, "deadSecret"), "got {members:?}");
    assert!(contains(&members, "neverCalled"), "got {members:?}");
    // TS: accessed private members are spared.
    assert!(!contains(&members, "liveSecret"), "got {members:?}");
    assert!(!contains(&members, "Widget.helper"), "got {members:?}");
    // Python: `_dead_helper` flagged, `_live_helper` and dunders spared.
    assert!(contains(&members, "_dead_helper"), "got {members:?}");
    assert!(!contains(&members, "_live_helper"), "got {members:?}");
    assert!(!contains(&members, "__init__"), "got {members:?}");
}

#[test]
fn unused_parameters_ts_and_python() {
    let params = flagged("unused-parameter");
    assert!(contains(&params, "deadArg"), "got {params:?}");
    assert!(contains(&params, "dead_arg"), "got {params:?}");
    // Used params are spared.
    assert!(!contains(&params, "usedArg"), "got {params:?}");
    assert!(!contains(&params, "used_arg"), "got {params:?}");
}
