//! Minimal SARIF 2.1.0 rendering for GitHub code scanning integration.

use crate::Report;
use noslop_graph::{Finding, Severity};
use serde_json::{json, Value};

pub(crate) fn render(report: &Report) -> String {
    let results: Vec<Value> = report.findings.iter().map(result).collect();
    let doc = json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "noslop",
                    "informationUri": "https://github.com/noslopcode/noslopcode",
                    "version": report.tool_version,
                }
            },
            "results": results,
        }]
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
}

fn result(f: &Finding) -> Value {
    json!({
        "ruleId": f.rule.as_str(),
        "level": sarif_level(f.severity),
        "message": { "text": f.message },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": f.file.display().to_string() },
                "region": {
                    "startLine": f.span.start_line,
                    "endLine": f.span.end_line,
                }
            }
        }]
    })
}

fn sarif_level(sev: Severity) -> &'static str {
    match sev {
        Severity::Error => "error",
        Severity::Warn => "warning",
        Severity::Off => "none",
    }
}
