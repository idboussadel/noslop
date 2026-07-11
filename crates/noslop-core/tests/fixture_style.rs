//! Styling-liveness fixture — unused design tokens, broken `var()` references,
//! and unused class selectors, with `var()`/`className` usage correctly sparing
//! the live token and class.

use noslop_core::{scan, ScanOptions};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/style")
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
fn unused_token_flags_only_the_dead_one() {
    let m = messages("unused-css-token");
    assert!(m.iter().any(|s| s.contains("--dead-token")), "{m:?}");
    assert!(
        !m.iter().any(|s| s.contains("--brand")),
        "used via var(): {m:?}"
    );
}

#[test]
fn broken_reference_detected() {
    let m = messages("broken-css-reference");
    assert!(m.iter().any(|s| s.contains("--missing")), "{m:?}");
}

#[test]
fn unused_class_flags_only_unused_selectors() {
    let m = messages("unused-css-class");
    assert!(m.iter().any(|s| s.contains(".ghost")), "{m:?}");
    assert!(m.iter().any(|s| s.contains(".broken")), "{m:?}");
    // `.card` is referenced in a className, so it is live.
    assert!(!m.iter().any(|s| s.contains(".card")), "{m:?}");
}
