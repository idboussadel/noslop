//! Duplication tokenizer — a flat leaf-token stream, normalized by lexical class.
//!
//! Operates on tree-sitter *leaves* (nodes with no children), so it is almost
//! entirely language-agnostic: only the identifier/number/string node-kind sets
//! differ, and those overlap across TS and Python. Comments are skipped; a string
//! is emitted as a single token rather than descending into its fragments.

use noslop_graph::{Language, Tok, TokKind};
use tree_sitter::Node;

const IDENT_KINDS: &[&str] = &[
    "identifier",
    "property_identifier",
    "type_identifier",
    "shorthand_property_identifier",
    "private_property_identifier",
    "field_identifier",
];
const NUM_KINDS: &[&str] = &["number", "integer", "float"];
const STRING_KINDS: &[&str] = &["string", "template_string", "concatenated_string"];
const STMT_END_KINDS: &[&str] = &[";", "}"];

/// Tokenize a parsed tree into the duplication stream.
pub fn tokenize(root: Node, src: &[u8], _lang: Language) -> Vec<Tok> {
    let mut out = Vec::new();
    walk(root, src, &mut out);
    out
}

fn walk(node: Node, src: &[u8], out: &mut Vec<Tok>) {
    let kind = node.kind();

    if node.kind() == "comment" {
        return; // comments never participate in duplication
    }
    // Strings are one token (don't descend into fragments/interpolations).
    if STRING_KINDS.contains(&kind) {
        push(node, src, TokKind::Str, out);
        return;
    }
    if node.child_count() == 0 {
        // A leaf: classify and emit (skip zero-width error/extra leaves).
        if node.byte_range().is_empty() {
            return;
        }
        let tk = if IDENT_KINDS.contains(&kind) {
            TokKind::Ident
        } else if NUM_KINDS.contains(&kind) {
            TokKind::Num
        } else {
            TokKind::Punct
        };
        push(node, src, tk, out);
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, out);
    }
}

fn push(node: Node, src: &[u8], kind: TokKind, out: &mut Vec<Tok>) {
    let text = node.utf8_text(src).unwrap_or("");
    out.push(Tok {
        hash: xxhash_rust::xxh3::xxh3_64(text.as_bytes()),
        kind,
        line: node.start_position().row as u32 + 1,
        stmt_end: STMT_END_KINDS.contains(&node.kind()),
    });
}
