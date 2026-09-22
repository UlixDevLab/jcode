#!/usr/bin/env python3
"""Exercise the native process-wide Cargo lock without compiling the workspace."""

from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile
import time


ROOT = pathlib.Path(__file__).resolve().parents[2]
HELPER = ROOT / "scripts" / "cargo_gate.sh"


def spawn_holder(lock_dir: pathlib.Path, hold_seconds: float) -> subprocess.Popen[str]:
    script = f'''\
set -euo pipefail
log() {{ printf '%s\\n' "$*" >&2; }}
source "{HELPER}"
cargo_gate_wait_ms=0
cargo_argv=(check)
JCODE_CARGO_GATE_DIR="{lock_dir}"
acquire_cargo_gate
printf 'acquired status=%s mode=%s wait_ms=%s\\n' "$cargo_gate_status" "$jcode_cargo_gate_lock_mode" "$cargo_gate_wait_ms"
sleep {hold_seconds}
jcode_release_cargo_gate_lock
'''
    return subprocess.Popen(
        ["bash", "-c", script],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=os.environ.copy(),
    )


def test_second_holder_waits_and_the_lock_is_removed() -> None:
    with tempfile.TemporaryDirectory(prefix="jcode-cargo-gate-") as temp_dir:
        lock_dir = pathlib.Path(temp_dir)
        lock_path = lock_dir / "jcode-cargo-build.lock"
        first = spawn_holder(lock_dir, 0.8)
        assert first.stdout is not None
        assert first.stdout.readline().startswith("acquired status=acquired")

        started = time.monotonic()
        second = spawn_holder(lock_dir, 0)
        second_out, second_err = second.communicate(timeout=10)
        elapsed = time.monotonic() - started
        first_out, first_err = first.communicate(timeout=10)

        assert first.returncode == 0, first_err
        assert second.returncode == 0, second_err
        assert "acquired status=acquired" in second_out
        assert elapsed >= 0.5, (elapsed, first_out, second_out)
        assert not lock_path.exists()


if __name__ == "__main__":
    test_second_holder_waits_and_the_lock_is_removed()
    print("cargo gate fallback: PASS")
