//! Facts — the language-neutral contract emitted by the *extract* stage.
//!
//! Extractors walk a tree-sitter CST and emit these plain structures: no
//! resolution, no graph, no opinions (ARCHITECTURE.md §6). Everything a later
//! pass might need must arrive here as a fact — a pass that "just peeks at the
//! AST" is a design bug.

use crate::ids::{FileId, Language, Span};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What kind of definition a symbol is. Drives severity defaults and which
/// "unused-*" rule a finding belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Class,
    Method,
    Variable,
    /// TS `type`/`interface`/`enum` — feeds `unused-type`.
    Type,
    /// A member of a TS `enum` (`Color.Red`) — feeds `unused-enum-member`.
    EnumMember,
    /// A class field / property — feeds `unused-class-member`.
    Field,
    /// A function/method parameter — feeds `unused-parameter`. Only *unused*
    /// parameters are ever emitted (usage is decided locally during extraction).
    Parameter,
}

/// A definition found in a file. Positions are the symbol's declaration span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    /// Whether the declaration is exported / part of the module's public surface.
    pub exported: bool,
    /// TS type-only declaration or Python `TYPE_CHECKING`-guarded symbol.
    pub is_type_only: bool,
    /// The enclosing type/class name for members (enum members, class members),
    /// used for attribution and stable ids. `None` for top-level declarations.
    #[serde(default)]
    pub parent: Option<String>,
    /// Visibility declared by an `@public`/`@internal`/`@expected-unused` comment
    /// on the line above this declaration (resolved in [`crate::facts`] extract).
    #[serde(default)]
    pub visibility: Option<Visibility>,
}

/// A declared visibility intent, from an annotation comment. Overrides the
/// default dead-code judgement for the symbol it sits above.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    /// Intentional public API — never report it as unused.
    Public,
    /// Internal-only — re-enable unused analysis even inside an entry file.
    Internal,
    /// Expected to be unused for now; suppress the unused finding, but report if
    /// it *becomes* used (the annotation has gone stale).
    ExpectedUnused,
}

/// An `@public` / `@internal` / `@expected-unused` annotation parsed from a
/// comment. Line-anchored to the declaration immediately below it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub visibility: Visibility,
    /// 1-based line the annotation applies to (the declaration below the comment).
    pub line: u32,
    pub reason: Option<String>,
}

/// How an import reaches a module — gates the confidence of downstream findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportKind {
    /// ESM `import ... from` / Python `import`/`from ... import`.
    Static,
    /// `import("literal")` — resolvable; a non-literal specifier is recorded as
    /// an [`Marker::UnresolvableDynamicImport`] instead.
    Dynamic,
    /// CommonJS `require(...)` — treated as static but lower confidence.
    Require,
}

/// A single name pulled in by an import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedName {
    /// The name as exported by the source module (`foo` in `{ foo as bar }`).
    pub imported: String,
    /// The local binding in this file (`bar` in `{ foo as bar }`).
    pub local: String,
}

/// An unresolved import: the specifier string plus the names it binds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawImport {
    pub specifier: String,
    pub names: Vec<ImportedName>,
    pub kind: ImportKind,
    /// `import * as ns` / bare `import x` / `import module` — the whole module
    /// namespace is bound, so we cannot tell which of its exports are used.
    pub is_namespace: bool,
    /// TS `import type` / Python `if TYPE_CHECKING` import.
    pub is_type_only: bool,
    /// Synthesized from an `export ... from` re-export rather than a real
    /// `import`. Such an edge keeps the target reachable but has no local binding
    /// to reference, so `unused-import` must skip it.
    pub is_reexport: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportKind {
    Named,
    Default,
    /// `export * from "src"` — re-exports every name of `src` transitively.
    ReExportAll,
    /// `export { a } from "src"` / `export * as ns from "src"`.
    ReExportNamed,
}

/// An exported name (or re-export) declared in a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawExport {
    pub name: String,
    pub kind: ExportKind,
    /// The re-export source specifier, if this export forwards another module.
    pub source: Option<String>,
    pub is_type_only: bool,
    pub span: Span,
}

/// A named CSS entity (a `--custom-property` or a `.class` selector) with its
/// declaration span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CssName {
    pub name: String,
    pub span: Span,
}

/// Styling facts, for the token/class-liveness passes. Populated from CSS files
/// (declarations, `var()` refs, class selectors) and from TS/JSX (`className`
/// refs). Empty for non-web files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StyleFacts {
    /// `--brand: …` custom-property declarations.
    pub declared_tokens: Vec<CssName>,
    /// `var(--brand)` references (name includes the leading `--`).
    pub var_refs: Vec<String>,
    /// `.card` class selectors (name without the dot).
    pub class_selectors: Vec<CssName>,
    /// Class names used via `className="card"` / `class="card"`.
    pub class_refs: Vec<String>,
}

