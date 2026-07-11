# JSON output contract

**schema_version: 1** · **tool_version: 0.1.0**

Authoritative schema: `schema/report.v1.schema.json` in the noslopcode repo.

Agents must check `schema_version` on every run. A breaking change bumps `schema_version`; update this skill's `metadata.schema_version` when that happens.

## Top-level shape

```json
{
  "schema_version": 1,
  "tool_version": "0.1.0",
  "repo": ".",
  "scan_roots": [ … ],
  "metrics": { … },
  "health": { … },
  "findings": [ … ],
  "suppressed_count": 0
}
```

No additional top-level properties (`additionalProperties: false`).

## `scan_roots[]`

Per-workspace discovery summary:

| Field | Type | Meaning |
|-------|------|---------|
| `package` | string | Workspace name |
| `root` | string | Path relative to repo root |
| `language` | enum | `typescript`, `javascript`, `python`, `css` |
| `plugins` | string[] | Matched framework plugins |
| `files` | int | Files scanned in workspace |
| `entry_points` | int | Detected entry points |

## `metrics`

Always present: `files`, `dead_files`, `dead_file_pct`, `dead_exports`, `unused_imports`, `cycles`, `unused_dependencies`, `only_used_in_tests`.

Optional counts when relevant: `unused_types`, `unused_enum_members`, `unused_class_members`, `unused_parameters`, `high_complexity`, `large_functions`, `duplicate_code`, `duplication_pct`, `grade`.

## `health`

| Field | Meaning |
|-------|---------|
| `score` | 0–100 health score |
| `grade` | `A`–`F` |
| `formula_version` | Scoring formula version (int) |
| `components[]` | Named penalty buckets (`name`, `score`, `penalty`, `findings`) |
| `refactor_targets[]` | Ranked cleanup list — **start here** |

### `refactor_targets[]`

| Field | Meaning |
|-------|---------|
| `rank` | 1 = highest priority |
| `path` | File to focus on |
| `kind` | e.g. `cycle`, `dead file`, `dead-code cleanup` |
| `payoff` | Impact score |
| `effort` | `small`, `medium`, `large` |
| `findings` | Count in this target |
| `reasons` | Human strings |

## `findings[]`

| Field | Required | Meaning |
|-------|----------|---------|
| `rule` | yes | Rule id (see cli-reference.md) |
| `severity` | yes | `off`, `warn`, `error` |
| `confidence` | yes | `low`, `medium`, `high` |
| `file` | yes | Repo-relative path |
| `span` | yes | `{ start_line, end_line }` |
| `message` | yes | Human sentence |
| `reason` | yes | Machine justification |
| `symbol` | no | Stable symbol id: `path::dotted.name` |

### Baseline stable keys

Used in `.noslopcode/baseline.json`:

- With symbol: `{rule}|{symbol}`
- Without symbol: `{rule}|{file}`

Example: `unused-export|apps/web/src/lib/format.ts::formatDate`

## Confidence policy for agents

| Confidence | Agent action |
|------------|--------------|
| `high` | Safe to propose fix after quick sanity check |
| `medium` | Verify (dynamic import, dep placement, CSS classes) |
| `low` | Human review required |

JSON includes all tiers. Terminal pretty output hides Medium/Low unless `--all`.

## Parsing example

```bash
noslop --format json | jq '{
  grade: .health.grade,
  targets: .health.refactor_targets[:3],
  high: [.findings[] | select(.confidence == "high") | .rule] | unique
}'
```
