//! `noslop-passes` — stage 4. Each pass is a pure function over the graph (and,
//! where a rule needs per-file facts, the facts) returning `Vec<Finding>`
//! (ARCHITECTURE.md §8). Passes are independently testable and order-independent;
//! `run_all` just concatenates them. Findings carry a *default* severity and a
//! *computed* confidence; config-driven severity overrides, suppression, and
//! ordering are applied later by the core/report stages.

mod complexity;
mod confidence;
mod cycles;
mod dead_files;
mod deps;
pub mod duplication;
mod exports;
mod imports;
mod policy;
mod reach;
mod style;
mod symbols;

pub use confidence::dead_confidence;
pub use duplication::FileTokens;
pub use reach::Reach;

use noslop_graph::{AnalysisConfig, FileFacts, Finding, Graph, Workspace};

/// Run every pass and return the combined, still-unsorted findings. `cfg` gates
/// the optional, config-driven passes (complexity, policy, …); a default config
/// runs only the always-on base rules.
pub fn run_all(
    graph: &Graph,
    facts: &[FileFacts],
    ws: &Workspace,
    cfg: &AnalysisConfig,
) -> Vec<Finding> {
    let reach = Reach::compute(graph);
    let mut findings = Vec::new();

    findings.extend(dead_files::run(graph, &reach));
    findings.extend(exports::run(graph, &reach, facts, ws));
    findings.extend(symbols::run(graph, &reach));
    findings.extend(imports::run(graph, facts));
    findings.extend(cycles::run(graph));
    findings.extend(deps::run(graph, ws));
    findings.extend(complexity::run(graph, &reach, facts, &cfg.complexity));
    findings.extend(policy::run(graph, facts, &cfg.policy));
    findings.extend(style::run(facts, &cfg.style));

    findings
}
