//! `noslop-extract` — stage 2. tree-sitter parse per file → language-neutral
//! [`FileFacts`]. Pure and per-file: no resolution, no graph, no filesystem
//! access beyond the bytes handed in, so it is embarrassingly parallel and
//! trivially cacheable by content hash (ARCHITECTURE.md §6).

mod complexity;
mod css;
mod python;
mod tokens;
mod typescript;

use noslop_graph::{
    Annotation, CallSite, CssName, FileFacts, FileMetrics, FunctionMetrics, ImportedName, Language,
    Marker, RawExport, RawImport, RawRef, RawSymbol, StyleFacts, Suppression, Visibility,
};
use std::collections::HashSet;
use std::path::PathBuf;

/// Hash a file's bytes for the cache key and the duplicate-window base. xxh3 is
/// fast and non-cryptographic — exactly right for cache invalidation.
pub fn content_hash(source: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(source)
}

/// Extract facts from a single already-read source file.
///
/// `rel_path` must be repo-relative — it becomes part of every stable symbol id.
/// A parse that fails to produce a tree yields empty-but-valid facts rather than
/// erroring: tree-sitter is error-tolerant, and one unparseable file must never
/// abort a whole-repo scan.
pub fn extract(rel_path: PathBuf, source: &str, file_id: usize, language: Language) -> FileFacts {
    let hash = content_hash(source.as_bytes());
    let mut acc = Acc::new(file_id, rel_path, language, hash);

    let mut parser = tree_sitter::Parser::new();
    let ts_language = tree_sitter_language(language);
    if parser.set_language(&ts_language).is_ok() {
        if let Some(tree) = parser.parse(source, None) {
            let src = source.as_bytes();
            match language {
                Language::Python => python::walk(tree.root_node(), src, &mut acc),
                Language::Css => css::walk(tree.root_node(), src, &mut acc),
                _ => typescript::walk(tree.root_node(), src, &mut acc),
            }
            if !language.is_css() {
                acc.functions = complexity::analyze(tree.root_node(), src, language);
            }
        }
    }
    acc.metrics.loc = source.lines().count() as u32;
    acc.finish()
}

/// Parse `source` and return its duplication token stream. Separate from
/// [`extract`] because tokens are only needed when duplication is requested, so
/// they never bloat the cached [`FileFacts`].
pub fn tokenize(source: &str, language: Language) -> Vec<noslop_graph::Tok> {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_language(language))
        .is_err()
    {
        return Vec::new();
    }
    match parser.parse(source, None) {
        Some(tree) => tokens::tokenize(tree.root_node(), source.as_bytes(), language),
        None => Vec::new(),
    }
}

fn tree_sitter_language(language: Language) -> tree_sitter::Language {
    match language {
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        // Plain `.js`/`.jsx` parse fine with the TSX grammar (a superset for our
        // purposes), and `.ts` with the TypeScript grammar.
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Language::JavaScript => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Language::Css => tree_sitter_css::LANGUAGE.into(),
    }
}

/// Mutable accumulator threaded through the recursive walk. Language modules push
/// into it; [`Acc::finish`] reconciles exports with symbols and builds the facts.
pub(crate) struct Acc {
    file_id: usize,
    path: PathBuf,
    language: Language,
    hash: u64,

    symbols: Vec<RawSymbol>,
    imports: Vec<RawImport>,
    exports: Vec<RawExport>,
    /// Names the module exports without a local declaration match yet
    /// (`export { a }`); reconciled onto symbols in [`Acc::finish`].
    exported_names: HashSet<String>,
    refs: Vec<RawRef>,
    member_accesses: Vec<String>,
    markers: Vec<Marker>,
    suppressions: Vec<Suppression>,
    annotations: Vec<Annotation>,
    string_literals: Vec<String>,
    metrics: FileMetrics,
    functions: Vec<FunctionMetrics>,
    calls: Vec<CallSite>,
    style: StyleFacts,
}

