//! Python module resolution (hand-written, per ARCHITECTURE.md §3/§7.1).
//!
//! Handles absolute dotted imports resolved under the repo's import roots,
//! relative imports (leading dots walk up package levels), namespace packages
//! (dirs without `__init__.py`), and the `pip-name ≠ import-name` mapping for
//! external-dependency attribution.

use crate::path::normalize;
use crate::{Ctx, Resolved};
use noslop_graph::RawImport;
use std::path::{Path, PathBuf};

pub(crate) fn resolve(ctx: &Ctx, specifier: &str, from_file: &Path) -> Resolved {
    if let Some((base, module)) = relative_base(specifier, from_file) {
        return match resolve_module(ctx, &base, &module) {
            Some(p) => Resolved::Internal(p),
            None => Resolved::Unresolved,
        };
    }

    // Absolute dotted import: try under each import root.
    let rel = specifier.replace('.', "/");
    for root in &ctx.py_roots {
        if let Some(p) = probe_module(ctx, &normalize(&root.join(&rel))) {
            return Resolved::Internal(p);
        }
    }

    Resolved::External(external_name(specifier))
}

/// For `from pkg import sub` where `sub` is itself a module, also yield the
/// submodule file so it stays reachable. Calls `sink` for each resolved target.
pub(crate) fn resolve_submodules(
    ctx: &Ctx,
    import: &RawImport,
    from_file: &Path,
    mut sink: impl FnMut(PathBuf),
) {
    // Establish the package base the names hang off of.
    let base = if import.specifier.starts_with('.') {
        match relative_base(&import.specifier, from_file) {
            Some((base, module)) if module.is_empty() => base,
            Some((base, module)) => normalize(&base.join(module.replace('.', "/"))),
            None => return,
        }
    } else {
        let rel = import.specifier.replace('.', "/");
        // Find whichever root actually contains the package.
        match ctx
            .py_roots
            .iter()
            .map(|r| normalize(&r.join(&rel)))
            .find(|p| dir_has_module(ctx, p))
        {
            Some(p) => p,
            None => return,
        }
    };

    for name in &import.names {
        if let Some(p) = probe_module(ctx, &base.join(&name.imported)) {
            sink(p);
        }
    }
}

/// Compute the (directory, remaining-dotted-module) base for a relative import.
/// `from . import x` in `a/b/c.py` → base `a/b`, module ``.
/// `from ..pkg import y` in `a/b/c.py` → base `a`, module `pkg`.
fn relative_base(specifier: &str, from_file: &Path) -> Option<(PathBuf, String)> {
    if !specifier.starts_with('.') {
        return None;
    }
    let dots = specifier.chars().take_while(|c| *c == '.').count();
    let rest = &specifier[dots..];

    let mut base = from_file.parent()?.to_path_buf();
    // One dot = current package (the file's dir); each extra dot goes up one.
    for _ in 0..dots.saturating_sub(1) {
        base = base.parent()?.to_path_buf();
    }
    let module = rest.trim_matches('.').replace('.', "/");
    Some((base, module))
}

/// Resolve a `base` dir + dotted `module` to a `.py`/package file.
fn resolve_module(ctx: &Ctx, base: &Path, module: &str) -> Option<PathBuf> {
    if module.is_empty() {
        // `from . import x` — the package itself; represent as its `__init__`.
        let init = base.join("__init__.py");
        if ctx.file_exists(&init) {
            return Some(init);
        }
        // Namespace package (no __init__): nothing concrete to point at.
        return None;
    }
    probe_module(ctx, &normalize(&base.join(module.replace('.', "/"))))
}

/// Probe a path stem for `stem.py`, `stem/__init__.py`, or `stem.pyi`.
fn probe_module(ctx: &Ctx, stem: &Path) -> Option<PathBuf> {
    [
        with_suffix(stem, ".py"),
        stem.join("__init__.py"),
        with_suffix(stem, ".pyi"),
    ]
    .into_iter()
    .find(|cand| ctx.file_exists(cand))
}

fn dir_has_module(ctx: &Ctx, stem: &Path) -> bool {
    probe_module(ctx, stem).is_some()
}

fn with_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// External distribution name for an unresolved top-level module, applying the
/// well-known import-name → pip-name mappings.
fn external_name(specifier: &str) -> String {
    let top = specifier.split('.').next().unwrap_or(specifier);
    match top {
        "cv2" => "opencv-python",
        "PIL" => "pillow",
        "sklearn" => "scikit-learn",
        "yaml" => "pyyaml",
        "bs4" => "beautifulsoup4",
        "dotenv" => "python-dotenv",
        "jwt" => "pyjwt",
        other => other,
    }
    .to_string()
}
