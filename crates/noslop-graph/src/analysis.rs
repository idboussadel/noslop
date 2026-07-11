//! Pass-facing analysis configuration — plain data the *analyze* stage reads.
//!
//! Lives in the IR crate (like [`crate::workspace`]) so `noslop-passes` can
//! consume it without depending on `noslop-core`, where it is built from
//! `noslop.toml`. Glob *patterns* are kept as strings here; the passes that need
//! matching compile them (they already depend on `globset`), keeping this crate
//! dependency-free.

use crate::Severity;

/// Everything the optional/config-driven passes need. Extended as features land;
/// complexity is on by default (fallow-parity headline); duplication, policy,
/// and style stay opt-in.
#[derive(Debug, Clone, Default)]
pub struct AnalysisConfig {
    pub complexity: ComplexityConfig,
    pub policy: PolicyConfig,
    pub duplication: DuplicationConfig,
    pub style: StyleConfig,
}

/// Styling-analysis toggle. Web-only and off by default (a new language in the
/// pipeline, so opt-in via `[style]`).
#[derive(Debug, Clone, Default)]
pub struct StyleConfig {
    pub enabled: bool,
}

/// Duplication-detection settings. Disabled by default (it re-tokenizes the repo,
/// so it is opt-in via `[duplication]` or the `dupes` subcommand).
#[derive(Debug, Clone)]
pub struct DuplicationConfig {
    pub enabled: bool,
    pub mode: DuplicationMode,
    /// Minimum matched token run to report (fallow default: 30).
    pub min_tokens: u32,
    /// Report only clones that span more than one directory.
    pub skip_local: bool,
}

impl Default for DuplicationConfig {
    fn default() -> Self {
        DuplicationConfig {
            enabled: false,
            mode: DuplicationMode::Mild,
            min_tokens: 30,
            skip_local: false,
        }
    }
}

/// Normalization ladder, weakest (most permissive) last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicationMode {
    /// Exact token text.
    Exact,
    /// Ignore differing numeric literals.
    Mild,
    /// Also ignore differing string literals.
    Weak,
    /// Also ignore renamed identifiers (verified for consistent 1-1 renaming).
    Semantic,
}

impl DuplicationMode {
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "exact" => Some(Self::Exact),
            "mild" => Some(Self::Mild),
            "weak" => Some(Self::Weak),
            "semantic" => Some(Self::Semantic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Mild => "mild",
            Self::Weak => "weak",
            Self::Semantic => "semantic",
        }
    }
}

/// Declarative policy: banned imports/calls/effects and architectural layers.
#[derive(Debug, Clone, Default)]
pub struct PolicyConfig {
    pub rules: Vec<PolicyRule>,
    pub layers: Vec<Layer>,
}

impl PolicyConfig {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.layers.is_empty()
    }
}

/// A single banned-import / banned-call / banned-effect rule from a pack.
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub id: String,
    pub kind: PolicyKind,
    /// For import/call: glob patterns on the specifier / dotted callee. For
    /// `banned-effect`: the effect *names* (`network`, `process`, `fs`), which the
    /// pass expands to concrete patterns.
    pub patterns: Vec<String>,
    /// Optional path scope; empty = the whole repo.
    pub paths: Vec<String>,
    pub severity: Severity,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyKind {
    BannedImport,
    BannedCall,
    BannedEffect,
}

/// An architectural layer: files under `paths` may import only their own layer
/// plus the layers named in `allow`.
#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    pub paths: Vec<String>,
    pub allow: Vec<String>,
}

/// Cyclomatic/cognitive thresholds and per-glob exemptions.
#[derive(Debug, Clone)]
pub struct ComplexityConfig {
    pub enabled: bool,
    pub max_cyclomatic: u32,
    pub max_cognitive: u32,
    /// Maximum function length in lines before it is reported as oversized
    /// (fallow parity: `health.maxUnitSize`, default 60).
    pub max_loc: u32,
    /// Maximum CRAP score before reporting (fallow default: 30).
    pub max_crap: f64,
    pub overrides: Vec<ComplexityOverride>,
}

impl Default for ComplexityConfig {
    fn default() -> Self {
        // On by default — zero-config scans report complexity + large functions.
        ComplexityConfig {
            enabled: true,
            max_cyclomatic: 20,
            max_cognitive: 15,
            max_loc: 60,
            max_crap: 30.0,
            overrides: Vec::new(),
        }
    }
}

/// A path-scoped threshold relaxation, requiring a documented `reason`.
#[derive(Debug, Clone)]
pub struct ComplexityOverride {
    pub paths: Vec<String>,
    pub max_cyclomatic: Option<u32>,
    pub max_cognitive: Option<u32>,
    pub max_loc: Option<u32>,
    pub max_crap: Option<f64>,
    pub severity: Option<Severity>,
    pub reason: String,
}
