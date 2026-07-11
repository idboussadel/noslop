//! Terminal renderer — react-doctor × Fallow DNA (ARCHITECTURE.md Appendix B):
//! a graded one-line summary, then sectioned findings with suppress hints and
//! `noslop explain` pointers. Honors `NO_COLOR`.

use crate::{Report, ScanRootReport};
use noslop_graph::{complexity_band, ComplexityMetrics, Confidence, Finding, RuleId};
use owo_colors::OwoColorize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

/// Fixed section order — dead code first, the headline of the tool.
const RULE_ORDER: &[RuleId] = &[
    RuleId::UnusedFile,
    RuleId::UnusedExport,
    RuleId::UnusedType,
    RuleId::UnusedImport,
    RuleId::UnusedEnumMember,
    RuleId::UnusedClassMember,
    RuleId::UnusedParameter,
    RuleId::ExpectedUnusedButUsed,
    RuleId::MissingSuppressionReason,
    RuleId::HighComplexity,
    RuleId::LargeFunction,
    RuleId::BannedImport,
    RuleId::BannedCall,
    RuleId::BannedEffect,
    RuleId::BoundaryViolation,
    RuleId::DuplicateCode,
    RuleId::UnusedCssToken,
    RuleId::BrokenCssReference,
    RuleId::UnusedCssClass,
    RuleId::CircularImports,
    RuleId::UnusedDependency,
    RuleId::OnlyUsedInTests,
];

/// Symbol rules where multiple hits in one file collapse under a single path header.
const GROUP_BY_FILE: &[RuleId] = &[
    RuleId::UnusedExport,
    RuleId::UnusedType,
    RuleId::UnusedImport,
    RuleId::UnusedEnumMember,
    RuleId::UnusedClassMember,
    RuleId::UnusedParameter,
];

pub(crate) fn render(
    report: &Report,
    show_all: bool,
    elapsed_ms: u128,
    warm_cache: bool,
) -> String {
    let color = std::env::var_os("NO_COLOR").is_none();
    let mut out = String::new();

    header(&mut out, report, elapsed_ms, warm_cache, color);
    summary_line(&mut out, report, color);
    render_refactor_targets(&mut out, report, color);

    let visible: Vec<&Finding> = report.visible(show_all).collect();
    let mut shown_rules = HashSet::new();
    let any_section = if report.scan_roots.len() > 1 {
        render_by_workspace(&mut out, report, &visible, color, &mut shown_rules)
    } else {
        render_findings(&mut out, &visible, color, &mut shown_rules)
    };

    if !any_section {
        let msg = "  ✓  No findings above the visibility threshold.";
        let _ = writeln!(out, "\n{}", paint(msg, color, Style::Success));
    }

    if !show_all {
        let hidden = report
            .findings
            .iter()
            .filter(|f| f.confidence != Confidence::High)
            .count();
        if hidden > 0 {
            let note = format!(
                "  {hidden} lower-confidence finding(s) hidden — run with --all to see them."
            );
            let _ = writeln!(out, "\n{}", paint(&note, color, Style::Muted));
        }
    }
    if report.suppressed_count > 0 {
        let note = format!(
            "  {} suppressed by noslop-ignore comments.",
            report.suppressed_count
        );
        let _ = writeln!(out, "{}", paint(&note, color, Style::Muted));
    }

    if !shown_rules.is_empty() {
        render_help_footer(&mut out, &shown_rules, color);
    }

    out
}

fn header(out: &mut String, report: &Report, elapsed_ms: u128, warm_cache: bool, color: bool) {
    let cache = if warm_cache {
        "warm cache"
    } else {
        "cold cache"
    };
    let roots = report.scan_roots.len();
    let version = paint(&format!("v{}", report.tool_version), color, Style::Muted);
    let _ = writeln!(
        out,
        "\n  {} {}",
        paint("noslopcode", color, Style::Brand),
        version
    );
    let unit = if roots == 1 {
        "scan root"
    } else {
        "workspaces"
    };
    let meta = format!(
        "{roots} {unit} · {} files · {cache} · {:.1}s",
        report.metrics.files,
        elapsed_ms as f64 / 1000.0,
    );
    let _ = writeln!(out, "  {}", paint(&meta, color, Style::Muted));
}

