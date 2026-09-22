#!/usr/bin/env bash
# Safely reclaim disk space from the Cargo target directory.
#
# This is designed to be safe to run even while other builds are in progress on
# this machine (e.g. parallel self-dev agents). It will:
#   - never touch a target/<profile> dir that has an active rustc/cargo process
#     or that was written to within a recent activity window
#   - by default only remove cross-compile / compat caches and obviously stale
#     profile dirs, plus run `cargo clean` on stale profiles
#
# Usage:
#   scripts/clean_target.sh                 # dry-run: report what would be freed
#   scripts/clean_target.sh --apply         # actually delete safe items
#   scripts/clean_target.sh --sweep 7       # also sweep stale artifact generations
#                                           # older than 7 days (keeps the newest
#                                           # generation per crate, so the warm
#                                           # cache survives; dry-run w/o --apply)
#   scripts/clean_target.sh --apply --aggressive  # cargo clean stale profiles
#
# Env:
#   JCODE_CLEAN_ACTIVE_WINDOW_MIN  activity window in minutes (default 20)

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
apply="false"
aggressive="false"
sweep_days=""
activity_window_min="${JCODE_CLEAN_ACTIVE_WINDOW_MIN:-20}"

expect_sweep_days="false"
for arg in "$@"; do
  if [[ "$expect_sweep_days" == "true" ]]; then
    sweep_days="$arg"
    expect_sweep_days="false"
    continue
  fi
  case "$arg" in
    --apply) apply="true" ;;
    --aggressive) aggressive="true" ;;
    --sweep) expect_sweep_days="true" ;;
    --sweep=*) sweep_days="${arg#--sweep=}" ;;
    -h|--help)
      sed -n '2,24p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      printf 'clean_target: unknown arg: %s\n' "$arg" >&2
      exit 2
      ;;
  esac
done
if [[ "$expect_sweep_days" == "true" ]]; then
  printf 'clean_target: --sweep requires a day count\n' >&2
  exit 2
fi
if [[ -n "$sweep_days" && ! "$sweep_days" =~ ^[0-9]+$ ]]; then
  printf 'clean_target: --sweep expects a whole number of days, got: %s\n' "$sweep_days" >&2
  exit 2
fi

log() { printf 'clean_target: %s\n' "$*" >&2; }

human() {
  # Bytes -> human readable
  numfmt --to=iec --suffix=B "${1:-0}" 2>/dev/null || printf '%sB' "${1:-0}"
}

dir_bytes() {
  if du -sb "$1" >/dev/null 2>&1; then
    du -sb "$1" 2>/dev/null | awk '{print $1}'
  else
    # BSD du has no -b. Its KiB result is available on every supported macOS.
    du -sk "$1" 2>/dev/null | awk '{print $1 * 1024}'
  fi
}

# Is any rustc/cargo process currently operating inside this path?
path_has_active_process() {
  local path="$1"
  local p
  for p in $(pgrep -x rustc 2>/dev/null) $(pgrep -x cargo 2>/dev/null); do
    local command_line
    if [[ -r "/proc/$p/cmdline" ]]; then
      command_line=$(tr '\0' ' ' < "/proc/$p/cmdline")
    else
      command_line=$(ps -ww -p "$p" -o command= 2>/dev/null || true)
    fi
    if printf '%s\n' "$command_line" | grep -qF "$path"; then
      return 0
    fi
  done
  return 1
}

# Was this path written to within the activity window?
path_recently_active() {
  local path="$1"
  [[ -d "$path" ]] || return 1
  local recent
  recent=$(python3 - "$path" "$activity_window_min" <<'PY'
import os
import sys
import time

root, minutes = sys.argv[1], int(sys.argv[2])
cutoff = time.time() - minutes * 60
for current, dirs, files in os.walk(root):
    if os.path.relpath(current, root).count(os.sep) >= 3:
        dirs[:] = []
    for name in files:
        try:
            if os.path.getmtime(os.path.join(current, name)) >= cutoff:
                print(name)
                raise SystemExit(0)
        except OSError:
            continue
raise SystemExit(1)
PY
  ) || true
  [[ -n "$recent" ]]
}

is_safe_to_remove() {
  local path="$1"
  [[ -d "$path" ]] || return 1
  if path_has_active_process "$path"; then
    log "SKIP (active process): $path"
    return 1
  fi
  if path_recently_active "$path"; then
    log "SKIP (written <${activity_window_min}min ago): $path"
    return 1
  fi
  return 0
}

total_reclaimed=0

remove_path() {
  local path="$1" reason="$2"
  [[ -e "$path" ]] || return 0
  local bytes
  bytes=$(dir_bytes "$path")
  bytes=${bytes:-0}
  if ! is_safe_to_remove "$path"; then
    return 0
  fi
  if [[ "$apply" == "true" ]]; then
    if rm -rf "$path" 2>/dev/null; then
      log "removed ($reason): $path  [$(human "$bytes")]"
      total_reclaimed=$((total_reclaimed + bytes))
    else
      log "FAILED to remove (permissions? try sudo): $path  [$(human "$bytes")]"
    fi
  else
    log "would remove ($reason): $path  [$(human "$bytes")]"
    total_reclaimed=$((total_reclaimed + bytes))
  fi
}

log "target dir: $target_dir (activity window: ${activity_window_min}min, apply=$apply, aggressive=$aggressive, sweep=${sweep_days:-off})"

