//! The `noslop graph` command family — repository graph views.

use crate::Format;
use clap::Subcommand;
use noslop_graph::Graph;
use noslop_report::{build_import_graph, ImportGraphOptions, ScanRootReport};
use noslop_viz::RenderOptions;

#[derive(Subcommand)]
pub(crate) enum GraphKind {
    /// Package/workspace import graph.
    Packages(GraphArgs),
    /// Overview: depth guide, tree preview, export hints.
    Dashboard(DashboardArgs),
}

#[derive(clap::Args)]
pub(crate) struct GraphArgs {
    /// Directory depth under each package root. `0` = packages only; `1` = first
    /// folder (`app`, `src`); `2` = two levels (`src/lib`, `src/app`).
    #[arg(long, default_value_t = 0)]
    depth: usize,
    /// Cap the number of nodes (stable truncation).
    #[arg(long, default_value_t = 40)]
    max_nodes: usize,
    /// Drop edges with fewer than this many underlying file imports.
    #[arg(long, default_value_t = 1)]
    min_weight: usize,
    /// Force ASCII-only tree glyphs (no Unicode).
    #[arg(long)]
    ascii: bool,
}

#[derive(clap::Args)]
pub(crate) struct DashboardArgs {
    /// Depth used for the main graph panel (default `2` shows intra-package dirs).
    #[arg(long, default_value_t = 2)]
    depth: usize,
    #[arg(long, default_value_t = 40)]
    max_nodes: usize,
}

pub(crate) fn run(
    kind: &GraphKind,
    graph: &Graph,
    scan_roots: &[ScanRootReport],
    format: Format,
) {
    match kind {
        GraphKind::Packages(args) => run_packages(args, graph, scan_roots, format),
        GraphKind::Dashboard(args) => run_dashboard(args, graph, scan_roots, format),
    }
}

fn run_packages(args: &GraphArgs, graph: &Graph, scan_roots: &[ScanRootReport], format: Format) {
    let view = build_view(graph, scan_roots, args.depth, args.max_nodes, args.min_weight);
    emit_view(
        &view,
        format,
        EmitCtx {
            title: graph_title(args.depth),
            depth: args.depth,
            ascii: args.ascii,
            hint_depth: args.depth == 0 && view.edges.is_empty() && has_multi_file_packages(graph),
        },
    );
}

fn run_dashboard(
    args: &DashboardArgs,
    graph: &Graph,
    scan_roots: &[ScanRootReport],
    format: Format,
) {
    if matches!(format, Format::Json) {
        let view = build_view(graph, scan_roots, args.depth, args.max_nodes, 1);
        println!(
            "{}",
            serde_json::json!({
                "schema_version": noslop_report::SCHEMA_VERSION,
                "depth": args.depth,
                "depth_guide": depth_guide(),
                "graph": serde_json::from_str::<serde_json::Value>(&noslop_viz::to_json(&view)).unwrap_or_default(),
                "scan_roots": scan_roots,
            })
        );
        return;
    }
    if !matches!(format, Format::Pretty | Format::Sarif | Format::Github) {
        eprintln!("noslop: dashboard supports --format pretty|json|svg|html");
        return;
    }

    println!();
    println!("  IMPORT GRAPH DASHBOARD");
    println!("  {} file(s) across {} package(s)", graph.files.len(), scan_roots.len());
    println!();
    print_depth_guide();
    println!();

    let view = build_view(graph, scan_roots, args.depth, args.max_nodes, 1);
    print_header(&view, args.depth);
    print_tree(&view, false);
    println!();
    print_footer(args.depth);
}

fn build_view(
    graph: &Graph,
    scan_roots: &[ScanRootReport],
    depth: usize,
    max_nodes: usize,
    min_weight: usize,
) -> noslop_viz::PackageGraph {
    let roots: Vec<(&str, &str)> = scan_roots
        .iter()
        .map(|r| (r.package.as_str(), r.root.as_str()))
        .collect();
    build_import_graph(
        graph,
        &roots,
        ImportGraphOptions {
            depth,
            max_nodes,
            min_weight,
        },
    )
}

struct EmitCtx {
    title: String,
    depth: usize,
    ascii: bool,
    hint_depth: bool,
}

fn emit_view(view: &noslop_viz::PackageGraph, format: Format, ctx: EmitCtx) {
    match format {
        Format::Json => println!("{}", noslop_viz::to_json(view)),
        Format::Svg => print!("{}", noslop_viz::to_svg(view)),
        Format::Html => print!("{}", noslop_viz::to_html(view, &ctx.title)),
        Format::Pretty | Format::Sarif | Format::Github => {
            print_header(view, ctx.depth);
            print_tree(view, ctx.ascii);
            println!();
            if ctx.hint_depth {
                println!("  tip: package view has no cross-package edges — try `--depth 2` or `graph dashboard`");
            }
            print_footer(ctx.depth);
        }
        Format::Dot | Format::Mermaid => {
            eprintln!("noslop: graph uses --format svg|html for vector output; terminal view is always a tree");
            print_header(view, ctx.depth);
            print_tree(view, ctx.ascii);
            println!();
            print_footer(ctx.depth);
        }
    }
}

fn print_tree(view: &noslop_viz::PackageGraph, ascii: bool) {
    let opts = RenderOptions {
        color: std::env::var_os("NO_COLOR").is_none(),
        ascii,
    };
    print!("{}", noslop_viz::render(view, &opts));
}

fn graph_title(depth: usize) -> String {
    if depth == 0 {
        "Package import graph".to_string()
    } else {
        format!("Import graph (depth {depth})")
    }
}

fn print_header(view: &noslop_viz::PackageGraph, depth: usize) {
    let cyclic = view.cycles.iter().map(|c| c.len()).sum::<usize>();
    println!();
    println!("  {}", graph_title(depth).to_uppercase());
    println!(
        "  depth {depth} · {} node(s) · {} edge(s){}",
        view.nodes.len(),
        view.edges.len(),
        if cyclic > 0 {
            format!(" · {cyclic} in cycles")
        } else {
            String::new()
        }
    );
    println!();
}

fn print_depth_guide() {
    println!("  DEPTH GUIDE");
    for (d, desc) in depth_guide() {
        println!("    {d}  {desc}");
    }
}

fn depth_guide() -> [(&'static str, &'static str); 4] {
    [
        ("0", "packages only (@acme/web, acme-api)"),
        ("1", "first folder under each package (app, src)"),
        ("2", "dirs + file stems (app/cycle_a, src/lib) — shows imports & cycles"),
        ("3+", "deeper directory buckets"),
    ]
}

fn print_footer(depth: usize) {
    println!("  ── import      ↺ cycle");
    println!(
        "  vector graph: --format svg|html   depth: --depth {depth}",
        depth = depth
    );
}

fn has_multi_file_packages(graph: &Graph) -> bool {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &graph.files {
        *counts.entry(f.package.as_str()).or_insert(0) += 1;
    }
    counts.values().any(|&n| n > 1)
}
