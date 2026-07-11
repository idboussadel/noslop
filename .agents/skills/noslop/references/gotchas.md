# Gotchas

Common agent mistakes when using noslop.

## Exit code 1 is success-shaped failure

**Wrong:** Treat exit code 1 as "command crashed" and stop the workflow.

**Correct:** Exit `1` means findings at/above `fail-on`. Only exit `2` is a runtime error. In CI, `noslop audit` should fail the job on `1`.

## Pretty output ≠ JSON confidence filter

**Wrong:** Run `noslop` without `--all`, see few findings, assume the repo is clean.

**Correct:** Pretty mode hides Medium/Low by default. Use `--format json` for the full set, or `--all` for terminal.

## Deleting before breaking cycles

**Wrong:** Delete `dead_tool.py` while `cycle_a.py ⇄ cycle_b.py` still exists.

**Correct:** Fix `circular-imports` first. Cycles distort reachability; dead-file conclusions are safer after cycles are gone.

## Auto-removing Medium-confidence deps

**Wrong:** Remove every `unused-dependency` from `package.json` / `pyproject.toml` without checking.

**Correct:** `unused-dependency` is Medium confidence. Verify the package isn't used via dynamic import, re-export edge, or a path noslop doesn't resolve. Monorepo workspace packages need cross-package import checks.

## Suppressing without a reason

**Wrong:**

```ts
// noslop-ignore-next-line unused-export
export const x = 1;
```

**Correct:**

```ts
// noslop-ignore-next-line unused-export -- external plugin API
export const x = 1;
```

When `require-suppression-reason` is enabled, bare suppressions become `missing-suppression-reason` findings.

## Using `@expected-unused` on live code

**Wrong:** Mark active exports `@expected-unused` to silence noise permanently.

**Correct:** Use `@expected-unused` only for intentionally retained dead surface. If code becomes used, noslop reports `expected-unused-but-used` — remove the annotation.

## Assuming JS-only

**Wrong:** Only check `package.json` workspaces in a repo that also has `pyproject.toml` apps.

**Correct:** noslop scans **TypeScript and Python** in one pass. Read `scan_roots[]` for both `apps/web` (TS) and `apps/api` (Python).

## Treating config files as dead source

**Wrong:** Panic when `eslint.config.mjs` or `__init__.py` appears in findings.

**Correct:** Framework/config roles are classified by path. If a config file is flagged, it's likely a real unreachable file, not a false positive from role misclassification — but verify before deleting.

## Full scan vs audit for PR work

**Wrong:** Run `noslop --format json` on every PR and try to zero out 500 legacy findings.

**Correct:** `noslop baseline update` once, then `noslop audit --base main` to gate **new** findings only.

## No auto-fix yet

**Wrong:** Look for `noslop fix` or `--apply` (Fallow has this; noslop does not yet).

**Correct:** Edit source manually, then re-run `noslop` or `noslop dead --format json` to verify.

## Duplication not in default full scan config

**Wrong:** Expect `duplicate-code` in every `noslop` run on a repo without `[duplication]` enabled.

**Correct:** Use `noslop dupes` or enable `[duplication]` in `noslop.toml`.