fn summary_line(out: &mut String, report: &Report, color: bool) {
    let m = &report.metrics;
    let label = if report.scan_roots.len() > 1 {
        "repo"
    } else {
        ""
    };
    let prefix = if label.is_empty() {
        String::new()
    } else {
        format!("{label} · ")
    };
    let _ = writeln!(
        out,
        "\n  {prefix}{} {}  {}  {} {}  {}  {} {}  {}  {} {}  {}  {} {}",
        paint("health", color, Style::Muted),
        paint(
            &format!("{} ({:.1})", report.health.grade, report.health.score),
            color,
            Style::Metric
        ),
        paint("·", color, Style::Muted),
        paint("dead files", color, Style::Muted),
        paint(
            &format!("{:.1}% ({})", m.dead_file_pct, m.dead_files),
            color,
            Style::Metric
        ),
        paint("·", color, Style::Muted),
        paint("exports", color, Style::Muted),
        paint(&m.dead_exports.to_string(), color, Style::Metric),
        paint("·", color, Style::Muted),
        paint("cycles", color, Style::Muted),
        paint(&m.cycles.to_string(), color, Style::Metric),
        paint("·", color, Style::Muted),
        paint("deps", color, Style::Muted),
        paint(&m.unused_dependencies.to_string(), color, Style::Metric),
    );
    out.push('\n');
}

fn render_refactor_targets(out: &mut String, report: &Report, color: bool) {
    let targets = &report.health.refactor_targets;
    if targets.is_empty() {
        return;
    }

    let first = &targets[0];
    let count = targets.len();
    let unit = if count == 1 {
        "refactor target"
    } else {
        "refactor targets"
    };
    let _ = writeln!(
        out,
        "  {} {} {} {}",
        paint(&count.to_string(), color, Style::Metric),
        paint(unit, color, Style::Muted),
        paint("- start with", color, Style::Muted),
        paint(&first.path.display().to_string(), color, Style::Path),
    );

    for target in targets.iter().take(3) {
        let reasons = target.reasons.join(", ");
        let detail = format!(
            "{} · {} · payoff {:.1} · {}",
            target.kind, target.effort, target.payoff, reasons
        );
        let _ = writeln!(
            out,
            "    {}. {}  {}",
            target.rank,
            paint(&target.path.display().to_string(), color, Style::Path),
            paint(&detail, color, Style::Muted),
        );
    }
    out.push('\n');
}

const RULE_W: usize = 62;

/// Longest package-root prefix wins — mirrors [`noslop_graph::Workspace::package_for`].
struct PackageMatcher {
    roots: Vec<(PathBuf, String)>,
    fallback: String,
}

impl PackageMatcher {
    fn new(scan_roots: &[ScanRootReport]) -> Self {
        let mut roots: Vec<(PathBuf, String)> = scan_roots
            .iter()
            .filter(|r| r.root != ".")
            .map(|r| (PathBuf::from(&r.root), r.package.clone()))
            .collect();
        roots.sort_by_key(|(root, _)| std::cmp::Reverse(root.as_os_str().len()));
        let fallback = scan_roots
            .iter()
            .find(|r| r.root == ".")
            .map(|r| r.package.clone())
            .unwrap_or_else(|| ".".to_string());
        Self { roots, fallback }
    }

    fn resolve(&self, path: &Path) -> &str {
        for (root, package) in &self.roots {
            if path.starts_with(root) {
                return package;
            }
        }
        &self.fallback
    }
}

fn group_findings_by_package<'a>(
    findings: &[&'a Finding],
    matcher: &PackageMatcher,
) -> HashMap<String, Vec<&'a Finding>> {
    let mut by_package: HashMap<String, Vec<&Finding>> = HashMap::new();
    for &finding in findings {
        by_package
            .entry(matcher.resolve(&finding.file).to_string())
            .or_default()
            .push(finding);
    }
    by_package
}

fn render_by_workspace(
    out: &mut String,
    report: &Report,
    visible: &[&Finding],
    color: bool,
    shown_rules: &mut HashSet<RuleId>,
) -> bool {
    let matcher = PackageMatcher::new(&report.scan_roots);
    let mut by_package = group_findings_by_package(visible, &matcher);
    let mut any_section = false;
    let mut first = true;

    for root in &report.scan_roots {
        let Some(findings) = by_package.remove(root.package.as_str()) else {
            continue;
        };
        if findings.is_empty() {
            continue;
        }
        if !first {
            out.push('\n');
        }
        first = false;
        workspace_banner(out, root, &findings, color);
        any_section |= render_findings(out, &findings, color, shown_rules);
    }

    for (package, findings) in by_package {
        if findings.is_empty() {
            continue;
        }
        if !first {
            out.push('\n');
        }
        first = false;
        workspace_banner_simple(out, &package, &findings, color);
        any_section |= render_findings(out, &findings, color, shown_rules);
    }

    any_section
}

