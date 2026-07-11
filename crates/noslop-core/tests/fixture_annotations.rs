//! Annotation-tag fixture — `@public` (spare), `@internal` (re-enable analysis in
//! an entry file), `@expected-unused` (suppress, and report when stale), and
//! `require-suppression-reason` enforcement, across TS and Python.

use noslop_core::{scan, ScanOptions};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/annotations")
}

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
        .map(|f| {
            f.symbol
                .clone()
                .unwrap_or_else(|| f.file.display().to_string())
        })
        .collect();
    out.sort();
    out
}

fn has(list: &[String], needle: &str) -> bool {
    list.iter().any(|s| s.contains(needle))
}

#[test]
fn public_symbols_are_spared_both_languages() {
    let dead = flagged("unused-export");
    assert!(!has(&dead, "publicApi"), "@public TS spared: {dead:?}");
    assert!(!has(&dead, "public_handler"), "@public py spared: {dead:?}");
    // ...but genuinely dead exports still fire.
    assert!(has(&dead, "trulyDead"), "{dead:?}");
    assert!(has(&dead, "dead_handler"), "{dead:?}");
}

#[test]
fn internal_reenables_analysis_in_entry_file() {
    // `internalHelper` lives in the entry `src/index.ts`; @internal makes it
    // eligible for unused-export despite the entry-file exemption.
    assert!(has(&flagged("unused-export"), "internalHelper"));
}

#[test]
fn expected_unused_suppresses_and_reports_stale() {
    let dead = flagged("unused-export");
    // Unused + annotated → suppressed.
    assert!(!has(&dead, "futureFlag"), "{dead:?}");
    assert!(!has(&dead, "noReasonFlag"), "{dead:?}");
    // Annotated but actually used → the inverse rule fires.
    assert!(has(&flagged("expected-unused-but-used"), "staleFlag"));
}

#[test]
fn require_suppression_reason_flags_reasonless_tags() {
    // `noReasonFlag`'s `@expected-unused` has no `-- reason`.
    let miss = flagged("missing-suppression-reason");
    assert_eq!(miss.len(), 1, "exactly one reasonless directive: {miss:?}");
}
