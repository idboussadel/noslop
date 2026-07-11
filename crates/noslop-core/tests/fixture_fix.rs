//! Integration test — auto-fix on the mixed fixture (dry-run in a temp copy).

use noslop_core::{scan, ScanOptions};
use noslop_fix::{fix, FixOptions};
use noslop_graph::Confidence;
use std::fs;
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mixed")
}

#[test]
fn fix_dry_run_targets_high_confidence_dead_code() {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir_all(&fixture_root(), tmp.path());

    let outcome = scan(&ScanOptions {
        root: tmp.path().to_path_buf(),
        use_cache: false,
        threads: None,
        ..Default::default()
    })
    .unwrap();

    let high: Vec<_> = outcome
        .report
        .findings
        .iter()
        .filter(|f| f.confidence == Confidence::High)
        .cloned()
        .collect();

    let result = fix(
        tmp.path(),
        &high,
        &outcome.facts,
        &FixOptions {
            dry_run: true,
            min_confidence: Confidence::High,
            include_deps: false,
        },
    )
    .unwrap();

    assert!(
        result.diffs.len() >= 4,
        "expected file/import/export deletes, got {} diffs",
        result.diffs.len()
    );
    assert!(result.diffs.iter().any(|d| d.contains("orphan.ts")));
    assert!(result.diffs.iter().any(|d| d.contains("unusedName")));
    assert!(result.diffs.iter().any(|d| d.contains("formatDead")));

    // Dry run must not touch the tree.
    assert!(tmp.path().join("apps/web/src/lib/orphan.ts").exists());
}

#[test]
fn fix_apply_then_restore_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    copy_dir_all(&fixture_root(), tmp.path());
    let orphan = tmp.path().join("apps/web/src/lib/orphan.ts");
    let before = fs::read_to_string(&orphan).unwrap();

    let outcome = scan(&ScanOptions {
        root: tmp.path().to_path_buf(),
        use_cache: false,
        threads: None,
        ..Default::default()
    })
    .unwrap();

    let high: Vec<_> = outcome
        .report
        .findings
        .iter()
        .filter(|f| f.confidence == Confidence::High)
        .cloned()
        .collect();

    fix(
        tmp.path(),
        &high,
        &outcome.facts,
        &FixOptions {
            dry_run: false,
            min_confidence: Confidence::High,
            include_deps: false,
        },
    )
    .unwrap();

    assert!(!orphan.exists(), "fix should delete orphan.ts");

    let restored = noslop_fix::restore(tmp.path()).unwrap();
    assert!(restored >= 1);
    assert_eq!(fs::read_to_string(&orphan).unwrap(), before);
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).unwrap();
        }
    }
}
