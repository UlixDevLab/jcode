#!/usr/bin/env python3
"""Validate an approved UI fidelity ledger against a captured UI observation.

This is intentionally invoked by ``desktop2_visual_check.sh`` rather than a
fixture-only test. The visual-check entrypoint is what a verifier uses to turn
an approval record and a named rendered observation into a PASS or BLOCK.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any


ID_PREFIXES = {
    "elements": "E",
    "states": "S",
    "interactions": "I",
    "motion": "M",
}
ESCAPE_HATCH_KINDS = {"warn-only-lint", "review-band-drift", "migrate-budget"}
HASH = re.compile(r"(?:sha256:)?[0-9a-f]{64}\Z")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--approval", type=Path, required=True)
    parser.add_argument("--observed", type=Path, required=True)
    parser.add_argument("--surface", choices=("browser", "desktop"), required=True)
    return parser.parse_args()


def load_json(path: Path, label: str) -> tuple[dict[str, Any] | None, list[str]]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return None, [f"{label} could not be read: {error}"]
    if not isinstance(value, dict):
        return None, [f"{label} must be a JSON object"]
    return value, []


def metadata(artifact: Any, escape_hatches: Any) -> str:
    if not isinstance(artifact, dict):
        return "artifact=<invalid> escape_hatches=<invalid>"
    kind = artifact.get("kind", "<missing>")
    digest = artifact.get("hash", "<missing>")
    if not isinstance(escape_hatches, list):
        return f"artifact.kind={kind} artifact.hash={digest} escape_hatches=<invalid>"
    kinds = [hatch.get("kind", "<missing>") if isinstance(hatch, dict) else "<invalid>" for hatch in escape_hatches]
    return f"artifact.kind={kind} artifact.hash={digest} escape_hatches={','.join(kinds) or 'none'}"


def approved_ids(approval: dict[str, Any], errors: list[str]) -> tuple[set[str], Any, Any]:
    artifact = approval.get("artifact")
    escape_hatches = approval.get("escape_hatches")
    if not isinstance(artifact, dict):
        errors.append("approval artifact must be an object")
    else:
        kind = artifact.get("kind")
        digest = artifact.get("hash")
        if not isinstance(kind, str) or not kind:
            errors.append("approval artifact.kind must be a non-empty string")
        if not isinstance(digest, str) or not HASH.fullmatch(digest):
            errors.append("approval artifact.hash must be a SHA-256 digest")

    if not isinstance(escape_hatches, list):
        errors.append("approval escape_hatches must be an array")
    else:
        for index, hatch in enumerate(escape_hatches):
            if not isinstance(hatch, dict):
                errors.append(f"escape_hatches[{index}] must be an object")
                continue
            kind = hatch.get("kind")
            reason = hatch.get("reason")
            if kind not in ESCAPE_HATCH_KINDS:
                errors.append(f"escape_hatches[{index}].kind is not an approved escape hatch")
            if not isinstance(reason, str) or not reason.strip():
                errors.append(f"escape_hatches[{index}].reason must be non-empty")

    ledger = approval.get("fidelity_ledger")
    if not isinstance(ledger, dict):
        errors.append("approval fidelity_ledger must be an object")
        return set(), artifact, escape_hatches

    ids: set[str] = set()
    for category, prefix in ID_PREFIXES.items():
        entries = ledger.get(category)
        if not isinstance(entries, list):
            errors.append(f"fidelity_ledger.{category} must be an array")
            continue
        for index, entry in enumerate(entries):
            if not isinstance(entry, dict):
                errors.append(f"fidelity_ledger.{category}[{index}] must be an object")
                continue
            identifier = entry.get("id")
            name = entry.get("name")
            if not isinstance(identifier, str) or not re.fullmatch(rf"{prefix}[1-9][0-9]*", identifier):
                errors.append(f"fidelity_ledger.{category}[{index}].id must use {prefix}<number>")
                continue
            if identifier in ids:
                errors.append(f"approved id {identifier} is duplicated")
            ids.add(identifier)
            if not isinstance(name, str) or not name.strip():
                errors.append(f"fidelity_ledger.{category}[{index}].name must be non-empty")
    return ids, artifact, escape_hatches


def observed_ids(observed: dict[str, Any], approved: set[str], errors: list[str]) -> tuple[set[str], set[str]]:
    present = observed.get("present")
    deviations = observed.get("deviations")
    if not isinstance(present, list) or not all(isinstance(identifier, str) for identifier in present):
        errors.append("observed present must be an array of ids")
        present_ids: set[str] = set()
    else:
        present_ids = set(present)
        unknown = sorted(present_ids - approved)
        if unknown:
            errors.append(f"observed present names unapproved ids: {', '.join(unknown)}")

    deviation_ids: set[str] = set()
    if not isinstance(deviations, list):
        errors.append("observed deviations must be an array")
    else:
        for index, deviation in enumerate(deviations):
            if not isinstance(deviation, dict):
                errors.append(f"observed deviations[{index}] must be an object")
                continue
            identifier = deviation.get("id")
            reason = deviation.get("reason")
            if not isinstance(identifier, str) or identifier not in approved:
                errors.append(f"observed deviations[{index}].id must name an approved id")
                continue
            if not isinstance(reason, str) or not reason.strip():
                errors.append(f"observed deviations[{index}].reason must be non-empty")
            if identifier in deviation_ids:
                errors.append(f"observed deviation {identifier} is duplicated")
            deviation_ids.add(identifier)

    both = sorted(present_ids & deviation_ids)
    if both:
        errors.append(f"observed ids cannot be both present and deviations: {', '.join(both)}")
    return present_ids, deviation_ids


def main() -> int:
    args = parse_args()
    approval, errors = load_json(args.approval, "approval")
    observed, observed_errors = load_json(args.observed, "observed")
    errors.extend(observed_errors)
    if approval is None or observed is None:
        print("BLOCK " + "; ".join(errors))
        return 1

    approved, artifact, escape_hatches = approved_ids(approval, errors)
    present, deviations = observed_ids(observed, approved, errors)
    if args.surface == "desktop" and isinstance(artifact, dict) and artifact.get("kind") == "browser-mock":
        errors.append("desktop surface cannot PASS against a browser-mock artifact")
    missing = sorted(approved - present - deviations)
    if missing:
        errors.append(f"approved ids neither present nor declared deviations: {', '.join(missing)}")

    consumed_metadata = metadata(artifact, escape_hatches)
    if errors:
        print(f"BLOCK {consumed_metadata}")
        for error in errors:
            print(f"  {error}")
        return 1
    print(f"PASS {consumed_metadata}")
    print(f"  approved={len(approved)} present={len(present)} deviations={len(deviations)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
