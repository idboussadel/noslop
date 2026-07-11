//! CSS extractor — custom-property declarations, `var()` references, and class
//! selectors, for the styling-liveness passes. Syntactic only (Slice 1); typed
//! value analysis (token drift) is a later slice that would want a value-grade
//! parser.

use crate::Acc;
use noslop_graph::{CssName, Span};
use tree_sitter::Node;

pub(crate) fn walk(root: Node, src: &[u8], acc: &mut Acc) {
    visit(root, src, acc);
}

fn visit(node: Node, src: &[u8], acc: &mut Acc) {
    match node.kind() {
        "declaration" => {
            if let Some(name) = custom_property(node, src) {
                acc.add_declared_token(CssName {
                    name,
                    span: span(node),
                });
            }
        }
        // `var(--brand)` — a call to `var` with a `--`-prefixed first argument.
        "call_expression" => {
            if let Some(name) = var_reference(node, src) {
                acc.add_var_ref(name);
            }
        }
        "class_selector" => {
            if let Some(name) = class_name(node, src) {
                acc.add_class_selector(CssName {
                    name,
                    span: span(node),
                });
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, src, acc);
    }
}

/// The declared name of a `--custom-property: …` declaration, else `None`.
fn custom_property(node: Node, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    let first = node.children(&mut cursor).next()?;
    let text = text(first, src);
    text.starts_with("--").then(|| text.to_string())
}

/// The `--name` referenced by a `var(--name)` call. tree-sitter-css exposes the
/// callee as a `function_name` child and the args as an `arguments` child (no
/// named fields), so we match by kind.
fn var_reference(node: Node, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let is_var = children
        .iter()
        .any(|c| c.kind() == "function_name" && text(*c, src) == "var");
    if !is_var {
        return None;
    }
    let args = children.iter().find(|c| c.kind() == "arguments")?;
    let mut acursor = args.walk();
    for a in args.children(&mut acursor) {
        let t = text(a, src);
        if t.starts_with("--") {
            return Some(t.to_string());
        }
    }
    None
}

/// The class name of a `.card` selector (without the dot).
fn class_name(node: Node, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if c.kind() == "class_name" || c.kind() == "identifier" {
            return Some(text(c, src).to_string());
        }
    }
    None
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

fn span(node: Node) -> Span {
    Span::from_rows(node.start_position().row, node.end_position().row)
}
