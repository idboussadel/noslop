---
name: noslop
description: Polyglot dead-code and health analysis for TypeScript, JavaScript, and Python. Finds unused files, exports, imports, dependencies, cycles, complexity, and duplication. Use when asked to find dead code, audit a PR, set up CI ratchet, explain a noslop finding, run noslop, or clean up a monorepo with mixed TS and Python.
metadata:
  version: 1.0.0
  noslop_version: 0.1.0
  schema_version: 1
  homepage: https://github.com/noslopcode/noslopcode
---

# noslop

Deterministic reachability analysis for **TypeScript/JavaScript and Python**. One scan builds an import graph and reports what nothing depends on — dead files, unused exports, cycles, unused deps, complexity, duplication.

## When to Use

- Find dead code, unused exports/imports, or unused dependencies in a TS or Python repo.
- Break import cycles before deleting files safely.
- Audit a PR with the CI ratchet (`noslop audit`) instead of fixing legacy debt in one shot.
- Read `health.refactor_targets` to prioritize cleanup work.
- Explain or suppress a specific rule (`noslop explain <rule>`).

## When NOT to Use

- Type checking (`tsc`, `mypy`) or lint style (ESLint, Ruff).
- Runtime debugging or test failures.
- Verified security scanning (noslop is static reachability only).
- Repos with no TS/JS/Python source to analyze.

## Prerequisites

```bash
cargo install --path crates/noslop-cli   # from noslopcode repo
# or: cargo run -p noslop-cli -- …
```

Zero config works on first run. Optional: `noslop init` writes `noslop.toml` with detected plugins and entry points.

## Agent Rules

1. **Always use `--format json`** for machine-readable output. JSON includes all confidence tiers; the pretty terminal view hides Medium/Low unless `--all`.
2. **Check `schema_version` and `tool_version`** before parsing. Current contract: `schema_version: 1`, `tool_version: 0.1.0`. If `schema_version` differs, read [references/json-schema.md](references/json-schema.md) and repo CHANGELOG.
3. **Exit code 1 means findings**, not a crash. Exit code 2 is execution error (misconfig, unreadable repo). Never conflate them in CI.
4. **Prefer High-confidence findings** for auto-fix. Ask a human before acting on `medium` or `low` (e.g. unused dependencies, dynamic imports).
5. **Fix cycles before deleting dead files** — cycles block safe reachability conclusions.
6. **Suppress narrowly** with rule name + reason, or use `@public` / `@expected-unused` annotations. See [references/gotchas.md](references/gotchas.md).
7. **Monorepo-aware**: read `scan_roots[]` for per-package context. Use `--root apps/web` to scope a single workspace.
8. **No `fix` command yet** — remove dead code manually; re-run noslop to verify.

## Task Cheat Sheet

| User intent | Command |
|-------------|---------|
| Full health scan | `noslop --format json` |
| Dead code only | `noslop dead --format json` |
| Import cycles | `noslop cycles --format json` |
| Unused deps | `noslop deps --format json` |
| Duplication | `noslop dupes --format json` |
| PR / CI gate | `noslop audit --base main --format json` |
| Accept legacy debt | `noslop baseline update` |
| Debug a rule | `noslop explain unused-export` |
| Narrow rules | `noslop --format json --filter unused-file,unused-export` |
| All confidence tiers in terminal | `noslop --all` |

## JSON Essentials

Top-level fields: `schema_version`, `tool_version`, `repo`, `scan_roots`, `metrics`, `health`, `findings`, `suppressed_count`.

Agents should read first:

- `health.refactor_targets` — ranked cleanup starting points (`payoff`, `effort`, `reasons`).
- `findings[].rule`, `findings[].confidence`, `findings[].file`, `findings[].span`.
- `findings[].symbol` — stable id when present (`path::dotted.symbol`).
- `metrics` — aggregate counts for summaries.

Full contract: [references/json-schema.md](references/json-schema.md). Schema file: `schema/report.v1.schema.json`.

## Baseline Ratchet

```bash
noslop baseline update              # one-time: snapshot current findings as legacy
noslop audit --base main --format json   # fail only on NEW findings
```

Baseline file: `.noslopcode/baseline.json` (array of stable keys: `rule|path` or `rule|symbol`).

## Common Workflows

### Local cleanup

```bash
noslop --format json | jq '.health.refactor_targets[:3]'
noslop dead --format json
```

Work order: cycles → dead files → unused exports/imports → deps.

### CI (GitHub Actions)

```bash
cargo install --path crates/noslop-cli --locked
noslop audit --base origin/main --format github
```

### Adoption in a legacy repo

1. `noslop --format json` — understand scope.
2. `noslop init` if config helps (optional).
3. Fix high-confidence issues in batches; re-run after each batch.
4. `noslop baseline update` — accept remaining legacy.
5. Wire `noslop audit` into CI.

Recipes: [references/patterns.md](references/patterns.md).

## Rules (summary)

| Rule | Finds |
|------|-------|
| `unused-file` | File unreachable from entry points |
| `unused-export` | Export never referenced |
| `unused-import` | Import never used (High confidence) |
| `unused-dependency` | Declared dep never imported (Medium) |
| `circular-imports` | Import cycle (SCC) |
| `only-used-in-tests` | Reachable only from test entry points |
| `high-complexity` / `large-function` | On by default; disable in `[complexity]` |
| `duplicate-code` | Opt-in via `[duplication]` or `noslop dupes` |

Full list + config: [references/cli-reference.md](references/cli-reference.md).

## Suppression

```ts
// noslop-ignore-next-line unused-export -- kept for plugin API
export function hook() {}

/** @public -- consumed by external SDKs */
export function pluginApi() {}

/** @expected-unused -- ships in v2 */
export const futureFlag = false;
```

```python
# noslop-ignore-file unused-file -- loaded dynamically by the job runner
```

Every suppression needs a **rule name and reason** when `require-suppression-reason` is on.

## References

- [CLI reference](references/cli-reference.md) — commands, flags, config
- [JSON schema](references/json-schema.md) — output contract for agents
- [Gotchas](references/gotchas.md) — pitfalls with wrong/correct patterns
- [Patterns](references/patterns.md) — CI, monorepo, adoption workflows