fn workspace_banner(out: &mut String, root: &ScanRootReport, findings: &[&Finding], color: bool) {
    let _ = writeln!(out);
    let _ = writeln!(out, "  {}", paint(&root.root, color, Style::Accent));
    rule_line(out, color);
    let _ = writeln!(
        out,
        "    {}",
        paint(
            &format!("{} · {} files", root.language, root.files),
            color,
            Style::Muted,
        )
    );
    if !root.plugins.is_empty() {
        let _ = writeln!(
            out,
            "    {}",
            paint(&root.plugins.join(", "), color, Style::Muted)
        );
    }
    workspace_stats_line(out, root.files, findings, color);
    out.push('\n');
}

fn workspace_banner_simple(out: &mut String, package: &str, findings: &[&Finding], color: bool) {
    let _ = writeln!(out);
    let _ = writeln!(out, "  {}", paint(package, color, Style::Accent));
    rule_line(out, color);
    workspace_stats_line(out, 0, findings, color);
    out.push('\n');
}

fn rule_line(out: &mut String, color: bool) {
    let _ = writeln!(out, "  {}", paint(&"─".repeat(RULE_W), color, Style::Rule));
}

fn workspace_stats_line(out: &mut String, files: usize, findings: &[&Finding], color: bool) {
    let count = |rule: RuleId| findings.iter().filter(|f| f.rule == rule).count();
    let dead_files = count(RuleId::UnusedFile);
    let dead_file_pct = if files == 0 {
        0.0
    } else {
        (dead_files as f64 * 1000.0 / files as f64).round() / 10.0
    };
    let dead_exports = count(RuleId::UnusedExport);
    let cycles = count(RuleId::CircularImports);
    if files == 0 {
        let _ = writeln!(
            out,
            "    {} {}  {} {}  {}",
            paint("exports", color, Style::Muted),
            paint(&dead_exports.to_string(), color, Style::Metric),
            paint("·", color, Style::Muted),
            paint("cycles", color, Style::Muted),
            paint(&cycles.to_string(), color, Style::Metric),
        );
        return;
    }
    let _ = writeln!(
        out,
        "    {} {}  {} {}  {} {}  {} {}",
        paint("dead files", color, Style::Muted),
        paint(
            &format!("{dead_file_pct:.1}% ({dead_files})"),
            color,
            Style::Metric
        ),
        paint("·", color, Style::Muted),
        paint("exports", color, Style::Muted),
        paint(&dead_exports.to_string(), color, Style::Metric),
        paint("·", color, Style::Muted),
        paint("cycles", color, Style::Muted),
        paint(&cycles.to_string(), color, Style::Metric),
    );
}

fn render_findings(
    out: &mut String,
    visible: &[&Finding],
    color: bool,
    shown_rules: &mut HashSet<RuleId>,
) -> bool {
    let mut any_section = false;
    for &rule in RULE_ORDER {
        let group: Vec<&Finding> = visible.iter().copied().filter(|f| f.rule == rule).collect();
        if group.is_empty() {
            continue;
        }
        any_section = true;
        section(out, rule, &group, color, shown_rules);
    }
    any_section
}

fn section(
    out: &mut String,
    rule: RuleId,
    group: &[&Finding],
    color: bool,
    shown_rules: &mut HashSet<RuleId>,
) {
    shown_rules.insert(rule);
    let conf = confidence_hint(group);
    let title = if conf == "high confidence" {
        format!("{} ({})", rule_title(rule), group.len())
    } else {
        format!("{} ({}) · {conf}", rule_title(rule), group.len())
    };
    let _ = writeln!(out, "  {}", paint(&title, color, Style::Section));

    if GROUP_BY_FILE.contains(&rule) {
        render_grouped_by_file(out, rule, group, color);
    } else if rule == RuleId::HighComplexity {
        render_high_complexity_functions(out, group, color);
    } else if rule == RuleId::LargeFunction {
        render_large_functions(out, group, color);
    } else {
        render_flat(out, rule, group, color);
    }
    out.push('\n');
}

