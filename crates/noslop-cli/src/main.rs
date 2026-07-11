//! `noslop` — the CLI. Thin by design (ARCHITECTURE.md §4): argument parsing,
//! output selection, and exit codes. All real work lives in `noslop-core`.

mod explain;
mod fix_cmd;
mod init;
mod watch;

use clap::{Parser, Subcommand, ValueEnum};
use noslop_core::{scan, ScanOptions};
use noslop_graph::{RuleId, Severity};
use noslop_report::Report;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Deterministic polyglot dead-code, cycle, and dependency analysis for
/// TypeScript and Python.
#[derive(Parser)]
#[command(name = "noslop", version, about)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    global: GlobalArgs,
}

#[derive(clap::Args)]
pub(crate) struct GlobalArgs {
    /// Repository root to scan.
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    /// Output format.
    #[arg(long, global = true, value_enum, default_value_t = Format::Pretty)]
    format: Format,
    /// Show Medium/Low-confidence findings (default: High only).
    #[arg(long, global = true)]
    all: bool,
    /// Restrict to a comma-separated set of rules.
    #[arg(long, global = true, value_delimiter = ',')]
    filter: Vec<String>,
    /// Worker thread count (default: number of CPUs).
    #[arg(long, global = true)]
    threads: Option<usize>,
    /// Bypass the on-disk parse cache.
    #[arg(long, global = true)]
    no_cache: bool,
    /// Apply high-confidence auto-fixes after the scan.
    #[arg(long, global = true)]
    fix: bool,
    /// Preview fixes without writing (use with `--fix` or `noslop fix`).
    #[arg(long, global = true)]
    dry_run: bool,
    /// Also remove unused dependencies (Medium confidence; use with `--fix`).
    #[arg(long, global = true)]
    include_deps: bool,
    /// Re-scan when files change (debounced).
    #[arg(long, global = true)]
    watch: bool,
}

#[derive(Copy, Clone, ValueEnum)]
enum Format {
    Pretty,
    Json,
    Sarif,
    Github,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Dead-code findings only (files, exports, imports, test-only).
    Dead,
    /// Circular import groups.
    Cycles,
    /// Unused declared dependencies.
    Deps,
    /// Duplicate-code clones (force-enables duplication for this run).
    Dupes,
    /// CI ratchet: fail only on findings new since the baseline.
    Audit {
        /// Base git ref (informational; the ratchet keys on the baseline file).
        #[arg(long, default_value = "main")]
        base: String,
    },
    /// Manage the accepted-legacy baseline.
    Baseline {
        #[command(subcommand)]
        action: BaselineAction,
    },
    /// Explain what a rule means, why it fires, and how to suppress it.
    Explain {
        /// Rule name, e.g. `unused-file`.
        rule: String,
    },
    /// Generate a noslop.toml annotated with detected plugins and entry points.
    Init,
    /// Auto-delete dead files, strip unused imports/exports, remove unused deps.
    Fix {
        #[command(subcommand)]
        action: Option<FixAction>,
        /// Preview changes without writing.
        #[arg(long)]
        dry_run: bool,
        /// Also remove unused dependencies (Medium confidence).
        #[arg(long)]
        include_deps: bool,
    },
    /// Re-scan on file save (same as `--watch`).
    Watch,
}

#[derive(Subcommand)]
enum FixAction {
    /// Undo the last `noslop fix` run (restores `.noslopcode/fix-rollback.json`).
    Restore,
}

#[derive(Subcommand)]
enum BaselineAction {
    /// Snapshot current findings as accepted legacy.
    Update,
}