# 1) Cross-compile / compat caches: not part of the local dev inner loop. They
#    are regenerated on demand by release/compat scripts.
for d in "$target_dir"/*-apple-darwin "$target_dir"/*-pc-windows-* "$target_dir"/linux-compat; do
  [[ -d "$d" ]] || continue
  # This Mac deliberately builds aarch64-apple-darwin locally. It is the native
  # cache, not a disposable cross-compile artifact.
  if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" \
    && "$(basename "$d")" == "aarch64-apple-darwin" ]]; then
    log "KEEP (native Apple Silicon cache): $d"
    continue
  fi
  remove_path "$d" "cross-compile/compat cache"
done

# 2) Sweep stale artifact generations. When a crate is recompiled (feature or
#    flag change, dependency bump), cargo writes a new `name-<hash>` artifact
#    into deps/ and leaves the old one behind forever. This keeps the newest
#    generation per (crate, extension) so the warm cache is preserved, and only
#    deletes older generations whose mtime exceeds the --sweep day threshold.
#    Cargo transparently rebuilds anything it still needs, so this is safe.
list_stale_dep_generations() {
  local deps="$1" days="$2"
  python3 - "$deps" "$days" <<'PY'
import os
import re
import sys
import time

deps, days = sys.argv[1], int(sys.argv[2])
cutoff = time.time() - days * 24 * 60 * 60
entries = []
for entry in os.scandir(deps):
    if not entry.is_file(follow_symlinks=False):
        continue
    try:
        stat = entry.stat(follow_symlinks=False)
    except OSError:
        continue
    entries.append((stat.st_mtime, stat.st_size, entry.path))

seen = set()
for mtime, size, path in sorted(entries, reverse=True):
    name = os.path.basename(path)
    stem, extension = os.path.splitext(name)
    key = re.sub(r"-[0-9a-f]{16,}$", "", stem) + extension
    if key not in seen:
        seen.add(key)
    elif mtime < cutoff:
        print(f"{size}\t{path}")
PY
}

list_stale_incremental_sessions() {
  local incremental="$1" days="$2"
  python3 - "$incremental" "$days" <<'PY'
import os
import sys
import time

root, days = sys.argv[1], int(sys.argv[2])
cutoff = time.time() - days * 24 * 60 * 60
for entry in os.scandir(root):
    if not entry.is_dir(follow_symlinks=False):
        continue
    try:
        if entry.stat(follow_symlinks=False).st_mtime < cutoff:
            print(entry.path)
    except OSError:
        continue
PY
}

if [[ -n "$sweep_days" ]]; then
  for profile_dir in "$target_dir"/debug "$target_dir"/release "$target_dir"/selfdev; do
    [[ -d "$profile_dir" ]] || continue
    if path_has_active_process "$profile_dir"; then
      log "SKIP sweep (active process): $profile_dir"
      continue
    fi
    profile=$(basename "$profile_dir")
    if [[ -d "$profile_dir/deps" ]]; then
      swept=0
      swept_count=0
      while IFS=$'\t' read -r bytes path; do
        [[ -n "$path" ]] || continue
        if [[ "$apply" == "true" ]]; then
          rm -f -- "$path" 2>/dev/null || continue
        fi
        swept=$((swept + bytes))
        swept_count=$((swept_count + 1))
      done < <(list_stale_dep_generations "$profile_dir/deps" "$sweep_days")
      verb=$([ "$apply" == true ] && echo "swept" || echo "would sweep")
      log "$verb $swept_count stale dep generations (>${sweep_days}d, kept newest per crate) from $profile  [$(human "$swept")]"
      total_reclaimed=$((total_reclaimed + swept))
    fi
    # Incremental compilation session dirs are cheap to regenerate and only
    # useful while their working set is hot; drop ones idle past the threshold.
    if [[ -d "$profile_dir/incremental" ]]; then
      while IFS= read -r d; do
        remove_path "$d" "stale incremental session (>${sweep_days}d)"
      done < <(list_stale_incremental_sessions "$profile_dir/incremental" "$sweep_days")
    fi
  done
fi

# 3) Aggressive: cargo clean on stale (not-recently-active, no active process)
#    profiles to drop accumulated fingerprints/old artifact generations.
if [[ "$aggressive" == "true" ]]; then
  for profile_dir in "$target_dir"/debug "$target_dir"/release "$target_dir"/selfdev; do
    [[ -d "$profile_dir" ]] || continue
    profile=$(basename "$profile_dir")
    [[ "$profile" == "debug" ]] && profile="dev"
    if ! is_safe_to_remove "$profile_dir"; then
      continue
    fi
    before=$(dir_bytes "$profile_dir"); before=${before:-0}
    if [[ "$apply" == "true" ]]; then
      log "cargo clean --profile $profile (stale) ..."
      cargo clean --profile "$profile" 2>/dev/null || log "  cargo clean failed for $profile"
      after=$(dir_bytes "$profile_dir"); after=${after:-0}
      freed=$((before - after))
      (( freed > 0 )) && total_reclaimed=$((total_reclaimed + freed))
      log "  freed $(human "$freed") from $profile"
    else
      log "would cargo clean --profile $profile  [up to $(human "$before")]"
      total_reclaimed=$((total_reclaimed + before))
    fi
  done
fi

log "total $([ "$apply" == true ] && echo reclaimed || echo reclaimable): $(human "$total_reclaimed")"