fn render_grouped_by_file(out: &mut String, _rule: RuleId, group: &[&Finding], color: bool) {
    let mut by_file: BTreeMap<&Path, Vec<&Finding>> = BTreeMap::new();
    for &f in group {
        by_file.entry(f.file.as_path()).or_default().push(f);
    }
    for (_, mut findings) in by_file {
        findings.sort_by_key(|f| f.span.start_line);
        if findings.len() == 1 {
            let _ = writeln!(out, "    {}", symbol_line(findings[0], color));
        } else {
            let path = findings[0].file.display().to_string();
            let _ = writeln!(out, "    {}", paint(&path, color, Style::Path));
            for f in findings {
                let _ = writeln!(out, "      {}", symbol_only(f, color));
            }
        }
    }
}

fn render_high_complexity_functions(out: &mut String, group: &[&Finding], color: bool) {
    let note = "CRAP scores are estimated from test reachability; pass Istanbul coverage for exact scores.";
    let _ = writeln!(out, "    {}", paint(note, color, Style::Muted));

    let mut sorted: Vec<&Finding> = group.to_vec();
    sorted.sort_by(|a, b| {
        crap_from_finding(b)
            .total_cmp(&crap_from_finding(a))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.span.start_line.cmp(&b.span.start_line))
    });

    let mut by_file: Vec<(&Path, Vec<&Finding>)> = Vec::new();
    for f in sorted {
        match by_file.last_mut() {
            Some((path, items)) if *path == f.file.as_path() => items.push(f),
            _ => by_file.push((f.file.as_path(), vec![f])),
        }
    }

    for (path, findings) in by_file {
        let _ = writeln!(
            out,
            "    {}",
            paint(&path.display().to_string(), color, Style::Path)
        );
        for f in findings {
            render_high_complexity_entry(out, f, color);
        }
    }
}

fn crap_from_finding(f: &Finding) -> f64 {
    ComplexityMetrics::decode(&f.reason)
        .map(|m| m.crap)
        .unwrap_or(0.0)
}

fn render_high_complexity_entry(out: &mut String, f: &Finding, color: bool) {
    let Some(m) = ComplexityMetrics::decode(&f.reason) else {
        let _ = writeln!(out, "      {}", line_for(f, color));
        return;
    };
    let fallback = f.file.display().to_string();
    let name = f.symbol.as_deref().map(symbol_label).unwrap_or(&fallback);
    let band = complexity_band(&m);
    let band_style = if band == "CRITICAL" {
        Style::Alert
    } else {
        Style::Accent
    };
    let _ = writeln!(
        out,
        "      :{} {} {}",
        paint(&f.span.start_line.to_string(), color, Style::Line),
        paint(name, color, Style::Symbol),
        paint(band, color, band_style),
    );
    let _ = writeln!(
        out,
        "            {} cyclomatic  {} cognitive  {} lines",
        paint(&m.cyclomatic.to_string(), color, Style::Metric),
        paint(&m.cognitive.to_string(), color, Style::Metric),
        paint(&m.loc.to_string(), color, Style::Metric),
    );
    let crap_line = format!("{:>11.1} CRAP", m.crap);
    let _ = writeln!(
        out,
        "           {}",
        paint(&crap_line, color, Style::Metric)
    );
}

fn render_large_functions(out: &mut String, group: &[&Finding], color: bool) {
    let mut sorted = group.to_vec();
    sorted.sort_by(|a, b| {
        function_span_loc(b)
            .cmp(&function_span_loc(a))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.span.start_line.cmp(&b.span.start_line))
    });
    for f in sorted {
        let _ = writeln!(out, "    {}", large_function_line(f, color));
    }
}

fn function_span_loc(f: &Finding) -> u32 {
    f.span.end_line.saturating_sub(f.span.start_line) + 1
}

fn large_function_line(f: &Finding, color: bool) -> String {
    let loc = function_span_loc(f);
    let fallback = f.file.display().to_string();
    let name = f.symbol.as_deref().map(symbol_label).unwrap_or(&fallback);
    format!(
        "{}  {}  {}",
        location(f, color),
        paint(name, color, Style::Symbol),
        paint(&format!("({loc} lines)"), color, Style::Metric),
    )
}

fn render_flat(out: &mut String, rule: RuleId, group: &[&Finding], color: bool) {
    if rule == RuleId::CircularImports {
        for (i, f) in group.iter().enumerate() {
            let _ = writeln!(out, "    {}", format_circular(i + 1, &f.message, color));
        }
        return;
    }
    for f in group {
        let _ = writeln!(out, "    {}", flat_line(f, color));
    }
}

