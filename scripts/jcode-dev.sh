#!/bin/bash
set -euo pipefail

repo="${JCODE_DEV_REPO:-$HOME/.jcode/source/jcode}"
jcode_bin="${JCODE_DEV_BIN:-$HOME/.local/bin/jcode}"

usage() {
  cat <<'EOF'
Launch Jcode in self-development mode from the canonical source checkout.

Usage:
  jcode-dev [jcode self-dev arguments]
  jcode-dev --check
  jcode-dev --print-path

Environment overrides:
  JCODE_DEV_REPO  Jcode source checkout (default: ~/.jcode/source/jcode)
  JCODE_DEV_BIN   Jcode launcher (default: ~/.local/bin/jcode)
EOF
}

case "${1:-}" in
  --help|-h)
    usage
    exit 0
    ;;
  --print-path)
    printf '%s\n' "$repo"
    exit 0
    ;;
  --check)
    [[ -d "$repo/.git" ]] || {
      echo "Jcode source checkout not found at: $repo" >&2
      exit 1
    }
    [[ -f "$repo/Cargo.toml" ]] || {
      echo "Cargo.toml not found at: $repo" >&2
      exit 1
    }
    [[ -x "$jcode_bin" ]] || {
      echo "Jcode launcher is not executable: $jcode_bin" >&2
      exit 1
    }
    printf 'Jcode source: %s\n' "$repo"
    printf 'Jcode launcher: %s\n' "$jcode_bin"
    printf 'Launch command: %q self-dev\n' "$jcode_bin"
    exit 0
    ;;
esac

[[ -d "$repo/.git" && -f "$repo/Cargo.toml" ]] || {
  echo "Jcode source checkout not found at: $repo" >&2
  echo "Set JCODE_DEV_REPO if your checkout lives elsewhere." >&2
  exit 1
}
[[ -x "$jcode_bin" ]] || {
  echo "Jcode launcher is not executable: $jcode_bin" >&2
  exit 1
}

cd "$repo"
exec "$jcode_bin" self-dev "$@"
