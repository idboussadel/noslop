#!/usr/bin/env python3
"""Generate noslop TOML plugins from Fallow's built-in Rust plugin sources."""

from __future__ import annotations

import re
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PLUGINS_DIR = ROOT / "plugins"
FALLOW_BASE = (
    "https://raw.githubusercontent.com/fallow-rs/fallow/main/crates/core/src/plugins"
)

# (rust stem, noslop name). Matches https://docs.fallow.tools/frameworks/built-in
FALLOW_PLUGINS: list[tuple[str, str]] = [
    ("nextjs", "nextjs"),
    ("nuxt", "nuxt"),
    ("remix", "remix"),
    ("sveltekit", "sveltekit"),
    ("gatsby", "gatsby"),
    ("astro", "astro"),
    ("angular", "angular"),
    ("react_router", "react-router"),
    ("redwoodsdk", "redwoodsdk"),
    ("tanstack_router", "tanstack-router"),
    ("react_native", "react-native"),
    ("expo", "expo"),
    ("nestjs", "nestjs"),
    ("adonis", "adonis"),
    ("docusaurus", "docusaurus"),
    ("nitro", "nitro"),
    ("capacitor", "capacitor"),
    ("sanity", "sanity"),
    ("vitepress", "vitepress"),
    ("next_intl", "next-intl"),
    ("relay", "relay"),
    ("electron", "electron"),
    ("qwik", "qwik"),
    ("i18next", "i18next"),
    ("wuchale", "wuchale"),
    ("convex", "convex"),
    ("lit", "lit"),
    ("lexical", "lexical"),
    ("ember", "ember"),
    ("expo_router", "expo-router"),
    ("supabase", "supabase"),
    ("content_collections", "content-collections"),
    ("contentlayer", "contentlayer"),
    ("fumadocs", "fumadocs"),
    ("mintlify", "mintlify"),
    ("velite", "velite"),
    ("rspress", "rspress"),
    ("vite", "vite"),
    ("webpack", "webpack"),
    ("rspack", "rspack"),
    ("rsbuild", "rsbuild"),
    ("rollup", "rollup"),
    ("rolldown", "rolldown"),
    ("tsup", "tsup"),
    ("tsdown", "tsdown"),
    ("pkg_utils", "pkg-utils"),
    ("parcel", "parcel"),
    ("vitest", "vitest"),
    ("jest", "jest"),
    ("cypress", "cypress"),
    ("mocha", "mocha"),
    ("ava", "ava"),
    ("tap", "tap"),
    ("tsd", "tsd"),
    ("storybook", "storybook"),
    ("karma", "karma"),
    ("cucumber", "cucumber"),
    ("webdriverio", "webdriverio"),
    ("k6", "k6"),
    ("stryker", "stryker"),
    ("eslint", "eslint"),
    ("biome", "biome"),
    ("stylelint", "stylelint"),
    ("prettier", "prettier"),
    ("oxlint", "oxlint"),
    ("markdownlint", "markdownlint"),
    ("cspell", "cspell"),
    ("remark", "remark"),
    ("typescript", "typescript"),
    ("babel", "babel"),
    ("swc", "swc"),
    ("tailwind", "tailwind"),
    ("postcss", "postcss"),
    ("unocss", "unocss"),
    ("pandacss", "pandacss"),
    ("prisma", "prisma"),
    ("drizzle", "drizzle"),
    ("knex", "knex"),
    ("typeorm", "typeorm"),
    ("kysely", "kysely"),
    ("turborepo", "turborepo"),
    ("nx", "nx"),
    ("changesets", "changesets"),
    ("syncpack", "syncpack"),
    ("pnpm", "pnpm"),
    ("semantic_release", "semantic-release"),
    ("commitlint", "commitlint"),
    ("commitizen", "commitizen"),
    ("wrangler", "wrangler"),
    ("sentry", "sentry"),
    ("danger", "danger"),
    ("husky", "husky"),
    ("lint_staged", "lint-staged"),
    ("lefthook", "lefthook"),
    ("simple_git_hooks", "simple-git-hooks"),
    ("graphql_codegen", "graphql-codegen"),
    ("typedoc", "typedoc"),
    ("openapi_ts", "openapi-ts"),
    ("plop", "plop"),
    ("svgo", "svgo"),
    ("svgr", "svgr"),
    ("c8", "c8"),
    ("nyc", "nyc"),
    ("msw", "msw"),
    ("nodemon", "nodemon"),
    ("pm2", "pm2"),
    ("dependency_cruiser", "dependency-cruiser"),
    ("hardhat", "hardhat"),
    ("bun", "bun"),
    ("opencode", "opencode"),
]