fn symbol_line(f: &Finding, color: bool) -> String {
    if f.rule == RuleId::UnusedImport {
        return format!(
            "{}  {}",
            location(f, color),
            paint(&import_detail(f), color, Style::Symbol)
        );
    }
    format!(
        "{}  {}",
        location(f, color),
        paint(&symbol_name(f), color, Style::Symbol)
    )
}

fn symbol_only(f: &Finding, color: bool) -> String {
    let label = if f.rule == RuleId::UnusedImport {
        import_detail(f)
    } else {
        symbol_name(f)
    };
    format!(
        ":{}  {}",
        paint(&f.span.start_line.to_string(), color, Style::Line),
        paint(&label, color, Style::Symbol)
    )
}

fn import_detail(f: &Finding) -> String {
    if let Some((name, spec)) = parse_import_message(&f.message) {
        format!("{name}  from {spec}")
    } else {
        f.message.clone()
    }
}

fn symbol_name(f: &Finding) -> String {
    let fallback = f.file.display().to_string();
    f.symbol
        .as_deref()
        .map(symbol_label)
        .unwrap_or(&fallback)
        .to_string()
}

fn flat_line(f: &Finding, color: bool) -> String {
    match f.rule {
        RuleId::UnusedFile | RuleId::OnlyUsedInTests => {
            paint(&f.file.display().to_string(), color, Style::Path)
        }
        RuleId::UnusedDependency => {
            format!(
                "{}  {}",
                paint(&f.file.display().to_string(), color, Style::Path),
                paint(&f.message, color, Style::Muted)
            )
        }
        _ => line_for(f, color),
    }
}

fn render_help_footer(out: &mut String, rules: &HashSet<RuleId>, color: bool) {
    let _ = writeln!(out);
    let _ = writeln!(out, "  {}", paint("help", color, Style::Section));
    let mut ordered: Vec<RuleId> = rules.iter().copied().collect();
    ordered.sort_by_key(|r| rule_title(*r));
    for rule in ordered {
        let explain = format!("noslop explain {}", rule.as_str());
        let line = match suppress_hint(rule) {
            Some(hint) => format!("  {explain}  —  {hint}"),
            None => format!("  {explain}  —  {}", rule_reason(rule)),
        };
        let _ = writeln!(out, "{}", paint(&line, color, Style::Muted));
    }
}

/// Human label from a stable symbol id (`path::name` or `path::Owner.member@line`).
fn symbol_label(id: &str) -> &str {
    let tail = id.rsplit_once("::").map(|(_, t)| t).unwrap_or(id);
    tail.split_once('@').map(|(name, _)| name).unwrap_or(tail)
}

fn location(f: &Finding, color: bool) -> String {
    format!(
        "{}:{}",
        paint(&f.file.display().to_string(), color, Style::Path),
        paint(&f.span.start_line.to_string(), color, Style::Line),
    )
}

fn line_for(f: &Finding, color: bool) -> String {
    let loc = location(f, color);
    match f.rule {
        RuleId::UnusedExport
        | RuleId::UnusedType
        | RuleId::UnusedEnumMember
        | RuleId::UnusedClassMember
        | RuleId::UnusedParameter => {
            let fallback = f.file.display().to_string();
            let label = f.symbol.as_deref().map(symbol_label).unwrap_or(&fallback);
            format!("{}  {}", loc, paint(label, color, Style::Symbol))
        }
        RuleId::UnusedImport => {
            if let Some((name, spec)) = parse_import_message(&f.message) {
                format!(
                    "{}  {}  {}",
                    loc,
                    paint(name, color, Style::Symbol),
                    paint(&format!("from {spec}"), color, Style::Muted),
                )
            } else {
                format!("{}  {}", loc, paint(&f.message, color, Style::Muted))
            }
        }
        RuleId::CircularImports => format_circular(1, &f.message, color),
        RuleId::LargeFunction => large_function_line(f, color),
        RuleId::UnusedFile | RuleId::OnlyUsedInTests => {
            paint(&f.file.display().to_string(), color, Style::Path)
        }
        RuleId::UnusedDependency
        | RuleId::ExpectedUnusedButUsed
        | RuleId::MissingSuppressionReason
        | RuleId::HighComplexity
        | RuleId::BannedImport
        | RuleId::BannedCall
        | RuleId::BannedEffect
        | RuleId::BoundaryViolation
        | RuleId::DuplicateCode
        | RuleId::UnusedCssToken
        | RuleId::BrokenCssReference
        | RuleId::UnusedCssClass => format!("{}  {}", loc, paint(&f.message, color, Style::Muted)),
    }
}

