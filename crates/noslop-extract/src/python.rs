//! Python extractor.
//!
//! Handles `import x`, `from x import y (as z)`, relative imports
//! (`from ..a import b` — the leading dots are preserved in the specifier so the
//! resolver can walk up package levels), `__all__` public surfaces, decorators
//! (captured as full dotted names for plugin matching), and `TYPE_CHECKING`
//! guards. String literals that look like dotted module paths are emitted as
//! markers for framework plugins; underscore-prefixed names stay lower severity.

use crate::{parse_annotation, parse_suppression, Acc};
use noslop_graph::{
    ExportKind, ImportKind, ImportedName, Marker, RawExport, RawImport, RawSymbol, Span, SymbolKind,
};
use std::collections::HashSet;
use tree_sitter::Node;

pub(crate) fn walk(root: Node, src: &[u8], acc: &mut Acc) {
    visit(root, src, acc, 0);
}

/// `depth` tracks nesting: only module-level (`depth == 0`) defs/classes and
/// `__all__` assignments contribute to the public export surface.
fn visit(node: Node, src: &[u8], acc: &mut Acc, depth: usize) {
    match node.kind() {
        "import_statement" => {
            parse_import(node, src, acc);
            return;
        }
        "import_from_statement" => {
            parse_from_import(node, src, acc);
            return;
        }
        "function_definition" => {
            declare(node, src, acc, SymbolKind::Function, depth);
            if depth == 0 {
                collect_params(node, src, acc);
            }
        }
        "class_definition" => {
            declare(node, src, acc, SymbolKind::Class, depth);
            if depth == 0 {
                collect_class_members(node, src, acc);
            }
        }
        "decorator" => parse_decorator(node, src, acc),
        "assignment" => parse_assignment(node, src, acc, depth),
        // `obj.attr` / `self._x` — record the accessed attribute for cross-file
        // member liveness (this is safe for inheritance: a subclass in another
        // file accessing `self._x` still lands here).
        "attribute" => {
            if let Some(attr) = node.child_by_field_name("attribute") {
                acc.add_member_access(text(attr, src));
            }
        }
        "call" => {
            if let Some(func) = node.child_by_field_name("function") {
                acc.add_call(py_dotted(func, src), span(node));
            }
        }
        "identifier" => acc.add_ref(text(node, src)),
        "string_content" => {
            let value = text(node, src).to_string();
            maybe_module_string(&value, acc);
            acc.add_string(value);
        }
        "comment" => {
            let comment = text(node, src);
            if let Some(s) = parse_suppression(comment, node.start_position().row as u32 + 1) {
                acc.add_suppression(s);
            }
            if let Some(a) = parse_annotation(comment, node.end_position().row as u32 + 2) {
                acc.add_annotation(a);
            }
        }
        _ => {}
    }

    // Nested-scope children descend at depth+1 for anything that opens a block.
    let child_depth = match node.kind() {
        "function_definition" | "class_definition" => depth + 1,
        _ => depth,
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, src, acc, child_depth);
    }
}

fn parse_import(node: Node, src: &[u8], acc: &mut Acc) {
    // `import a.b.c as d, e` — each dotted_name / aliased_import becomes a
    // namespace import of its top module.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let (module, alias) = match child.kind() {
            "dotted_name" | "identifier" => (text(child, src).to_string(), None),
            "aliased_import" => {
                let module = child
                    .child_by_field_name("name")
                    .map(|n| text(n, src).to_string());
                let alias = child
                    .child_by_field_name("alias")
                    .map(|n| text(n, src).to_string());
                match module {
                    Some(m) => (m, alias),
                    None => continue,
                }
            }
            _ => continue,
        };
        let local =
            alias.unwrap_or_else(|| module.split('.').next().unwrap_or(&module).to_string());
        acc.add_import(RawImport {
            specifier: module,
            names: vec![ImportedName {
                imported: "*".to_string(),
                local,
            }],
            kind: ImportKind::Static,
            is_namespace: true,
            is_type_only: false,
            is_reexport: false,
            span: span(node),
        });
    }
}

