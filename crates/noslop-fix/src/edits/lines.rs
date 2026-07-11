//! Line-range helpers for in-place source edits.

/// Remove 1-based inclusive line range `[start, end]`. Returns `None` if nothing remains.
pub fn remove_line_range(source: &str, start: u32, end: u32) -> Option<String> {
  let lines: Vec<&str> = source.lines().collect();
  if lines.is_empty() || start == 0 || start > lines.len() as u32 {
    return None;
  }
  let start_idx = (start - 1) as usize;
  let end_idx = ((end as usize).min(lines.len())).saturating_sub(1);
  if start_idx > end_idx {
    return None;
  }
  let mut out: Vec<&str> = lines[..start_idx].to_vec();
  out.extend_from_slice(&lines[end_idx + 1..]);
  if out.is_empty() {
    return None;
  }
  Some(join_lines(&out, source.ends_with('\n')))
}

/// Replace 1-based inclusive line range with `replacement` (may be multiple lines).
pub fn replace_line_range(source: &str, start: u32, end: u32, replacement: &str) -> String {
  let lines: Vec<&str> = source.lines().collect();
  let start_idx = (start.saturating_sub(1)) as usize;
  let end_idx = ((end as usize).min(lines.len())).saturating_sub(1);
  let mut out: Vec<&str> = lines[..start_idx].to_vec();
  out.extend(replacement.lines());
  if end_idx + 1 < lines.len() {
    out.extend_from_slice(&lines[end_idx + 1..]);
  }
  join_lines(&out, source.ends_with('\n'))
}

fn join_lines(lines: &[&str], trailing_newline: bool) -> String {
  let mut out = lines.join("\n");
  if trailing_newline && !out.is_empty() {
    out.push('\n');
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn removes_single_line() {
    let src = "a\nb\nc\n";
    let out = remove_line_range(src, 2, 2).unwrap();
    assert_eq!(out, "a\nc\n");
  }

  #[test]
  fn removes_multiline_block() {
    let src = "keep\nexport function dead() {\n  return 1;\n}\nkeep2\n";
    let out = remove_line_range(src, 2, 4).unwrap();
    assert_eq!(out, "keep\nkeep2\n");
  }
}
