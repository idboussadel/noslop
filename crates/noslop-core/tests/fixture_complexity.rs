//! Complexity fixture — flags over-threshold functions (TS + Python), spares
//! simple ones, and honors a path override with a documented reason.

use noslop_core::{scan, ScanOptions};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/complexity")
}

fn complex_functions() -> Vec<String> {
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
        .filter(|f| f.rule.as_str() == "high-complexity")
        .filter_map(|f| f.symbol.clone())
        .collect();
    out.sort();
    out
}

#[test]
fn flags_complex_spares_simple_and_overridden() {
    let flagged = complex_functions();
    assert!(flagged.iter().any(|s| s.contains("gnarly")), "{flagged:?}");
    assert!(flagged.iter().any(|s| s.contains("deep")), "{flagged:?}");
    // `simple` is under threshold; `legacyFlow` is exempted by the override.
    assert!(!flagged.iter().any(|s| s.contains("simple")), "{flagged:?}");
    assert!(
        !flagged.iter().any(|s| s.contains("legacyFlow")),
        "{flagged:?}"
    );
}

fn large_functions() -> Vec<String> {
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
        .filter(|f| f.rule.as_str() == "large-function")
        .filter_map(|f| f.symbol.clone())
        .collect();
    out.sort();
    out
}

#[test]
fn flags_large_functions_by_line_count() {
    let flagged = large_functions();
    assert!(flagged.iter().any(|s| s.contains("bigPage")), "{flagged:?}");
    assert!(!flagged.iter().any(|s| s.contains("simple")), "{flagged:?}");
    assert!(!flagged.iter().any(|s| s.contains("gnarly")), "{flagged:?}");
}
