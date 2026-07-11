//! Role-classification fixture — proves the false-positive fixes for tooling
//! configs, ambient declarations, and Python `__init__.py` (see
//! FALSE_POSITIVES_AND_FALLOW.md §1). A framework/config/decl file must never be
//! `unused-file`; a genuine orphan still must be.

use noslop_core::{scan, ScanOptions};
use noslop_graph::Confidence;
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/roles")
}

fn scan_roles() -> noslop_report::Report {
    scan(&ScanOptions {
        root: fixture_root(),
        use_cache: false,
        threads: None,
        ..Default::default()
    })
    .expect("scan should succeed")
    .report
}

fn unused_files() -> Vec<String> {
    let mut out: Vec<String> = scan_roles()
        .findings
        .iter()
        .filter(|f| f.rule.as_str() == "unused-file")
        .map(|f| f.file.display().to_string())
        .collect();
    out.sort();
    out
}

#[test]
fn configs_and_decls_are_never_unused_files() {
    let flagged = unused_files();
    for live in [
        "eslint.config.mjs",
        "next-env.d.ts",
        "pkg/__init__.py",
        "pkg/sub/__init__.py",
        "pkg/sub/mod.py",
        "main.py",
    ] {
        assert!(
            !flagged.iter().any(|f| f == live),
            "{live} must not be reported as unused-file, got {flagged:?}"
        );
    }
}

#[test]
fn genuine_orphans_are_still_flagged() {
    let flagged = unused_files();
    assert!(flagged.iter().any(|f| f == "src/lib/orphan.ts"));
    // A dead package is reported, but only at Low confidence (hidden by default).
    let dead_pkg = scan_roles()
        .findings
        .into_iter()
        .find(|f| f.file.display().to_string() == "deadpkg/__init__.py")
        .expect("dead package should be reported");
    assert_eq!(dead_pkg.confidence, Confidence::Low);
}

#[test]
fn high_confidence_view_is_clean_except_the_orphan() {
    let report = scan_roles();
    let high: Vec<String> = report
        .findings
        .iter()
        .filter(|f| f.rule.as_str() == "unused-file" && f.confidence == Confidence::High)
        .map(|f| f.file.display().to_string())
        .collect();
    assert_eq!(high, vec!["src/lib/orphan.ts".to_string()]);
}
