#!/usr/bin/env python3
"""Idempotent mcp.json merge for Jcode Lite / Jcode Lite Free install + update.

Reads the bundled `preset/mcp/manifest.json` (or a passed manifest path) and
the recipient's `mcp.json` (or creates one if absent), then merges in the
bundled MCP server entries WITHOUT:

  * overwriting any server the user has already configured,
  * embedding any API keys (only bundled template entries are written),
  * clobbering user-added env values for bundled servers,
  * removing user-added servers.

The merged entry for a bundled stdio server uses the canonical Jcode MCP
schema fields only (`command`, `args`, `env`, `shared`, `type`). Optional
env keys that the package advertises as `free_tier_key_supported` are
seeded with `${VAR:-}` placeholders so the JSON round-trips through Jcode's
`McpServerConfig` (whose `env` is `HashMap<String, String>` — JSON `null`
fails serde parsing). Jcode's MCP loader expands `${VAR}` / `${VAR:-default}`
at run time before spawning the child process.

Browser-backed servers (`playwright`) are stateful and must NOT be shared
across sessions; all other bundled servers stay `shared: true`.

Merged result preserves JSON formatting and field order. Run during install,
update, and rollback so the user's MCP surface survives a package upgrade.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path

# This file lives at common/preset/mcp/bin/merge-mcp.py, so `parents[3]` is
# the package's `common/` directory which owns `preset/`.
ROOT = Path(__file__).resolve().parents[3]
MANIFEST_DEFAULT = ROOT / "preset" / "mcp" / "manifest.json"
BUNDLED_CONFIG_DEFAULT = ROOT / "preset" / "config.toml"

# Server names whose browsers / browser-tabs are stateful. They must not be
# shared across Jcode sessions or the Chromium profile gets corrupted.
NON_SHARED_SERVERS = {"playwright"}


def _load_json(path: Path, *, default):
    if not path.exists():
        return default
    text = path.read_text(encoding="utf-8")
    if not text.strip():
        return default
    return json.loads(text)


def _bundled_servers(manifest: dict) -> list[dict]:
    return manifest.get("servers", [])


def _shared_default(server_name: str) -> bool:
    """A server is shared unless its stateful browser is in flight."""
    return server_name not in NON_SHARED_SERVERS


def _build_bundled_entry(server: dict, manifest_root: Path) -> dict:
    """Return the MCP server entry the package template ships.

    Uses ONLY canonical Jcode McpServerConfig fields:
      - command (str)
      - args (list[str])
      - env (dict[str,str]) — values are ${VAR:-} placeholders or omitted
      - shared (bool)
      - type ("stdio")

    Optional env keys (free-tier optional user keys) are seeded as
    "${KEY:-}" placeholders so the JSON value is a String (not null), which
    matches `HashMap<String, String>` in `McpServerConfig`. The expansion to
    the actual environment value happens at MCP load time.
    """
    entry: dict = {}
    entry["command"] = "node"
    wrapper_rel = server.get("wrapper", f"bin/{server['name']}")
    # `manifest_root` is the directory holding manifest.json. Wrappers ship
    # alongside the manifest at `<install>/preset/mcp/bin/<name>`, so the
    # wrapper path is resolved relative to `manifest_root` (NOT
    # `manifest_root.parent`, which would jump to a non-existent
    # `<install>/preset/bin/` and silently break every MCP launch).
    wrapper_path = manifest_root / wrapper_rel
    entry["args"] = [str(wrapper_path)]
    entry["type"] = "stdio"
    entry["shared"] = _shared_default(server["name"])

    env_section: dict[str, str] = {}
    for opt_key in server.get("env_keys_optional", []) or []:
        env_section[opt_key] = "${%s:-}" % opt_key
    for req_key in (
        server.get("auth_modes", {})
        .get("authenticated", {})
        .get("env_keys_required", [])
        or []
    ):
        if req_key not in env_section:
            env_section[req_key] = "${%s:-}" % req_key
    if env_section:
        entry["env"] = env_section
    return entry


def _merge_into_user(user_path: Path, manifest: dict, *, dry_run: bool) -> dict:
    user_data = _load_json(user_path, default={"servers": {}})
    if "servers" not in user_data or not isinstance(user_data["servers"], dict):
        user_data["servers"] = {}
    manifest_root = Path(manifest.get("__source__", str(ROOT)))
    added, preserved = [], []
    for server in _bundled_servers(manifest):
        name = server["name"]
        bundled_entry = _build_bundled_entry(server, manifest_root)
        if name in user_data["servers"]:
            preserved.append(name)
            continue
        user_data["servers"][name] = bundled_entry
        added.append(name)
    if not dry_run:
        user_path.parent.mkdir(parents=True, exist_ok=True)
        user_path.write_text(
            json.dumps(user_data, indent=2, sort_keys=False) + "\n",
            encoding="utf-8",
        )
    return {"added": added, "preserved": preserved, "path": str(user_path)}


# Section-level merge (below) only ever ADDS a [section] the user's config is
# missing; it cannot touch a key inside a section the user already has,
# because it has no way to tell "this is still an old bundled default" apart
# from "the user deliberately set this". That is safe for user edits but
# means a genuine default change silently never reaches anyone who installed
# before the change.
#
# This module is shared by every Jcode Lite edition, including Free, whose
# packaged archive must never contain a Lite-private model route string (its
# own build verifier enforces this). So the specific old-value strings are
# NOT hardcoded here: each edition's own install.command passes its own
# --migrate SECTION.KEY=OLD_VALUE flags. Free's install.command passes none,
# so this mechanism is a no-op for Free.
#
# Each rule migrates ONE key forward, and ONLY when the recipient's current
# value exactly matches an OLD_VALUE the caller named. The new value is
# always read from the bundled config actually passed in, never hardcoded, so
# a rule can't fire past what the running release ships (e.g. if a future
# release reverts a key, this can't force a newer value back on top of it). A
# value that does not match any named old value was set by the user (by hand,
# by /model, or by a release with no migration rule) and is left untouched.


def _parse_migrate_flag(raw: str) -> dict:
    """Parse one --migrate SECTION.KEY=OLD_VALUE argument."""
    try:
        path, old_value = raw.split("=", 1)
        section, key = path.split(".", 1)
    except ValueError:
        raise argparse.ArgumentTypeError(
            f"--migrate must be SECTION.KEY=OLD_VALUE, got: {raw!r}"
        )
    return {"section": section, "key": key, "old_values": [old_value]}


def _section_span(text: str, section: str) -> tuple[int, int] | None:
    """Return (start, end) of `[section]`'s body in `text`, or None."""
    match = re.search(rf"^\[{re.escape(section)}\]\s*$", text, re.MULTILINE)
    if not match:
        return None
    start = match.end()
    next_section = re.search(r"^\[", text[start:], re.MULTILINE)
    end = start + next_section.start() if next_section else len(text)
    return start, end


