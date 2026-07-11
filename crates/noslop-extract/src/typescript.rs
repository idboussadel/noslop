//! TypeScript / JavaScript / TSX extractor.
//!
//! Handles the import/export forms that matter for a reference graph: ESM
//! import/export in all shapes, `export * from` re-export chains (modeled as
//! namespace imports so liveness propagates), CommonJS `require`, dynamic
//! `import()` (literal → resolvable, expression → confidence-capping marker),
//! and JSX element names as references. In-file scoping is approximate by
//! design (ARCHITECTURE.md §6): every identifier occurrence counts as a use.

use crate::{parse_annotation, parse_suppression, plain_name, Acc};
use noslop_graph::{
    ExportKind, ImportKind, ImportedName, Marker, RawExport, RawImport, RawSymbol, Span, SymbolKind,
};
use std::collections::HashSet;
use tree_sitter::Node;

pub(crate) fn walk(root: Node, src: &[u8], acc: &mut Acc) {
    visit(root, src, acc, false);
}

/// Recursive descent. `in_import` suppresses reference collection inside an
/// import statement so that imported names are not counted as their own uses
/// (which would make `unused-import` impossible to detect).
fn visit(node: Node, src: &[u8], acc: &mut Acc, in_import: bool) {
    match node.kind() {
        "import_statement" => {
            parse_import(node, src, acc);
            // Do not recurse: names here are bindings, not references.
            return;
        }
        "export_statement" => {
            parse_export(node, src, acc);
            // Fall through to recurse so `export default expr` / `export const x
            // = rhs` reference collection still happens.
        }
        "call_expression" => parse_call(node, src, acc),
        "function_declaration" | "generator_function_declaration" => {
            declare(node, src, acc, SymbolKind::Function);
            collect_params(node, src, acc);
        }
        "class_declaration" | "abstract_class_declaration" => {
            declare(node, src, acc, SymbolKind::Class);
            collect_class_members(node, src, acc);
        }
        // Method params (the method symbol itself is handled by the class scan).
        "method_definition" => collect_params(node, src, acc),
        "interface_declaration" | "type_alias_declaration" => {
            declare(node, src, acc, SymbolKind::Type);
        }
        "enum_declaration" => {
            declare(node, src, acc, SymbolKind::Type);
            collect_enum_members(node, src, acc);
        }
        "lexical_declaration" | "variable_declaration" => {
            declare_variables(node, src, acc);
        }
        // `obj.prop` / `this.#priv` — record the accessed property so cross-file
        // member liveness can see it (declaration sites are not member expressions).
        "member_expression" => {
            if let Some(prop) = node.child_by_field_name("property") {
                acc.add_member_access(text(prop, src));
            }
        }
        "identifier" | "type_identifier" => {
            if !in_import {
                acc.add_ref(text(node, src));
            }
        }
        // `{ x }` — a shorthand read that is also a destructuring member access.
        "shorthand_property_identifier" => {
            if !in_import {
                acc.add_ref(text(node, src));
                acc.add_member_access(text(node, src));
            }
        }
        // JSX `<Foo />` and `<Foo.Bar />` are references to the component.
        "jsx_opening_element" | "jsx_self_closing_element" => {
            if let Some(name) = node.child_by_field_name("name") {
                acc.add_ref(&root_identifier(name, src));
            }
        }
        // `className="card lg"` / `class="card"` — record class usages for CSS
        // liveness (whitespace-separated, string-literal values only).
        "jsx_attribute" => collect_class_attribute(node, src, acc),
        "string_fragment" => acc.add_string(text(node, src).to_string()),
        "comment" => collect_suppression(node, src, acc),
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, src, acc, in_import);
    }
}

fn parse_import(node: Node, src: &[u8], acc: &mut Acc) {
    let Some(specifier) = string_field(node, "source", src) else {
        return;
    };
    let is_type_only = text(node, src).trim_start().starts_with("import type");
    let mut names = Vec::new();
    let mut is_namespace = false;

    if let Some(clause) = child_of_kind(node, "import_clause") {
        let mut cursor = clause.walk();
        for child in clause.children(&mut cursor) {
            match child.kind() {
                // Default import: `import Foo from '...'`.
                "identifier" => names.push(ImportedName {
                    imported: "default".to_string(),
                    local: text(child, src).to_string(),
                }),
                "namespace_import" => is_namespace = true,
                "named_imports" => collect_named_imports(child, src, &mut names),
                _ => {}
            }
        }
    }

    acc.add_import(RawImport {
        specifier,
        names,
        kind: ImportKind::Static,
        is_namespace,
        is_type_only,
        is_reexport: false,
        span: span(node),
    });
}

