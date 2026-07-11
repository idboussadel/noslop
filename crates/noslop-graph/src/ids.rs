//! Primitive identifiers and location types shared across every stage.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Dense index into [`crate::ir::Graph::files`]. Assigned during graph build and
/// stable only within a single run; never serialize it into the output contract
/// (use the path or the stable symbol id for that).
pub type FileId = usize;

/// The languages noslop analyzes. Two done excellently beats five done poorly
/// (ARCHITECTURE.md §1), so this list stays short on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    TypeScript,
    /// TSX / JSX — a distinct tree-sitter grammar, same downstream handling.
    Tsx,
    JavaScript,
    Python,
    /// CSS / CSS Modules — analyzed for token and class liveness only.
    Css,
}

impl Language {
    /// Best-effort language detection from a file extension. Returns `None` for
    /// files noslop does not analyze so the walker can skip them cheaply.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "ts" | "mts" | "cts" => Some(Language::TypeScript),
            "tsx" | "jsx" => Some(Language::Tsx),
            "js" | "mjs" | "cjs" => Some(Language::JavaScript),
            "py" | "pyi" => Some(Language::Python),
            "css" => Some(Language::Css),
            _ => None,
        }
    }

    /// True for the TypeScript/JavaScript family (shared resolver + plugins).
    pub fn is_js_family(self) -> bool {
        matches!(
            self,
            Language::TypeScript | Language::Tsx | Language::JavaScript
        )
    }

    pub fn is_python(self) -> bool {
        matches!(self, Language::Python)
    }

    pub fn is_css(self) -> bool {
        matches!(self, Language::Css)
    }
}

/// What *kind* of file this is for dead-code purposes — the type system that
/// keeps framework/tooling/ambient files out of `unused-file` without a
/// hand-maintained ignore list (see FALSE_POSITIVES_AND_FALLOW.md §1.2). Derived
/// purely from the path + language, so it is cheap and deterministic. Only
/// [`FileRole::Source`] files are judged by reachability rules; the rest are live
/// via channels the import graph cannot see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileRole {
    /// Ordinary code — the only role `unused-file`/`unused-export` judge.
    Source,
    /// A tool config loaded by convention (`eslint.config.mjs`, `vite.config.ts`,
    /// `.prettierrc.js`). Never `unused-file`; still a reachability *seed* so the
    /// helpers it imports stay live.
    Config,
    /// Ambient type declarations the compiler auto-includes and nothing imports by
    /// name (`*.d.ts`, `next-env.d.ts`, Python `.pyi` stubs). Never `unused-file`.
    TypeDecl,
    /// A Python package marker (`__init__.py`). Executed whenever any module in its
    /// package is imported, so it is alive iff its package subtree is reachable —
    /// judged specially, never by a direct inbound edge.
    PackageInit,
}

impl FileRole {
    /// Classify a repo-relative path. Framework-agnostic: these are language /
    /// ecosystem conventions (small, stable, universal), not per-framework knowledge.
    pub fn classify(rel_path: &Path, language: Language) -> Self {
        let name = rel_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if language.is_python() {
            if name == "__init__.py" {
                return FileRole::PackageInit;
            }
            if name.ends_with(".pyi") {
                return FileRole::TypeDecl;
            }
            return FileRole::Source;
        }

        // JS/TS family.
        if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
            return FileRole::TypeDecl;
        }
        if is_tooling_config(&name) {
            return FileRole::Config;
        }
        FileRole::Source
    }

    /// A leaf reachability root: its own imports keep helpers alive, but it is
    /// never itself reported dead.
    pub fn is_reachability_seed(self) -> bool {
        matches!(self, FileRole::Config)
    }

    /// Should `unused-file` / `unused-export` judge this file at all?
    pub fn is_source(self) -> bool {
        matches!(self, FileRole::Source)
    }
}

/// The JS-family extensions a tooling config can wear.
const JS_CONFIG_EXTS: &[&str] = &["js", "cjs", "mjs", "ts", "mts", "cts", "jsx", "tsx"];

/// Universal JS-ecosystem tool-config conventions — matched by *shape*, not a
/// literal filename list, so a new tool's `foo.config.ts` is covered for free.
fn is_tooling_config(name_lower: &str) -> bool {
    let Some((stem, ext)) = name_lower.rsplit_once('.') else {
        return false;
    };
    if !JS_CONFIG_EXTS.contains(&ext) {
        return false;
    }
    // `eslint.config.mjs`, `tailwind.config.ts`, `vite.config.js`, `jest.config.js`.
    if stem.ends_with(".config") {
        return true;
    }
    // Dotfile rc with a JS body: `.eslintrc.js`, `.prettierrc.cjs`, `.mocharc.mjs`.
    if name_lower.starts_with('.') && stem.ends_with("rc") {
        return true;
    }
    false
}

/// A 1-based, inclusive line range within a file. Line-level is deliberately
/// coarse: findings are attributed to symbols by their stable id, and spans are
/// only for human display, so byte offsets would be needless churn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start_line: u32,
    pub end_line: u32,
}

impl Span {
    pub fn new(start_line: u32, end_line: u32) -> Self {
        Self {
            start_line,
            end_line,
        }
    }

    /// Build a span from a tree-sitter node's 0-based row range.
    pub fn from_rows(start_row: usize, end_row: usize) -> Self {
        Self {
            start_line: start_row as u32 + 1,
            end_line: end_row as u32 + 1,
        }
    }
}
