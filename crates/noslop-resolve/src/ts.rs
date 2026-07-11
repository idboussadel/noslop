//! TypeScript/JavaScript module resolution.
//!
//! A pragmatic Node/tsconfig resolver: relative specifiers, tsconfig `paths`
//! aliases with `baseUrl`, workspace-package imports (resolved into the member's
//! files), and bare specifiers (external deps). Candidate paths are checked
//! against the discovered-file set, so resolution is deterministic and I/O-free.

use crate::path::join_normalized;
use crate::{Ctx, Resolved};
use noslop_graph::Package;
use std::path::{Path, PathBuf};

/// Extension probe order, mirroring TypeScript's own resolution preference.
const EXTENSIONS: &[&str] = &["ts", "tsx", "d.ts", "js", "jsx", "mjs", "cjs"];
const INDEX_FILES: &[&str] = &["index.ts", "index.tsx", "index.js", "index.jsx"];

pub(crate) fn resolve(
    ctx: &Ctx,
    specifier: &str,
    from_dir: &Path,
    pkg: Option<&Package>,
) -> Resolved {
    if specifier.starts_with('.') {
        return match probe(ctx, &join_normalized(from_dir, specifier)) {
            Some(p) => Resolved::Internal(p),
            None => Resolved::Unresolved,
        };
    }

    // tsconfig `paths` alias.
    if let Some(pkg) = pkg {
        if let Some(resolved) = resolve_ts_paths(ctx, specifier, pkg) {
            return resolved;
        }
    }

    // Workspace package import (`@acme/shared` → that member's entry/subpath).
    if let Some(resolved) = resolve_workspace(ctx, specifier) {
        return resolved;
    }

    // Otherwise external. Keep the scope for scoped packages (`@scope/name`).
    Resolved::External(external_name(specifier))
}

/// Probe a base path (without extension) for a concrete file or an index file.
fn probe(ctx: &Ctx, base: &Path) -> Option<PathBuf> {
    if ctx.file_exists(base) {
        return Some(base.to_path_buf());
    }
    for ext in EXTENSIONS {
        let cand = with_extension(base, ext);
        if ctx.file_exists(&cand) {
            return Some(cand);
        }
    }
    for index in INDEX_FILES {
        let cand = base.join(index);
        if ctx.file_exists(&cand) {
            return Some(cand);
        }
    }
    None
}

/// Append `.ext` to a path's file name (not `set_extension`, which would replace
/// a `.` already present in the stem, e.g. `foo.config`).
fn with_extension(base: &Path, ext: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

fn resolve_ts_paths(ctx: &Ctx, specifier: &str, pkg: &Package) -> Option<Resolved> {
    let base_url = pkg.ts_base_url.clone().unwrap_or_else(|| pkg.root.clone());
    for (alias, targets) in &pkg.ts_paths {
        if let Some(remainder) = match_alias(alias, specifier) {
            for target in targets {
                let substituted = target.replace('*', &remainder);
                if let Some(p) = probe(ctx, &join_normalized(&base_url, &substituted)) {
                    return Some(Resolved::Internal(p));
                }
            }
        }
    }
    None
}

/// Match a tsconfig alias pattern (`@app/*` or `@app`) against a specifier,
/// returning the captured `*` remainder (empty for exact aliases).
fn match_alias(alias: &str, specifier: &str) -> Option<String> {
    if let Some(prefix) = alias.strip_suffix('*') {
        specifier.strip_prefix(prefix).map(|r| r.to_string())
    } else if alias == specifier {
        Some(String::new())
    } else {
        None
    }
}

fn resolve_workspace(ctx: &Ctx, specifier: &str) -> Option<Resolved> {
    let top = package_top(specifier);
    let pkg = ctx
        .packages
        .iter()
        .find(|p| !p.name.is_empty() && (p.name == top || specifier == p.name))?;

    let subpath = specifier.strip_prefix(&pkg.name).unwrap_or("");
    let subpath = subpath.trim_start_matches('/');
    let base = if subpath.is_empty() {
        pkg.root.clone()
    } else {
        pkg.root.join(subpath)
    };
    probe(ctx, &base).map(Resolved::Internal)
}

/// The distribution name of a bare specifier: `@scope/pkg/sub` → `@scope/pkg`,
/// `lodash/fp` → `lodash`.
fn external_name(specifier: &str) -> String {
    let mut parts = specifier.split('/');
    let first = parts.next().unwrap_or(specifier);
    if first.starts_with('@') {
        match parts.next() {
            Some(second) => format!("{first}/{second}"),
            None => first.to_string(),
        }
    } else {
        first.to_string()
    }
}

/// Leading path segment used to match a workspace package by name.
fn package_top(specifier: &str) -> String {
    external_name(specifier)
}