# Hand-authored noslop plugins — never overwritten by this script.
KEEP_LOCAL: set[str] = {
    "_fallback",
    "express",
    "fastapi",
    "flask",
    "django",
    "celery",
    "click",
    "typer",
    "gunicorn",
    "uvicorn",
    "pytest",
}

# Extra detection / patterns when Fallow uses custom `is_enabled` logic.
MANUAL_OVERRIDES: dict[str, dict] = {
    "pnpm": {
        "detect_any": [
            {"file_exists": "pnpm-workspace.yaml"},
            {"file_exists": "pnpm-lock.yaml"},
            {"dependency": "pnpm"},
        ],
        "config_patterns": [
            "pnpm-workspace.yaml",
            "pnpm-lock.yaml",
            ".pnpmfile.cjs",
            ".pnpmfile.mjs",
            ".npmrc",
        ],
        "tooling_dependencies": ["pnpm"],
    },
    "supabase": {
        "detect_any": [
            {"dependency": "supabase"},
            {"file_exists": "supabase/config.toml"},
        ],
        "entry_points": ["supabase/functions/*/index.{ts,js,mts,mjs}"],
    },
    "wuchale": {
        "detect_any": [
            {"dependency": "wuchale"},
            {"dependency": "@wuchale/vite-plugin"},
            {"file_exists": "wuchale.config.js"},
        ],
    },
    "contentlayer": {
        "detect_any": [
            {"dependency": "contentlayer"},
            {"dependency": "contentlayer2"},
            {"dependency": "next-contentlayer"},
            {"dependency": "next-contentlayer2"},
            {"file_exists": "contentlayer.config.ts"},
            {"file_exists": "contentlayer.config.js"},
        ],
    },
    "fumadocs": {
        "detect_any": [
            {"dependency": "fumadocs-mdx"},
            {"dependency": "fumadocs-core"},
            {"dependency": "fumadocs-ui"},
            {"file_exists": "source.config.ts"},
            {"file_exists": "source.config.js"},
        ],
    },
    "mintlify": {
        "detect_any": [
            {"dependency": "mint"},
            {"dependency": "mintlify"},
            {"file_exists": "docs.json"},
            {"file_exists": "mint.json"},
        ],
    },
    "velite": {
        "detect_any": [
            {"dependency": "velite"},
            {"file_exists": "velite.config.ts"},
            {"file_exists": "velite.config.js"},
        ],
    },
    "cypress": {
        "detect_any": [{"dependency": "cypress"}],
        "entry_points": [
            "**/*.cy.{ts,tsx,js,jsx}",
            "cypress/**/*.{ts,tsx,js,jsx}",
            "cypress/support/**/*.{ts,js}",
        ],
        "config_patterns": ["cypress.config.{ts,js,mjs,cjs}"],
        "tooling_dependencies": ["cypress", "@cypress/react", "@cypress/vue"],
    },
    "k6": {
        "detect_any": [
            {"dependency": "k6"},
        ],
        "test_patterns": ["**/*.k6.{js,ts,mjs,cjs,mts,cts}"],
    },
    "opencode": {
        "detect_any": [
            {"file_exists": "opencode.json"},
            {"file_exists": ".opencode"},
            {"dependency": "@opencode-ai/sdk"},
        ],
    },
    "eslint": {
        "detect_any": [
            {"dependency": "eslint"},
            {"dependency": "@eslint/js"},
            {"file_exists": "eslint.config.js"},
            {"file_exists": "eslint.config.mjs"},
            {"file_exists": "eslint.config.ts"},
            {"file_exists": ".eslintrc.js"},
            {"file_exists": ".eslintrc.json"},
        ],
    },
    "vitest": {
        "detect_any": [
            {"dependency": "vitest"},
            {"file_exists": "vitest.config.ts"},
            {"file_exists": "vitest.config.js"},
            {"file_exists": "vitest.config.mts"},
        ],
    },
    "stryker": {
        "detect_any": [
            {"dependency": "@stryker-mutator/core"},
            {"dependency": "stryker"},
            {"file_exists": "stryker.conf.js"},
            {"file_exists": "stryker.config.mjs"},
        ],
    },
    "typescript": {
        "config_patterns": ["tsconfig.json", "tsconfig.*.json"],
    },
    "nx": {
        "config_patterns": ["nx.json", "project.json", "**/project.json"],
        "entry_points": ["**/project.json"],
    },
    "angular": {
        "config_patterns": ["angular.json", "ng-package.json", "ng-package.prod.json"],
        "entry_points": ["src/public_api.ts", "src/main.ts"],
    },
    "webpack": {
        "config_patterns": ["webpack.config.{js,ts,mjs,cjs}"],
        "entry_points": ["webpack.config.{js,ts,mjs,cjs}"],
    },
    "lit": {
        "detect_any": [{"dependency": "lit"}, {"dependency": "lit-element"}, {"dependency": "lit-html"}],
    },
    "sanity": {
        "detect_any": [{"dependency": "sanity"}, {"dependency": "@sanity/"}],
    },
    "rspress": {
        "detect_any": [{"dependency": "rspress"}, {"dependency": "@rspress/"}],
    },
    "parcel": {
        "detect_any": [{"dependency": "parcel"}, {"dependency": "@parcel/"}],
    },
    "unocss": {
        "detect_any": [{"dependency": "unocss"}, {"dependency": "@unocss/"}],
    },
    "lexical": {
        "detect_any": [{"dependency": "lexical"}, {"dependency": "@lexical/"}],
    },
    "nuxt": {
        "detect_any": [{"dependency": "nuxt"}, {"dependency": "nuxt3"}],
    },
}


