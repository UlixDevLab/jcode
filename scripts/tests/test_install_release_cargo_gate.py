#!/usr/bin/env python3
"""Regression test: release installs must use Jcode's host-wide Cargo gate."""

from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "scripts" / "install_release.sh"


def main() -> None:
    source = INSTALLER.read_text()
    expected = '"$repo_root/scripts/dev_cargo.sh" build --profile "$profile" --manifest-path "$repo_root/Cargo.toml"'

    assert source.count(expected) == 2, "both release build branches must use dev_cargo"
    assert "cargo build --profile" not in source, "installer must not bypass the host-wide Cargo gate"
    print("release installer cargo gate: PASS")


if __name__ == "__main__":
    main()