/// `'{name}' is imported from '{spec}' but never used.` → (`name`, `spec`)
fn parse_import_message(message: &str) -> Option<(&str, &str)> {
    let rest = message.strip_prefix('\'')?;
    let (name, rest) = rest.split_once('\'')?;
    let rest = rest.strip_prefix(" is imported from '")?;
    let (spec, _) = rest.split_once('\'')?;
    Some((name, spec))
}

fn format_circular(index: usize, message: &str, color: bool) -> String {
    let Some((prefix, paths)) = message.split_once(": ") else {
        return paint(message, color, Style::Muted);
    };
    let file_count = prefix
        .strip_prefix("Circular import group (")
        .and_then(|s| s.strip_suffix(" files)"))
        .unwrap_or("?");
    let chain: Vec<&str> = paths.split(" ⇄ ").map(str::trim).collect();
    let mut out = format!(
        "{}  {} files",
        paint(&format!("cycle {index}"), color, Style::Symbol),
        paint(file_count, color, Style::Metric),
    );
    for path in chain {
        out.push_str("\n      ");
        out.push_str(&paint(path, color, Style::Path));
    }
    out
}

fn confidence_hint(group: &[&Finding]) -> &'static str {
    if group.iter().all(|f| f.confidence == Confidence::High) {
        "high confidence"
    } else {
        "mixed confidence"
    }
}

fn rule_title(rule: RuleId) -> &'static str {
    match rule {
        RuleId::UnusedFile => "Unused files",
        RuleId::UnusedExport => "Unused exports",
        RuleId::UnusedType => "Unused types",
        RuleId::UnusedImport => "Unused imports",
        RuleId::UnusedEnumMember => "Unused enum members",
        RuleId::UnusedClassMember => "Unused class members",
        RuleId::UnusedParameter => "Unused parameters",
        RuleId::ExpectedUnusedButUsed => "Stale @expected-unused",
        RuleId::MissingSuppressionReason => "Missing suppression reasons",
        RuleId::HighComplexity => "High complexity functions",
        RuleId::LargeFunction => "Large functions",
        RuleId::BannedImport => "Banned imports",
        RuleId::BannedCall => "Banned calls",
        RuleId::BannedEffect => "Banned effects",
        RuleId::BoundaryViolation => "Boundary violations",
        RuleId::DuplicateCode => "Duplicate code",
        RuleId::UnusedCssToken => "Unused CSS tokens",
        RuleId::BrokenCssReference => "Broken CSS references",
        RuleId::UnusedCssClass => "Unused CSS classes",
        RuleId::CircularImports => "Circular imports",
        RuleId::UnusedDependency => "Unused dependencies",
        RuleId::OnlyUsedInTests => "Only used in tests",
    }
}

fn rule_reason(rule: RuleId) -> &'static str {
    match rule {
        RuleId::UnusedFile => "Not reachable from any entry point",
        RuleId::UnusedExport => "No live file references these",
        RuleId::UnusedType => "No live file references these types",
        RuleId::UnusedImport => "Imported but never used",
        RuleId::UnusedEnumMember => "Enum members never accessed",
        RuleId::UnusedClassMember => "Private members never accessed",
        RuleId::UnusedParameter => "Parameters never used in the body",
        RuleId::ExpectedUnusedButUsed => "Annotation is stale — the symbol is used",
        RuleId::MissingSuppressionReason => "Suppressions must document a reason",
        RuleId::HighComplexity => "Cyclomatic, cognitive, or CRAP over threshold",
        RuleId::LargeFunction => "Functions longer than the configured line limit",
        RuleId::BannedImport => "Forbidden by a policy rule",
        RuleId::BannedCall => "Forbidden by a policy rule",
        RuleId::BannedEffect => "Forbidden side effect",
        RuleId::BoundaryViolation => "Illegal cross-layer import",
        RuleId::DuplicateCode => "Copy-pasted token blocks",
        RuleId::UnusedCssToken => "Design tokens never referenced",
        RuleId::BrokenCssReference => "var() with no declaration",
        RuleId::UnusedCssClass => "Selectors never used in markup",
        RuleId::CircularImports => "Import cycles (start with the smallest)",
        RuleId::UnusedDependency => "Declared but never imported",
        RuleId::OnlyUsedInTests => "Reachable only from tests",
    }
}

