//! Self-contained HTML page wrapping the SVG graph — open in any browser.

use crate::model::PackageGraph;
use crate::svg::to_svg;

pub fn to_html(g: &PackageGraph, title: &str) -> String {
    let svg = to_svg(g);
    let nodes = g.nodes.len();
    let edges = g.edges.len();
    let cyclic: usize = g.cycles.iter().map(|c| c.len()).sum();
    let cycle_note = if cyclic > 0 {
        format!(" · {cyclic} in cycle(s)")
    } else {
        String::new()
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>{title}</title>
  <style>
    :root {{ color-scheme: light dark; }}
    body {{
      margin: 0; padding: 2rem; font-family: ui-sans-serif, system-ui, sans-serif;
      background: #0b1220; color: #e2e8f0; line-height: 1.5;
    }}
    h1 {{ font-size: 1.25rem; font-weight: 600; margin: 0 0 .25rem; }}
    .meta {{ color: #94a3b8; font-size: .875rem; margin-bottom: 1.5rem; }}
    .panel {{
      background: #111827; border: 1px solid #1e293b; border-radius: 12px;
      padding: 1.5rem; overflow: auto; max-width: 100%;
    }}
    svg {{ display: block; max-width: 100%; height: auto; }}
    .legend {{ margin-top: 1rem; font-size: .8125rem; color: #94a3b8; }}
    .legend span {{ margin-right: 1.25rem; }}
    .cycle {{ color: #fb7185; }}
  </style>
</head>
<body>
  <h1>{title}</h1>
  <p class="meta">{nodes} node(s) · {edges} edge(s){cycle_note}</p>
  <div class="panel">{svg}</div>
  <p class="legend">
    <span>── import</span>
    <span class="cycle">↺ cycle</span>
  </p>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;

    #[test]
    fn html_wraps_svg() {
        let g = crate::model::PackageGraph {
            nodes: vec![Node {
                id: "a".into(),
                label: "api".into(),
                files: 1,
                package: "a".into(),
            }],
            edges: vec![],
            cycles: vec![],
        };
        let html = to_html(&g, "test");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("api"));
    }
}
