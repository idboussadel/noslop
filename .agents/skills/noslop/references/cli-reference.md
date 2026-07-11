# noslop CLI reference

Matched to **noslop v0.1.0**. Bump this file when commands or flags change.

## Global flags

| Flag | Default | Description |
|------|---------|-------------|
| `--root <path>` | `.` | Repository root to scan |
| `--format pretty\|json\|sarif\|github` | `pretty` | Output format |
| `--all` | off | Pretty output: include Medium/Low confidence (JSON always includes all) |
| `--filter <rules>` | — | Comma-separated rule ids (e.g. `unused-file,unused-export`) |
| `--threads N` | CPU count | Parallel extraction workers |
| `--no-cache` | off | Bypass on-disk parse cache |
| `--fix` | off | Apply High-confidence auto-fixes after scan |
| `--dry-run` | off | Preview fixes without writing (with `--fix` or `noslop fix`) |
| `--include-deps` | off | Also remove unused deps in fix (Medium confidence) |
| `--watch` | off | Re-scan on file save (debounced) |

## Commands

| Command | Scope | Notes |
|---------|-------|-------|
| `noslop` | Full scan | All enabled rules |
| `noslop dead` | Dead-code subset | `unused-file`, `unused-export`, `unused-import`, `only-used-in-tests` |
| `noslop cycles` | `circular-imports` only | |
| `noslop deps` | `unused-dependency` only | Medium confidence |
| `noslop dupes` | `duplicate-code` only | Force-enables duplication for this run |
| `noslop fix` | Auto-fix | High-confidence: dead files, unused imports/exports |
| `noslop fix --dry-run` | Fix preview | Unified diff, no writes |
| `noslop fix restore` | Rollback | Undo last applied fix (`.noslopcode/fix-rollback.json`) |
| `noslop fix --include-deps` | Fix + deps | Also removes unused manifest deps (Medium) |
| `noslop watch` | Watch mode | Re-scan on save; same as `--watch` |
| `noslop audit --base <ref>` | Full scan minus baseline | `--base` is informational; ratchet uses `.noslopcode/baseline.json` |
| `noslop baseline update` | Writes baseline | No scan output semantics beyond update message |
| `noslop explain <rule>` | Text help | No scan |
| `noslop init` | Writes `noslop.toml` | Detected plugins + entry points |

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | No findings at/above `fail-on` severity (or baseline accepted them in audit) |
| `1` | Findings at/above `fail-on` severity |
| `2` | Execution error — never treat as "issues found" |

Default `fail-on` is `error`. Configure in `noslop.toml` under `[audit]`.

## Auto-fix (`noslop fix`)

| What | Confidence | Notes |
|------|------------|-------|
| Delete `unused-file` | High | Removes file from disk |
| Strip `unused-import` bindings | High | TS + Python import statements |
| Remove `unused-export` / `unused-type` | High | Deletes declaration lines |
| Remove `unused-dependency` | Medium | Only with `--include-deps` |

**Not auto-fixed:** `circular-imports`, `only-used-in-tests`, complexity, duplication, CSS rules.

Before every real `noslop fix`, a rollback snapshot is written to `.noslopcode/fix-rollback.json`. Undo with `noslop fix restore` (recreates deleted files). In git: `git checkout -- .`.

Combine with scan: `noslop dead --fix --dry-run`, `noslop --fix`.

## Watch mode

`noslop watch` or `noslop --watch` — debounced (300ms) re-scan on file changes. Ignores `node_modules`, `.git`, `target`, `.noslopcode`. Warm cache: only changed files re-parse.

Supports `--fix`, `--dry-run`, `--format`, subcommand scopes (`noslop dead --watch`).

## Rule ids (`--filter` and findings)

Core (always on):

- `unused-file`, `unused-export`, `unused-type`, `unused-import`
- `unused-enum-member`, `unused-class-member`, `unused-parameter`
- `unused-dependency`, `circular-imports`, `only-used-in-tests`

Complexity (on by default):

- `high-complexity`, `large-function` — disable with `[complexity] enabled = false`

Opt-in (config section enables):

- `banned-import`, `banned-call`, `banned-effect` — `[policy]`
- `boundary-violation` — `[boundaries]`
- `duplicate-code` — `[duplication]` or `noslop dupes`
- `unused-css-token`, `broken-css-reference`, `unused-css-class` — `[style]`
- `expected-unused-but-used`, `missing-suppression-reason`

## Configuration (`noslop.toml`)

```toml
schema = 1

[rules]
unused-file = "warn"
unused-export = "warn"

[audit]
fail-on = "error"

[complexity]
enabled = true
max-cyclomatic = 20
max-cognitive = 15
max-crap = 30
max-unit-size = 60

[duplication]
enabled = true
min-tokens = 50
mode = "mild"

[policy]
packs = ["policy/architecture.toml"]

[boundaries]
preset = "layered"
```

Config **refines** behavior; zero-config scans still run.

## Framework plugins

Auto-detected from manifests (Next.js, FastAPI, pytest, etc.). Listed per workspace in JSON `scan_roots[].plugins`.

Framework **convention exports** (e.g. Next.js `metadata`, FastAPI route handlers) should be recognized via plugins — if flagged, check plugin detection or add entry points in config.

## Output formats

| Format | Use |
|--------|-----|
| `json` | Agents, automation — see [json-schema.md](json-schema.md) |
| `pretty` | Human terminal (High confidence default) |
| `sarif` | SARIF consumers |
| `github` | GitHub Actions annotations (`noslop audit --format github`) |