impl Acc {
    fn new(file_id: usize, path: PathBuf, language: Language, hash: u64) -> Self {
        Self {
            file_id,
            path,
            language,
            hash,
            symbols: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
            exported_names: HashSet::new(),
            refs: Vec::new(),
            member_accesses: Vec::new(),
            markers: Vec::new(),
            suppressions: Vec::new(),
            annotations: Vec::new(),
            string_literals: Vec::new(),
            metrics: FileMetrics::default(),
            functions: Vec::new(),
            calls: Vec::new(),
            style: StyleFacts::default(),
        }
    }

    pub(crate) fn add_symbol(&mut self, sym: RawSymbol) {
        self.symbols.push(sym);
    }
    pub(crate) fn add_import(&mut self, import: RawImport) {
        self.imports.push(import);
    }
    pub(crate) fn add_export(&mut self, export: RawExport) {
        self.exported_names.insert(export.name.clone());
        self.exports.push(export);
    }
    pub(crate) fn add_ref(&mut self, name: &str) {
        self.refs.push(RawRef {
            name: name.to_string(),
        });
    }
    /// Record a property name read via member access (`obj.foo` → `foo`). Feeds
    /// cross-file member liveness without polluting the `unused-import` ref set.
    pub(crate) fn add_member_access(&mut self, name: &str) {
        // `#priv` is written both with and without the hash at different sites;
        // normalise to the bare name so declaration and access agree.
        let name = name.trim_start_matches('#');
        if !name.is_empty() {
            self.member_accesses.push(name.to_string());
        }
    }
    pub(crate) fn add_marker(&mut self, marker: Marker) {
        self.markers.push(marker);
    }
    pub(crate) fn add_suppression(&mut self, s: Suppression) {
        self.suppressions.push(s);
    }
    pub(crate) fn add_annotation(&mut self, a: Annotation) {
        self.annotations.push(a);
    }
    pub(crate) fn add_string(&mut self, s: String) {
        self.string_literals.push(s);
    }
    pub(crate) fn add_call(&mut self, callee: String, span: noslop_graph::Span) {
        if !callee.is_empty() {
            self.calls.push(CallSite { callee, span });
        }
    }
    pub(crate) fn add_declared_token(&mut self, t: CssName) {
        self.style.declared_tokens.push(t);
    }
    pub(crate) fn add_var_ref(&mut self, name: String) {
        self.style.var_refs.push(name);
    }
    pub(crate) fn add_class_selector(&mut self, c: CssName) {
        self.style.class_selectors.push(c);
    }
    pub(crate) fn add_class_ref(&mut self, name: &str) {
        if !name.is_empty() {
            self.style.class_refs.push(name.to_string());
        }
    }

    fn finish(mut self) -> FileFacts {
        // Reconcile: a symbol whose name is in an `export {}` clause or an
        // `export const/function/...` declaration is exported.
        for sym in &mut self.symbols {
            if self.exported_names.contains(&sym.name) {
                sym.exported = true;
            }
        }
        // Resolve annotations onto the declaration directly below them, by exact
        // line adjacency (a floating tag attaches to nothing — see the design doc).
        for ann in &self.annotations {
            if let Some(sym) = self
                .symbols
                .iter_mut()
                .find(|s| s.span.start_line == ann.line && s.visibility.is_none())
            {
                sym.visibility = Some(ann.visibility);
            }
        }
        FileFacts {
            file: self.file_id,
            path: self.path,
            language: self.language,
            content_hash: self.hash,
            symbols: self.symbols,
            imports: self.imports,
            exports: self.exports,
            refs: self.refs,
            member_accesses: self.member_accesses,
            markers: self.markers,
            suppressions: self.suppressions,
            annotations: self.annotations,
            string_literals: self.string_literals,
            metrics: self.metrics,
            functions: self.functions,
            calls: self.calls,
            style: self.style,
        }
    }
}

