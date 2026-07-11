//! Per-function complexity — cyclomatic (McCabe) + cognitive (SonarSource).
//!
//! A dedicated walk keeps this orthogonal to the reference/symbol extraction: it
//! maintains a stack of open functions and a per-function nesting depth, applying
//! increments to the innermost function. The increment tables differ per
//! language; the traversal is identical.
//!
//! Fidelity note: boolean-operator sequences are counted per operator (not
//! Sonar's "one per run of the same operator") and JS `else if` chains nest
//! rather than flatten — both are deterministic simplifications that over- rather
//! than under-count, documented so the numbers are explainable.

use noslop_graph::{FunctionMetrics, Language, Span};
use tree_sitter::Node;

/// One increment contributed by a node to the enclosing function.
#[derive(Default, Clone, Copy)]
struct Inc {
    cyclomatic: u32,
    cognitive: u32,
    /// Does this structure add a nesting penalty and deepen nesting for its body?
    nests: bool,
    /// Does this node open a new function scope?
    is_fn: bool,
    /// Does this node open a class scope (for method attribution)?
    is_class: bool,
}

/// Analyze a parsed tree, returning one [`FunctionMetrics`] per function.
pub(crate) fn analyze(root: Node, src: &[u8], lang: Language) -> Vec<FunctionMetrics> {
    let mut out = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    walk(root, src, lang, &mut stack, &mut out, 0, None);
    out
}

struct Frame {
    name: String,
    parent: Option<String>,
    span: Span,
    cyclomatic: u32,
    cognitive: u32,
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    src: &[u8],
    lang: Language,
    stack: &mut Vec<Frame>,
    out: &mut Vec<FunctionMetrics>,
    nesting: u32,
    class: Option<&str>,
) {
    let inc = classify(node, src, lang);

    if inc.is_fn {
        stack.push(Frame {
            name: fn_name(node, src),
            parent: class.map(String::from),
            span: span(node),
            cyclomatic: 1,
            cognitive: 0,
        });
        // A function body starts fresh at nesting 0.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, src, lang, stack, out, 0, class);
        }
        let f = stack.pop().expect("frame pushed above");
        out.push(FunctionMetrics {
            name: f.name,
            parent: f.parent,
            span: f.span,
            cyclomatic: f.cyclomatic,
            cognitive: f.cognitive,
            loc: f.span.end_line.saturating_sub(f.span.start_line) + 1,
        });
        return;
    }

    if let Some(frame) = stack.last_mut() {
        frame.cyclomatic += inc.cyclomatic;
        frame.cognitive += inc.cognitive + if inc.nests { nesting } else { 0 };
    }

    let child_nesting = if inc.nests && !stack.is_empty() {
        nesting + 1
    } else {
        nesting
    };
    let child_class = if inc.is_class {
        node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(src).ok())
            .or(class)
    } else {
        class
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, lang, stack, out, child_nesting, child_class);
    }
}

fn classify(node: Node, src: &[u8], lang: Language) -> Inc {
    match lang {
        Language::Python => classify_py(node),
        _ => classify_ts(node, src),
    }
}

fn classify_ts(node: Node, src: &[u8]) -> Inc {
    let branch = Inc {
        cyclomatic: 1,
        cognitive: 1,
        nests: true,
        ..Inc::default()
    };
    match node.kind() {
        "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "arrow_function"
        | "method_definition" => Inc {
            is_fn: true,
            ..Inc::default()
        },
        "class_declaration" | "abstract_class_declaration" => Inc {
            is_class: true,
            ..Inc::default()
        },
        "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
        | "do_statement" | "ternary_expression" | "catch_clause" => branch,
        // The switch itself nests once; each case adds a branch to cyclomatic.
        "switch_statement" => Inc {
            cyclomatic: 0,
            cognitive: 1,
            nests: true,
            ..Inc::default()
        },
        "switch_case" => Inc {
            cyclomatic: 1,
            ..Inc::default()
        },
        // `else` / `else if`: +1 cognitive, no nesting penalty.
        "else_clause" => Inc {
            cognitive: 1,
            ..Inc::default()
        },
        "binary_expression" if is_logical_op(node, src) => Inc {
            cyclomatic: 1,
            cognitive: 1,
            ..Inc::default()
        },
        _ => Inc::default(),
    }
}

fn classify_py(node: Node) -> Inc {
    let branch = Inc {
        cyclomatic: 1,
        cognitive: 1,
        nests: true,
        ..Inc::default()
    };
    match node.kind() {
        "function_definition" | "lambda" => Inc {
            is_fn: true,
            ..Inc::default()
        },
        "class_definition" => Inc {
            is_class: true,
            ..Inc::default()
        },
        "if_statement"
        | "for_statement"
        | "while_statement"
        | "except_clause"
        | "conditional_expression" => branch,
        "match_statement" => Inc {
            cognitive: 1,
            nests: true,
            ..Inc::default()
        },
        "case_clause" => Inc {
            cyclomatic: 1,
            ..Inc::default()
        },
        // `elif`: a full branch minus the nesting penalty. `else`: just +1.
        "elif_clause" => Inc {
            cyclomatic: 1,
            cognitive: 1,
            ..Inc::default()
        },
        "else_clause" => Inc {
            cognitive: 1,
            ..Inc::default()
        },
        "boolean_operator" => Inc {
            cyclomatic: 1,
            cognitive: 1,
            ..Inc::default()
        },
        _ => Inc::default(),
    }
}

fn is_logical_op(node: Node, src: &[u8]) -> bool {
    node.child_by_field_name("operator")
        .and_then(|n| n.utf8_text(src).ok())
        .map(|op| matches!(op, "&&" | "||" | "??"))
        .unwrap_or(false)
}

/// Best-effort function name: the `name` field, else the binding it is assigned
/// to (`const f = () => …`), else `(anonymous)`.
fn fn_name(node: Node, src: &[u8]) -> String {
    if let Some(name) = node.child_by_field_name("name") {
        return text(name, src);
    }
    if let Some(parent) = node.parent() {
        if matches!(parent.kind(), "variable_declarator" | "assignment" | "pair") {
            if let Some(name) = parent
                .child_by_field_name("name")
                .or_else(|| parent.child_by_field_name("left"))
            {
                return text(name, src);
            }
        }
    }
    "(anonymous)".to_string()
}

fn text(node: Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

fn span(node: Node) -> Span {
    Span::from_rows(node.start_position().row, node.end_position().row)
}
