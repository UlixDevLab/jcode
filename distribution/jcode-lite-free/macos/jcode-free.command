#!/bin/bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
INSTALL_ROOT="$HOME/Library/Application Support/LeGrin/JcodeLiteFree"
case "$SCRIPT_DIR/" in
  "$INSTALL_ROOT/"*) ;;
  *) exec "$SCRIPT_DIR/install.command" "$@" ;;
esac
cd "$HOME"
exec "$SCRIPT_DIR/jcode-free" "$@"
