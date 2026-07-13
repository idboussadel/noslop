//! `noslop-viz` — pure, deterministic renderers for noslop's graph views.
//!
//! Terminal output is always an indented tree. Use [`to_svg`] or [`to_html`] for
//! a vector graph.

mod export;
mod html;
mod layout;
mod model;
mod svg;
mod theme;
mod tree;

pub use export::to_json;
pub use html::to_html;
pub use svg::to_svg;
pub use model::{Edge, EdgeKind, Node, PackageGraph};

use layout::Layout;

/// Rendering knobs for the terminal tree view.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    /// Emit ANSI color (the CLI disables this for `NO_COLOR` / non-terminals).
    pub color: bool,
    /// Use ASCII-only glyphs (`+-|`) instead of Unicode box-drawing.
    pub ascii: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            color: false,
            ascii: false,
        }
    }
}

/// Render a package graph as an indented dependency tree.
pub fn render(g: &PackageGraph, opts: &RenderOptions) -> String {
    let layout = Layout::compute(g);
    tree::render(g, &layout, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PackageGraph {
        PackageGraph {
            nodes: vec![
                node("root", 1),
                node("api", 22),
                node("service", 31),
                node("db", 12),
            ],
            edges: vec![edge(0, 1), edge(1, 2), edge(2, 3)],
            cycles: vec![],
        }
    }

    fn node(label: &str, files: usize) -> Node {
        Node {
            id: label.into(),
            label: label.into(),
            files,
            package: label.into(),
        }
    }

    fn edge(from: usize, to: usize) -> Edge {
        Edge {
            from,
            to,
            kind: EdgeKind::Import,
            weight: 1,
        }
    }

    #[test]
    fn tree_is_deterministic() {
        let g = sample();
        let opts = RenderOptions {
            color: false,
            ..Default::default()
        };
        let a = render(&g, &opts);
        let b = render(&g, &opts);
        assert_eq!(a, b);
        assert!(a.contains("service"));
        assert!(a.contains("(31 files)"));
    }

    #[test]
    fn cycles_are_marked() {
        let mut g = sample();
        g.edges.push(edge(2, 1));
        g.cycles = vec![vec![1, 2]];
        let out = render(&g, &RenderOptions::default());
        assert!(out.contains('↺'));
    }

    #[test]
    fn svg_and_json_export_nodes() {
        let g = sample();
        let json = to_json(&g);
        let svg = to_svg(&g);
        for n in &g.nodes {
            assert!(json.contains(&n.label));
            assert!(svg.contains(&n.label));
        }
        assert!(svg.starts_with("<svg"));
    }
}
