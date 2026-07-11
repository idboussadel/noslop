//! Reachability — the shared BFS both `unused-file` and `only-used-in-tests`
//! build on, computed once and handed to the passes.

use noslop_graph::{FileId, Graph};
use std::collections::VecDeque;

/// Per-file reachability from the two entry-point classes.
pub struct Reach {
    /// Reachable from *any* entry point (including tests).
    pub reachable: Vec<bool>,
    /// Reachable from *production* entry points only (tests excluded), which is
    /// what distinguishes genuinely-dead code from test-only code.
    pub prod_reachable: Vec<bool>,
    /// Reachable from test entry points — used for CRAP coverage estimates.
    pub test_reachable: Vec<bool>,
}

impl Reach {
    pub fn compute(graph: &Graph) -> Self {
        // A config file (`eslint.config.mjs`, `vite.config.ts`) is a leaf entry:
        // never itself reported dead, but a seed so the helpers it imports stay live.
        let is_seed = |f: &noslop_graph::FileNode| {
            f.is_entry || f.is_implicit_used || f.role.is_reachability_seed()
        };
        let all_seeds: Vec<FileId> = graph
            .files
            .iter()
            .filter(|f| is_seed(f))
            .map(|f| f.id)
            .collect();
        // Production seeds: entries that are not test files.
        let prod_seeds: Vec<FileId> = graph
            .files
            .iter()
            .filter(|f| is_seed(f) && !f.is_test)
            .map(|f| f.id)
            .collect();
        let test_seeds: Vec<FileId> = graph
            .files
            .iter()
            .filter(|f| is_seed(f) && f.is_test)
            .map(|f| f.id)
            .collect();

        Reach {
            reachable: bfs(graph, &all_seeds),
            prod_reachable: bfs(graph, &prod_seeds),
            test_reachable: bfs(graph, &test_seeds),
        }
    }
}

/// Multi-source BFS over the file import graph.
fn bfs(graph: &Graph, seeds: &[FileId]) -> Vec<bool> {
    let mut seen = vec![false; graph.files.len()];
    let mut queue: VecDeque<FileId> = VecDeque::new();
    for &s in seeds {
        if !seen[s] {
            seen[s] = true;
            queue.push_back(s);
        }
    }
    while let Some(f) = queue.pop_front() {
        for &next in &graph.imports[f] {
            if !seen[next] {
                seen[next] = true;
                queue.push_back(next);
            }
        }
    }
    seen
}
