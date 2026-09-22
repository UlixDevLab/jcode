#!/usr/bin/env bash
# Build a Jcode Lite / Lite Free Windows package on the Windows host.
#
# One command, no improvisation. Everything this needs is expected to already
# exist on `kingdom-windows-pc`; this script never installs a toolchain, never
# researches the host, and never falls back to Wine or a Mac cross-build.
#
#   scripts/windows/build-windows-package.sh --edition lite
#   scripts/windows/build-windows-package.sh --edition free
#   scripts/windows/build-windows-package.sh --edition both
#
# Options:
#   --edition lite|free|both   which package(s) to build (default: both)
#   --host <ssh-host>          ssh target (default: kingdom-windows-pc)
#   --remote-root <path>       Windows source/build root (default: D:\jcode-win-build)
#   --artifact-root <path>     Windows artifact/cache/temp root (default: D:\jcode-artifacts)
#   --local-artifact-dir <dir> bounded local retrieval directory (default: JCODE_SCRATCH_DIR/lite-release)
#   --skip-sync                reuse the source already on the host
#   --keep-remote              do not delete the remote build tree afterwards
#
# Exit codes:
#   0  success
#   2  bad usage
#   3  Windows host unavailable  (warn-and-stop, by policy)
#   4  remote build failed
#   5  remote packaging/verification failed
#   6  artifact retrieval failed
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
HOST="kingdom-windows-pc"
EDITION="both"
SKIP_SYNC=0
KEEP_REMOTE=0
KEY_FILE=""
REMOTE_ROOT='D:\jcode-win-build'
ARTIFACT_ROOT='D:\jcode-artifacts'
LOCAL_ARTIFACT_DIR="${JCODE_SCRATCH_DIR:-$HOME/.jcode/scratch}/lite-release"
LOG_DIR="${JCODE_SCRATCH_DIR:-$HOME/.jcode/scratch}/windows-build-logs"
STAMP="$(date +%Y%m%dT%H%M%SZ)"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --edition) shift; EDITION="${1:-}" ;;
    --host)    shift; HOST="${1:-}" ;;
    --key-file) shift; KEY_FILE="${1:?--key-file requires a path}" ;;
    --remote-root) shift; REMOTE_ROOT="${1:?--remote-root requires a path}" ;;
    --artifact-root) shift; ARTIFACT_ROOT="${1:?--artifact-root requires a path}" ;;
    --local-artifact-dir) shift; LOCAL_ARTIFACT_DIR="${1:?--local-artifact-dir requires a directory}" ;;
    --skip-sync)  SKIP_SYNC=1 ;;
    --keep-remote) KEEP_REMOTE=1 ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done
case "$EDITION" in lite|free|both) ;; *) echo "--edition must be lite, free or both" >&2; exit 2 ;; esac

mkdir -p "$LOG_DIR"
LOG="$LOG_DIR/windows-build-$STAMP.log"

# Everything goes to the log as well as the terminal, so a failure is always
# diagnosable after the fact instead of needing a re-run to observe.
exec > >(tee -a "$LOG") 2>&1

step()  { printf '\n=== %s ===\n' "$*"; }
info()  { printf '  %s\n' "$*"; }
warn()  { printf '  WARNING: %s\n' "$*" >&2; }
die()   { local code=$1; shift; printf '\nFAILED: %s\n' "$*" >&2; printf 'Full log: %s\n' "$LOG" >&2; exit "$code"; }

printf 'Jcode Windows package build\n'
printf '  started : %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
printf '  host    : %s\n' "$HOST"
printf '  edition : %s\n' "$EDITION"
printf '  remote root : %s\n' "$REMOTE_ROOT"
printf '  artifact root: %s\n' "$ARTIFACT_ROOT"
printf '  log     : %s\n' "$LOG"

step "Checking Windows host availability"
if ! ssh -o ConnectTimeout=15 -o BatchMode=yes "$HOST" "echo ok" >/dev/null 2>&1; then
  warn "Windows host '$HOST' is not reachable."
  warn "Windows packages are built ONLY on that host (policy: no Wine, no Mac cross-build)."
  warn "Bring the host online on tailscale and re-run this script."
  die 3 "Windows host unavailable"
fi
info "host reachable"

step "Verifying required toolchain on the host"
TOOLS="$(ssh "$HOST" 'powershell -NoProfile -Command "cargo --version; rustc --version; node --version"' 2>&1)" \
  || die 3 "could not query toolchain on $HOST: $TOOLS"
printf '%s\n' "$TOOLS" | sed 's/^/  /'
printf '%s' "$TOOLS" | grep -q '^cargo ' || die 3 "cargo not found on $HOST (expected preinstalled)"

