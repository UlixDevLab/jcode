#!/usr/bin/env bash
# Shared process lock for compile-capable Jcode Cargo commands.
#
# Linux images normally provide flock(1). macOS does not, but it does ship
# shlock(1), whose atomic dot-locking semantics include stale-PID recovery.
# This file is sourced by dev_cargo.sh so the acquired lock remains owned by
# the invoking shell for the entire Cargo action.

jcode_cargo_gate_lock_mode=""
jcode_cargo_gate_lock_path=""

jcode_acquire_cargo_gate_lock() {
  local gate_path="$1"
  local wait_started_ns wait_finished_ns
  jcode_cargo_gate_lock_mode=""
  jcode_cargo_gate_lock_path="$gate_path"

  if command -v flock >/dev/null 2>&1; then
    exec {cargo_gate_fd}>"$gate_path"
    if ! flock -n "$cargo_gate_fd"; then
      log "waiting for the host-wide Cargo gate ($gate_path)"
      wait_started_ns=$(date +%s%N)
      flock "$cargo_gate_fd"
      wait_finished_ns=$(date +%s%N)
      cargo_gate_wait_ms=$(( (wait_finished_ns - wait_started_ns) / 1000000 ))
    fi
    jcode_cargo_gate_lock_mode="flock"
    return 0
  fi

  if command -v shlock >/dev/null 2>&1; then
    if ! shlock -f "$gate_path" -p "$$" >/dev/null 2>&1; then
      log "waiting for the host-wide Cargo gate ($gate_path)"
      wait_started_ns=$(date +%s%N)
      until shlock -f "$gate_path" -p "$$" >/dev/null 2>&1; do
        sleep 0.1
      done
      wait_finished_ns=$(date +%s%N)
      cargo_gate_wait_ms=$(( (wait_finished_ns - wait_started_ns) / 1000000 ))
    fi
    jcode_cargo_gate_lock_mode="shlock"
    return 0
  fi

  return 2
}

jcode_release_cargo_gate_lock() {
  if [[ "${jcode_cargo_gate_lock_mode:-}" == "shlock" \
    && -n "${jcode_cargo_gate_lock_path:-}" ]]; then
    rm -f -- "$jcode_cargo_gate_lock_path"
  fi
  jcode_cargo_gate_lock_mode=""
  jcode_cargo_gate_lock_path=""
}

jcode_cargo_action_needs_gate() {
  case "${cargo_argv[0]:-}" in
    build|check|clippy|test|bench|run|rustc|rustdoc) return 0 ;;
    *) return 1 ;;
  esac
}

# Cargo only coordinates processes sharing exactly the same package cache and
# target directory. Jcode worktrees and toolchains do not always share either,
# so serialize compile-capable local actions across the host.
acquire_cargo_gate() {
  cargo_gate_status="not-needed"
  cargo_gate_wait_ms=0
  jcode_cargo_action_needs_gate || return 0

  case "${JCODE_CARGO_GATE:-on}" in
    0|false|no|off|disabled)
      cargo_gate_status="disabled"
      return 0
      ;;
  esac
  if [[ "${JCODE_CARGO_GATE_HELD:-0}" == "1" ]]; then
    cargo_gate_status="inherited"
    return 0
  fi

  local gate_lock_status=0 gate_dir gate_path
  gate_dir="${JCODE_CARGO_GATE_DIR:-${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}}"
  mkdir -p "$gate_dir"
  gate_path="${JCODE_CARGO_GATE_PATH:-$gate_dir/jcode-cargo-build.lock}"
  if jcode_acquire_cargo_gate_lock "$gate_path"; then
    :
  else
    gate_lock_status=$?
  fi
  if [[ "$gate_lock_status" -ne 0 ]]; then
    cargo_gate_status="unavailable"
    log "no supported host-wide Cargo gate (need flock or shlock); running without serialization"
    return 0
  fi
  export JCODE_CARGO_GATE_HELD=1
  cargo_gate_status="acquired:${jcode_cargo_gate_lock_mode}"
  log "acquired host-wide Cargo gate (waited ${cargo_gate_wait_ms}ms)"
}
