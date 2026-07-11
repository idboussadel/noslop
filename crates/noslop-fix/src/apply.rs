//! Apply a fix plan to disk (or preview with diffs).

use crate::diff::{delete_diff, unified_diff};
use crate::edits::{patch_import, remove_dependency, remove_line_range};
use crate::plan::{affected_paths, FixAction, FixOptions, FixPlan};
use crate::snapshot;
use anyhow::{Context, Result};
use noslop_graph::{FileFacts, Span};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct FixOutcome {
  pub applied: usize,
  pub diffs: Vec<String>,
  pub errors: Vec<String>,
}

pub fn apply(
  root: &Path,
  plan: &FixPlan,
  facts: &[FileFacts],
  opts: &FixOptions,
) -> Result<FixOutcome> {
  let facts_by_path: HashMap<&Path, &FileFacts> =
    facts.iter().map(|f| (f.path.as_path(), f)).collect();

  let mut deletes: Vec<PathBuf> = Vec::new();
  let mut file_edits: HashMap<PathBuf, FileEditBatch> = HashMap::new();
  let mut manifests: Vec<(PathBuf, String)> = Vec::new();

  for action in &plan.actions {
    match action {
      FixAction::DeleteFile { path } => deletes.push(path.clone()),
      FixAction::RemoveImportNames { path, span, locals } => {
        file_edits
          .entry(path.clone())
          .or_default()
          .imports
          .entry(*span)
          .or_default()
          .extend(locals.iter().cloned());
      }
      FixAction::RemoveLines { path, start, end } => {
        file_edits
          .entry(path.clone())
          .or_default()
          .lines
          .push((*start, *end));
      }
      FixAction::RemoveDependency { manifest, dep } => {
        manifests.push((manifest.clone(), dep.clone()));
      }
    }
  }

  let mut outcome = FixOutcome::default();

  if !opts.dry_run {
    let paths = affected_paths(plan);
    if !paths.is_empty() {
      let snap = snapshot::capture(root, paths)?;
      snap.save(root)?;
    }
  }

  for path in deletes {
    let abs = root.join(&path);
    outcome.diffs.push(delete_diff(&path));
    if !opts.dry_run {
      std::fs::remove_file(&abs)
        .with_context(|| format!("delete {}", abs.display()))?;
      outcome.applied += 1;
    }
  }

  for (path, batch) in file_edits {
    let abs = root.join(&path);
    let old = std::fs::read_to_string(&abs)
      .with_context(|| format!("read {}", abs.display()))?;
    let facts = facts_by_path
      .get(path.as_path())
      .with_context(|| format!("no facts for {}", path.display()))?;

    let new_content = match apply_file_edits(&old, facts, &batch) {
      Ok(c) => c,
      Err(err) => {
        outcome.errors.push(format!("{}: {err:#}", path.display()));
        continue;
      }
    };

    if old != new_content {
      outcome.diffs.push(unified_diff(&path, &old, &new_content));
      if !opts.dry_run {
        std::fs::write(&abs, &new_content)
          .with_context(|| format!("write {}", abs.display()))?;
        outcome.applied += 1;
      }
    }
  }

  for (manifest, dep) in manifests {
    let abs = root.join(&manifest);
    let old = std::fs::read_to_string(&abs)
      .with_context(|| format!("read {}", abs.display()))?;
    match remove_dependency(&manifest, &old, &dep) {
      Ok(new_content) => {
        if old != new_content {
          outcome.diffs.push(unified_diff(&manifest, &old, &new_content));
          if !opts.dry_run {
            std::fs::write(&abs, &new_content)
              .with_context(|| format!("write {}", abs.display()))?;
            outcome.applied += 1;
          }
        }
      }
      Err(err) => outcome.errors.push(format!("{}: {err:#}", manifest.display())),
    }
  }

  Ok(outcome)
}

#[derive(Default)]
struct FileEditBatch {
  imports: HashMap<Span, HashSet<String>>,
  lines: Vec<(u32, u32)>,
}

fn apply_file_edits(
  source: &str,
  facts: &FileFacts,
  batch: &FileEditBatch,
) -> Result<String> {
  let mut content = source.to_string();

  for (span, locals) in &batch.imports {
    let import = facts
      .imports
      .iter()
      .find(|imp| imp.span == *span)
      .with_context(|| "import span missing after replan")?;
    content = match patch_import(&content, import, locals) {
      Some(next) => next,
      None => remove_line_range(&content, import.span.start_line, import.span.end_line)
        .with_context(|| format!("remove import at lines {}-{}", import.span.start_line, import.span.end_line))?,
    };
  }

  let mut line_removals = batch.lines.clone();
  line_removals.sort_by(|a, b| b.0.cmp(&a.0));
  for (start, end) in &line_removals {
    content = remove_line_range(&content, *start, *end)
      .with_context(|| format!("line removal {start}-{end} failed"))?;
  }

  Ok(content)
}

pub fn fix(
  root: &Path,
  findings: &[noslop_graph::Finding],
  facts: &[FileFacts],
  opts: &FixOptions,
) -> Result<FixOutcome> {
  let plan = crate::plan::plan(findings, facts, opts);
  apply(root, &plan, facts, opts)
}