/// Parse an `@public` / `@internal` / `@expected-unused` annotation from a
/// comment. `applies_line` is the 1-based line of the declaration below the
/// comment. Scans every line of the comment (so a multi-line JSDoc block works)
/// and requires the tag to open the payload, so prose mentioning "@public" in a
/// sentence does not match.
pub(crate) fn parse_annotation(comment: &str, applies_line: u32) -> Option<Annotation> {
    for raw in comment.lines() {
        let line = raw.trim().trim_start_matches(['/', '*', '#', ' ']).trim();
        let (tag, rest) = match line.split_once(char::is_whitespace) {
            Some((t, r)) => (t, r),
            None => (line, ""),
        };
        let visibility = match tag {
            "@public" => Visibility::Public,
            "@internal" => Visibility::Internal,
            "@expected-unused" => Visibility::ExpectedUnused,
            _ => continue,
        };
        let reason = rest
            .split_once("--")
            .map(|(_, r)| r.trim().to_string())
            .filter(|s| !s.is_empty());
        return Some(Annotation {
            visibility,
            line: applies_line,
            reason,
        });
    }
    None
}

/// Convenience: build an [`ImportedName`] where the imported and local names
/// coincide (the common `import { foo }` case).
pub(crate) fn plain_name(name: &str) -> ImportedName {
    ImportedName {
        imported: name.to_string(),
        local: name.to_string(),
    }
}

