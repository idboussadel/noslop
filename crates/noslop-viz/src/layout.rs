//! Layered (Sugiyama-style) layout: assign each node a rank and a stable order
//! within that rank. Pure and deterministic — same graph in, same layout out.
//!
//! Ranking runs on the DAG formed by dropping *back edges* (edges that close a
//! cycle), found with a depth-first search. Forward edges therefore always point
//! to a strictly greater rank, which is exactly the invariant the box renderer
//! relies on to route without crossing boxes.

use crate::model::PackageGraph;
use std::collections::HashSet;

/// The computed placement for a graph.
pub struct Layout {
    /// Node indices grouped by rank, each group ordered left-to-right.
    pub layers: Vec<Vec<usize>>,
    /// `rank[node]` — its vertical layer (0 = top).
    pub rank: Vec<usize>,
    /// `order[node]` — its horizontal position within its layer.
    pub order: Vec<usize>,
    /// Edges `(from, to)` that close a cycle; drawn as returning arrows.
    pub back_edges: HashSet<(usize, usize)>,
}

impl Layout {
    pub fn compute(g: &PackageGraph) -> Layout {
        let n = g.nodes.len();
        let adj = adjacency(g);
        let back_edges = classify_back_edges(n, &adj);
        let rank = longest_path_ranks(n, &adj, &back_edges);

        let num_layers = rank.iter().copied().max().map(|m| m + 1).unwrap_or(0);
        let mut layers: Vec<Vec<usize>> = vec![Vec::new(); num_layers];
        for (node, &r) in rank.iter().enumerate() {
            layers[r].push(node);
        }
        // Stable, label-then-id ordering within each layer.
        for layer in &mut layers {
            layer.sort_by(|&a, &b| {
                g.nodes[a]
                    .label
                    .cmp(&g.nodes[b].label)
                    .then_with(|| g.nodes[a].id.cmp(&g.nodes[b].id))
            });
        }
        let mut order = vec![0usize; n];
        for layer in &layers {
            for (pos, &node) in layer.iter().enumerate() {
                order[node] = pos;
            }
        }

        Layout {
            layers,
            rank,
            order,
            back_edges,
        }
    }
}

/// Sorted forward adjacency, for deterministic traversal.
fn adjacency(g: &PackageGraph) -> Vec<Vec<usize>> {
    let mut adj = vec![Vec::new(); g.nodes.len()];
    for e in &g.edges {
        adj[e.from].push(e.to);
    }
    for out in &mut adj {
        out.sort_unstable();
        out.dedup();
    }
    adj
}

/// Depth-first classification: an edge to a node currently on the DFS stack is a
/// back edge (it closes a cycle). Iterative to avoid stack overflow on large
/// graphs; nodes visited in index order for determinism.
fn classify_back_edges(n: usize, adj: &[Vec<usize>]) -> HashSet<(usize, usize)> {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;

    let mut color = vec![WHITE; n];
    let mut back = HashSet::new();

    for start in 0..n {
        if color[start] != WHITE {
            continue;
        }
        // Stack of (node, next-neighbor-index).
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        color[start] = GRAY;
        while let Some(&(node, idx)) = stack.last() {
            if idx < adj[node].len() {
                stack.last_mut().unwrap().1 += 1;
                let next = adj[node][idx];
                match color[next] {
                    WHITE => {
                        color[next] = GRAY;
                        stack.push((next, 0));
                    }
                    GRAY => {
                        back.insert((node, next));
                    }
                    _ => {}
                }
            } else {
                color[node] = BLACK;
                stack.pop();
            }
        }
    }
    back
}

/// Longest-path layering over the DAG (forward edges only). Kahn's algorithm
/// processes nodes in topological order, so a single max-relaxation per edge
/// yields the longest path from any source.
fn longest_path_ranks(n: usize, adj: &[Vec<usize>], back: &HashSet<(usize, usize)>) -> Vec<usize> {
    let mut indeg = vec![0usize; n];
    for (u, outs) in adj.iter().enumerate() {
        for &v in outs {
            if !back.contains(&(u, v)) {
                indeg[v] += 1;
            }
        }
    }

    let mut rank = vec![0usize; n];
    let mut queue: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut head = 0;
    while head < queue.len() {
        let u = queue[head];
        head += 1;
        for &v in &adj[u] {
            if back.contains(&(u, v)) {
                continue;
            }
            if rank[u] + 1 > rank[v] {
                rank[v] = rank[u] + 1;
            }
            indeg[v] -= 1;
            if indeg[v] == 0 {
                queue.push(v);
            }
        }
    }
    rank
}