fn parse_from_import(node: Node, src: &[u8], acc: &mut Acc) {
    // The `module_name` field already carries the full specifier, including any
    // leading dots for a relative import (its node is a `relative_import` wrapping
    // the dotted name). Reading it directly avoids double-counting the prefix.
    let specifier = node
        .child_by_field_name("module_name")
        .map(|m| text(m, src).to_string())
        .unwrap_or_default();
    let is_type_only = false;

    let mut names = Vec::new();
    let mut is_wildcard = false;
    let mut cursor = node.walk();
    let mut past_import_kw = false;
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import" => past_import_kw = true,
            "wildcard_import" => is_wildcard = true,
            "dotted_name" | "identifier" if past_import_kw => {
                let name = text(child, src).to_string();
                names.push(ImportedName {
                    imported: name.clone(),
                    local: name,
                });
            }
            "aliased_import" if past_import_kw => {
                let imported = child
                    .child_by_field_name("name")
                    .map(|n| text(n, src).to_string());
                let alias = child
                    .child_by_field_name("alias")
                    .map(|n| text(n, src).to_string());
                if let Some(imported) = imported {
                    names.push(ImportedName {
                        local: alias.unwrap_or_else(|| imported.clone()),
                        imported,
                    });
                }
            }
            _ => {}
        }
    }

    acc.add_import(RawImport {
        specifier: if specifier.is_empty() {
            ".".to_string()
        } else {
            specifier
        },
        names,
        kind: ImportKind::Static,
        is_namespace: is_wildcard,
        is_type_only,
        is_reexport: false,
        span: span(node),
    });
}

fn declare(node: Node, src: &[u8], acc: &mut Acc, kind: SymbolKind, depth: usize) {
    if depth != 0 {
        return; // Only module-level defs/classes are part of the public surface.
    }
    if let Some(name) = node.child_by_field_name("name") {
        let name = text(name, src).to_string();
        // A module's top-level names are importable, but underscore-prefixed names
        // are conventionally private helpers. They are still symbols, just not
        // public API unless `__all__` explicitly promotes them in `Acc::finish`.
        acc.add_symbol(RawSymbol {
            exported: !is_private_name(&name),
            name,
            kind,
            span: span(node),
            is_type_only: false,
            parent: None,
            visibility: None,
        });
    }
}

/// Emit `unused-parameter` candidates for a module-level function. Skips `self`/
/// `cls`, `_`-prefixed, `*args`/`**kwargs`, and decorated functions (whose
/// signature a decorator may pin). Only trailing unused params are reported, so a
/// param before a used one — which callers may still pass positionally — is safe.
fn collect_params(fn_node: Node, src: &[u8], acc: &mut Acc) {
    if fn_node
        .parent()
        .map(|p| p.kind() == "decorated_definition")
        .unwrap_or(false)
    {
        return;
    }
    let Some(params) = fn_node.child_by_field_name("parameters") else {
        return;
    };
    let Some(body) = fn_node.child_by_field_name("body") else {
        return;
    };
    let parent = fn_node
        .child_by_field_name("name")
        .map(|n| text(n, src).to_string());

    let mut ordered: Vec<(String, Span)> = Vec::new();
    let mut cursor = params.walk();
    for p in params.children(&mut cursor) {
        match p.kind() {
            "identifier" => ordered.push((text(p, src).to_string(), span(p))),
            "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
                if let Some(name) = first_identifier(p, src) {
                    ordered.push((name, span(p)));
                }
            }
            // *args / **kwargs / `/` / `*` — a barrier: don't flag before them.
            "list_splat_pattern" | "dictionary_splat_pattern" => {
                ordered.push((String::new(), span(p)))
            }
            _ => {}
        }
    }
    // `self`/`cls` receivers are mandatory, never a finding.
    if matches!(ordered.first(), Some((n, _)) if n == "self" || n == "cls") {
        ordered.remove(0);
    }
    if ordered.is_empty() {
        return;
    }

    let mut used = HashSet::new();
    collect_idents(body, src, &mut used);
    let last_kept = ordered
        .iter()
        .rposition(|(n, _)| n.is_empty() || used.contains(n.as_str()));
    let start = last_kept.map(|i| i + 1).unwrap_or(0);
    for (name, sp) in &ordered[start..] {
        if name.is_empty() || name.starts_with('_') {
            continue;
        }
        acc.add_symbol(RawSymbol {
            name: name.clone(),
            kind: SymbolKind::Parameter,
            span: *sp,
            exported: false,
            is_type_only: false,
            parent: parent.clone(),
            visibility: None,
        });
    }
}

