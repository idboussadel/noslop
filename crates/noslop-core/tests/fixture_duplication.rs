//! Duplication fixture — semantic clones across TS and Python (reported once per
//! block), while chained calls to a shared abstraction are not flagged, and a
//! TS↔Python clone is never invented (the index is language-partitioned).

use noslop_core::{scan, ScanOptions};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/duplication")
}

fn dup_files() -> Vec<String> {
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
        .filter(|f| f.rule.as_str() == "duplicate-code")
        .flat_map(|f| {
            // The primary file plus every "also at <file>:<line>" in the message.
            let mut files = vec![f.file.display().to_string()];
            for part in f.message.split("also at ").skip(1) {
                if let Some(loc) = part.split(':').next() {
                    files.push(loc.trim_end_matches('.').to_string());
                }
            }
            files
        })
        .collect()
}

#[test]
fn semantic_clones_reported_ts_and_python() {
    let files = dup_files();
    // The TS pair (renamed identifiers) and the Python pair are both caught.
    assert!(files.iter().any(|f| f.contains("src/a.ts")), "{files:?}");
    assert!(files.iter().any(|f| f.contains("src/b.ts")), "{files:?}");
    assert!(files.iter().any(|f| f.contains("worker_a.py")), "{files:?}");
    assert!(files.iter().any(|f| f.contains("worker_b.py")), "{files:?}");
}

#[test]
fn chained_calls_and_cross_language_not_flagged() {
    let files = dup_files();
    // The abstraction chain is not refactorable duplication.
    assert!(!files.iter().any(|f| f.contains("chain.ts")), "{files:?}");
    // No clone should pair a .ts with a .py (index is per language).
    let outcome = scan(&ScanOptions {
        root: fixture_root(),
        use_cache: false,
        threads: None,
        ..Default::default()
    })
    .unwrap();
    for f in outcome
        .report
        .findings
        .iter()
        .filter(|f| f.rule.as_str() == "duplicate-code")
    {
        let is_py = f.file.extension().map(|e| e == "py").unwrap_or(false);
        // A python clone must not mention a .ts file, and vice versa.
        let cross_language = if is_py {
            f.message.contains(".ts")
        } else {
            f.message.contains(".py")
        };
        assert!(!cross_language, "cross-language clone: {}", f.message);
    }
}
