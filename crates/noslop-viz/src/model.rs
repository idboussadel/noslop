//! The view model — plain, serializable data the renderers consume.
//!
//! Nothing here knows about `noslop-graph`, git, or config. A view model is
//! built by an upstream crate (see `noslop-report::graphs`) and handed to the
//! pure renderers in this crate. Keeping it dependency-free is what makes every
//! render byte-for-byte snapshot-testable.

use serde::Serialize;

/// A package/workspace-level import graph: one node per package, one edge per
/// import relation between two packages.
#[derive(Debug, Clone, Serialize)]
pub struct PackageGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Groups of node indices that form an import cycle (each a multi-node SCC),
    /// sorted for stable output.
    pub cycles: Vec<Vec<usize>>,
}

/// One graph node (package or directory bucket).
#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// Stable full identity (package id, or `package/dir/...` at depth > 0).
    pub id: String,
    /// Short display label (usually the last path segment).
    pub label: String,
    /// Number of source files in this bucket.
    pub files: usize,
    /// Owning package/workspace id (for grouping in multi-depth views).
    pub package: String,
}

/// A directed edge between two package nodes (indices into [`PackageGraph::nodes`]).
#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub kind: EdgeKind,
    /// How many underlying file-level edges this package edge aggregates.
    pub weight: usize,
}

/// The relation an edge encodes. Only [`EdgeKind::Import`] is produced today;
/// `Call`/`CoChange` are reserved for the insights layer (see
/// GRAPHS_AND_INSIGHTS.md §5) and already flow through the exporters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeKind {
    Import,
    Call,
    CoChange,
}

impl PackageGraph {
    /// Is `node` part of any import cycle?
    pub fn in_cycle(&self, node: usize) -> bool {
        self.cycles.iter().any(|c| c.contains(&node))
    }
}
