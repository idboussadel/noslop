//! `noslop fix` — apply high-confidence auto-fixes from scan findings.

use noslop_fix::{fix, FixOptions};
use noslop_graph::{Confidence, RuleId};
use std::path::Path;

pub struct FixRunOptions {
  pub dry_run: bool,
  pub include_deps: bool,
}

pub fn run_fix(
  root: &Path,
  findings: &[noslop_graph::Finding],
  facts: &[noslop_graph::FileFacts],
  opts: &FixRunOptions,
) -> anyhow::Result<()> {
  let fixable: Vec<_> = findings
    .iter()
    .filter(|f| is_autofix_rule(f.rule))
    .cloned()
    .collect();

  if fixable.is_empty() {
    println!("No high-confidence auto-fixable findings.");
    return Ok(());
  }

  let outcome = fix(
    root,
    &fixable,
    facts,
    &FixOptions {
      dry_run: opts.dry_run,
      min_confidence: Confidence::High,
      include_deps: opts.include_deps,
    },
  )?;

  for diff in &outcome.diffs {
    print!("{diff}");
  }
  for err in &outcome.errors {
    eprintln!("noslop fix: skipped: {err}");
  }

  if opts.dry_run {
    println!(
      "Dry run: {} change(s) previewed. Re-run with `noslop fix` (no --dry-run) to apply.",
      outcome.diffs.len()
    );
  } else {
    println!("Applied {} change(s).", outcome.applied);
    if outcome.applied > 0 {
      println!("Undo with: noslop fix restore  (or git checkout -- .)");
    }
  }

  Ok(())
}

pub fn run_restore(root: &Path) -> anyhow::Result<()> {
  let n = noslop_fix::restore(root)?;
  println!("Restored {n} file(s) from the last fix snapshot.");
  Ok(())
}

fn is_autofix_rule(rule: RuleId) -> bool {
  matches!(
    rule,
    RuleId::UnusedFile
      | RuleId::UnusedImport
      | RuleId::UnusedExport
      | RuleId::UnusedType
      | RuleId::UnusedDependency
  )
}
