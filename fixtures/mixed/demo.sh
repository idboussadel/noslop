#!/usr/bin/env bash
# Record this fixture for Twitter/X — run from repo root:
#   ./fixtures/mixed/demo.sh

set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
NOSLOP="${NOSLOP:-cargo run -p noslop-cli --}"

echo "=== 1/4 Full scan (default confidence) ==="
$NOSLOP --root "$ROOT" --no-cache

echo ""
echo "=== 2/4 Everything including deps + complexity ==="
$NOSLOP --root "$ROOT" --all --no-cache

echo ""
echo "=== 3/4 Import graph dashboard (tree in terminal) ==="
$NOSLOP graph dashboard --root "$ROOT" --no-cache

echo ""
echo "=== 4/4 HTML graph (open in browser for screenshot) ==="
$NOSLOP graph packages --root "$ROOT" --depth 2 --format html --no-cache > /tmp/noslop-demo-graph.html
echo "Wrote /tmp/noslop-demo-graph.html"
