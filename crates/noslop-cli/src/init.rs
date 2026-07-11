//! `noslop init` — generate a `noslop.toml`, annotated with what auto-detection
//! found, so users can see (and refine) the plugins and entry points in effect.

use std::fmt::Write;
use std::path::Path;

pub fn run(root: &Path) -> anyhow::Result<()> {
    let target = root.join("noslop.toml");
    if target.exists() {
        anyhow::bail!("noslop.toml already exists — refusing to overwrite");
    }

    let canonical = std::fs::canonicalize(root)?;
    let workspace = noslop_discover::discover(&canonical);

    let mut out = String::new();
    let _ = writeln!(out, "schema = 1\n");

    let _ = writeln!(
        out,
        "# Detected scan roots (auto-detection — edit to refine):"
    );
    for pkg in &workspace.packages {
        let files = workspace
            .files
            .iter()
            .filter(|f| f.package == pkg.id)
            .count();
        if files == 0 {
            continue;
        }
        let plugins = if pkg.plugins.is_empty() {
            "fallback heuristics".to_string()
        } else {
            pkg.plugins.join(", ")
        };
        let root_disp = if pkg.root.as_os_str().is_empty() {
            ".".to_string()
        } else {
            pkg.root.display().to_string()
        };
        let _ = writeln!(out, "#   {root_disp} ({files} files) — plugins: {plugins}");
    }

    out.push_str(
        "\n[rules]\n\
         unused-file = \"warn\"\n\
         unused-export = \"warn\"\n\
         unused-import = \"warn\"\n\
         unused-dependency = \"warn\"\n\
         circular-imports = \"warn\"\n\
         only-used-in-tests = \"warn\"\n\
         \n\
         [ignore]\n\
         paths = [\"**/migrations/**\", \"**/generated/**\", \"**/*.stories.tsx\"]\n\
         \n\
         [entry_points]\n\
         # Add globs for files loaded by convention but not imported (merged with plugins).\n\
         # add = [\"scripts/one-off/*.py\", \"**/cli/**\"]\n\
         \n\
         [plugins]\n\
         # paths = [\".noslop/plugins\"]\n\
         \n\
         [audit]\n\
         baseline = \".noslopcode/baseline.json\"\n\
         fail-on = [\"error\"]\n",
    );

    std::fs::write(&target, out)?;
    println!("Wrote {}", target.display());
    Ok(())
}