const BASELINE_REL: &str = ".noslopcode/baseline.json";

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("noslop: error: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    if matches!(cli.command, Some(Command::Watch)) || cli.global.watch {
        watch::run(&cli)?;
        return Ok(ExitCode::SUCCESS);
    }

    match &cli.command {
        Some(Command::Fix {
            action: Some(FixAction::Restore),
            ..
        }) => {
            fix_cmd::run_restore(&cli.global.root)?;
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Explain { rule }) => {
            println!("{}", explain::explain(rule));
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Init) => {
            init::run(&cli.global.root)?;
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }

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

    let fail_on = noslop_core::fail_on(&cli.global.root);
    let root = cli.global.root.clone();

    match &cli.command {
        Some(Command::Baseline {
            action: BaselineAction::Update,
        }) => {
            write_baseline(&root, &report)?;
            println!(
                "Baseline updated: {} finding(s) accepted as legacy.",
                report.findings.len()
            );
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Audit { base }) => {
            let baseline = load_baseline(&root);
            let accepted = report.subtract_baseline(&baseline);
            emit(&report, &cli.global, outcome.elapsed_ms, outcome.warm_cache);
            eprintln!(
                "audit against '{base}': {} new finding(s), {accepted} accepted from baseline.",
                report.findings.len()
            );
            return Ok(exit_for(&report, fail_on));
        }
        Some(Command::Fix {
            action: None,
            dry_run,
            include_deps,
        }) => {
            fix_cmd::run_fix(
                &root,
                &report.findings,
                &outcome.facts,
                &fix_cmd::FixRunOptions {
                    dry_run: *dry_run,
                    include_deps: *include_deps,
                },
            )?;
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }

    emit(&report, &cli.global, outcome.elapsed_ms, outcome.warm_cache);

    if cli.global.fix {
        fix_cmd::run_fix(
            &root,
            &report.findings,
            &outcome.facts,
            &fix_cmd::FixRunOptions {
                dry_run: cli.global.dry_run,
                include_deps: cli.global.include_deps,
            },
        )?;
    }

    Ok(exit_for(&report, fail_on))
}

pub(crate) fn emit(report: &Report, global: &GlobalArgs, elapsed_ms: u128, warm_cache: bool) {
    match global.format {
        Format::Pretty => {
            print!("{}", report.to_pretty(global.all, elapsed_ms, warm_cache))
        }
        Format::Json => println!("{}", report.to_json()),
        Format::Sarif => println!("{}", report.to_sarif()),
        Format::Github => print!("{}", report.to_github()),
    }
}

pub(crate) fn exit_for(report: &Report, fail_on: Severity) -> ExitCode {
    ExitCode::from(report.exit_code(fail_on) as u8)
}

pub(crate) fn rule_filter(
    command: &Option<Command>,
    filter: &[String],
) -> anyhow::Result<Option<Vec<RuleId>>> {
    let base: Option<Vec<RuleId>> = match command {
        Some(Command::Dead) => Some(vec![
            RuleId::UnusedFile,
            RuleId::UnusedExport,
            RuleId::UnusedImport,
            RuleId::OnlyUsedInTests,
        ]),
        Some(Command::Cycles) => Some(vec![RuleId::CircularImports]),
        Some(Command::Deps) => Some(vec![RuleId::UnusedDependency]),
        Some(Command::Dupes) => Some(vec![RuleId::DuplicateCode]),
        Some(Command::Fix { .. }) => Some(vec![
            RuleId::UnusedFile,
            RuleId::UnusedExport,
            RuleId::UnusedType,
            RuleId::UnusedImport,
            RuleId::UnusedDependency,
        ]),
        _ => None,
    };

    if filter.is_empty() {
        return Ok(base);
    }

    let mut wanted = HashSet::new();
    for name in filter {
        wanted.insert(parse_rule(name)?);
    }
    let combined = match base {
        Some(base) => base.into_iter().filter(|r| wanted.contains(r)).collect(),
        None => wanted.into_iter().collect(),
    };
    Ok(Some(combined))
}

fn parse_rule(name: &str) -> anyhow::Result<RuleId> {
    let rule = match name.trim() {
        "unused-file" => RuleId::UnusedFile,
        "unused-export" => RuleId::UnusedExport,
        "unused-type" => RuleId::UnusedType,
        "unused-import" => RuleId::UnusedImport,
        "unused-enum-member" => RuleId::UnusedEnumMember,
        "unused-class-member" => RuleId::UnusedClassMember,
        "unused-parameter" => RuleId::UnusedParameter,
        "expected-unused-but-used" => RuleId::ExpectedUnusedButUsed,
        "missing-suppression-reason" => RuleId::MissingSuppressionReason,
        "high-complexity" => RuleId::HighComplexity,
        "large-function" => RuleId::LargeFunction,
        "banned-import" => RuleId::BannedImport,
        "banned-call" => RuleId::BannedCall,
        "banned-effect" => RuleId::BannedEffect,
        "boundary-violation" => RuleId::BoundaryViolation,
        "duplicate-code" => RuleId::DuplicateCode,
        "unused-css-token" => RuleId::UnusedCssToken,
        "broken-css-reference" => RuleId::BrokenCssReference,
        "unused-css-class" => RuleId::UnusedCssClass,
        "unused-dependency" => RuleId::UnusedDependency,
        "circular-imports" => RuleId::CircularImports,
        "only-used-in-tests" => RuleId::OnlyUsedInTests,
        other => anyhow::bail!("unknown rule '{other}'"),
    };
    Ok(rule)
}

fn write_baseline(root: &Path, report: &Report) -> anyhow::Result<()> {
    let path = root.join(BASELINE_REL);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&report.baseline_keys())?;
    std::fs::write(path, json)?;
    Ok(())
}

fn load_baseline(root: &Path) -> HashSet<String> {
    let path = root.join(BASELINE_REL);
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}