def fetch(stem: str) -> str | None:
    url = f"{FALLOW_BASE}/{stem}.rs"
    try:
        with urllib.request.urlopen(url, timeout=20) as resp:
            return resp.read().decode("utf-8")
    except OSError:
        return None


def extract_array(src: str, names: list[str]) -> list[str]:
    for name in names:
        m = re.search(
            rf"(?:const\s+{name}|{name})\s*:\s*&\[&str\]\s*=\s*&\[([^\]]*)\]",
            src,
            re.DOTALL,
        )
        if m:
            items = re.findall(r'"([^"]+)"', m.group(1))
            if items:
                return items
    return []


def extract_define_plugin_block(src: str) -> str | None:
    m = re.search(r"define_plugin!\s*\((.*?)^\);", src, re.DOTALL | re.MULTILINE)
    return m.group(1) if m else None


def extract_from_block(block: str, field: str) -> list[str]:
    m = re.search(rf"{field}\s*:\s*&\[([^\]]*)\]", block, re.DOTALL)
    if not m:
        return []
    return re.findall(r'"([^"]+)"', m.group(1))


def emit_detect(rules: list[dict[str, str]]) -> list[str]:
    if not rules:
        return []
    lines = ["[detect]"]
    if len(rules) == 1 and len(rules[0]) == 1:
        k, v = next(iter(rules[0].items()))
        if k == "dependency":
            lines.append(f'dependency = "{v}"')
        elif k == "file_exists":
            lines.append(f'file_exists = "{v}"')
        return lines
    lines.append("any = [")
    for rule in rules:
        if "dependency" in rule:
            lines.append(f'  {{ dependency = "{rule["dependency"]}" }},')
        elif "file_exists" in rule:
            lines.append(f'  {{ file_exists = "{rule["file_exists"]}" }},')
    lines.append("]")
    return lines