/// Emit underscore-private methods of a class (`_helper`, `__mangled`, but never
/// dunders like `__init__`). Liveness is decided by the pass against the repo-wide
/// attribute-access index, which is inheritance-safe.
fn collect_class_members(class_node: Node, src: &[u8], acc: &mut Acc) {
    let parent = class_node
        .child_by_field_name("name")
        .map(|n| text(n, src).to_string());
    let Some(body) = class_node.child_by_field_name("body") else {
        return;
    };
    let mut cursor = body.walk();
    for stmt in body.children(&mut cursor) {
        let (func, decorated) = match stmt.kind() {
            "function_definition" => (Some(stmt), false),
            "decorated_definition" => (
                stmt.child_by_field_name("definition")
                    .filter(|d| d.kind() == "function_definition"),
                true,
            ),
            _ => (None, false),
        };
        let Some(f) = func else { continue };
        let Some(nn) = f.child_by_field_name("name") else {
            continue;
        };
        let name = text(nn, src).to_string();
        if !is_private_name(&name) || decorated {
            continue;
        }
        acc.add_symbol(RawSymbol {
            name,
            kind: SymbolKind::Method,
            span: span(f),
            exported: false,
            is_type_only: false,
            parent: parent.clone(),
            visibility: None,
        });
    }
}

/// Underscore-private but not a dunder (`__init__`, `__str__`, …).
fn is_private_name(name: &str) -> bool {
    name.starts_with('_') && !(name.starts_with("__") && name.ends_with("__"))
}

/// The first direct `identifier` child (the bound name of a typed/default param).
fn first_identifier(node: Node, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if c.kind() == "identifier" {
            return Some(text(c, src).to_string());
        }
    }
    None
}

/// Bare identifier uses within a subtree (for parameter liveness).
fn collect_idents(node: Node, src: &[u8], out: &mut HashSet<String>) {
    if node.kind() == "identifier" {
        out.insert(text(node, src).to_string());
    }
    let mut cursor = node.walk();
    for ch in node.children(&mut cursor) {
        collect_idents(ch, src, out);
    }
}

fn parse_decorator(node: Node, src: &[u8], acc: &mut Acc) {
    // The decorated symbol is the def/class immediately following.
    let on_symbol = node
        .next_named_sibling()
        .and_then(|s| s.child_by_field_name("name"))
        .map(|n| text(n, src).to_string())
        .unwrap_or_default();
    // Full dotted decorator name, stripping any call arguments: `app.route(...)`.
    let dotted = decorator_dotted(node, src);
    if !dotted.is_empty() {
        acc.add_marker(Marker::Decorator { dotted, on_symbol });
    }
}

fn decorator_dotted(node: Node, src: &[u8]) -> String {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" | "dotted_name" | "attribute" => return text(child, src).to_string(),
            "call" => {
                if let Some(func) = child.child_by_field_name("function") {
                    return text(func, src).to_string();
                }
            }
            _ => {}
        }
    }
    String::new()
}

fn parse_assignment(node: Node, src: &[u8], acc: &mut Acc, depth: usize) {
    if depth != 0 {
        return;
    }
    let Some(left) = node.child_by_field_name("left") else {
        return;
    };
    if text(left, src) != "__all__" {
        return;
    }
    // `__all__ = ["a", "b"]` defines the public re-export surface. Record each as
    // an export so unused-export respects the declared public API.
    let Some(right) = node.child_by_field_name("right") else {
        return;
    };
    let mut cursor = right.walk();
    for child in right.children(&mut cursor) {
        if child.kind() == "string" {
            if let Some(name) = inner_string(child, src) {
                acc.add_export(RawExport {
                    name,
                    kind: ExportKind::Named,
                    source: None,
                    is_type_only: false,
                    span: span(node),
                });
            }
        }
    }
}

/// Emit a marker when a string literal looks like a dotted module path, so
/// framework plugins (Django settings, Celery task names) can turn it into an
/// edge. Cheap heuristic; false markers are harmless (plugins decide).
fn maybe_module_string(value: &str, acc: &mut Acc) {
    let looks_dotted = value.contains('.')
        && value
            .split('.')
            .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_alphanumeric() || c == '_'));
    if looks_dotted {
        acc.add_marker(Marker::ModuleStringLiteral {
            value: value.to_string(),
        });
    }
}

/// Dotted callee path: `run` → `run`, `subprocess.run` → `subprocess.run`,
/// `os.path.join` → `os.path.join`. Non-identifier bases yield an empty string.
fn py_dotted(node: Node, src: &[u8]) -> String {
    match node.kind() {
        "identifier" => text(node, src).to_string(),
        "attribute" => {
            let object = node.child_by_field_name("object");
            let attribute = node.child_by_field_name("attribute");
            match (object, attribute) {
                (Some(o), Some(a)) => {
                    let base = py_dotted(o, src);
                    if base.is_empty() {
                        String::new()
                    } else {
                        format!("{base}.{}", text(a, src))
                    }
                }
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

fn span(node: Node) -> Span {
    Span::from_rows(node.start_position().row, node.end_position().row)
}

fn inner_string(node: Node, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string_content" {
            return Some(text(child, src).to_string());
        }
    }
    None
}
