//! Remove unused import bindings from source text.

use noslop_graph::RawImport;
use std::collections::HashSet;

/// Drop `remove_locals` from the import statement spanning `import.span`.
/// Returns `None` when the entire statement should be deleted.
pub fn patch_import(source: &str, import: &RawImport, remove_locals: &HashSet<String>) -> Option<String> {
  let lines: Vec<&str> = source.lines().collect();
  let start = import.span.start_line.saturating_sub(1) as usize;
  let end = (import.span.end_line as usize).min(lines.len()).saturating_sub(1);
  if start > end || start >= lines.len() {
    return None;
  }

  let block = lines[start..=end].join("\n");
  let patched = patch_import_block(&block, import, remove_locals)?;
  let mut out = crate::edits::lines::replace_line_range(
    source,
    import.span.start_line,
    import.span.end_line,
    &patched,
  );
  while out.contains("\n\n\n") {
    out = out.replace("\n\n\n", "\n\n");
  }
  Some(out)
}

fn patch_import_block(block: &str, import: &RawImport, remove_locals: &HashSet<String>) -> Option<String> {
  let trimmed = block.trim();
  if trimmed.is_empty() {
    return None;
  }

  if trimmed.starts_with("from ") {
    return patch_python_from_import(trimmed, remove_locals);
  }
  if trimmed.starts_with("import ") && !trimmed.contains(" from ") {
    return patch_python_import(trimmed, remove_locals);
  }

  patch_ts_import(trimmed, import, remove_locals)
}

fn patch_python_from_import(stmt: &str, remove_locals: &HashSet<String>) -> Option<String> {
  let rest = stmt.strip_prefix("from ")?;
  let (module, names) = rest.split_once(" import ")?;
  if names.trim() == "*" {
    return None;
  }
  let kept = filter_import_names(names, remove_locals);
  if kept.is_empty() {
    return None;
  }
  Some(format!("from {module} import {kept}"))
}

fn patch_python_import(stmt: &str, remove_locals: &HashSet<String>) -> Option<String> {
  let names = stmt.strip_prefix("import ")?.trim();
  let kept = filter_import_names(names, remove_locals);
  if kept.is_empty() {
    return None;
  }
  Some(format!("import {kept}"))
}

fn patch_ts_import(stmt: &str, import: &RawImport, remove_locals: &HashSet<String>) -> Option<String> {
  if import.is_namespace {
    let local = import.names.first().map(|n| n.local.as_str()).unwrap_or("");
    return if remove_locals.contains(local) {
      None
    } else {
      Some(stmt.to_string())
    };
  }

  if import.names.len() == 1 && import.names[0].imported == "default" {
    return if remove_locals.contains(&import.names[0].local) {
      None
    } else {
      Some(stmt.to_string())
    };
  }

  let brace_start = stmt.find('{')?;
  let brace_end = stmt.rfind('}')?;
  let from_idx = stmt[brace_end..].find(" from ").map(|i| brace_end + i)?;
  let inside = &stmt[brace_start + 1..brace_end];
  let kept = filter_import_names(inside, remove_locals);
  if kept.is_empty() {
    return None;
  }
  let prefix = &stmt[..=brace_start];
  let suffix = &stmt[from_idx..];
  Some(format!("{prefix}{kept}}}{suffix}"))
}

fn filter_import_names(names: &str, remove_locals: &HashSet<String>) -> String {
  names
    .split(',')
    .filter_map(|part| {
      let part = part.trim();
      if part.is_empty() {
        return None;
      }
      let local = part
        .split(" as ")
        .last()
        .unwrap_or(part)
        .trim()
        .trim_start_matches("type ")
        .trim();
      if remove_locals.contains(local) {
        None
      } else {
        Some(part.to_string())
      }
    })
    .collect::<Vec<_>>()
    .join(", ")
}

#[cfg(test)]
mod tests {
  use super::*;
  use noslop_graph::{ImportKind, ImportedName, Span};

  fn import(span: Span, names: Vec<(&str, &str)>, specifier: &str) -> RawImport {
    RawImport {
      specifier: specifier.to_string(),
      names: names
        .into_iter()
        .map(|(imported, local)| ImportedName {
          imported: imported.to_string(),
          local: local.to_string(),
        })
        .collect(),
      kind: ImportKind::Static,
      is_namespace: false,
      is_type_only: false,
      is_reexport: false,
      span,
    }
  }

  #[test]
  fn removes_named_ts_import_binding() {
    let src = "import { formatPrice, unusedName } from \"@/lib/format\";\n";
    let imp = import(
      Span::new(1, 1),
      vec![("formatPrice", "formatPrice"), ("unusedName", "unusedName")],
      "@/lib/format",
    );
    let mut remove = HashSet::new();
    remove.insert("unusedName".to_string());
    let out = patch_import(src, &imp, &remove).unwrap();
    assert!(out.contains("formatPrice"));
    assert!(!out.contains("unusedName"));
  }

  #[test]
  fn removes_whole_line_when_last_binding_gone() {
    let src = "import { unusedName } from \"@/lib/format\";\n";
    let imp = import(Span::new(1, 1), vec![("unusedName", "unusedName")], "@/lib/format");
    let mut remove = HashSet::new();
    remove.insert("unusedName".to_string());
    assert!(patch_import(src, &imp, &remove).is_none());
  }
}