fn suppress_hint(rule: RuleId) -> Option<&'static str> {
    match rule {
        RuleId::UnusedFile => Some("Suppress: # noslop-ignore-file unused-file -- <reason>"),
        RuleId::UnusedExport => {
            Some("Suppress: // noslop-ignore-next-line unused-export -- <reason>")
        }
        RuleId::UnusedType => Some("Suppress: // noslop-ignore-next-line unused-type -- <reason>"),
        RuleId::UnusedEnumMember => {
            Some("Suppress: // noslop-ignore-next-line unused-enum-member -- <reason>")
        }
        RuleId::UnusedClassMember => {
            Some("Suppress: // noslop-ignore-next-line unused-class-member -- <reason>")
        }
        RuleId::UnusedParameter => {
            Some("Suppress: prefix the parameter with `_`, or remove it (that is the fix).")
        }
        _ => None,
    }
}

// ── explicit palette (never bare `.bold()` — it renders red on many themes) ──

enum Style {
    Brand,
    Accent,
    Alert,
    Section,
    Path,
    Line,
    Symbol,
    Metric,
    Success,
    Muted,
    Rule,
}

fn paint(s: &str, color: bool, style: Style) -> String {
    if !color {
        return s.to_string();
    }
    match style {
        Style::Brand => s.bright_white().bold().to_string(),
        Style::Accent => s.cyan().bold().to_string(),
        Style::Alert => s.bright_red().bold().to_string(),
        Style::Section => s.white().bold().to_string(),
        Style::Path => s.bright_black().to_string(),
        Style::Line => s.bright_black().to_string(),
        Style::Symbol => s.yellow().to_string(),
        Style::Metric => s.cyan().to_string(),
        Style::Success => s.green().to_string(),
        Style::Muted => s.bright_black().to_string(),
        Style::Rule => s.bright_black().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HealthReport, Metrics, ScanRootReport};
    use noslop_graph::{Confidence, Finding, RuleId, Severity, Span};
    use std::path::{Path, PathBuf};

    #[test]
    fn unused_import_line_includes_file_and_line() {
        let f = Finding {
            rule: RuleId::UnusedImport,
            severity: Severity::Warn,
            confidence: Confidence::High,
            symbol: None,
            file: PathBuf::from("apps/web/src/app/page.tsx"),
            span: Span::new(5, 5),
            message: "'unusedName' is imported from '@/lib/format' but never used.".to_string(),
            reason: "imported name has no in-file references".to_string(),
        };
        let line = line_for(&f, false);
        assert!(line.starts_with("apps/web/src/app/page.tsx:5  "));
        assert!(line.contains("unusedName"));
        assert!(line.contains("from @/lib/format"));
    }

    #[test]
    fn symbol_label_strips_path_and_line_suffix() {
        assert_eq!(
            symbol_label("apps/agent/src/api/dependencies.py::settings_dep"),
            "settings_dep"
        );
        assert_eq!(
            symbol_label("src/widget.ts::Widget.deadSecret@42"),
            "Widget.deadSecret"
        );
    }

    #[test]
    fn unused_export_line_shows_path_line_and_name() {
        let f = Finding {
            rule: RuleId::UnusedExport,
            severity: Severity::Warn,
            confidence: Confidence::High,
            symbol: Some("apps/agent/src/api/dependencies.py::settings_dep".to_string()),
            file: PathBuf::from("apps/agent/src/api/dependencies.py"),
            span: Span::new(12, 12),
            message: "Exported function 'settings_dep' has no references from any live file."
                .to_string(),
            reason: "0 inbound reference edges; file reachable".to_string(),
        };
        assert_eq!(
            line_for(&f, false),
            "apps/agent/src/api/dependencies.py:12  settings_dep"
        );
    }

    #[test]
    fn parse_import_message_extracts_name_and_specifier() {
        assert_eq!(
            parse_import_message("'h2' is imported from 'h2' but never used."),
            Some(("h2", "h2"))
        );
    }

    #[test]
    fn package_matcher_prefers_longest_root() {
        let roots = vec![
            ScanRootReport {
                package: "web".into(),
                root: "apps/web".into(),
                language: "typescript".into(),
                plugins: vec![],
                files: 10,
                entry_points: 1,
            },
            ScanRootReport {
                package: "agent".into(),
                root: "apps/agent".into(),
                language: "python".into(),
                plugins: vec!["fastapi".into()],
                files: 20,
                entry_points: 1,
            },
        ];
        let matcher = PackageMatcher::new(&roots);
        assert_eq!(matcher.resolve(Path::new("apps/web/src/page.tsx")), "web");
        assert_eq!(
            matcher.resolve(Path::new("apps/agent/src/main.py")),
            "agent"
        );
    }

    #[test]
    fn grouped_exports_collapse_repeated_paths() {
        let mk = |line: u32, sym: &str| Finding {
            rule: RuleId::UnusedExport,
            severity: Severity::Warn,
            confidence: Confidence::High,
            symbol: Some(format!("apps/platform/src/auth/roles.ts::{sym}")),
            file: PathBuf::from("apps/platform/src/auth/roles.ts"),
            span: Span::new(line, line),
            message: "unused".into(),
            reason: "none".into(),
        };
        let group = [mk(7, "DEFAULT_ROLE"), mk(10, "ROLE_ADMIN")];
        let refs: Vec<&Finding> = group.iter().collect();
        let mut out = String::new();
        render_grouped_by_file(&mut out, RuleId::UnusedExport, &refs, false);
        assert!(out.contains("apps/platform/src/auth/roles.ts\n"));
        assert!(out.contains(":7  DEFAULT_ROLE"));
        assert!(out.contains(":10  ROLE_ADMIN"));
        assert_eq!(out.matches("apps/platform/src/auth/roles.ts").count(), 1);
    }

    #[test]
    fn monorepo_render_groups_findings_by_workspace() {
        let report = Report {
            schema_version: 1,
            tool_version: "0.1.0".into(),
            repo: ".".into(),
            scan_roots: vec![
                ScanRootReport {
                    package: "web".into(),
                    root: "apps/web".into(),
                    language: "typescript".into(),
                    plugins: vec![],
                    files: 10,
                    entry_points: 1,
                },
                ScanRootReport {
                    package: "agent".into(),
                    root: "apps/agent".into(),
                    language: "python".into(),
                    plugins: vec![],
                    files: 20,
                    entry_points: 1,
                },
            ],
            metrics: Metrics::for_files(
                30,
                &[
                    Finding {
                        rule: RuleId::UnusedExport,
                        severity: Severity::Warn,
                        confidence: Confidence::High,
                        symbol: Some("apps/web/src/a.ts::dead".into()),
                        file: PathBuf::from("apps/web/src/a.ts"),
                        span: Span::new(1, 1),
                        message: "unused".into(),
                        reason: "none".into(),
                    },
                    Finding {
                        rule: RuleId::UnusedExport,
                        severity: Severity::Warn,
                        confidence: Confidence::High,
                        symbol: Some("apps/agent/src/b.py::dead".into()),
                        file: PathBuf::from("apps/agent/src/b.py"),
                        span: Span::new(2, 2),
                        message: "unused".into(),
                        reason: "none".into(),
                    },
                ],
            ),
            health: HealthReport {
                score: 90.0,
                grade: "A".into(),
                formula_version: 1,
                components: Vec::new(),
                refactor_targets: Vec::new(),
            },
            findings: vec![
                Finding {
                    rule: RuleId::UnusedExport,
                    severity: Severity::Warn,
                    confidence: Confidence::High,
                    symbol: Some("apps/web/src/a.ts::dead".into()),
                    file: PathBuf::from("apps/web/src/a.ts"),
                    span: Span::new(1, 1),
                    message: "unused".into(),
                    reason: "none".into(),
                },
                Finding {
                    rule: RuleId::UnusedExport,
                    severity: Severity::Warn,
                    confidence: Confidence::High,
                    symbol: Some("apps/agent/src/b.py::dead".into()),
                    file: PathBuf::from("apps/agent/src/b.py"),
                    span: Span::new(2, 2),
                    message: "unused".into(),
                    reason: "none".into(),
                },
            ],
            suppressed_count: 0,
        };

        let out = render(&report, false, 100, true);
        let web_pos = out.find("apps/web\n").expect("web workspace banner");
        let agent_pos = out.find("apps/agent\n").expect("agent workspace banner");
        let web_finding = out.find("apps/web/src/a.ts:1").expect("web finding");
        let agent_finding = out.find("apps/agent/src/b.py:2").expect("agent finding");
        assert!(web_pos < web_finding);
        assert!(web_finding < agent_pos);
        assert!(agent_pos < agent_finding);
        assert!(out.contains("2 workspaces"));
    }
}
