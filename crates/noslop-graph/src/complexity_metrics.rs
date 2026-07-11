//! Per-function complexity metrics attached to `high-complexity` findings.
//!
//! CRAP follows Savoia & Evans (2007): `CC² × (1 - cov/100)³ + CC`. Coverage is
//! estimated from test reachability until Istanbul JSON lands.

/// Prefix on [`Finding::reason`](crate::Finding::reason) for machine-readable metrics.
pub const METRICS_PREFIX: &str = "metrics:";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComplexityMetrics {
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub loc: u32,
    pub crap: f64,
    /// Estimated test-coverage percentage used for CRAP (0–100).
    pub coverage_pct: f64,
}

impl ComplexityMetrics {
    pub fn encode(&self) -> String {
        format!(
            "{METRICS_PREFIX}cyclomatic={},cognitive={},loc={},crap={:.1},coverage={:.0}",
            self.cyclomatic, self.cognitive, self.loc, self.crap, self.coverage_pct
        )
    }

    pub fn decode(reason: &str) -> Option<Self> {
        let rest = reason.strip_prefix(METRICS_PREFIX)?;
        let mut cyclomatic = None;
        let mut cognitive = None;
        let mut loc = None;
        let mut crap = None;
        let mut coverage_pct = None;
        for part in rest.split(',') {
            let (key, value) = part.split_once('=')?;
            match key {
                "cyclomatic" => cyclomatic = value.parse().ok(),
                "cognitive" => cognitive = value.parse().ok(),
                "loc" => loc = value.parse().ok(),
                "crap" => crap = value.parse().ok(),
                "coverage" => coverage_pct = value.parse().ok(),
                _ => {}
            }
        }
        Some(Self {
            cyclomatic: cyclomatic?,
            cognitive: cognitive?,
            loc: loc?,
            crap: crap?,
            coverage_pct: coverage_pct?,
        })
    }
}

/// CRAP score from cyclomatic complexity and coverage percentage (0–100).
pub fn crap_score(cyclomatic: u32, coverage_pct: f64) -> f64 {
    let cc = cyclomatic as f64;
    let uncovered = 1.0 - (coverage_pct.clamp(0.0, 100.0) / 100.0);
    cc * cc * uncovered.powi(3) + cc
}

/// Fallow-style triage band for terminal output.
pub fn complexity_band(metrics: &ComplexityMetrics) -> &'static str {
    if metrics.crap >= 500.0 || metrics.cyclomatic >= 50 || metrics.cognitive >= 50 {
        "CRITICAL"
    } else if metrics.crap >= 100.0 || metrics.cyclomatic >= 30 || metrics.cognitive >= 30 {
        "HIGH"
    } else if metrics.crap >= 30.0 {
        "ELEVATED"
    } else {
        "OVER"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crap_matches_fallow_untested_formula() {
        assert!((crap_score(5, 0.0) - 30.0).abs() < f64::EPSILON);
        assert!((crap_score(115, 0.0) - 13_340.0).abs() < f64::EPSILON);
        assert!((crap_score(5, 100.0) - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn metrics_round_trip() {
        let m = ComplexityMetrics {
            cyclomatic: 37,
            cognitive: 41,
            loc: 102,
            crap: 1406.0,
            coverage_pct: 0.0,
        };
        let decoded = ComplexityMetrics::decode(&m.encode()).expect("decode");
        assert_eq!(decoded, m);
    }
}
