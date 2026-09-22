#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
GRAPHIFY_BIN="${GRAPHIFY_BIN:-$(command -v graphify || true)}"

[[ -n "$GRAPHIFY_BIN" && -x "$GRAPHIFY_BIN" ]] || {
  echo "graphify is not installed or not on PATH." >&2
  exit 1
}

interpreter="$(head -n 1 "$GRAPHIFY_BIN" | sed 's/^#!//')"
[[ -x "$interpreter" ]] || {
  echo "Cannot resolve Graphify's Python interpreter from: $GRAPHIFY_BIN" >&2
  exit 1
}

if [[ "${1:-}" == "--check" ]]; then
  printf 'Graphify: %s\n' "$GRAPHIFY_BIN"
  printf 'Repository: %s\n' "$ROOT"
  if [[ -f "$ROOT/graphify-out/graph.json" ]]; then
    printf 'Graph: %s\n' "$ROOT/graphify-out/graph.json"
  else
    echo "Graph: not generated yet"
  fi
  exit 0
fi
[[ $# -eq 0 ]] || { echo "Usage: scripts/refresh-graphify.sh [--check]" >&2; exit 2; }

cd "$ROOT"
"$interpreter" "$ROOT/scripts/refresh_graphify.py"
