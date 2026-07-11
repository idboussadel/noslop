//! Snapshot taken before the last applied fix — powers `noslop fix restore`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const ROLLBACK_REL: &str = ".noslopcode/fix-rollback.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixRollback {
  /// Repo-relative paths → file contents immediately before the fix ran.
  pub files: HashMap<PathBuf, String>,
}

impl FixRollback {
  pub fn is_empty(&self) -> bool {
    self.files.is_empty()
  }

  pub fn path(root: &Path) -> PathBuf {
    root.join(ROLLBACK_REL)
  }

  pub fn load(root: &Path) -> Result<Option<Self>> {
    let path = Self::path(root);
    let Ok(bytes) = std::fs::read(&path) else {
      return Ok(None);
    };
    let snap: Self = serde_json::from_slice(&bytes).context("parse fix rollback snapshot")?;
    Ok(Some(snap))
  }

  pub fn save(&self, root: &Path) -> Result<()> {
    let path = Self::path(root);
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(self)?;
    std::fs::write(path, bytes)?;
    Ok(())
  }

  pub fn clear(root: &Path) -> Result<()> {
    let path = Self::path(root);
    if path.is_file() {
      std::fs::remove_file(path)?;
    }
    Ok(())
  }
}

/// Read pre-change contents for every path the plan will touch.
pub fn capture(
  root: &Path,
  paths: impl IntoIterator<Item = PathBuf>,
) -> Result<FixRollback> {
  let mut files = HashMap::new();
  for rel in paths {
    let abs = root.join(&rel);
    let content = std::fs::read_to_string(&abs)
      .with_context(|| format!("snapshot read {}", abs.display()))?;
    files.insert(rel, content);
  }
  Ok(FixRollback { files })
}

/// Restore files from the last snapshot and remove the snapshot file.
pub fn restore(root: &Path) -> Result<usize> {
  let snap = FixRollback::load(root)?
    .filter(|s| !s.is_empty())
    .context("no fix rollback snapshot — run `noslop fix` first, or use git")?;

  let mut restored = 0usize;
  for (rel, content) in &snap.files {
    let abs = root.join(rel);
    if let Some(parent) = abs.parent() {
      std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&abs, content).with_context(|| format!("restore {}", abs.display()))?;
    restored += 1;
  }
  FixRollback::clear(root)?;
  Ok(restored)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;

  #[test]
  fn round_trip_snapshot_and_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let rel = PathBuf::from("src/a.ts");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join(&rel), "original\n").unwrap();

    let snap = FixRollback {
      files: [(rel.clone(), "original\n".to_string())]
        .into_iter()
        .collect(),
    };
    snap.save(root).unwrap();
    fs::write(root.join(&rel), "broken\n").unwrap();

    let n = restore(root).unwrap();
    assert_eq!(n, 1);
    assert_eq!(fs::read_to_string(root.join(&rel)).unwrap(), "original\n");
    assert!(!FixRollback::path(root).exists());
  }
}
