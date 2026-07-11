//! `unused-dependency` — a declared dependency no import resolves to.
//!
//! Declared dependencies routinely include tooling used only via config or CLI
//! (bundlers, test runners, type stubs), so findings are capped at `Medium`
//! confidence: hidden from the default High-only view, visible under `--all`.

use noslop_graph::{Confidence, Finding, Graph, ManifestKind, RuleId, Severity, Span, Workspace};

pub fn run(graph: &Graph, ws: &Workspace) -> Vec<Finding> {
    let mut findings = Vec::new();

    for pkg in &ws.packages {
        let manifest = match pkg.manifest_kind {
            ManifestKind::PackageJson => pkg.root.join("package.json"),
            ManifestKind::PyProject => pkg.root.join("pyproject.toml"),
            ManifestKind::SetupPy => pkg.root.join("setup.py"),
            ManifestKind::Implicit => continue,
        };

        let mut deps: Vec<&String> = pkg.dependencies.iter().collect();
        deps.sort(); // deterministic order
        for dep in deps {
            // Framework deps (the plugin's trigger, e.g. `next`, `fastapi`) are
            // used implicitly by being the framework — never flag them.
            if pkg.framework_deps.contains(dep) || dependency_is_used(graph, dep) {
                continue;
            }
            findings.push(Finding {
                rule: RuleId::UnusedDependency,
                severity: Severity::Warn,
                confidence: Confidence::Medium,
                symbol: None,
                file: manifest.clone(),
                span: Span::new(1, 1),
                message: format!("Declared dependency '{dep}' is never imported."),
                reason: "no import in the package resolves to this dependency".to_string(),
            });
        }
    }

    findings
}

fn dependency_is_used(graph: &Graph, dep: &str) -> bool {
    if graph.external_used.contains(dep) {
        return true;
    }
    // `@types/foo` is "used" when `foo` is imported; `@types/node` maps to Node
    // builtins we do not track, so treat any `@types/*` as used conservatively.
    if let Some(stub_target) = dep.strip_prefix("@types/") {
        return stub_target == "node" || graph.external_used.contains(stub_target);
    }
    false
}