GIT_HASH="$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
GIT_DATE="$(git -C "$REPO_ROOT" log -1 --format=%ci)"
GIT_TAG="$(git -C "$REPO_ROOT" describe --tags --always)"
# The tarball below is `git archive HEAD`, so it contains committed content
# only: uncommitted edits in this working tree are never shipped. Reporting the
# working tree's dirtiness would therefore mislabel a release artifact as
# `dirty` because of unrelated in-progress work (e.g. another agent's edit).
# The shipped source IS clean at HEAD, so say so.
GIT_DIRTY=false
if [[ -n "$(git -C "$REPO_ROOT" status --porcelain)" ]]; then
  warn "working tree has uncommitted changes; they are NOT shipped (archive is HEAD)"
fi

# The build script derives the patch number from commits-since-base-tag. We
# ship a `git archive` tarball, which has no .git, so that lookup silently
# falls back to the base patch and the Windows binary reports a different
# version than macOS built from the same commit (observed: 0.75.2-dev vs
# 0.75.114-dev). Pin the semver explicitly so both platforms agree.
BUILD_SEMVER="$(
  cd "$REPO_ROOT" &&
  ./target/release-lto/jcode --version 2>/dev/null |
    sed -E 's/^jcode v([0-9]+\.[0-9]+\.[0-9]+).*/\1/'
)"
if [[ ! "$BUILD_SEMVER" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  BASE="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+)".*/\1/p' "$REPO_ROOT/Cargo.toml" | head -1)"
  OFFSET="$(git -C "$REPO_ROOT" rev-list --count "v$BASE..HEAD" 2>/dev/null || echo 0)"
  BUILD_SEMVER="$(python3 -c "b='$BASE'.split('.'); print(f\"{b[0]}.{b[1]}.{int(b[2])+$OFFSET}\")")"
fi
info "source: $GIT_HASH (dirty=$GIT_DIRTY) semver=$BUILD_SEMVER"
[[ "$GIT_DIRTY" == "true" ]] && warn "working tree is dirty; the built binary will be marked dirty"

if [[ "$SKIP_SYNC" -eq 0 ]]; then
  step "Syncing source to the host"
  TARBALL="${JCODE_SCRATCH_DIR:-$HOME/.jcode/scratch}/jcode-src-$STAMP.tar.gz"
  git -C "$REPO_ROOT" archive --format=tar HEAD | gzip -1 > "$TARBALL" \
    || die 4 "could not create source archive"
  info "archive: $(du -h "$TARBALL" | cut -f1)"
  ssh "$HOST" "powershell -NoProfile -Command \"New-Item -ItemType Directory -Force '$ARTIFACT_ROOT\\transfer','$ARTIFACT_ROOT\\cargo-home','$ARTIFACT_ROOT\\tmp','$ARTIFACT_ROOT\\npm-cache' | Out-Null\"" \
    || die 4 "could not create remote D: build directories"
  scp -q "$TARBALL" "$HOST:${ARTIFACT_ROOT//\\//}/transfer/jcode-src.tar.gz" || die 4 "scp of source archive failed"
  rm -f "$TARBALL"
  ssh "$HOST" "powershell -NoProfile -Command \"Remove-Item -Recurse -Force '$REMOTE_ROOT' -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force '$REMOTE_ROOT' | Out-Null; tar -xzf '$ARTIFACT_ROOT\\transfer\\jcode-src.tar.gz' -C '$REMOTE_ROOT'\"" \
    || die 4 "extracting source on the host failed"
  info "source extracted to $REMOTE_ROOT"
else
  info "skipping source sync (--skip-sync)"
fi

step "Uploading build script"
# Generated here so the git metadata is pinned to this checkout and both
# platforms report identical versions.
#
# NOTE: $ErrorActionPreference is deliberately NOT "Stop" around cargo. cargo
# writes ordinary warnings to stderr, and PowerShell turns native-command
# stderr into a terminating NativeCommandError, which aborted this build in
# ~7 seconds with a benign "profile package spec did not match" warning. Gate
# on $LASTEXITCODE, which is the only reliable success signal.
REMOTE_SCRIPT="${JCODE_SCRATCH_DIR:-$HOME/.jcode/scratch}/win-build-$STAMP.ps1"
cat > "$REMOTE_SCRIPT" <<PS1
\$ErrorActionPreference = "Continue"
Set-Location '$REMOTE_ROOT'

\$env:JCODE_BUILD_SEMVER    = "$BUILD_SEMVER"
\$env:JCODE_BUILD_GIT_HASH  = "$GIT_HASH"
\$env:JCODE_BUILD_GIT_DIRTY = "$GIT_DIRTY"
\$env:JCODE_BUILD_GIT_DATE  = "$GIT_DATE"
\$env:JCODE_BUILD_GIT_TAG   = "$GIT_TAG"
\$env:CARGO_INCREMENTAL     = "0"
\$env:CARGO_HOME            = Join-Path '$ARTIFACT_ROOT' 'cargo-home'
\$env:CARGO_TARGET_DIR      = Join-Path '$REMOTE_ROOT' 'target'
\$env:TEMP                  = Join-Path '$ARTIFACT_ROOT' 'tmp'
\$env:TMP                   = \$env:TEMP
\$env:npm_config_cache      = Join-Path '$ARTIFACT_ROOT' 'npm-cache'
\$env:npm_config_tmp        = \$env:TEMP
New-Item -ItemType Directory -Force \$env:CARGO_HOME,\$env:CARGO_TARGET_DIR,\$env:TEMP,\$env:npm_config_cache | Out-Null

\$vswhere = Join-Path \${env:ProgramFiles(x86)} 'Microsoft Visual Studio\\Installer\\vswhere.exe'
\$vsRoot = & \$vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
\$vsDevCmd = if (\$vsRoot) { Join-Path \$vsRoot 'Common7\\Tools\\VsDevCmd.bat' }
if (!(\$vsDevCmd -and (Test-Path \$vsDevCmd))) { Write-Host 'BUILD_FAILED missing VS2022 x64 C++ tools'; exit 4 }

Write-Host "== cargo build (release-lto, pdf+embeddings) =="
\$buildCommand = 'call "' + \$vsDevCmd + '" -arch=x64 -host_arch=x64 >nul && cargo build --locked --profile release-lto --no-default-features --features pdf,embeddings --bin jcode'
cmd.exe /d /s /c \$buildCommand 2>&1 |
  ForEach-Object { Write-Host \$_ }
if (\$LASTEXITCODE -ne 0) {
  Write-Host "BUILD_FAILED exit=\$LASTEXITCODE"
  exit 4
}

\$exe = Join-Path '$REMOTE_ROOT' 'target\release-lto\jcode.exe'
if (!(Test-Path \$exe)) { Write-Host "BUILD_FAILED missing \$exe"; exit 4 }

Write-Host "== built binary =="
& \$exe --version
if (\$LASTEXITCODE -ne 0) { Write-Host "BUILD_FAILED binary did not run"; exit 4 }
Write-Host ("SHA256 " + (Get-FileHash \$exe -Algorithm SHA256).Hash)
Write-Host "BUILD_OK"
PS1
scp -q "$REMOTE_SCRIPT" "$HOST:${ARTIFACT_ROOT//\\//}/transfer/jcode-win-build.ps1" || die 4 "uploading build script failed"
rm -f "$REMOTE_SCRIPT"
info "uploaded"

step "Building jcode.exe on the host (release-lto; this takes a while)"
BUILD_OUT="$(ssh "$HOST" "powershell -NoProfile -ExecutionPolicy Bypass -File $ARTIFACT_ROOT\\transfer\\jcode-win-build.ps1" 2>&1)"
printf '%s\n' "$BUILD_OUT" | tail -30 | sed 's/^/  /'
printf '%s' "$BUILD_OUT" | grep -q 'BUILD_OK' || die 4 "cargo build failed on the host (see log)"
WIN_VERSION="$(printf '%s' "$BUILD_OUT" | grep -oE 'jcode v[0-9][^ ]*( \([^)]*\))?' | head -1)"
info "built: ${WIN_VERSION:-unknown}"

package_edition() {
  local edition="$1" script rel out_name
  case "$edition" in
    lite) script='distribution\jcode-lite\build.ps1';      rel='distribution/jcode-lite/release.json' ;;
    free) script='distribution\jcode-lite-free\build.ps1'; rel='distribution/jcode-lite-free/release.json' ;;
  esac
  local version
  version="$(python3 -c "import json;print(json.load(open('$REPO_ROOT/$rel'))['version'])")"
  case "$edition" in
    lite) out_name="jcode-lite-windows-$version.zip" ;;
    free) out_name="jcode-lite-free-windows-$version.zip" ;;
  esac

  step "Packaging $edition ($version)"

  # Lite embeds the Stables credential; Free ships none by design. Without
  # -KeyFile the Lite archive is the GENERIC keyless build, which installs
  # fine and then resolves no models at all. That shipped once (2026-08-18)
  # and is exactly what this flag prevents.
  local key_arg=""
  if [[ "$edition" == "lite" ]]; then
    if [[ -z "$KEY_FILE" ]]; then
      die 5 "lite packaging requires --key-file; refusing to build a keyless private package"
    fi
    [[ -f "$KEY_FILE" ]] || die 5 "key file not found: $KEY_FILE"
    scp -q "$KEY_FILE" "$HOST:${ARTIFACT_ROOT//\\//}/transfer/jcode-stables-key.env" || die 5 "uploading key file failed"
    key_arg=" -KeyFile '$ARTIFACT_ROOT\\transfer\\jcode-stables-key.env'"
  fi

  # Driven from an uploaded .ps1 rather than an inline `ssh ... -Command`
  # string: the nested bash/ssh/PowerShell quoting mangled `$LASTEXITCODE`
  # into a literal, and dropping the mandatory -Platform argument was invisible
  # until the remote script failed. A file has one level of quoting.
  local drv="${JCODE_SCRATCH_DIR:-$HOME/.jcode/scratch}/pkg-$edition-$STAMP.ps1"
  cat > "$drv" <<PKG
