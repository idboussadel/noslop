//! Standalone SVG export — a real vector graph from the same layout engine.
//!
//! ponytail: no Graphviz subprocess; coordinates come from [`crate::layout`].

use crate::layout::Layout;
use crate::model::PackageGraph;
use std::fmt::Write;

const BOX_H: f64 = 52.0;
const BOX_PAD: f64 = 14.0;
const H_GAP: f64 = 48.0;
const V_GAP: f64 = 56.0;
const ROW_STEP: f64 = BOX_H + V_GAP;
const MARGIN: f64 = 24.0;

pub fn to_svg(g: &PackageGraph) -> String {
    if g.nodes.is_empty() {
        return "<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_string();
    }
    let layout = Layout::compute(g);
    let inner = inner_width(g);
    let box_w = inner as f64 * 7.2 + BOX_PAD * 2.0;
    let col_step = box_w + H_GAP;
    let max_cols = layout.layers.iter().map(|l| l.len()).max().unwrap_or(0);
    let width = MARGIN * 2.0
        + if max_cols == 0 {
            0.0
        } else {
            (max_cols - 1) as f64 * col_step + box_w
        };
    let num_layers = layout.layers.len();
    let height = MARGIN * 2.0
        + if num_layers == 0 {
            0.0
        } else {
            (num_layers - 1) as f64 * ROW_STEP + BOX_H
        };

    let mut out = String::new();
    writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width:.0} {height:.0}\" \
         font-family=\"ui-sans-serif,system-ui,sans-serif\" font-size=\"13\">"
    )
    .unwrap();
    writeln!(
        out,
        "  <defs><marker id=\"arrow\" markerWidth=\"8\" markerHeight=\"8\" refX=\"7\" refY=\"3\" orient=\"auto\">\
         <path d=\"M0,0 L0,6 L7,3 z\" fill=\"#64748b\"/></marker>\
         <marker id=\"arrow-cycle\" markerWidth=\"8\" markerHeight=\"8\" refX=\"7\" refY=\"3\" orient=\"auto\">\
         <path d=\"M0,0 L0,6 L7,3 z\" fill=\"#e5484d\"/></marker></defs>"
    )
    .unwrap();

    for e in &g.edges {
        let is_back = layout.back_edges.contains(&(e.from, e.to));
        let (x0, y0) = node_anchor(&layout, e.from, col_step, box_w, true);
        let (x1, y1) = node_anchor(&layout, e.to, col_step, box_w, is_back);
        let (stroke, marker) = if g.in_cycle(e.from) && g.in_cycle(e.to) {
            ("#e5484d", "arrow-cycle")
        } else {
            ("#64748b", "arrow")
        };
        let path = if is_back {
            let lane = width - MARGIN;
            format!("M{x0:.1},{y0:.1} C{lane:.1},{y0:.1} {lane:.1},{y1:.1} {x1:.1},{y1:.1}")
        } else {
            let mid_y = (y0 + y1) / 2.0;
            format!("M{x0:.1},{y0:.1} C{x0:.1},{mid_y:.1} {x1:.1},{mid_y:.1} {x1:.1},{y1:.1}")
        };
        writeln!(
            out,
            "  <path d=\"{path}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" \
             marker-end=\"url(#{marker})\" opacity=\"0.9\"/>"
        )
        .unwrap();
        if e.weight > 1 {
            let lx = (x0 + x1) / 2.0;
            let ly = (y0 + y1) / 2.0 - 6.0;
            writeln!(
                out,
                "  <text x=\"{lx:.1}\" y=\"{ly:.1}\" text-anchor=\"middle\" fill=\"#94a3b8\" font-size=\"11\">×{}</text>",
                e.weight
            )
            .unwrap();
        }
    }

    for (i, n) in g.nodes.iter().enumerate() {
        let (x, y) = node_pos(&layout, i, col_step);
        let cycle = g.in_cycle(i);
        let fill = if cycle { "#fff1f2" } else { "#f8fafc" };
        let stroke = if cycle { "#e5484d" } else { "#cbd5e1" };
        writeln!(
            out,
            "  <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{box_w:.1}\" height=\"{BOX_H:.1}\" \
             rx=\"8\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1.5\"/>"
        )
        .unwrap();
        writeln!(
            out,
            "  <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-weight=\"600\" fill=\"#0f172a\">{}</text>",
            x + box_w / 2.0,
            y + 22.0,
            escape_xml(&n.label)
        )
        .unwrap();
        writeln!(
            out,
            "  <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" fill=\"#64748b\" font-size=\"11\">{} files</text>",
            x + box_w / 2.0,
            y + 40.0,
            n.files
        )
        .unwrap();
        if !n.package.is_empty() && n.id != n.package {
            writeln!(
                out,
                "  <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" fill=\"#94a3b8\" font-size=\"10\">{}</text>",
                x + box_w / 2.0,
                y + BOX_H - 6.0,
                escape_xml(&short_package(&n.package))
            )
            .unwrap();
        }
    }

    out.push_str("</svg>\n");
    out
}

fn node_pos(layout: &Layout, node: usize, col_step: f64) -> (f64, f64) {
    (
        MARGIN + layout.order[node] as f64 * col_step,
        MARGIN + layout.rank[node] as f64 * ROW_STEP,
    )
}

/// `from_bottom`: anchor on the box bottom (source); `to_bottom`: anchor on bottom vs top.
fn node_anchor(
    layout: &Layout,
    node: usize,
    col_step: f64,
    box_w: f64,
    from_bottom: bool,
) -> (f64, f64) {
    let (x, y) = node_pos(layout, node, col_step);
    let cy = if from_bottom { y + BOX_H } else { y };
    (x + box_w / 2.0, cy)
}

fn inner_width(g: &PackageGraph) -> usize {
    g.nodes
        .iter()
        .map(|n| {
            let files = format!("{} files", n.files).chars().count();
            n.label.chars().count().max(files)
        })
        .max()
        .unwrap_or(4)
        + 2
}

fn short_package(package: &str) -> String {
    package.rsplit('/').next().unwrap_or(package).to_string()
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, EdgeKind, Node};

    #[test]
    fn svg_contains_nodes() {
        let g = PackageGraph {
            nodes: vec![
                Node {
                    id: "a".into(),
                    label: "api".into(),
                    files: 3,
                    package: "a".into(),
                },
                Node {
                    id: "b".into(),
                    label: "db".into(),
                    files: 2,
                    package: "b".into(),
                },
            ],
            edges: vec![Edge {
                from: 0,
                to: 1,
                kind: EdgeKind::Import,
                weight: 2,
            }],
            cycles: vec![],
        };
        let svg = to_svg(&g);
        assert!(svg.contains("<svg"));
        assert!(svg.contains("api"));
        assert!(svg.contains("×2"));
    }
}
