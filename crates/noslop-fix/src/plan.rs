//! Build a fix plan from scan findings.

use noslop_graph::{Confidence, FileFacts, Finding, RuleId, Span};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FixOptions {
  pub dry_run: bool,
  pub min_confidence: Confidence,
  pub include_deps: bool,
}

impl Default for FixOptions {
  fn default() -> Self {
    Self {
      dry_run: true,
      min_confidence: Confidence::High,
      include_deps: false,
    }
  }
}

#[derive(Debug, Clone)]
pub enum FixAction {
  DeleteFile { path: PathBuf },
  RemoveImportNames {
    path: PathBuf,
    span: Span,
    locals: HashSet<String>,
  },
  RemoveLines {
    path: PathBuf,
    start: u32,
    end: u32,
  },
  RemoveDependency { manifest: PathBuf, dep: String },
}

#[derive(Debug, Default)]
pub struct FixPlan {
  pub actions: Vec<FixAction>,
  pub skipped: usize,
}

pub fn plan(findings: &[Finding], facts: &[FileFacts], opts: &FixOptions) -> FixPlan {
  let mut out = FixPlan::default();
  let facts_by_path: HashMap<&Path, &FileFacts> =
    facts.iter().map(|f| (f.path.as_path(), f)).collect();

  let mut file_deletes: HashSet<PathBuf> = HashSet::new();
  let mut import_edits: HashMap<(PathBuf, Span), HashSet<String>> = HashMap::new();
  let mut line_removals: Vec<(PathBuf, u32, u32)> = Vec::new();
  let mut dep_removals: Vec<(PathBuf, String)> = Vec::new();

  for finding in findings {
    if !is_fixable(finding, opts) {
      out.skipped += 1;
      continue;
    }
    match finding.rule {
      RuleId::UnusedFile => {
        file_deletes.insert(finding.file.clone());
      }
      RuleId::UnusedImport => {
        let Some(local) = parse_import_local(&finding.message) else {
          out.skipped += 1;
          continue;
        };
        let Some(facts) = facts_by_path.get(finding.file.as_path()) else {
          out.skipped += 1;
          continue;
        };
        let Some(import) = find_import(facts, finding, &local) else {
          out.skipped += 1;
          continue;
        };
        import_edits
          .entry((finding.file.clone(), import.span))
          .or_default()
          .insert(local);
      }
      RuleId::UnusedExport | RuleId::UnusedType => {
        line_removals.push((
          finding.file.clone(),
          finding.span.start_line,
          finding.span.end_line,
        ));
      }
      RuleId::UnusedDependency if opts.include_deps => {
        let Some(dep) = parse_dependency_name(&finding.message) else {
          out.skipped += 1;
          continue;
        };
        dep_removals.push((finding.file.clone(), dep));
      }
      _ => out.skipped += 1,
    }
  }

  for path in file_deletes {
    out.actions.push(FixAction::DeleteFile { path });
  }
  for ((path, span), locals) in import_edits {
    out.actions.push(FixAction::RemoveImportNames { path, span, locals });
  }
  for (path, start, end) in line_removals {
    out.actions.push(FixAction::RemoveLines { path, start, end });
  }
  for (manifest, dep) in dep_removals {
    out.actions.push(FixAction::RemoveDependency { manifest, dep });
  }

  out.actions.sort_by(|a, b| action_sort_key(a).cmp(&action_sort_key(b)));
  out
}

/// Repo-relative paths that a plan will modify or delete.
pub fn affected_paths(plan: &FixPlan) -> Vec<PathBuf> {
  let mut paths: HashSet<PathBuf> = HashSet::new();
  for action in &plan.actions {
    match action {
      FixAction::DeleteFile { path }
      | FixAction::RemoveImportNames { path, .. }
      | FixAction::RemoveLines { path, .. } => {
        paths.insert(path.clone());
      }
      FixAction::RemoveDependency { manifest, .. } => {
        paths.insert(manifest.clone());
      }
    }
  }
  let mut out: Vec<_> = paths.into_iter().collect();
  out.sort();
  out
}

fn is_fixable(finding: &Finding, opts: &FixOptions) -> bool {
  if finding.confidence < opts.min_confidence {
    return false;
  }
  match finding.rule {
    RuleId::UnusedFile | RuleId::UnusedImport | RuleId::UnusedExport | RuleId::UnusedType => true,
    RuleId::UnusedDependency => opts.include_deps && finding.confidence >= Confidence::Medium,
    _ => false,
  }
}

fn find_import<'a>(facts: &'a FileFacts, finding: &Finding, local: &str) -> Option<&'a noslop_graph::RawImport> {
  facts.imports.iter().find(|imp| {
    spans_overlap(imp.span, finding.span)
      && imp.names.iter().any(|n| n.local == local)
  })
}

fn spans_overlap(a: Span, b: Span) -> bool {
  a.start_line <= b.end_line && b.start_line <= a.end_line
}

fn action_sort_key(action: &FixAction) -> (u8, PathBuf, u32) {
  match action {
    FixAction::DeleteFile { path } => (0, path.clone(), 0),
    FixAction::RemoveImportNames { path, span, .. } => (1, path.clone(), span.start_line),
    FixAction::RemoveLines { path, start, .. } => (2, path.clone(), *start),
    FixAction::RemoveDependency { manifest, .. } => (3, manifest.clone(), 0),
  }
}

fn parse_import_local(message: &str) -> Option<String> {
  let start = message.find('\'')? + 1;
  let rest = &message[start..];
  let end = rest.find('\'')?;
  Some(rest[..end].to_string())
}

fn parse_dependency_name(message: &str) -> Option<String> {
  let start = message.find('\'')? + 1;
  let rest = &message[start..];
  let end = rest.find('\'')?;
  Some(rest[..end].to_string())
}
