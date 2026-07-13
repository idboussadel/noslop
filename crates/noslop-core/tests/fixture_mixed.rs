//! Fixture integration test — scans the mixed TS+Python monorepo end to end and
//! asserts the exact set of expected findings (ARCHITECTURE.md §13). This is the
//! "no slop" enforcement: a scenario is only fixed once it has a fixture.

use noslop_core::{scan, ScanOptions};
use noslop_graph::Confidence;
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    // crates/noslop-core → repo root → fixtures/mixed
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mixed")
}

fn findings_for(rule: &str) -> Vec<String> {
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

fn findings_for_confidence(rule: &str, confidence: Confidence) -> Vec<String> {
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
        .filter(|f| f.rule.as_str() == rule && f.confidence == confidence)
        .filter_map(|f| f.symbol.clone())
        .collect();
    out.sort();
    out
}

#[test]
fn unused_files_are_exactly_the_orphans() {
    assert_eq!(
        findings_for("unused-file"),
        vec![
            "apps/api/app/billing/invoice_a.py".to_string(),
            "apps/api/app/billing/invoice_b.py".to_string(),
            "apps/api/app/dead_tool.py".to_string(),
            "apps/api/app/legacy/spike.py".to_string(),
            "apps/api/app/middleware/trace.py".to_string(),
            "apps/web/src/components/DeadWidget.tsx".to_string(),
            "apps/web/src/hooks/useDebounce.ts".to_string(),
            "apps/web/src/lib/analytics.ts".to_string(),
            "apps/web/src/lib/orphan.ts".to_string(),
            "apps/worker/worker/jobs/email.py".to_string(),
            "packages/shared/src/legacy/orphan.ts".to_string(),
        ]
    );
}

#[test]
fn unused_export_finds_only_the_dead_export() {
    // formatPrice (used externally), unusedName (imported elsewhere), and
    // formatLocalOnly (used in-module) must NOT appear in the high-confidence view.
    assert_eq!(
        findings_for_confidence("unused-export", Confidence::High),
        vec![
            "apps/web/src/lib/api.ts::apiLabel".to_string(),
            "apps/web/src/lib/format.ts::formatDead".to_string(),
        ]
    );
}

#[test]
fn same_file_used_exports_are_api_surface_only() {
    let medium = findings_for_confidence("unused-export", Confidence::Medium);
    assert!(medium
        .iter()
        .any(|f| f == "apps/web/src/lib/format.ts::formatLocalOnly"));
}

#[test]
fn unused_import_finds_the_unused_binding() {
    let f = findings_for("unused-import");
    assert_eq!(f, vec!["apps/web/src/app/page.tsx".to_string()]);
}

#[test]
fn python_cycles_are_detected_as_errors() {
    let outcome = scan(&ScanOptions {
        root: fixture_root(),
        use_cache: false,
        threads: None,
        ..Default::default()
    })
    .unwrap();
    let cycles: Vec<_> = outcome
        .report
        .findings
        .iter()
        .filter(|f| f.rule.as_str() == "circular-imports")
        .collect();
    assert_eq!(cycles.len(), 2);
    assert!(cycles
        .iter()
        .any(|c| c.message.contains("cycle_a.py") && c.message.contains("cycle_b.py")));
    assert!(cycles
        .iter()
        .any(|c| c.message.contains("invoice_a.py") && c.message.contains("invoice_b.py")));
    assert!(cycles.iter().all(|c| c.severity == noslop_graph::Severity::Error));
}

#[test]
fn framework_dep_not_flagged_but_orphan_deps_are() {
    let deps = findings_for("unused-dependency");
    assert!(deps
        .iter()
        .all(|d| !d.contains("next") && !d.contains("fastapi") && !d.contains("click")));
    assert_eq!(deps.len(), 4);
}

#[test]
fn scan_is_deterministic() {
    let json = || {
        scan(&ScanOptions {
            root: fixture_root(),
            use_cache: false,
            threads: None,
            ..Default::default()
        })
        .unwrap()
        .report
        .to_json()
    };
    assert_eq!(json(), json(), "identical input must yield identical JSON");
}
