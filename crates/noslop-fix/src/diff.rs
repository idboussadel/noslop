//! Unified diff output for dry-run previews.

use std::path::Path;

/// Minimal unified diff between two texts (line-based).
pub fn unified_diff(path: &Path, old: &str, new: &str) -> String {
  let old_lines: Vec<&str> = old.lines().collect();
  let new_lines: Vec<&str> = new.lines().collect();
  let mut out = format!("--- a/{}\n+++ b/{}\n", path.display(), path.display());

  if old == new {
    return out;
  }

  // ponytail: single-hunk diff is enough for fix previews; full Myers is overkill.
  let start = old_lines
    .iter()
    .zip(new_lines.iter())
    .position(|(a, b)| a != b)
    .unwrap_or(0);
  let old_tail = old_lines.len().saturating_sub(
    old_lines
      .iter()
      .rev()
      .zip(new_lines.iter().rev())
      .take_while(|(a, b)| a == b)
      .count(),
  );
  let new_tail = new_lines.len().saturating_sub(
    new_lines
      .iter()
      .rev()
      .zip(old_lines.iter().rev())
      .take_while(|(a, b)| a == b)
      .count(),
  );

  out.push_str(&format!(
    "@@ -{},{} +{},{} @@\n",
    start + 1,
    old_tail.saturating_sub(start),
    start + 1,
    new_tail.saturating_sub(start)
  ));
  for line in &old_lines[start..old_tail] {
    out.push('-');
    out.push_str(line);
    out.push('\n');
  }
  for line in &new_lines[start..new_tail] {
    out.push('+');
    out.push_str(line);
    out.push('\n');
  }
  out
}

pub fn delete_diff(path: &Path) -> String {
  format!("--- a/{}\n+++ /dev/null\n(deleted)\n", path.display())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::Path;

  #[test]
  fn shows_removed_line() {
    let diff = unified_diff(
      Path::new("x.ts"),
      "keep\nremove\n",
      "keep\n",
    );
    assert!(diff.contains("-remove"));
  }
}