/// The lexical class of a duplication token — governs which normalization each
/// duplication mode applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokKind {
    Ident,
    Num,
    Str,
    /// Keywords, operators, punctuation — always compared by exact text.
    Punct,
}

/// One normalized token for duplication detection: the hash of its raw text, its
/// lexical class, and the 1-based line it starts on. Two hashes are not stored —
/// the raw-text hash plus the class serve every mode (the index is built at the
/// most permissive normalization, then candidates are verified down to the
/// requested mode).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Tok {
    pub hash: u64,
    pub kind: TokKind,
    pub line: u32,
    /// True if this token ends a statement (`;`, newline-significant, `}`), used
    /// by the "not just one expression" filter without another traversal.
    pub stmt_end: bool,
}

/// A call site, recorded as its (possibly dotted) callee path — `subprocess.run`,
/// `child_process.exec`, `fetch`. Arguments never matter to policy, so only the
/// callee is kept. Drives `banned-call` / `banned-effect`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSite {
    pub callee: String,
    pub span: Span,
}

/// An identifier *used* somewhere in the file. In-file scoping is intentionally
/// approximate (ARCHITECTURE.md §6): any occurrence of a name counts as a use,
/// which is the conservative choice — it can only suppress a finding, never
/// invent one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawRef {
    pub name: String,
}

/// Facts the *plugin* engine consumes to decide entry points and liveness that
/// import edges cannot express (decorators, framework instantiation, dynamic
/// module strings). Kept as a small closed set for v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Marker {
    /// A decorator applied to a symbol, captured as its full dotted name
    /// (`fastapi.APIRouter.get`, `app.route`, `shared_task`).
    Decorator { dotted: String, on_symbol: String },
    /// A non-literal dynamic import / `importlib.import_module(expr)`. Its mere
    /// presence caps `unused-*` confidence for the whole package.
    UnresolvableDynamicImport,
    /// A string literal that looks like a dotted module path in a position that
    /// matters (Django settings, Celery task names). Plugins turn these into edges.
    ModuleStringLiteral { value: String },
}

/// A `noslop-ignore` suppression parsed from a comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suppression {
    pub rule: String,
    /// `true` for `noslop-ignore-file`, `false` for `noslop-ignore-next-line`.
    pub file_scoped: bool,
    /// 1-based line the suppression applies to (the line *after* the comment for
    /// `next-line`; ignored for file-scoped).
    pub line: u32,
    pub reason: Option<String>,
}

/// Cheap per-file metrics gathered during extraction (inputs to future health
/// passes). Free to collect while we already have the tree in hand.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileMetrics {
    pub loc: u32,
}

/// Per-function complexity, computed during extraction. Cyclomatic is classic
/// McCabe (1 + decision points); cognitive follows the SonarSource model
/// (increments + a nesting penalty on nested control structures).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMetrics {
    /// Function/method name, or `(anonymous)` for unnamed callables.
    pub name: String,
    /// Enclosing class name, for attribution — same convention as `RawSymbol`.
    pub parent: Option<String>,
    pub span: Span,
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub loc: u32,
}

/// Everything one file contributes to the graph. Content-hash keyed so a warm
/// run reparses only files whose bytes changed (ARCHITECTURE.md §4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileFacts {
    pub file: FileId,
    pub path: PathBuf,
    pub language: Language,
    pub content_hash: u64,
    pub symbols: Vec<RawSymbol>,
    pub imports: Vec<RawImport>,
    pub exports: Vec<RawExport>,
    pub refs: Vec<RawRef>,
    /// Property names read via member access (`obj.foo`, `self._x`, `E.Red`).
    /// Kept separate from `refs` so it feeds cross-file member liveness without
    /// weakening `unused-import` (which counts only local binding identifiers).
    #[serde(default)]
    pub member_accesses: Vec<String>,
    pub markers: Vec<Marker>,
    pub suppressions: Vec<Suppression>,
    /// `@public`/`@internal`/`@expected-unused` annotations found in this file.
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    /// Raw string-literal contents, fed into the graph's global set for the
    /// "name appears in a string" confidence dampener (ARCHITECTURE.md §12).
    pub string_literals: Vec<String>,
    pub metrics: FileMetrics,
    /// Per-function cyclomatic + cognitive complexity.
    #[serde(default)]
    pub functions: Vec<FunctionMetrics>,
    /// Call sites (dotted callee paths) for `banned-call`/`banned-effect`.
    #[serde(default)]
    pub calls: Vec<CallSite>,
    /// CSS token / class facts (web files only).
    #[serde(default)]
    pub style: StyleFacts,
}

impl FileFacts {
    /// True if any marker in this file should cap dead-code confidence for its
    /// whole package (unresolvable dynamism).
    pub fn has_unresolvable_dynamism(&self) -> bool {
        self.markers
            .iter()
            .any(|m| matches!(m, Marker::UnresolvableDynamicImport))
    }
}
