//! Indented-tree renderer for terminal graph output. Each node is expanded once;
//! later references show `↑` instead of re-expanding shared dependencies.

use crate::layout::Layout;
use crate::model::PackageGraph;
use crate::theme::{paint, CellStyle};
use crate::RenderOptions;
use std::fmt::Write;

pub fn render(g: &PackageGraph, layout: &Layout, opts: &RenderOptions) -> String {
    if g.nodes.is_empty() {
        return String::new();
    }
    let children = sorted_children(g, layout);

    // Roots: rank-0 nodes (nothing forward-points at them), stable order.
    let mut roots: Vec<usize> = (0..g.nodes.len()).filter(|&n| layout.rank[n] == 0).collect();
    roots.sort_by(|&a, &b| g.nodes[a].label.cmp(&g.nodes[b].label));
    if roots.is_empty() {
        roots.push(0); // fully-cyclic graph: start somewhere deterministic
    }

    let mut out = String::new();
    let mut expanded = vec![false; g.nodes.len()];
    let (glyphs, seen_mark) = glyph_set(opts.ascii);

    for (i, &root) in roots.iter().enumerate() {
        let last = i == roots.len() - 1;
        walk(
            g, &children, root, "", last, true, &mut expanded, &mut out, opts, glyphs, seen_mark,
        );
    }
    out
}

type Glyphs = [&'static str; 4]; // [tee, elbow, pipe, blank]

#[allow(clippy::too_many_arguments)]
fn walk(
    g: &PackageGraph,
    children: &[Vec<usize>],
    node: usize,
    prefix: &str,
    is_last: bool,
    is_root: bool,
    expanded: &mut [bool],
    out: &mut String,
    opts: &RenderOptions,
    glyphs: Glyphs,
    seen_mark: (&str, &str),
) {
    let [tee, elbow, pipe, blank] = glyphs;
    let cycle = g.in_cycle(node);
    let style = if cycle { CellStyle::Cycle } else { CellStyle::Node };

    let connector = if is_root {
        ""
    } else if is_last {
        elbow
    } else {
        tee
    };

    let first_time = !expanded[node];
    let n = &g.nodes[node];
    let label = paint(&n.label, style, opts.color);
    let count = paint(&format!("({} files)", n.files), CellStyle::Edge, opts.color);
    let mut suffix = String::new();
    if cycle {
        suffix.push_str(seen_mark.1);
    }
    if !first_time && !children[node].is_empty() {
        suffix.push_str(seen_mark.0); // already expanded elsewhere
    }
    let _ = writeln!(
        out,
        "{}{}{} {}{}",
        paint(prefix, CellStyle::Edge, opts.color),
        paint(connector, CellStyle::Edge, opts.color),
        label,
        count,
        suffix
    );

    if !first_time {
        return; // don't re-expand shared/visited subtrees
    }
    expanded[node] = true;

    let kids = &children[node];
    let child_prefix = if is_root {
        String::new()
    } else {
        format!("{prefix}{}", if is_last { blank } else { pipe })
    };
    for (i, &child) in kids.iter().enumerate() {
        let last = i == kids.len() - 1;
        walk(
            g,
            children,
            child,
            &child_prefix,
            last,
            false,
            expanded,
            out,
            opts,
            glyphs,
            seen_mark,
        );
    }
}

/// Forward-edge children only (so cycles don't recurse), stable by label.
fn sorted_children(g: &PackageGraph, layout: &Layout) -> Vec<Vec<usize>> {
    let mut children = vec![Vec::new(); g.nodes.len()];
    for e in &g.edges {
        if !layout.back_edges.contains(&(e.from, e.to)) {
            children[e.from].push(e.to);
        }
    }
    for kids in &mut children {
        kids.sort_by(|&a, &b| g.nodes[a].label.cmp(&g.nodes[b].label));
        kids.dedup();
    }
    children
}

/// `(glyphs, (already-expanded marker, cycle marker))`.
fn glyph_set(ascii: bool) -> (Glyphs, (&'static str, &'static str)) {
    if ascii {
        (["+- ", "`- ", "|  ", "   "], (" ...", " (cycle)"))
    } else {
        (["├─ ", "└─ ", "│  ", "   "], (" ↑", " ↺"))
    }
}