def _migrate_known_defaults(
    text: str, bundled_text: str, rules: list[dict]
) -> tuple[str, list[str]]:
    """Advance recognized stale default values forward in place.

    `rules` comes from the caller (see `--migrate`), never from a constant in
    this shared module: this file also ships inside Free's archive, and
    Free's build verifier rejects any Lite-private model route string
    appearing anywhere in that archive.

    The new value for each migrated key is read from `bundled_text` (the
    config this release actually ships), never hardcoded, so a rule can only
    ever move a key toward what THIS release's bundled default is.

    Returns the (possibly modified) text and the list of "section.key"
    entries that were migrated, for the install/update report.
    """
    migrated: list[str] = []
    for rule in rules:
        bundled_span = _section_span(bundled_text, rule["section"])
        if bundled_span is None:
            continue
        b_start, b_end = bundled_span
        bundled_key_match = re.search(
            rf'^{re.escape(rule["key"])}\s*=\s*"([^"]*)"',
            bundled_text[b_start:b_end],
            re.MULTILINE,
        )
        if not bundled_key_match:
            continue
        new_value = bundled_key_match.group(1)

        span = _section_span(text, rule["section"])
        if span is None:
            continue
        start, end = span
        body = text[start:end]
        key_pattern = re.compile(
            rf'^({re.escape(rule["key"])}\s*=\s*")([^"]*)(")', re.MULTILINE
        )
        key_match = key_pattern.search(body)
        if not key_match or key_match.group(2) not in rule["old_values"]:
            continue
        if key_match.group(2) == new_value:
            continue
        new_body = (
            body[: key_match.start()]
            + key_match.group(1)
            + new_value
            + key_match.group(3)
            + body[key_match.end() :]
        )
        text = text[:start] + new_body + text[end:]
        migrated.append(f"{rule['section']}.{rule['key']}")
    return text, migrated