fn collect_named_imports(node: Node, src: &[u8], names: &mut Vec<ImportedName>) {
    let mut cursor = node.walk();
    for spec in node.children(&mut cursor) {
        if spec.kind() != "import_specifier" {
            continue;
        }
        let imported = spec
            .child_by_field_name("name")
            .map(|n| text(n, src).to_string());
        let alias = spec
            .child_by_field_name("alias")
            .map(|n| text(n, src).to_string());
        if let Some(imported) = imported {
            names.push(ImportedName {
                local: alias.unwrap_or_else(|| imported.clone()),
                imported,
            });
        }
    }
}

fn parse_export(node: Node, src: &[u8], acc: &mut Acc) {
    // Re-export: `export ... from 'source'` — model as an import so the target
    // file stays reachable and its used names are recorded.
    if let Some(specifier) = string_field(node, "source", src) {
        if let Some(clause) = child_of_kind(node, "export_clause") {
            let mut names = Vec::new();
            collect_export_specifiers(clause, src, &mut names);
            acc.add_import(RawImport {
                specifier,
                names,
                kind: ImportKind::Static,
                is_namespace: false,
                is_type_only: false,
                is_reexport: true,
                span: span(node),
            });
        } else {
            // `export * from` / `export * as ns from` — whole namespace kept live.
            acc.add_import(RawImport {
                specifier,
                names: Vec::new(),
                kind: ImportKind::Static,
                is_namespace: true,
                is_type_only: false,
                is_reexport: true,
                span: span(node),
            });
        }
        return;
    }

    // `export { a, b as c }` (local re-export of local names).
    if let Some(clause) = child_of_kind(node, "export_clause") {
        let mut names = Vec::new();
        collect_export_specifiers(clause, src, &mut names);
        for n in names {
            acc.add_export(RawExport {
                name: n.local,
                kind: ExportKind::Named,
                source: None,
                is_type_only: false,
                span: span(node),
            });
        }
        return;
    }

    // `export default ...`.
    if text(node, src).trim_start().starts_with("export default") {
        acc.add_export(RawExport {
            name: "default".to_string(),
            kind: ExportKind::Default,
            source: None,
            is_type_only: false,
            span: span(node),
        });
    }
    // `export const/function/class/...` declarations are handled when the walk
    // descends into the declaration node; the name lands in `exported_names`
    // via `add_export` below.
    if let Some(decl) = node.child_by_field_name("declaration") {
        for name in declared_names(decl, src) {
            acc.add_export(RawExport {
                name,
                kind: ExportKind::Named,
                source: None,
                is_type_only: matches!(
                    decl.kind(),
                    "interface_declaration" | "type_alias_declaration"
                ),
                span: span(decl),
            });
        }
    }
}

fn collect_export_specifiers(clause: Node, src: &[u8], names: &mut Vec<ImportedName>) {
    let mut cursor = clause.walk();
    for spec in clause.children(&mut cursor) {
        if spec.kind() != "export_specifier" {
            continue;
        }
        let name = spec
            .child_by_field_name("name")
            .map(|n| text(n, src).to_string());
        let alias = spec
            .child_by_field_name("alias")
            .map(|n| text(n, src).to_string());
        if let Some(name) = name {
            names.push(ImportedName {
                local: alias.unwrap_or_else(|| name.clone()),
                imported: name,
            });
        }
    }
}

fn parse_call(node: Node, src: &[u8], acc: &mut Acc) {
    let Some(func) = node.child_by_field_name("function") else {
        return;
    };
    // Record the callee path for policy (`banned-call`/`banned-effect`).
    acc.add_call(member_dotted(func, src), span(node));

    let is_require = func.kind() == "identifier" && text(func, src) == "require";
    let is_dynamic_import = func.kind() == "import";
    if !is_require && !is_dynamic_import {
        return;
    }
    let arg = node
        .child_by_field_name("arguments")
        .and_then(|a| a.named_child(0));
    match arg {
        Some(a) if a.kind() == "string" => {
            if let Some(spec) = string_text(a, src) {
                acc.add_import(RawImport {
                    specifier: spec,
                    names: vec![plain_name("default")],
                    kind: if is_require {
                        ImportKind::Require
                    } else {
                        ImportKind::Dynamic
                    },
                    is_namespace: true,
                    is_type_only: false,
                    is_reexport: false,
                    span: span(node),
                });
            }
        }
        // Non-literal specifier: unresolvable, caps confidence for the package.
        _ if is_dynamic_import => acc.add_marker(Marker::UnresolvableDynamicImport),
        _ => {}
    }
}