def build_toml(
    name: str,
    enablers: list[str],
    entry_points: list[str],
    config_patterns: list[str],
    test_patterns: list[str],
    tooling_deps: list[str],
    file_exists: list[str],
    override: dict | None,
) -> str:
    lines = [f'name = "{name}"', 'languages = ["typescript", "javascript"]', ""]

    detect_rules: list[dict[str, str]] = []
    if override and "detect_any" in override:
        detect_rules = override["detect_any"]
    else:
        for f in file_exists:
            detect_rules.append({"file_exists": f})
        for dep in enablers:
            detect_rules.append({"dependency": dep})

    if detect_rules:
        lines.extend(emit_detect(detect_rules))
        lines.append("")

    ovr = override or {}
    entry_points = list(dict.fromkeys(ovr.get("entry_points", []) + entry_points))
    config_patterns = list(dict.fromkeys(config_patterns + ovr.get("config_patterns", [])))
    test_patterns = list(dict.fromkeys(test_patterns + ovr.get("test_patterns", [])))
    tooling_deps = list(dict.fromkeys(tooling_deps + ovr.get("tooling_dependencies", [])))

    config_set = set(config_patterns)
    entry_points = [g for g in entry_points if g not in config_set]

    for g in entry_points:
        lines.extend([f"[[entry_points]]", f'glob = "{g}"', ""])
    for g in config_patterns:
        lines.extend([f"[[config_patterns]]", f'glob = "{g}"', ""])
    for g in test_patterns:
        lines.extend([f"[[test_patterns]]", f'glob = "{g}"', ""])
    if tooling_deps:
        deps = ", ".join(f'"{d}"' for d in tooling_deps)
        lines.append(f"tooling_dependencies = [{deps}]")
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def generate_plugin(stem: str, name: str) -> str | None:
    src = fetch(stem)
    if not src:
        print(f"  skip {name}: no source")
        return None

    block = extract_define_plugin_block(src)
    if block:
        enablers = extract_from_block(block, "enablers")
        entry_points = extract_from_block(block, "entry_patterns")
        config_patterns = extract_from_block(block, "config_patterns") + extract_from_block(
            block, "always_used"
        )
        test_patterns = extract_from_block(block, "test_patterns") + extract_from_block(
            block, "fixture_glob_patterns"
        )
        tooling_deps = extract_from_block(block, "tooling_dependencies")
        file_exists = []
    else:
        enablers = extract_array(
            src,
            ["ENABLERS", "ENABLER_PACKAGES", "DEPENDENCY_ENABLERS"],
        )
        entry_points = extract_array(
            src, ["ENTRY_PATTERNS", "ENTRY_POINT_PATTERNS", "RUNTIME_ENTRY_PATTERNS"]
        )
        config_patterns = extract_array(
            src, ["CONFIG_PATTERNS", "ALWAYS_USED", "CONFIG_FILE_PATTERNS"]
        )
        test_patterns = extract_array(src, ["TEST_PATTERNS", "TEST_FILE_PATTERNS"])
        tooling_deps = extract_array(src, ["TOOLING_DEPENDENCIES", "TOOLING_DEPS"])
        file_exists = extract_array(
            src,
            [
                "ESLINT_CONFIG_FILES",
                "CONFIG_FILES",
                "ACTIVATION_FILES",
                "DETECTION_FILES",
            ],
        )

    override = MANUAL_OVERRIDES.get(name)
    if not enablers and not file_exists and not (override and "detect_any" in override):
        print(f"  skip {name}: no detection")
        return None

    toml = build_toml(
        name,
        enablers,
        entry_points,
        config_patterns,
        test_patterns,
        tooling_deps,
        file_exists,
        override,
    )
    return toml


def main() -> None:
    PLUGINS_DIR.mkdir(parents=True, exist_ok=True)
    written = 0
    for stem, name in FALLOW_PLUGINS:
        if name in KEEP_LOCAL:
            continue
        toml = generate_plugin(stem, name)
        if not toml:
            continue
        path = PLUGINS_DIR / f"{name}.toml"
        path.write_text(toml, encoding="utf-8")
        written += 1
        print(f"  wrote {path.name}")

    print(f"\n{written} Fallow plugins written (local Python plugins preserved)")


if __name__ == "__main__":
    main()