def _merge_config_toml(
    user_path: Path, bundled: Path, *, dry_run: bool, migrate_rules: list[dict] | None = None
) -> dict:
    """Append any new top-level sections from bundled without touching existing keys."""
    if not bundled.exists() or not user_path.exists():
        return {"appended": [], "path": str(user_path)}
    existing = user_path.read_text(encoding="utf-8")
    bundled_text = bundled.read_text(encoding="utf-8")
    existing_keys = set(re.findall(r"^\[([^\]]+)\]", existing, re.MULTILINE))
    bundled_keys = set(re.findall(r"^\[([^\]]+)\]", bundled_text, re.MULTILINE))
    appended = sorted(bundled_keys - existing_keys)
    migrated_text, migrated = _migrate_known_defaults(
        existing, bundled_text, migrate_rules or []
    )
    if migrated and not dry_run:
        existing = migrated_text
    elif migrated:
        # dry-run: report what would change without writing it.
        pass
    if appended and not dry_run:
        with user_path.open("a", encoding="utf-8") as fh:
            for section in appended:
                # Copy the section verbatim from the bundled config.
                start = bundled_text.index(f"[{section}]")
                end = bundled_text.find("\n[", start + 1)
                snippet = bundled_text[start:] if end == -1 else bundled_text[start:end]
                fh.write("\n" + snippet.rstrip() + "\n")
    if migrated and not dry_run:
        user_path.write_text(existing, encoding="utf-8")
    return {"appended": appended, "migrated": migrated, "path": str(user_path)}


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--user-mcp", type=Path, default=None,
                        help="Path to recipient mcp.json (default: $JCODE_HOME/mcp.json)")
    parser.add_argument("--user-config", type=Path, default=None,
                        help="Path to recipient config.toml (default: $JCODE_HOME/config.toml)")
    parser.add_argument("--bundled-config", type=Path, default=None,
                        help="Path to bundled config.toml to merge from (default: <root>/preset/config.toml)")
    parser.add_argument("--manifest", type=Path, default=MANIFEST_DEFAULT,
                        help="Path to bundled MCP manifest")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument(
        "--migrate", action="append", default=[], type=_parse_migrate_flag,
        metavar="SECTION.KEY=OLD_VALUE",
        help=(
            "Advance a stale bundled default forward on update. Repeatable. "
            "Only fires when the recipient's current value for SECTION.KEY "
            "exactly equals OLD_VALUE; the new value is read from the "
            "bundled config passed via --bundled-config. Edition-specific: "
            "each install.command supplies its own flags so this shared "
            "script carries no edition-specific strings."
        ),
    )
    args = parser.parse_args(argv)

    home = Path(os.environ.get("JCODE_HOME", str(Path.home() / ".jcode")))
    user_mcp = args.user_mcp or home / "mcp.json"
    user_config = args.user_config or home / "config.toml"
    bundled_config = args.bundled_config or BUNDLED_CONFIG_DEFAULT

    manifest = _load_json(args.manifest, default={"servers": []})
    manifest["__source__"] = str(args.manifest.resolve().parent)

    mcp_summary = _merge_into_user(user_mcp, manifest, dry_run=args.dry_run)
    config_summary = _merge_config_toml(
        user_config, bundled_config, dry_run=args.dry_run, migrate_rules=args.migrate
    )

    print(json.dumps(
        {"mcp.json": mcp_summary, "config.toml": config_summary},
        indent=2,
    ))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
