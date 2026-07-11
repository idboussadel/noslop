//! Safe auto-fix for high-confidence dead-code findings.

mod apply;
mod diff;
mod edits;
mod plan;
mod snapshot;

pub use apply::{apply, fix, FixOutcome};
pub use plan::{affected_paths, plan, FixAction, FixOptions, FixPlan};
pub use snapshot::{restore, FixRollback, ROLLBACK_REL};
