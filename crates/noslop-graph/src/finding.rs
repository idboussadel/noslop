//! Findings — what analysis passes emit and the report stage renders.

use crate::ids::Span;
use serde::Serialize;
use std::path::PathBuf;

/// Stable rule identifier. Serialized into the output contract, so the string
/// values are API and must not change casually.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleId {
    UnusedFile,
    UnusedExport,
    UnusedType,
    UnusedImport,
    UnusedDependency,
    UnusedEnumMember,
    UnusedClassMember,
    UnusedParameter,
    ExpectedUnusedButUsed,
    MissingSuppressionReason,
    HighComplexity,
    LargeFunction,
    BannedImport,
    BannedCall,
    BannedEffect,
    BoundaryViolation,
    DuplicateCode,
    UnusedCssToken,
    BrokenCssReference,
    UnusedCssClass,
    CircularImports,
    OnlyUsedInTests,
}

impl RuleId {
    /// The canonical kebab-case name, matching the JSON encoding and config keys.
    pub fn as_str(self) -> &'static str {
        match self {
            RuleId::UnusedFile => "unused-file",
            RuleId::UnusedExport => "unused-export",
            RuleId::UnusedType => "unused-type",
            RuleId::UnusedImport => "unused-import",
            RuleId::UnusedDependency => "unused-dependency",
            RuleId::UnusedEnumMember => "unused-enum-member",
            RuleId::UnusedClassMember => "unused-class-member",
            RuleId::UnusedParameter => "unused-parameter",
            RuleId::ExpectedUnusedButUsed => "expected-unused-but-used",
            RuleId::MissingSuppressionReason => "missing-suppression-reason",
            RuleId::HighComplexity => "high-complexity",
            RuleId::LargeFunction => "large-function",
            RuleId::BannedImport => "banned-import",
            RuleId::BannedCall => "banned-call",
            RuleId::BannedEffect => "banned-effect",
            RuleId::BoundaryViolation => "boundary-violation",
            RuleId::DuplicateCode => "duplicate-code",
            RuleId::UnusedCssToken => "unused-css-token",
            RuleId::BrokenCssReference => "broken-css-reference",
            RuleId::UnusedCssClass => "unused-css-class",
            RuleId::CircularImports => "circular-imports",
            RuleId::OnlyUsedInTests => "only-used-in-tests",
        }
    }
}

/// Configurable severity. `Off` disables a rule entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Off,
    Warn,
    Error,
}

impl Severity {
    /// Parse a config severity keyword (`off`/`warn`/`error`).
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Severity::Off),
            "warn" => Some(Severity::Warn),
            "error" => Some(Severity::Error),
            _ => None,
        }
    }
}

/// Computed (never configured) trust tier for a finding. Default terminal output
/// shows `High` only; `--all` reveals the rest; JSON always includes everything
/// (ARCHITECTURE.md §12). A `High` false positive is a release blocker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// One reported problem. Ordering across a report is stabilized by the report
/// stage (by file, line, rule) so identical input yields byte-identical output.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: RuleId,
    pub severity: Severity,
    pub confidence: Confidence,
    /// Stable symbol id, when the finding is about a symbol rather than a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub file: PathBuf,
    pub span: Span,
    /// Human-readable sentence.
    pub message: String,
    /// Machine-checkable justification ("no inbound edges from any entry point").
    pub reason: String,
}

impl Finding {
    /// The stable id used for baselines and deduplication: the symbol id when
    /// present, otherwise the file path (ARCHITECTURE.md Appendix A).
    pub fn stable_key(&self) -> String {
        match &self.symbol {
            Some(sym) => format!("{}|{}", self.rule.as_str(), sym),
            None => format!("{}|{}", self.rule.as_str(), self.file.display()),
        }
    }
}
