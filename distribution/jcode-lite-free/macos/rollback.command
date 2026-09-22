#!/bin/bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
state="$ROOT/state.json"
[[ -f "$state" ]] || { echo "No Jcode Lite Free state record." >&2; exit 1; }
current="$(/usr/bin/plutil -extract current raw -o - "$state")"
previous="$(/usr/bin/plutil -extract previous raw -o - "$state")"
[[ -n "$previous" && -x "$ROOT/$previous/jcode-free" ]] || { echo "No rollback version is available." >&2; exit 1; }
printf '{"current":"%s","previous":"%s"}\n' "$previous" "$current" > "$state"
chmod 600 "$state"
echo "Rolled back Jcode Lite Free from $current to $previous."
