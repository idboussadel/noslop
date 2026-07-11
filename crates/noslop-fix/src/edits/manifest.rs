//! Remove an unused dependency from package.json or pyproject.toml.

use anyhow::{Context, Result};
use std::path::Path;

const DEP_SECTIONS: &[&str] = &[
  "dependencies",
  "devDependencies",
  "peerDependencies",
  "optionalDependencies",
];

/// Remove `dep` from a manifest file. Returns new file contents.
pub fn remove_dependency(path: &Path, text: &str, dep: &str) -> Result<String> {
  let name = path
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or_default();
  match name {
    "package.json" => remove_from_package_json(text, dep),
    "pyproject.toml" => remove_from_pyproject(text, dep),
    other => anyhow::bail!("unsupported manifest '{other}' for dependency removal"),
  }
}

fn remove_from_package_json(text: &str, dep: &str) -> Result<String> {
  let mut value: serde_json::Value =
    serde_json::from_str(text).context("package.json is not valid JSON")?;
  let mut removed = false;
  if let Some(obj) = value.as_object_mut() {
    for key in DEP_SECTIONS {
      if let Some(deps) = obj.get_mut(*key).and_then(|v| v.as_object_mut()) {
        if deps.remove(dep).is_some() {
          removed = true;
        }
      }
    }
  }
  if !removed {
    anyhow::bail!("dependency '{dep}' not found in package.json");
  }
  Ok(serde_json::to_string_pretty(&value).map(|s| format!("{s}\n"))?)
}

fn remove_from_pyproject(text: &str, dep: &str) -> Result<String> {
  let mut doc = text.parse::<toml::Value>().context("pyproject.toml is not valid TOML")?;
  let mut removed = false;

  if let Some(project) = doc.get_mut("project") {
    if let Some(deps) = project.get_mut("dependencies").and_then(|d| d.as_array_mut()) {
      let before = deps.len();
      deps.retain(|entry| !dep_matches_entry(dep, entry));
      removed |= deps.len() < before;
    }
  }
  if let Some(poetry) = doc
    .get_mut("tool")
    .and_then(|t| t.get_mut("poetry"))
    .and_then(|p| p.get_mut("dependencies"))
    .and_then(|d| d.as_table_mut())
  {
    if poetry.remove(dep).is_some() {
      removed = true;
    }
  }

  if !removed {
    anyhow::bail!("dependency '{dep}' not found in pyproject.toml");
  }
  Ok(toml::to_string_pretty(&doc).map(|s| format!("{s}\n"))?)
}

fn dep_matches_entry(dep: &str, entry: &toml::Value) -> bool {
  let Some(spec) = entry.as_str() else {
    return false;
  };
  spec.trim()
    .split(|c: char| " <>=!~;[(".contains(c))
    .next()
    .unwrap_or(spec)
    .trim()
    == dep
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn removes_dev_dependency_from_package_json() {
    let src = r#"{
  "name": "app",
  "dependencies": { "react": "18" },
  "devDependencies": { "orphan-pkg": "1.0.0" }
}
"#;
    let out = remove_dependency(Path::new("package.json"), src, "orphan-pkg").unwrap();
    assert!(!out.contains("orphan-pkg"));
    assert!(out.contains("react"));
  }
}
