//! JSON export — the stable graph data contract.

use crate::model::PackageGraph;

pub fn to_json(g: &PackageGraph) -> String {
    serde_json::to_string_pretty(g).unwrap_or_else(|_| "{}".to_string())
}
