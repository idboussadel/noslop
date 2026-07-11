//! `noslop watch` — re-scan on file save with debounced incremental cache.

use crate::{emit, rule_filter, Cli, Command};
use noslop_core::{scan, ScanOptions};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(300);

pub fn run(cli: &Cli) -> anyhow::Result<()> {
  let root = std::fs::canonicalize(&cli.global.root)?;
  let (tx, rx) = mpsc::channel();
  let mut watcher = RecommendedWatcher::new(
    move |res| {
      let _ = tx.send(res);
    },
    Config::default(),
  )?;
  watcher.watch(&root, RecursiveMode::Recursive)?;

  eprintln!("Watching {} — save a file to re-scan (Ctrl+C to stop).\n", root.display());

  let mut pending = false;
  let mut last_change = Instant::now();

  scan_and_print(cli)?;

  loop {
    match rx.recv_timeout(Duration::from_millis(100)) {
      Ok(Ok(event)) if should_rescan(&event) => {
        pending = true;
        last_change = Instant::now();
      }
      Ok(Ok(_)) => {}
      Ok(Err(err)) => eprintln!("watch error: {err}"),
      Err(mpsc::RecvTimeoutError::Timeout) => {}
      Err(mpsc::RecvTimeoutError::Disconnected) => break,
    }

    if pending && last_change.elapsed() >= DEBOUNCE {
      pending = false;
      eprintln!("\n--- change detected, rescanning ---\n");
      scan_and_print(cli)?;
    }
  }

  Ok(())
}

fn scan_and_print(cli: &Cli) -> anyhow::Result<()> {
  let outcome = scan(&ScanOptions {
    root: cli.global.root.clone(),
    use_cache: !cli.global.no_cache,
    threads: cli.global.threads,
    force_duplication: matches!(cli.command, Some(Command::Dupes)),
  })?;

  let mut report = outcome.report;
  let rules = rule_filter(&cli.command, &cli.global.filter)?;
  if let Some(rules) = &rules {
    report = report.filtered(rules);
  }

  emit(
    &report,
    &cli.global,
    outcome.elapsed_ms,
    outcome.warm_cache,
  );

  if cli.global.fix {
    crate::fix_cmd::run_fix(
      &cli.global.root,
      &report.findings,
      &outcome.facts,
      &crate::fix_cmd::FixRunOptions {
        dry_run: cli.global.dry_run,
        include_deps: cli.global.include_deps,
      },
    )?;
  }

  let fail_on = noslop_core::fail_on(&cli.global.root);
  let code = report.exit_code(fail_on);
  if code != 0 {
    std::process::exit(code);
  }
  Ok(())
}

fn should_rescan(event: &notify::Event) -> bool {
  match &event.kind {
    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {}
    _ => return false,
  }
  event.paths.iter().any(|p| {
    p.extension()
      .and_then(|e| e.to_str())
      .is_some_and(|ext| matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "css" | "json" | "toml"))
      && !p.to_string_lossy().contains("/node_modules/")
      && !p.to_string_lossy().contains("/.git/")
      && !p.to_string_lossy().contains("/target/")
      && !p.to_string_lossy().contains("/.noslopcode/")
  })
}
