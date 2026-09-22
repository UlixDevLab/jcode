#!/bin/bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
if [[ -x "$SCRIPT_DIR/runtime/node/bin/node" ]]; then
  export PATH="$SCRIPT_DIR/runtime/node/bin:$PATH"
fi
manifest_url="${JCODE_LITE_FREE_MACOS_MANIFEST_URL:-https://files.legrin-tech.net/jcode-lite-free/macos/manifest.json}"
temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/jcode-lite-free-update.XXXXXX")"
trap 'rm -rf "$temp_dir"' EXIT INT TERM
manifest="$temp_dir/manifest.json"
curl --connect-timeout 10 --max-time 300 --fail --location --proto-redir '=https' --proto '=https' --tlsv1.2 --silent --show-error --output "$manifest" "$manifest_url"
current_version="$(/usr/bin/plutil -extract version raw -o - "$SCRIPT_DIR/release.json")"
current_channel="$(/usr/bin/plutil -extract channel raw -o - "$SCRIPT_DIR/release.json")"
verify_args=(
  "$SCRIPT_DIR/verify-update-manifest.mjs" "$manifest" "$SCRIPT_DIR/update-trust.json"
  --product jcode-lite-free --platform macos
  --channel "${JCODE_LITE_FREE_UPDATE_CHANNEL:-$current_channel}"
  --current-version "$current_version"
)
[[ "${JCODE_LITE_FREE_ALLOW_DOWNGRADE:-0}" == "1" ]] && verify_args+=(--allow-downgrade)
node "${verify_args[@]}"
version="$(/usr/bin/plutil -extract version raw -o - "$manifest")"
if [[ "$version" == "$current_version" ]]; then
  echo "Free $version is current. Revalidating client and isolated server."
fi
url="$(/usr/bin/plutil -extract url raw -o - "$manifest")"
sha="$(/usr/bin/plutil -extract sha256 raw -o - "$manifest")"
expected_size="$(/usr/bin/plutil -extract size_bytes raw -o - "$manifest")"
[[ "$url" == https://files.legrin-tech.net/jcode-lite-free/* ]] || { echo "Manifest package URL is not trusted." >&2; exit 1; }
package="$temp_dir/package.zip"
curl --connect-timeout 10 --max-time 300 --fail --location --proto-redir '=https' --proto '=https' --tlsv1.2 --silent --show-error --output "$package" "$url"
actual="$(shasum -a 256 "$package" | awk '{print $1}')"
[[ "$actual" == "$sha" ]] || { echo "Downloaded package hash mismatch." >&2; exit 1; }
actual_size="$(stat -f '%z' "$package")"
[[ "$actual_size" == "$expected_size" ]] || { echo "Downloaded package size mismatch." >&2; exit 1; }
entries="$temp_dir/archive-entries.txt"
unzip -Z1 "$package" > "$entries"
while IFS= read -r entry; do
  case "$entry" in
    ""|/*|*\\*|../*|*/../*|*/..|./*|*/./*|*/.)
      echo "Downloaded package contains an unsafe archive path." >&2
      exit 1
      ;;
  esac
done < "$entries"
if unzip -Z -l "$package" | awk '$1 ~ /^l/ {found=1} END {exit !found}'; then
  echo "Downloaded package contains a symbolic link." >&2; exit 1
fi
mkdir -p "$temp_dir/stage"
unzip -q "$package" -d "$temp_dir/stage"
[[ -f "$temp_dir/stage/allowlist.txt" ]] || { echo "Downloaded package has no allowlist." >&2; exit 1; }
(cd "$temp_dir/stage" && find . -type f -print | sed 's#^\./##' | LC_ALL=C sort) > "$temp_dir/actual-files.txt"
LC_ALL=C sort "$temp_dir/stage/allowlist.txt" > "$temp_dir/allowed-files.txt"
cmp -s "$temp_dir/actual-files.txt" "$temp_dir/allowed-files.txt" || {
  echo "Downloaded package contents do not match its allowlist." >&2
  exit 1
}
echo "Installing Jcode Lite Free $version..."
JCODE_LITE_FREE_INSTALL_NO_LAUNCH=1 "$temp_dir/stage/install.command"
rm -rf "$temp_dir"
trap - EXIT INT TERM
exec "$HOME/.local/bin/jcodef" "$@"
