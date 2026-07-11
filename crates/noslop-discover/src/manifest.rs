//! Manifest parsing — package.json, tsconfig.json, pyproject.toml.
//!
//! Parsing is deliberately lenient: a malformed or exotic manifest degrades to
//! "fewer facts", never an error that aborts the scan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Parsed `package.json` fields relevant to discovery.
#[derive(Debug, Default)]
pub struct PackageJson {
    pub name: Option<String>,
    pub dependencies: HashSet<String>,
    /// Workspace globs (`workspaces: [...]` or `workspaces: { packages: [...] }`).
    pub workspaces: Vec<String>,
    /// Entry targets declared via `main` / `bin` (repo-relative to the pkg root).
    pub entry_targets: Vec<String>,
}

pub fn parse_package_json(text: &str) -> PackageJson {
    let mut out = PackageJson::default();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return out;
    };
    out.name = v.get("name").and_then(|n| n.as_str()).map(String::from);

    for key in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(deps) = v.get(key).and_then(|d| d.as_object()) {
            out.dependencies.extend(deps.keys().cloned());
        }
    }

    match v.get("workspaces") {
        Some(serde_json::Value::Array(arr)) => {
            out.workspaces
                .extend(arr.iter().filter_map(|s| s.as_str().map(String::from)));
        }
        Some(serde_json::Value::Object(obj)) => {
            if let Some(arr) = obj.get("packages").and_then(|p| p.as_array()) {
                out.workspaces
                    .extend(arr.iter().filter_map(|s| s.as_str().map(String::from)));
            }
        }
        _ => {}
    }

    if let Some(main) = v.get("main").and_then(|m| m.as_str()) {
        out.entry_targets.push(main.to_string());
    }
    match v.get("bin") {
        Some(serde_json::Value::String(s)) => out.entry_targets.push(s.clone()),
        Some(serde_json::Value::Object(obj)) => out
            .entry_targets
            .extend(obj.values().filter_map(|s| s.as_str().map(String::from))),
        _ => {}
    }
    out
}

/// Parsed `tsconfig.json` resolution settings.
#[derive(Debug, Default)]
pub struct TsConfig {
    pub base_url: Option<String>,
    pub paths: Vec<(String, Vec<String>)>,
}

pub fn parse_tsconfig(text: &str) -> TsConfig {
    let mut out = TsConfig::default();
    let cleaned = strip_jsonc(text);
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&cleaned) else {
        return out;
    };
    let Some(co) = v.get("compilerOptions") else {
        return out;
    };
    out.base_url = co.get("baseUrl").and_then(|b| b.as_str()).map(String::from);
    if let Some(paths) = co.get("paths").and_then(|p| p.as_object()) {
        for (alias, targets) in paths {
            let targets = targets
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            out.paths.push((alias.clone(), targets));
        }
    }
    out
}

/// Parsed `pyproject.toml` fields.
#[derive(Debug, Default)]
pub struct PyProject {
    pub name: Option<String>,
    pub dependencies: HashSet<String>,
    /// `module.path` targets from `[project.scripts]` (colon-form split off).
    pub script_modules: Vec<String>,
    /// True if a `src/` layout is declared or conventional.
    pub src_layout: bool,
}

pub fn parse_pyproject(text: &str, pkg_root: &Path) -> PyProject {
    let mut out = PyProject::default();
    let Ok(v) = text.parse::<toml::Value>() else {
        return out;
    };

    if let Some(project) = v.get("project") {
        out.name = project
            .get("name")
            .and_then(|n| n.as_str())
            .map(String::from);
        if let Some(deps) = project.get("dependencies").and_then(|d| d.as_array()) {
            out.dependencies
                .extend(deps.iter().filter_map(|d| d.as_str()).map(dep_name));
        }
        if let Some(scripts) = project.get("scripts").and_then(|s| s.as_table()) {
            for target in scripts.values().filter_map(|t| t.as_str()) {
                let module = target.split(':').next().unwrap_or(target);
                out.script_modules.push(module.to_string());
            }
        }
    }
    // Poetry-style dependencies table.
    if let Some(poetry) = v.get("tool").and_then(|t| t.get("poetry")) {
        if let Some(deps) = poetry.get("dependencies").and_then(|d| d.as_table()) {
            out.dependencies
                .extend(deps.keys().filter(|k| *k != "python").cloned());
        }
    }

    out.src_layout = pkg_root.join("src").is_dir();
    out
}

/// Extract the distribution name from a PEP 508 dependency string
/// (`fastapi>=0.100` → `fastapi`, `uvicorn[standard]` → `uvicorn`).
fn dep_name(spec: &str) -> String {
    spec.trim()
        .split(|c: char| " <>=!~;[(".contains(c))
        .next()
        .unwrap_or(spec)
        .trim()
        .to_string()
}

/// Strip `//` and `/* */` comments so a JSONC tsconfig parses with serde_json.
/// String-aware so `"http://x"` is not mangled.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Resolve a manifest-declared entry target (e.g. `dist/main.js` or
/// `pkg.module`) to a candidate source path under the package, if one exists.
pub fn resolve_entry_target(pkg_root: &Path, target: &str, is_python: bool) -> Option<PathBuf> {
    if is_python {
        // `pkg.sub.module` → pkg/sub/module.py
        let rel = target.replace('.', "/");
        for cand in [format!("{rel}.py"), format!("{rel}/__main__.py")] {
            let p = pkg_root.join(&cand);
            if p.is_file() {
                return Some(p);
            }
        }
        None
    } else {
        let direct = pkg_root.join(target);
        if direct.is_file() {
            return Some(direct);
        }
        // `main` often points at a build output; try the .ts source sibling.
        let stem = target.trim_end_matches(".js").trim_end_matches(".cjs");
        for ext in ["ts", "tsx", "mts"] {
            let p = pkg_root.join(format!("{stem}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }
}
