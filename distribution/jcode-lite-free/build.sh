#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd -P)"
SHARED="$ROOT/../jcode-lite/common/preset"
platform=""
binary=""
output=""
skip_npm=""
node_runtime=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --platform) shift; platform="${1:?--platform requires macos or windows}" ;;
    --binary) shift; binary="${1:?--binary requires a path}" ;;
    --output) shift; output="${1:?--output requires a path}" ;;
    --skip-mcp-npm-ci) skip_npm=1 ;;
    --node-runtime) shift; node_runtime="${1:?--node-runtime requires a path}" ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done
[[ "$platform" == "macos" || "$platform" == "windows" ]] || { echo "--platform must be macos or windows" >&2; exit 2; }
[[ -f "$binary" ]] || { echo "Jcode binary not found: $binary" >&2; exit 1; }
[[ -z "$node_runtime" || -f "$node_runtime" ]] || { echo "Node runtime archive not found: $node_runtime" >&2; exit 1; }
[[ -x "$binary" ]] || { echo "Native binary is not executable" >&2; exit 1; }
source_hash="$(git -C "$ROOT" rev-parse --short HEAD)"
binary_provenance="$("$ROOT/../jcode-lite/common/verify-binary-version.sh" "$binary" "$source_hash")"
version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$ROOT/release.json")"
output="${output:-$ROOT/jcode-lite-free-$platform-$version.zip}"
[[ "$output" == /* ]] || output="$(pwd -P)/$output"

stage="$(mktemp -d "${TMPDIR:-/tmp}/jcode-lite-free-build.XXXXXX")"
files="${stage}.files"
cleanup() { rm -rf "$stage"; rm -f "$files"; }
trap cleanup EXIT INT TERM

mkdir -p "$stage/preset"
cp "$ROOT/preset/config.toml" "$stage/preset/config.toml"
cp "$ROOT/preset/swarm-prompt.md" "$stage/preset/swarm-prompt.md"
cp "$ROOT/preset/install-assets.mjs" "$stage/preset/install-assets.mjs"
cp -R "$SHARED/skills" "$stage/preset/skills"
cp -R "$SHARED/roles" "$stage/preset/roles"
cp -R "$SHARED/knowledge-os" "$stage/preset/knowledge-os"
cp -R "$SHARED/mcp" "$stage/preset/mcp"
# __pycache__ is gitignored but not glob-excluded, so a leftover bytecode
# cache from running the test suite locally could ship stale compiled
# strings that no longer exist in source. Nothing here is ever meant to run
# as compiled bytecode.
find "$stage" -type d -name "__pycache__" -prune -exec rm -rf {} +
# Free does NOT inherit shared/preset/swarm-prompt.md because the shared
# prompt pins private Lite model routes. Free ships a provider-neutral
# override at $ROOT/preset/swarm-prompt.md.
cp -R "$ROOT/$platform/." "$stage/"
cp "$ROOT/../jcode-lite/common/verify-update-manifest.mjs" "$stage/verify-update-manifest.mjs"
cp "$ROOT/../jcode-lite/common/update-trust.json" "$stage/update-trust.json"
cp "$ROOT/../jcode-lite/common/check-update.mjs" "$stage/check-update.mjs"
cp "$ROOT/release.json" "$ROOT/lite-manifest.json" "$stage/"
cp "$ROOT/../../LICENSE" "$stage/LICENSE"
python3 - "$stage/release.json" "$stage/jcode-provenance.json" "$binary_provenance" <<'PROVENANCE'
import json,sys
release_path,provenance_path,raw=sys.argv[1:]
provenance=json.loads(raw)
release=json.load(open(release_path))
release['jcode_version']=provenance['jcode_version']
open(release_path,'w').write(json.dumps(release,indent=2)+'\n')
open(provenance_path,'w').write(json.dumps(provenance,indent=2)+'\n')
PROVENANCE
# Plain-language instructions at the archive root. The recipient may have no
# terminal instincts and nobody to ask, so this must be the first thing visible
# after extracting, not buried under preset/.
cp "$ROOT/preset/START-HERE.md" "$stage/START-HERE.md"
mkdir -p "$stage/bin"
if [[ "$platform" == "macos" ]]; then
  [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]] || { echo "macOS Free packages must be assembled natively on Apple Silicon" >&2; exit 1; }
  cp "$binary" "$stage/bin/jcode"
  cp -R "$ROOT/../jcode-lite/macos/lifecycle" "$stage/lifecycle"
  cp "$ROOT/macos/edition.mjs" "$stage/lifecycle/edition.mjs"
  cp "$ROOT/macos/lifecycle-launcher" "$stage/jcode-free"
  cp "$ROOT/macos/lifecycle-install.command" "$stage/install.command"
  rm -f "$stage/edition.mjs" "$stage/lifecycle-launcher" "$stage/lifecycle-install.command"
  node_args=(--manifest "$ROOT/../jcode-lite/common/node-runtime.json" --stage "$stage")
  [[ -z "$node_runtime" ]] || node_args+=(--archive "$node_runtime")
  python3 "$ROOT/support/bundle-node.py" "${node_args[@]}"
  export PATH="$stage/runtime/node/bin:$PATH"
  for payload in "$stage/bin/jcode" "$stage/runtime/node/bin/node"; do
    /usr/bin/codesign --force --sign - "$payload"
    /usr/bin/codesign --verify --strict "$payload"
  done
  chmod 755 "$stage/bin/jcode" "$stage/jcode-free" "$stage"/*.command \
    "$stage/preset/knowledge-os/bin/knowledge-os-lite.mjs" \
    "$stage/preset/mcp/bin/playwright" "$stage/preset/mcp/bin/context7" \
    "$stage/preset/mcp/bin/omnisearch" "$stage/preset/mcp/bin/reddit" \
    "$stage/preset/mcp/bin/merge-mcp.py"
else
  cp "$binary" "$stage/bin/jcode.exe"
fi

if [[ -z "$skip_npm" ]]; then
  command -v node >/dev/null 2>&1 || { echo "Node.js is required to vendor MCP packages." >&2; exit 1; }
  command -v npm >/dev/null 2>&1 || { echo "npm is required to vendor MCP packages (use --skip-mcp-npm-ci if node_modules is pre-staged)." >&2; exit 1; }
  echo "Vendoring MCP Node packages via npm ci..."
  # PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 keeps the package deterministic:
  # the Playwright wrapper insists on a system Chrome/Edge and we never
  # want npm to download a browser binary into node_modules.
  (cd "$stage/preset/mcp" && PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 \
    npm ci --omit=dev --ignore-scripts --no-audit --no-fund)
fi

if grep -RIlE 'llmr_[A-Za-z0-9]{44}|STABLES_API_KEY|router\.legrin-tech\.net' "$stage" --exclude=jcode --exclude=jcode.exe | grep -q .; then
  echo "Credential or Stables routing material found in free package." >&2
  exit 1
fi

: > "$stage/allowlist.txt"
(cd "$stage" && find . -type f -print | sed 's#^\./##' | LC_ALL=C sort > allowlist.txt)
touch -t 202608160000 "$stage"/* 2>/dev/null || true
(cd "$stage" && find . -type f -print0 | xargs -0 touch -t 202608160000)
(cd "$stage" && find . -type f -print | sed 's#^\./##' | LC_ALL=C sort > "$files")
[[ ! -e "$output" ]] || { echo "Refusing to overwrite an existing artifact: $output" >&2; exit 1; }
if command -v zip >/dev/null 2>&1; then
  (cd "$stage" && zip -X -q "$output" -@ < "$files")
else
  python3 "$ROOT/support/create-zip.py" "$stage" "$output" "$files"
fi
python3 "$ROOT/tests/verify-package.py" "$output"
echo "Built $output"
shasum -a 256 "$output"