\$ErrorActionPreference = "Continue"
Set-Location '$REMOTE_ROOT'
\$env:CARGO_HOME       = Join-Path '$ARTIFACT_ROOT' 'cargo-home'
\$env:TEMP             = Join-Path '$ARTIFACT_ROOT' 'tmp'
\$env:TMP              = \$env:TEMP
\$env:npm_config_cache = Join-Path '$ARTIFACT_ROOT' 'npm-cache'
\$env:npm_config_tmp   = \$env:TEMP
New-Item -ItemType Directory -Force \$env:TEMP,\$env:npm_config_cache | Out-Null
& '$REMOTE_ROOT\\$script' -Platform windows -Binary '$REMOTE_ROOT\\target\\release-lto\\jcode.exe' -Output '$ARTIFACT_ROOT\\$out_name'$key_arg 2>&1 |
  ForEach-Object { Write-Host \$_ }
if (\$LASTEXITCODE -ne 0) { Write-Host "PACKAGE_FAILED exit=\$LASTEXITCODE"; exit 5 }
if (!(Test-Path '$ARTIFACT_ROOT\\$out_name')) { Write-Host "PACKAGE_FAILED missing archive"; exit 5 }
Write-Host "PACKAGE_OK"
PKG
  scp -q "$drv" "$HOST:${ARTIFACT_ROOT//\\//}/transfer/pkg-$edition.ps1" || die 5 "uploading $edition packaging script failed"
  rm -f "$drv"

  local out
  out="$(ssh "$HOST" "powershell -NoProfile -ExecutionPolicy Bypass -File $ARTIFACT_ROOT\\transfer\\pkg-$edition.ps1" 2>&1)"
  printf '%s\n' "$out" | tail -20 | sed 's/^/  /'
  printf '%s' "$out" | grep -q 'PACKAGE_OK' || die 5 "$edition packaging failed on the host"
  printf '%s' "$out" | grep -qiE 'PASS|contract verified' || die 5 "$edition did not report verifier PASS"

  local dest="$LOCAL_ARTIFACT_DIR/$out_name"
  mkdir -p "$(dirname "$dest")"
  scp -q "$HOST:${ARTIFACT_ROOT//\\//}/$out_name" "$dest" || die 6 "could not retrieve $out_name"
  info "retrieved: $dest ($(du -h "$dest" | cut -f1))"
  shasum -a 256 "$dest" | sed 's/^/  /'
}

[[ "$EDITION" == "lite" || "$EDITION" == "both" ]] && package_edition lite
[[ "$EDITION" == "free" || "$EDITION" == "both" ]] && package_edition free

if [[ "$KEEP_REMOTE" -eq 0 ]]; then
  step "Cleaning up the host"
  # Windows artifacts are built and kept on Windows; the local disk here is
  # scarce. Only the finished zips come back.
  ssh "$HOST" "powershell -NoProfile -Command \"Remove-Item -Force '$ARTIFACT_ROOT\\transfer\\jcode-src.tar.gz','$ARTIFACT_ROOT\\transfer\\jcode-win-build.ps1','$ARTIFACT_ROOT\\transfer\\jcode-stables-key.env','$ARTIFACT_ROOT\\transfer\\pkg-lite.ps1','$ARTIFACT_ROOT\\transfer\\pkg-free.ps1' -ErrorAction SilentlyContinue\"" >/dev/null 2>&1
  info "removed transfer artifacts (build tree kept for incremental reuse)"
fi

step "Done"
printf 'Windows package(s) built from %s\n' "$GIT_HASH"
printf 'Log: %s\n' "$LOG"
