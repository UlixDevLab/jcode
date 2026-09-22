#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
[[ "$(uname -s)" == "Darwin" ]] || { echo "This package requires macOS." >&2; exit 1; }
if [[ -x "$SCRIPT_DIR/runtime/node/bin/node" ]]; then
  export PATH="$SCRIPT_DIR/runtime/node/bin:$PATH"
fi
command -v node >/dev/null 2>&1 || { echo "Install Node.js 20 or newer, then retry." >&2; exit 1; }
node_major="$(node -p 'process.versions.node.split(`.`)[0]')"
(( node_major >= 20 )) || { echo "Install Node.js 20 or newer, then retry." >&2; exit 1; }

version="$(/usr/bin/plutil -extract version raw -o - "$SCRIPT_DIR/release.json")"
root="$HOME/Library/Application Support/LeGrin/JcodeLiteFree"
target="$root/$version"
stage_target="$root/.staging-$version-$$"
home_dir="$root/home"
previous=""
if [[ -f "$root/state.json" ]]; then
  previous="$(/usr/bin/plutil -extract current raw -o - "$root/state.json" 2>/dev/null || true)"
  [[ "$previous" == "$version" ]] && previous="$(/usr/bin/plutil -extract previous raw -o - "$root/state.json" 2>/dev/null || true)"
fi

umask 077
cleanup_stage() { rm -rf "$stage_target"; }
trap cleanup_stage EXIT INT TERM
rm -rf "$stage_target"
mkdir -p "$stage_target" "$home_dir"
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  from="$SCRIPT_DIR/$file"
  to="$stage_target/$file"
  [[ -f "$from" ]] || { echo "Package file missing: $file" >&2; exit 1; }
  mkdir -p "$(dirname "$to")"
  cp "$from" "$to"
done < "$SCRIPT_DIR/allowlist.txt"
chmod 755 "$stage_target/bin/jcode" "$stage_target/jcode-free" "$stage_target"/*.command \
  "$stage_target/preset/knowledge-os/bin/knowledge-os-lite.mjs" \
  "$stage_target/preset/mcp/bin/playwright" "$stage_target/preset/mcp/bin/context7" \
  "$stage_target/preset/mcp/bin/omnisearch" "$stage_target/preset/mcp/bin/reddit" \
  "$stage_target/preset/mcp/bin/merge-mcp.py"

# Preserve the publisher's signature and macOS security checks. Unsigned builds
# may require the recipient's normal Privacy & Security confirmation.

# Install bundled MCP node_modules on first install or when missing.
# PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 keeps the package deterministic: the
# Playwright wrapper refuses to invoke a vendored browser download at runtime
# and the install flow never tries either.
if [[ ! -d "$stage_target/preset/mcp/node_modules" ]]; then
  echo "Vendoring MCP packages via npm ci (one-time, deterministic)…"
  (cd "$stage_target/preset/mcp" && PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 \
    npm ci --omit=dev --ignore-scripts --no-audit --no-fund)
fi

JCODE_HOME="$home_dir" "$stage_target/bin/jcode" --no-update version >/dev/null
rm -rf "$target"
mv "$stage_target" "$target"
trap - EXIT INT TERM
if [[ -x "$target/runtime/node/bin/node" ]]; then
  export PATH="$target/runtime/node/bin:$PATH"
fi

# Free is provider-neutral. NEVER overwrite the user's existing config.toml —
# preserve any provider profile or model the user has already configured.
if [[ ! -f "$home_dir/config.toml" ]]; then
  cp "$target/preset/config.toml" "$home_dir/config.toml"
  chmod 600 "$home_dir/config.toml"
else
  JCODE_HOME="$home_dir" node "$target/preset/mcp/bin/merge-mcp.mjs" \
    --user-config "$home_dir/config.toml" \
    --bundled-config "$target/preset/config.toml" >/dev/null
fi

node "$target/preset/install-assets.mjs" "$target/preset" "$home_dir"

# Idempotent mcp.json merge.
JCODE_HOME="$home_dir" node "$target/preset/mcp/bin/merge-mcp.mjs" \
  --manifest "$target/preset/mcp/manifest.json"

state_new="$root/state.json.new.$$"
printf '{"current":"%s","previous":"%s"}\n' "$version" "$previous" > "$state_new"
chmod 600 "$state_new" "$home_dir/config.toml"
mv "$state_new" "$root/state.json"

cli_dir="$HOME/.local/bin"
cli="$cli_dir/jcodef"
mkdir -p "$cli_dir"
cat > "$cli" <<'EOF'
#!/bin/bash
set -euo pipefail
root="$HOME/Library/Application Support/LeGrin/JcodeLiteFree"
state="$root/state.json"
[[ -f "$state" ]] || { echo "Jcode Lite Free is not installed." >&2; exit 1; }
current="$(/usr/bin/plutil -extract current raw -o - "$state" 2>/dev/null)"
if [[ "${1:-}" == "remove" ]]; then
  shift
  remove="$root/$current/remove.command"
  [[ -x "$remove" ]] || { echo "The Jcode Lite Free removal command is missing." >&2; exit 1; }
  cd "$HOME"
  exec "$remove" "$@"
fi
launcher="$root/$current/jcode-free"
[[ -x "$launcher" ]] || { echo "The current Jcode Lite Free launcher is missing." >&2; exit 1; }
exec "$launcher" "$@"
EOF
chmod 755 "$cli"

path_marker="# Jcode Lite Free CLI"
path_line='export PATH="$HOME/.local/bin:$PATH"'
case "$(basename "${SHELL:-/bin/zsh}")" in
  zsh) profiles=("$HOME/.zprofile" "$HOME/.zshrc") ;;
  bash) profiles=("$HOME/.bash_profile" "$HOME/.bashrc") ;;
  *) profiles=("$HOME/.profile") ;;
esac
for profile in "${profiles[@]}"; do
  touch "$profile"
  if ! grep -Fq "$path_marker" "$profile"; then
    printf '\n%s\n%s\n' "$path_marker" "$path_line" >> "$profile"
  fi
done

echo "Installed Jcode Lite Free $version."
echo "Installed terminal command: jcodef (available in new terminal sessions)."
echo "No provider is bundled. Jcode will guide you through provider setup on first use."
echo "MCP surface: playwright, context7, omnisearch, reddit (anonymous by default)."
if [[ "${JCODE_LITE_FREE_INSTALL_NO_LAUNCH:-0}" == "1" ]]; then
  echo "Start with: $target/jcode-free.command"
  exit 0
fi
echo "Launching Jcode Lite Free..."
cd "$HOME"
exec "$target/jcode-free" "$@"
