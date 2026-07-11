//! `noslop explain <rule>` — react-doctor DX: what the rule means, why it fires,
//! and how to suppress it.

pub fn explain(rule: &str) -> String {
    let body = match rule.trim() {
        "unused-file" => {
            "unused-file — a source file not reachable from any entry point.\n\n\
             Why it fires: starting from every detected entry point (framework \
             routes, CLI targets, tests, package main/bin), noslop follows import \
             edges. Files never reached are reported.\n\n\
             False-positive traps: files loaded dynamically by string, plugin \
             conventions noslop doesn't know. Confidence is capped to Medium when \
             the package has unresolvable dynamic imports.\n\n\
             Suppress: # noslop-ignore-file unused-file -- <reason>"
        }
        "unused-export" => {
            "unused-export — an exported symbol no live file references by name.\n\n\
             Why it fires: the file is reachable, but no other file imports this \
             name (and it isn't part of an entry point's public API).\n\n\
             Suppress: // noslop-ignore-next-line unused-export -- <reason>"
        }
        "unused-type" => {
            "unused-type — an exported type/interface/enum no live file references \
             by name. Same detection as unused-export, split into its own rule so \
             types can be triaged and configured separately.\n\n\
             Suppress: // noslop-ignore-next-line unused-type -- <reason>"
        }
        "unused-enum-member" => {
            "unused-enum-member — a member of an enum never accessed anywhere in \
             the repo (`Color.Red`, `{ Red } = Color`). Checked against a repo-wide \
             member-access index, so a member used in any file is spared; the check \
             can only miss a use, never invent one. Names that appear in a string \
             literal are capped to Medium (serialized-by-name enums).\n\n\
             Suppress: // noslop-ignore-next-line unused-enum-member -- <reason>"
        }
        "unused-class-member" => {
            "unused-class-member — a private class member (`#field`, TS `private`, \
             Python `_method`) never accessed anywhere in the repo. Only private \
             members are checked — a public member may be reached dynamically or \
             through an interface — so the finding is inheritance-safe.\n\n\
             Suppress: // noslop-ignore-next-line unused-class-member -- <reason>"
        }
        "unused-css-token" | "broken-css-reference" | "unused-css-class" => {
            "CSS liveness (Slice 1) — unused-css-token: a `--custom-property` no \
             `var()` references; broken-css-reference: a `var(--x)` with no \
             declaration; unused-css-class: a `.class` selector never used in any \
             `className`/`class` attribute (Medium — dynamic classes are common). \
             Enable with `[style]`."
        }
        "duplicate-code" => {
            "duplicate-code — a block of tokens repeated across the repo, found via \
             a suffix array over normalized token streams. Modes: exact, mild \
             (default, ignores numbers), weak (ignores strings), semantic (ignores \
             consistent renames). Enable with `[duplication]` or `noslop dupes`; \
             tune `min-tokens` and `skip-local`."
        }
        "banned-import" | "banned-call" | "banned-effect" => {
            "banned-import / banned-call / banned-effect — a policy rule pack \
             forbids an import specifier, a call callee, or a whole effect class \
             (network/process/fs). Define packs under `[policy]` in noslop.toml or \
             a referenced `*.toml` pack file; each finding names the rule id."
        }
        "boundary-violation" => {
            "boundary-violation — a file imported another architectural layer it is \
             not allowed to depend on. Configure `[boundaries]` with a `preset` \
             (layered/hexagonal/feature-sliced) or explicit `[[boundaries.layer]]` \
             entries with an `allow` list."
        }
        "high-complexity" => {
            "high-complexity — a function whose cyclomatic (McCabe), cognitive \
             (SonarSource), or CRAP change-risk score exceeds the configured \
             threshold. On by default with fallow-parity limits (max-cyclomatic \
             20, max-cognitive 15, max-crap 30); tune under `[complexity]`; \
             disable with `[complexity] enabled = false`. Relax per path with \
             `[[complexity.override]]` + a reason."
        }
        "large-function" => {
            "large-function — a function whose line count exceeds the configured \
             limit (fallow parity: `max-unit-size`, default 60). On by default \
             under `[complexity]`; disable with `[complexity] enabled = false`. \
             Relax per path with `[[complexity.override]]` + a reason."
        }
        "expected-unused-but-used" => {
            "expected-unused-but-used — a symbol annotated `@expected-unused` that \
             now has references. The annotation has served its purpose (or was \
             wrong); remove it so real dead code is caught again."
        }
        "missing-suppression-reason" => {
            "missing-suppression-reason — a `noslop-ignore-*` comment or \
             `@expected-unused` tag with no `-- <reason>`. Enabled by \
             `[rules].require-suppression-reason = \"warn\"|\"error\"`. Documented \
             suppressions age far better than bare ones."
        }
        "unused-parameter" => {
            "unused-parameter — a parameter never referenced in its function body. \
             Follows TypeScript's noUnusedParameters rule: only trailing unused \
             params are reported (a param before a used one can't be removed), and \
             `_`-prefixed params are treated as intentionally unused. Local and \
             syntactic, so always High confidence.\n\n\
             Fix: remove the parameter, or prefix it with `_`."
        }
        "unused-import" => {
            "unused-import — an imported name never used in its file.\n\n\
             Cheap and effectively false-positive-free, so always High confidence.\n\n\
             Suppress: remove the import (that is the fix)."
        }
        "unused-dependency" => {
            "unused-dependency — a declared dependency no import resolves to.\n\n\
             Reported at Medium confidence: tooling used only via config or CLI \
             (bundlers, test runners, type stubs) legitimately has no imports."
        }
        "circular-imports" => {
            "circular-imports — a group of files that import each other, directly \
             or transitively (a strongly-connected component).\n\n\
             Python cycles default to error (runtime import bugs waiting to fire); \
             TypeScript cycles default to warn. Start with the smallest group."
        }
        "only-used-in-tests" => {
            "only-used-in-tests — a file reachable from test entry points but not \
             from any production entry point. Often means dead production code \
             kept alive only by its tests."
        }
        other => {
            return format!(
                "Unknown rule '{other}'. Known rules: unused-file, unused-export, \
                 unused-type, unused-import, unused-enum-member, unused-class-member, \
                 unused-parameter, expected-unused-but-used, missing-suppression-reason, \
                 high-complexity, large-function, banned-import, banned-call, banned-effect, \
                 boundary-violation, duplicate-code, unused-css-token, broken-css-reference, \
                 unused-css-class, unused-dependency, circular-imports, only-used-in-tests."
            );
        }
    };
    body.to_string()
}
