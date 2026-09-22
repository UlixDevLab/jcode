#!/usr/bin/env python3
"""Check lightweight crate dependency boundaries.

Type crates should remain data-contract crates. This guard intentionally starts
small: it blocks direct dependencies from any `jcode-*-types` crate to root or
runtime-heavy internal crates. It allows external dependencies for now, while
making internal domain leaks visible and easy to extend.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Internal crates that are allowed as dependencies of type crates.
# Keep this list narrow. Add a crate only if it is itself a data-contract crate.
ALLOWED_INTERNAL_TYPE_DEPS = {
    "jcode-message-types",
}

# Internal crates that type crates must not depend on directly. Most are runtime,
# provider, UI, storage, or root behavior crates. `jcode-core` is intentionally
# blocked so it does not become the backdoor catch-all dependency for DTO crates.
FORBIDDEN_INTERNAL_DEPS = {
    "jcode",
    "jcode-agent-runtime",
    "jcode-azure-auth",
    "jcode-core",
    "jcode-embedding",
    "jcode-mobile-core",
    "jcode-mobile-sim",
    "jcode-notify-email",
    "jcode-pdf",
    "jcode-plan",
    "jcode-provider-core",
    "jcode-provider-gemini",
    "jcode-provider-metadata",
    "jcode-provider-openrouter",
    "jcode-protocol",
    "jcode-terminal-launch",
    "jcode-tui-core",
    "jcode-tui-markdown",
    "jcode-tui-mermaid",
    "jcode-tui-render",
    "jcode-tui-workspace",
}


def cargo_metadata() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    )
    return json.loads(result.stdout)


def is_type_crate(name: str) -> bool:
    return name.startswith("jcode-") and name.endswith("-types")


DESKTOP_FORBIDDEN_DEPENDENCY_FRAGMENTS = (
    "tauri",
    "gpui",
    "winit",
    "vello",
    "parley",
    "wry",
)


def resolved_dependency_names(metadata: dict, package_name: str) -> set[str]:
    package_by_id = {package["id"]: package for package in metadata["packages"]}
    root_id = next(
        package_id
        for package_id, package in package_by_id.items()
        if package["name"] == package_name
    )
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    pending = [root_id]
    visited: set[str] = set()
    names: set[str] = set()
    while pending:
        package_id = pending.pop()
        if package_id in visited:
            continue
        visited.add(package_id)
        package = package_by_id[package_id]
        names.add(package["name"])
        pending.extend(dep["pkg"] for dep in nodes[package_id]["deps"])
    return names


def desktop_dependency_errors(metadata: dict) -> list[str]:
    errors: list[str] = []
    core_names = resolved_dependency_names(metadata, "jcode-desktop-core")
    runtime_names = resolved_dependency_names(metadata, "jcode-desktop-runtime")

    for name in sorted(core_names):
        if name == "jcode-sdk" or name == "jcode-harness-api" or any(
            fragment in name.lower() for fragment in DESKTOP_FORBIDDEN_DEPENDENCY_FRAGMENTS
        ):
            errors.append(
                "jcode-desktop-core must remain SDK/UI/platform neutral; "
                f"resolved forbidden dependency {name}"
            )
    if "jcode-sdk" not in runtime_names:
        errors.append("jcode-desktop-runtime must depend on jcode-sdk")
    for name in sorted(runtime_names):
        if any(fragment in name.lower() for fragment in DESKTOP_FORBIDDEN_DEPENDENCY_FRAGMENTS):
            errors.append(
                "jcode-desktop-runtime must remain UI/platform neutral; "
                f"resolved forbidden dependency {name}"
            )
    return errors


def main() -> int:
    metadata = cargo_metadata()
    package_by_id = {package["id"]: package for package in metadata["packages"]}
    workspace_ids = set(metadata["workspace_members"])
    workspace_names = {
        package_by_id[package_id]["name"] for package_id in workspace_ids if package_id in package_by_id
    }

    errors: list[str] = []
    warnings: list[str] = []

    for package_id in sorted(workspace_ids, key=lambda pid: package_by_id[pid]["name"]):
        package = package_by_id[package_id]
        name = package["name"]
        if not is_type_crate(name):
            continue

        for dep in package.get("dependencies", []):
            dep_name = dep["name"]
            if dep_name not in workspace_names:
                continue
            if dep_name in ALLOWED_INTERNAL_TYPE_DEPS:
                continue
            if is_type_crate(dep_name):
                continue
            if dep_name in FORBIDDEN_INTERNAL_DEPS:
                errors.append(f"{name} must not depend on runtime/internal crate {dep_name}")
            else:
                warnings.append(
                    f"{name} depends on internal non-type crate {dep_name}; review boundary policy"
                )

    errors.extend(desktop_dependency_errors(metadata))

    for warning in warnings:
        print(f"warning: {warning}", file=sys.stderr)
    for error in errors:
        print(f"error: {error}", file=sys.stderr)

    if errors:
        print(f"dependency boundary check failed: {len(errors)} error(s)", file=sys.stderr)
        return 1

    print("dependency boundary check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
