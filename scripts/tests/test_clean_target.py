#!/usr/bin/env python3
"""Exercise portable stale-artifact discovery without deleting real targets."""

from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile
import time


ROOT = pathlib.Path(__file__).resolve().parents[2]
CLEANER = ROOT / "scripts" / "clean_target.sh"


def write_old(path: pathlib.Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("fixture", encoding="utf-8")
    old = time.time() - 2 * 24 * 60 * 60
    os.utime(path, (old, old))


def test_dry_run_discovers_stale_incremental_and_dependency_entries() -> None:
    with tempfile.TemporaryDirectory(prefix="jcode-clean-target-") as temp_dir:
        target = pathlib.Path(temp_dir)
        write_old(target / "debug" / "deps" / "crate-1111111111111111.rlib")
        write_old(target / "debug" / "deps" / "crate-2222222222222222.rlib")
        write_old(target / "aarch64-apple-darwin" / "debug" / "native")
        stale_incremental = target / "debug" / "incremental" / "crate-old"
        write_old(stale_incremental / "state")
        old = time.time() - 2 * 24 * 60 * 60
        os.utime(stale_incremental, (old, old))

        result = subprocess.run(
            ["bash", str(CLEANER), "--sweep", "1"],
            env={**os.environ, "CARGO_TARGET_DIR": str(target)},
            text=True,
            capture_output=True,
            check=False,
        )

        assert result.returncode == 0, result.stderr
        assert "would sweep 1 stale dep generations" in result.stderr
        assert "would remove (stale incremental session" in result.stderr
        if os.uname().sysname == "Darwin" and os.uname().machine == "arm64":
            assert "KEEP (native Apple Silicon cache)" in result.stderr
        assert stale_incremental.exists(), "dry-run must not remove artifacts"


if __name__ == "__main__":
    test_dry_run_discovers_stale_incremental_and_dependency_entries()
    print("clean target portability: PASS")