/// Parse a `noslop-ignore-...` directive out of a comment's text, returning the
/// suppression on a match. Requires a rule name — blanket ignores are rejected
/// by construction (ARCHITECTURE.md §9).
pub(crate) fn parse_suppression(comment: &str, comment_line: u32) -> Option<Suppression> {
    let text = comment
        .trim_start_matches(['/', '#', '*', ' '])
        .trim_end_matches(['*', '/', ' ']);
    let (directive, rest) = text.split_once(char::is_whitespace)?;
    let (file_scoped, applies_line) = match directive {
        "noslop-ignore-file" => (true, comment_line),
        "noslop-ignore-next-line" => (false, comment_line + 1),
        _ => return None,
    };
    let rest = rest.trim();
    let (rule, reason) = match rest.split_once("--") {
        Some((rule, reason)) => (rule.trim(), Some(reason.trim().to_string())),
        None => (rest.split_whitespace().next().unwrap_or("").trim(), None),
    };
    if rule.is_empty() {
        return None;
    }
    Some(Suppression {
        rule: rule.split_whitespace().next().unwrap_or(rule).to_string(),
        file_scoped,
        line: applies_line,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use noslop_graph::{ExportKind, ImportKind, SymbolKind};

    fn facts(src: &str, lang: Language) -> FileFacts {
        extract(PathBuf::from("test"), src, 0, lang)
    }

    #[test]
    fn ts_named_import_and_export() {
        let f = facts(
            "import { a, b as c } from './m';\nexport const used = 1;\nexport function dead() {}\n",
            Language::TypeScript,
        );
        let imp = &f.imports[0];
        assert_eq!(imp.specifier, "./m");
        assert_eq!(imp.kind, ImportKind::Static);
        let locals: Vec<_> = imp.names.iter().map(|n| n.local.as_str()).collect();
        assert!(locals.contains(&"a") && locals.contains(&"c"));
        let exports: Vec<_> = f.exports.iter().map(|e| e.name.as_str()).collect();
        assert!(exports.contains(&"used") && exports.contains(&"dead"));
        assert!(f.symbols.iter().any(|s| s.name == "dead" && s.exported));
    }

    #[test]
    fn ts_reexport_becomes_import() {
        let f = facts("export * from './barrel';\n", Language::TypeScript);
        assert_eq!(f.imports[0].specifier, "./barrel");
        assert!(f.imports[0].is_namespace);
    }

    #[test]
    fn ts_dynamic_literal_vs_expr() {
        let f = facts(
            "const a = import('./ok');\nconst b = import(dynamicPath);\n",
            Language::TypeScript,
        );
        assert!(f.imports.iter().any(|i| i.kind == ImportKind::Dynamic));
        assert!(f.has_unresolvable_dynamism());
    }

    #[test]
    fn ts_default_export() {
        let f = facts("export default function foo() {}\n", Language::TypeScript);
        assert!(f.exports.iter().any(|e| e.kind == ExportKind::Default));
    }

    #[test]
    fn py_from_import_and_defs() {
        let f = facts(
            "from a.b import c, d as e\nimport os\n\ndef used():\n    pass\n\ndef _private():\n    pass\n",
            Language::Python,
        );
        assert!(f.imports.iter().any(|i| i.specifier == "a.b"));
        assert!(f.imports.iter().any(|i| i.specifier == "os"));
        assert!(f
            .symbols
            .iter()
            .any(|s| s.name == "used" && s.kind == SymbolKind::Function));
        assert!(f.symbols.iter().any(|s| s.name == "used" && s.exported));
        assert!(f
            .symbols
            .iter()
            .any(|s| s.name == "_private" && !s.exported));
    }

    #[test]
    fn py_all_can_promote_private_api() {
        let f = facts(
            "__all__ = ['_plugin_hook']\n\ndef _plugin_hook():\n    pass\n",
            Language::Python,
        );
        assert!(f
            .symbols
            .iter()
            .any(|s| s.name == "_plugin_hook" && s.exported));
    }

    #[test]
    fn py_decorator_marker() {
        let f = facts(
            "import app\n\n@app.route('/x')\ndef handler():\n    pass\n",
            Language::Python,
        );
        assert!(f
            .markers
            .iter()
            .any(|m| matches!(m, Marker::Decorator { dotted, .. } if dotted == "app.route")));
    }

    #[test]
    fn suppression_requires_rule() {
        assert!(parse_suppression("// noslop-ignore-file unused-file -- dynamic", 3).is_some());
        assert!(parse_suppression("// noslop-ignore-file", 3).is_none());
    }

    fn names_of(f: &FileFacts, kind: SymbolKind) -> Vec<&str> {
        f.symbols
            .iter()
            .filter(|s| s.kind == kind)
            .map(|s| s.name.as_str())
            .collect()
    }

    #[test]
    fn ts_enum_members_and_access() {
        let f = facts(
            "export enum Color { Red, Green, Blue }\nconst x = Color.Red;\n",
            Language::TypeScript,
        );
        let mut members = names_of(&f, SymbolKind::EnumMember);
        members.sort();
        assert_eq!(members, vec!["Blue", "Green", "Red"]);
        // The enum itself is still a Type (feeds unused-type), and `Red` is an access.
        assert!(f
            .symbols
            .iter()
            .any(|s| s.name == "Color" && s.kind == SymbolKind::Type));
        assert!(f.member_accesses.iter().any(|m| m == "Red"));
    }

    #[test]
    fn ts_private_members_only() {
        let f = facts(
            "class W {\n  #secret = 1;\n  private hidden() { return this.#secret; }\n  public shown() {}\n}\n",
            Language::Tsx,
        );
        let mut priv_members: Vec<&str> = f
            .symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Method | SymbolKind::Field))
            .map(|s| s.name.as_str())
            .collect();
        priv_members.sort();
        assert_eq!(priv_members, vec!["hidden", "secret"]); // `shown` (public) excluded
        assert!(f.member_accesses.iter().any(|m| m == "secret"));
    }

    #[test]
    fn ts_trailing_unused_param() {
        let f = facts(
            "function f(a: number, b: number) { return a; }\n",
            Language::TypeScript,
        );
        let params = names_of(&f, SymbolKind::Parameter);
        assert_eq!(params, vec!["b"]); // `a` used, `b` trailing-unused
    }

    #[test]
    fn ts_used_and_underscore_params_are_spared() {
        let f = facts(
            "function f(a: number, _ignored: number) { return a; }\n",
            Language::TypeScript,
        );
        assert!(names_of(&f, SymbolKind::Parameter).is_empty());
    }

    #[test]
    fn ts_constructor_parameter_properties_are_not_unused_params() {
        let f = facts(
            "class C {\n  constructor(private readonly admin: AdminService, readonly cache: Cache) {}\n}\n",
            Language::TypeScript,
        );
        assert!(names_of(&f, SymbolKind::Parameter).is_empty());
    }

    #[test]
    fn py_private_methods_and_params() {
        let f = facts(
            "def compute(used, dead):\n    return used\n\nclass S:\n    def run(self):\n        return self._helper()\n    def _helper(self):\n        return 1\n    def __init__(self):\n        pass\n",
            Language::Python,
        );
        // Module-level function: `dead` is a trailing unused param.
        assert_eq!(names_of(&f, SymbolKind::Parameter), vec!["dead"]);
        // Private method emitted; dunder and public method are not.
        let methods = names_of(&f, SymbolKind::Method);
        assert!(methods.contains(&"_helper"));
        assert!(!methods.contains(&"__init__"));
        assert!(!methods.contains(&"run"));
        // `self._helper()` is a recorded attribute access.
        assert!(f.member_accesses.iter().any(|m| m == "_helper"));
    }

    #[test]
    fn py_decorated_private_methods_are_framework_hooks() {
        let f = facts(
            "class Settings:\n    @field_validator('path')\n    @classmethod\n    def _resolve_path(cls, value):\n        return value\n",
            Language::Python,
        );
        assert!(names_of(&f, SymbolKind::Method).is_empty());
    }

    #[test]
    fn py_relative_import_specifier_not_doubled() {
        let f = facts("from .sub.mod import thing\n", Language::Python);
        assert_eq!(f.imports[0].specifier, ".sub.mod");
    }

    fn func<'a>(f: &'a FileFacts, name: &str) -> &'a noslop_graph::FunctionMetrics {
        f.functions
            .iter()
            .find(|fm| fm.name == name)
            .unwrap_or_else(|| panic!("no function {name} in {:?}", f.functions))
    }

    #[test]
    fn cx_trivial_function() {
        let f = facts(
            "function f(a: number) { return a + 1; }\n",
            Language::TypeScript,
        );
        let m = func(&f, "f");
        assert_eq!((m.cyclomatic, m.cognitive), (1, 0));
    }

    #[test]
    fn cx_if_else_and_boolean() {
        // if (+1 cyclo, +1 cog), else (+1 cog), `&&` (+1 cyclo, +1 cog).
        let f = facts(
            "function f(x: number) { if (x > 0 && x < 9) { return 1; } else { return 2; } }\n",
            Language::TypeScript,
        );
        let m = func(&f, "f");
        assert_eq!((m.cyclomatic, m.cognitive), (3, 3));
    }

    #[test]
    fn cx_nesting_penalty() {
        // Nested `if` inside `for` inside `if`: cognitive gets +1, +2, +3.
        let f = facts(
            "function f(x: number) { if (x) { for (;;) { if (x) { return 1; } } } }\n",
            Language::TypeScript,
        );
        let m = func(&f, "f");
        assert_eq!(m.cyclomatic, 4); // 1 base + if + for + if
        assert_eq!(m.cognitive, 1 + 2 + 3); // 6
    }

    #[test]
    fn cx_python_elif_flat() {
        // elif adds cyclomatic + cognitive(1, no nesting); each method scored alone.
        let f = facts(
            "def f(x):\n    if x:\n        return 1\n    elif x > 2:\n        return 2\n    else:\n        return 3\n",
            Language::Python,
        );
        let m = func(&f, "f");
        assert_eq!((m.cyclomatic, m.cognitive), (3, 3)); // if(1,1) elif(1,1) else(0,1)
    }

    #[test]
    fn cx_methods_attributed_to_class() {
        let f = facts(
            "class W { run(x: number) { if (x) { return 1; } return 0; } }\n",
            Language::Tsx,
        );
        let m = func(&f, "run");
        assert_eq!(m.parent.as_deref(), Some("W"));
        assert_eq!((m.cyclomatic, m.cognitive), (2, 1));
    }
}
