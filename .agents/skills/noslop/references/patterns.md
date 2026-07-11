# Workflow patterns

## Adopt noslop in an existing repo

Goal: clean high-confidence issues, model intentional exceptions, ratchet the rest.

```
1. noslop --format json > report.json
2. noslop init                    # optional: noslop.toml with detected plugins
3. Fix in order: cycles → dead files → exports/imports → deps
   - Preview: `noslop fix --dry-run`
   - Apply High-confidence: `noslop fix`
   - Undo if needed: `noslop fix restore`
4. noslop explain <rule>          # before each suppression
5. Re-run after each batch
6. noslop baseline update         # accept remaining legacy
7. noslop audit --base main       # verify gate is clean
8. Add CI step (see below)
```

Prefer fixing in code over suppressing. Prefer config-level rules over repeated inline ignores.

## CI ratchet (GitHub Actions)

```yaml
- name: Install noslop
  run: cargo install --path crates/noslop-cli --locked

- name: Ratchet
  run: noslop audit --base origin/main --format github
```

Commit `.noslopcode/baseline.json` after `noslop baseline update`. PRs fail only on findings **not** in the baseline.

Exit codes: `0` pass, `1` new findings, `2` misconfiguration.

## Monorepo: full repo vs single package

```bash
noslop --root . --format json              # all workspaces
noslop --root apps/web --format json       # single workspace tree
```

Use `scan_roots[]` in JSON to summarize per-package health.

## Agent-driven cleanup loop

```
1. jq '.health.refactor_targets[0]' report.json
2. noslop explain <rule from finding>
3. noslop fix --dry-run                 # preview changes
4. noslop fix                           # apply High-confidence fixes
5. noslop --format json                 # verify
6. If broken: noslop fix restore        # or git checkout -- .
7. Repeat until targets empty or user stops
```

Manual edits still valid for cycles, Medium/Low findings, and cases fix skips.

## Watch during active development

```bash
noslop watch                      # re-scan on every save
noslop dead --watch               # dead-code rules only
noslop watch --fix --dry-run      # preview fixes after each rescan
```

Debounce is 300ms. Parse cache keeps rescans fast on large repos.

## Filtered analysis

```bash
noslop dead --format json
noslop --format json --filter unused-file,unused-export,circular-imports
```

## Duplication pass

```bash
noslop dupes --format json
# or enable in noslop.toml:
# [duplication]
# enabled = true
# mode = "mild"
```

## Copy-paste adoption prompt

```text
Adopt noslop in this repository.

Goal:
- run full-repo analysis first (noslop --format json), not only audit
- fix real dead code and cycles in code
- model intentional exceptions with the narrowest mechanism
- end with noslop baseline update + noslop audit as PR gate

Process:
1. Run noslop --format json. Check schema_version and health.refactor_targets.
2. If helpful, run noslop init for noslop.toml.
3. Fix high-confidence findings first: cycles (manual), then `noslop fix --dry-run` → `noslop fix` for dead files/imports/exports.
4. For each remaining finding: fix in code (preferred), `noslop fix` where High-confidence, config rule, or narrow suppression with reason.
5. Re-run noslop after each batch. If fix breaks something: `noslop fix restore`.
6. noslop baseline update, then noslop audit --base main.
7. Wire noslop audit into CI.

Report: code changes, config changes, exceptions and why, final audit output.
```
