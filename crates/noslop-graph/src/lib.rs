//! `noslop-graph` — the shared intermediate representation (IR).
//!
//! This crate depends on nothing internal. Every other crate in the pipeline
//! depends on it, and siblings never depend on each other; all data flows
//! through the plain, serializable types defined here. That single rule is
//! what keeps the pipeline strictly one-directional (see ARCHITECTURE.md §4).
//!
//! It holds three families of types:
//! * [`facts`] — the language-neutral output contract of the *extract* stage.
//! * [`ir`]    — the resolved graph the *analyze* stage queries.
//! * [`finding`] — what passes emit and the *report* stage renders.

pub mod analysis;
pub mod complexity_metrics;
pub mod facts;
pub mod finding;
pub mod ids;
pub mod ir;
pub mod workspace;

pub use analysis::*;
pub use complexity_metrics::*;
pub use facts::*;
pub use finding::*;
pub use ids::*;
pub use ir::*;
pub use workspace::*;
