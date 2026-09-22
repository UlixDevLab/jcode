#!/bin/bash
# Emit trusted package provenance for an exact-current supplied Jcode binary.
set -euo pipefail

binary="${1:?usage: verify-binary-version.sh <jcode-binary> <expected-git-hash>}"
expected_hash="${2:?usage: verify-binary-version.sh <jcode-binary> <expected-git-hash>}"
[[ -x "$binary" ]] || { echo "Jcode binary is not executable: $binary" >&2; exit 1; }

report="$("$binary" --no-update version --json)" || {
  echo "Could not read supplied Jcode binary version." >&2
  exit 1
}
python3 - "$expected_hash" "$report" <<'PY'
import json
import re
import sys

expected_hash, report = sys.argv[1:]
try:
    data = json.loads(report)
except json.JSONDecodeError as error:
    raise SystemExit(f"Supplied Jcode binary returned invalid version JSON: {error}")

binary_hash = data.get("git_hash")
version = data.get("version")
if not isinstance(binary_hash, str) or not re.fullmatch(r"[0-9a-f]{7,40}", binary_hash):
    raise SystemExit("Supplied Jcode binary reported an invalid version git_hash.")
if not isinstance(version, str):
    raise SystemExit("Supplied Jcode binary reported an invalid version string.")
match = re.fullmatch(r"v([^ ]+) \([0-9a-f]{7,40}(?:, [^)]+)?\)", version)
if not match:
    raise SystemExit("Supplied Jcode binary reported an invalid version string.")
if binary_hash != expected_hash:
    raise SystemExit(
        f"Supplied Jcode binary git_hash {binary_hash} does not match current source {expected_hash}. "
        "Build the native release-lto binary from the current checkout before packaging."
    )
print(json.dumps({"git_hash": binary_hash, "jcode_version": match.group(1)}, sort_keys=True))
PY
