//! Confidence computation — the policy that keeps noslop from crying "dead
//! code" (ARCHITECTURE.md §12). Confidence is *computed* from concrete
//! dampeners, never configured. A `High` false positive is a release blocker,
//! so every dampener can only *lower* confidence.

use noslop_graph::{Confidence, FileId, Graph};

/// Confidence for a dead-code finding on `file_id`. `name` is the symbol name
/// for symbol-level findings, or the file stem for file-level ones.
pub fn dead_confidence(graph: &Graph, file_id: FileId, name: Option<&str>) -> Confidence {
    let package = &graph.file(file_id).package;

    // The package contains an unresolvable dynamic import — anything could be
    // reached at runtime, so cap the whole package at Medium.
    if graph.package_is_dynamic(package) {
        return Confidence::Medium;
    }

    // The name appears in some string literal (dynamic dispatch by name,
    // reflection, framework registration). Cap at Medium.
    if let Some(name) = name {
        if graph.string_literals.contains(name) {
            return Confidence::Medium;
        }
    }

    Confidence::High
}