/// Record a named declaration as a symbol.
fn declare(node: Node, src: &[u8], acc: &mut Acc, kind: SymbolKind) {
    let exported = is_exported(node);
    if let Some(name) = node.child_by_field_name("name") {
        acc.add_symbol(RawSymbol {
            name: text(name, src).to_string(),
            kind,
            span: span(node),
            exported,
            is_type_only: matches!(kind, SymbolKind::Type),
            parent: None,
            visibility: None,
        });
    }
}

/// Record `const a = ..., b = ...` declarators as variable symbols.
fn declare_variables(node: Node, src: &[u8], acc: &mut Acc) {
    let exported = is_exported(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "variable_declarator" {
            continue;
        }
        if let Some(name) = child.child_by_field_name("name") {
            if name.kind() == "identifier" {
                acc.add_symbol(RawSymbol {
                    name: text(name, src).to_string(),
                    kind: SymbolKind::Variable,
                    span: span(node),
                    exported,
                    is_type_only: false,
                    parent: None,
                    visibility: None,
                });
            }
        }
    }
}

/// Names introduced by a declaration under `export`.
fn declared_names(decl: Node, src: &[u8]) -> Vec<String> {
    if let Some(name) = decl.child_by_field_name("name") {
        return vec![text(name, src).to_string()];
    }
    let mut out = Vec::new();
    let mut cursor = decl.walk();
    for child in decl.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(name) = child.child_by_field_name("name") {
                if name.kind() == "identifier" {
                    out.push(text(name, src).to_string());
                }
            }
        }
    }
    out
}

fn collect_suppression(node: Node, src: &[u8], acc: &mut Acc) {
    let comment = text(node, src);
    if let Some(s) = parse_suppression(comment, node.start_position().row as u32 + 1) {
        acc.add_suppression(s);
    }
    // An annotation applies to the declaration on the line after the comment's
    // last line (a JSDoc block spans several rows).
    if let Some(a) = parse_annotation(comment, node.end_position().row as u32 + 2) {
        acc.add_annotation(a);
    }
}

// ── symbol-level (member / parameter) extraction ─────────────────────────────

/// Emit `unused-parameter` candidates for a named function/method. Follows
/// TypeScript's `noUnusedParameters` rule — only *trailing* unused params are
/// reported, since a param before a used one cannot be removed. `_`-prefixed
/// params are treated as intentionally unused. Anonymous arrows are skipped
/// (callback params are unused by design too often to flag safely).
fn collect_params(fn_node: Node, src: &[u8], acc: &mut Acc) {
    let Some(params) = fn_node.child_by_field_name("parameters") else {
        return;
    };
    // No body → an overload signature or abstract/ambient method; nothing to check.
    let Some(body) = fn_node.child_by_field_name("body") else {
        return;
    };
    let parent = fn_node
        .child_by_field_name("name")
        .map(|n| text(n, src).to_string());

    // Ordered params; a non-identifier pattern (destructuring/rest) is a barrier.
    let mut ordered: Vec<(String, Span)> = Vec::new();
    let mut cursor = params.walk();
    for p in params.children(&mut cursor) {
        if !matches!(p.kind(), "required_parameter" | "optional_parameter") {
            continue;
        }
        if is_parameter_property(p, src) {
            continue;
        }
        match p.child_by_field_name("pattern") {
            Some(pat) if pat.kind() == "identifier" => {
                ordered.push((text(pat, src).to_string(), span(p)));
            }
            _ => ordered.push((String::new(), span(p))), // barrier
        }
    }
    if ordered.is_empty() {
        return;
    }

    let mut used = HashSet::new();
    collect_idents(body, src, &mut used);

    // Everything after the last used/barrier param is trailing-removable.
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

fn is_parameter_property(param: Node, src: &[u8]) -> bool {
    let mut cursor = param.walk();
    for child in param.children(&mut cursor) {
        if child.kind() == "accessibility_modifier" || text(child, src).trim() == "readonly" {
            return true;
        }
    }
    false
}

/// Emit private class members (`#field`, `private method()`). Liveness is decided
/// by the pass against the repo-wide member-access index; a `private`/`#` member
/// can only be reached from within its own class, so that check is safe.
fn collect_class_members(class_node: Node, src: &[u8], acc: &mut Acc) {
    let parent = class_node
        .child_by_field_name("name")
        .map(|n| text(n, src).to_string());
    let Some(body) = class_node.child_by_field_name("body") else {
        return;
    };

    let mut cursor = body.walk();
    for m in body.children(&mut cursor) {
        let (kind, name_node) = match m.kind() {
            "method_definition" => (SymbolKind::Method, m.child_by_field_name("name")),
            "public_field_definition" | "field_definition" => {
                (SymbolKind::Field, m.child_by_field_name("name"))
            }
            _ => continue,
        };
        let Some(nn) = name_node else { continue };
        if !is_private_member(m, nn, src) {
            continue;
        }
        let name = text(nn, src).trim_start_matches('#').to_string();
        if name == "constructor" || name.is_empty() {
            continue;
        }
        acc.add_symbol(RawSymbol {
            name,
            kind,
            span: span(m),
            exported: false,
            is_type_only: false,
            parent: parent.clone(),
            visibility: None,
        });
    }
}

/// Emit every member of a TS `enum` as an `EnumMember` symbol; the pass decides
/// liveness against the repo-wide reference index.
fn collect_enum_members(enum_node: Node, src: &[u8], acc: &mut Acc) {
    let parent = enum_node
        .child_by_field_name("name")
        .map(|n| text(n, src).to_string());
    let Some(body) = enum_node.child_by_field_name("body") else {
        return;
    };
    let mut cursor = body.walk();
    for m in body.children(&mut cursor) {
        let name_node = match m.kind() {
            "enum_assignment" => m.child_by_field_name("name"),
            "property_identifier" => Some(m),
            _ => None,
        };
        if let Some(nn) = name_node {
            acc.add_symbol(RawSymbol {
                name: text(nn, src).to_string(),
                kind: SymbolKind::EnumMember,
                span: span(m),
                exported: false,
                is_type_only: false,
                parent: parent.clone(),
                visibility: None,
            });
        }
    }
}

/// A member is private if it uses `#` hard-privacy or a `private` modifier.
fn is_private_member(member: Node, name_node: Node, src: &[u8]) -> bool {
    if name_node.kind() == "private_property_identifier" {
        return true;
    }
    let mut cursor = member.walk();
    for c in member.children(&mut cursor) {
        if c.kind() == "accessibility_modifier" && text(c, src) == "private" {
            return true;
        }
    }
    false
}

/// Collect bare identifier uses (for parameter liveness).
fn collect_idents(node: Node, src: &[u8], out: &mut HashSet<String>) {
    if matches!(node.kind(), "identifier" | "shorthand_property_identifier") {
        out.insert(text(node, src).to_string());
    }
    let mut cursor = node.walk();
    for ch in node.children(&mut cursor) {
        collect_idents(ch, src, out);
    }
}

// ── small node helpers ───────────────────────────────────────────────────────

fn is_exported(node: Node) -> bool {
    node.parent()
        .map(|p| p.kind() == "export_statement")
        .unwrap_or(false)
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

fn span(node: Node) -> Span {
    Span::from_rows(node.start_position().row, node.end_position().row)
}

fn child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).find(|c| c.kind() == kind);
    found
}

