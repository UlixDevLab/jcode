#!/bin/bash
set -euo pipefail
umask 077
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
"$SCRIPT_DIR/runtime/node/bin/node" "$SCRIPT_DIR/lifecycle/install.mjs" "$SCRIPT_DIR"
if [[ "${JCODE_LITE_FREE_INSTALL_NO_LAUNCH:-0}" == "1" ]]; then exit 0; fi
exec "$HOME/.local/bin/jcodef" "$@"
