#!/bin/bash
set -euo pipefail

PRODUCT_NAME="Jcode Lite Free"
ROOT="$HOME/Library/Application Support/LeGrin/JcodeLiteFree"
CLI="$HOME/.local/bin/jcodef"
NO_CONFIRM_VAR="${JCODE_LITE_FREE_REMOVE_NO_CONFIRM:-0}"
purge=false

for arg in "$@"; do
  case "$arg" in
    --purge-data) purge=true ;;
    --help|-h)
      cat <<'EOF'
Remove Jcode Lite Free app files while keeping sessions, memory, credentials, and config.

Usage:
  remove.command                 Keep user data in JcodeLiteFree/home (default)
  remove.command --purge-data    Delete the app and all isolated user data
EOF
      exit 0
      ;;
    *) echo "Unknown argument: $arg" >&2; exit 2 ;;
  esac
done

[[ -d "$ROOT" ]] || { echo "$PRODUCT_NAME is not installed."; exit 0; }

if $purge && [[ "$NO_CONFIRM_VAR" != "1" ]]; then
  if [[ ! -t 0 ]]; then
    echo "Refusing non-interactive data purge. Set JCODE_LITE_FREE_REMOVE_NO_CONFIRM=1 to confirm." >&2
    exit 1
  fi
  echo "This permanently deletes all Jcode Lite Free sessions, memory, credentials, and config in:"
  echo "  $ROOT/home"
  read -r -p "Type DELETE to continue: " answer
  [[ "$answer" == "DELETE" ]] || { echo "Cancelled. Nothing was removed."; exit 1; }
fi

rm -f "$CLI"
if $purge; then
  rm -rf "$ROOT"
  echo "Removed $PRODUCT_NAME and all isolated user data."
else
  find "$ROOT" -mindepth 1 -maxdepth 1 ! -name home -exec rm -rf {} +
  echo "Removed $PRODUCT_NAME app files."
  echo "Kept sessions, memory, credentials, and config in:"
  echo "  $ROOT/home"
  echo "A future install will resume from the same data."
fi
