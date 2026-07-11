//! `circular-imports` — Tarjan strongly-connected components over the file
//! import graph. Whole SCC groups are reported smallest-first (the smallest is
//! the easiest to break — "start here"). Python cycles are runtime bugs waiting
//! to fire, so they default to `error`; TS cycles default to `warn`
//! (ARCHITECTURE.md §8).

use noslop_graph::{Confidence, Finding, Graph, RuleId, Severity, Span};
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};

pub fn run(graph: &Graph) -> Vec<Finding> {
    let mut g: DiGraph<usize, ()> = DiGraph::new();
    let nodes: Vec<NodeIndex> = (0..graph.files.len()).map(|i| g.add_node(i)).collect();
    for (from, targets) in graph.imports.iter().enumerate() {
        for &to in targets {
            g.add_edge(nodes[from], nodes[to], ());
        }
    }

    // Collect multi-file SCCs as sorted file-id groups.
    let mut groups: Vec<Vec<usize>> = tarjan_scc(&g)
        .into_iter()
        .filter(|scc| scc.len() > 1)
        .map(|scc| {
            let mut ids: Vec<usize> = scc.into_iter().map(|n| g[n]).collect();
            ids.sort_by(|a, b| graph.files[*a].path.cmp(&graph.files[*b].path));
            ids
        })
        .collect();

    // Smallest group first, then by first member for a fully stable order.
    groups.sort_by(|a, b| {
        a.len()
            .cmp(&b.len())
            .then_with(|| graph.files[a[0]].path.cmp(&graph.files[b[0]].path))
    });

    groups
        .into_iter()
        .map(|ids| finding_for_group(graph, &ids))
        .collect()
}

fn finding_for_group(graph: &Graph, ids: &[usize]) -> Finding {
    let all_python = ids.iter().all(|&id| graph.files[id].language.is_python());
    let severity = if all_python {
        Severity::Error
    } else {
        Severity::Warn
    };
    let members: Vec<String> = ids
        .iter()
        .map(|&id| graph.files[id].path.display().to_string())
        .collect();

    Finding {
        rule: RuleId::CircularImports,
        severity,
        confidence: Confidence::High,
        symbol: None,
        file: graph.files[ids[0]].path.clone(),
        span: Span::new(1, 1),
        message: format!(
            "Circular import group ({} files): {}",
            ids.len(),
            members.join(" \u{21c4} ")
        ),
        reason: "files form a strongly-connected component in the import graph".to_string(),
    }
}
