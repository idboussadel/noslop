//! Pure path helpers used by both resolvers. Resolution is done against the
//! *set of discovered files*, not the live filesystem, so it is deterministic
//! and needs no I/O.

use std::path::{Component, Path, PathBuf};

/// Normalize a path lexically: fold `.` and `..` without touching the disk.
/// `a/b/../c` → `a/c`. Leading `..` that escapes the root are dropped (an import
/// outside the repo is treated as unresolved).
pub fn normalize(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(out.last(), Some(Component::Normal(_))) {
                    // Cannot go above the repo root — ignore.
                    continue;
                }
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.iter().collect()
}

/// Join `base` (a directory) with a relative `spec` and normalize.
pub fn join_normalized(base: &Path, spec: &str) -> PathBuf {
    normalize(&base.join(spec))
}