/// The value of a string-valued field, quotes stripped.
fn string_field(node: Node, field: &str, src: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|n| string_text(n, src))
}

/// Strip the surrounding quotes from a `string` node.
fn string_text(node: Node, src: &[u8]) -> Option<String> {
    let raw = text(node, src);
    let trimmed = raw
        .strip_prefix(['"', '\'', '`'])
        .and_then(|s| s.strip_suffix(['"', '\'', '`']))
        .unwrap_or(raw);
    Some(trimmed.to_string())
}

/// Record class names from a `className`/`class` JSX attribute with a string
/// literal value. Dynamic values (`className={cx(...)}`) are left alone.
fn collect_class_attribute(node: Node, src: &[u8], acc: &mut Acc) {
    let Some(name) = node.child(0) else { return };
    if !matches!(text(name, src), "className" | "class") {
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string" {
            if let Some(value) = string_text(child, src) {
                for class in value.split_whitespace() {
                    acc.add_class_ref(class);
                }
            }
        }
    }
}

/// The dotted callee path of a call target: `foo` → `foo`,
/// `child_process.exec` → `child_process.exec`, `a.b.c` → `a.b.c`. Computed
/// callees (`obj[x]()`) and other shapes yield an empty string (recorded as
/// nothing), because a garbage path is worse than a missing one.
fn member_dotted(node: Node, src: &[u8]) -> String {
    match node.kind() {
        "identifier" | "this" | "super" | "property_identifier" => text(node, src).to_string(),
        "member_expression" => {
            let object = node.child_by_field_name("object");
            let property = node.child_by_field_name("property");
            match (object, property) {
                (Some(o), Some(p)) if p.kind() == "property_identifier" => {
                    let base = member_dotted(o, src);
                    if base.is_empty() {
                        String::new()
                    } else {
                        format!("{base}.{}", text(p, src))
                    }
                }
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

/// For `Foo` or `Foo.Bar` in JSX, return the leading identifier `Foo`.
fn root_identifier(node: Node, src: &[u8]) -> String {
    let mut current = node;
    while current.kind() == "member_expression" || current.kind() == "jsx_member_expression" {
        match current.child_by_field_name("object") {
            Some(obj) => current = obj,
            None => break,
        }
    }
    text(current, src).to_string()
}
