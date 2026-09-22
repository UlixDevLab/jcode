#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_json(value: dict) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def signing_key_id(signing_key: Path) -> str:
    public_der = subprocess.run(
        ["openssl", "pkey", "-in", str(signing_key), "-pubout", "-outform", "DER"],
        check=True,
        capture_output=True,
    ).stdout
    return "ed25519-" + hashlib.sha256(public_der).hexdigest()[:16]


def sign_manifest(payload: dict, signing_key: Path) -> dict:
    with tempfile.NamedTemporaryFile() as canonical_file:
        canonical_file.write(canonical_json(payload))
        canonical_file.flush()
        signature = subprocess.run(
            [
                "openssl",
                "pkeyutl",
                "-sign",
                "-rawin",
                "-inkey",
                str(signing_key),
                "-in",
                canonical_file.name,
            ],
            check=True,
            capture_output=True,
        ).stdout
    return {
        "algorithm": "ed25519",
        "key_id": signing_key_id(signing_key),
        "value": base64.b64encode(signature).decode("ascii"),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--macos", type=Path, required=True)
    parser.add_argument("--windows", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--base-url", default="https://files.legrin-tech.net/jcode-lite-free")
    parser.add_argument(
        "--signing-key",
        type=Path,
        default=Path(os.environ["JCODE_LITE_MANIFEST_SIGNING_KEY"]) if os.environ.get("JCODE_LITE_MANIFEST_SIGNING_KEY") else None,
    )
    args = parser.parse_args()

    if args.signing_key is None or not args.signing_key.is_file():
        raise SystemExit("--signing-key or JCODE_LITE_MANIFEST_SIGNING_KEY must name an Ed25519 private key")

    root = Path(__file__).parents[1]
    release = json.loads((root / "release.json").read_text())
    output = args.output.resolve()
    if output.exists():
        shutil.rmtree(output)
    release_root = output / "jcode-lite-free"
    shutil.copytree(Path(__file__).parent / "jcode-lite-free", release_root)
    shutil.copytree(root.parent / "jcode-lite" / "site" / "jcode-lite-assets", output / "jcode-lite-assets")
    for platform, artifact in (("macos", args.macos), ("windows", args.windows)):
        artifact = artifact.resolve()
        platform_dir = release_root / platform
        platform_dir.mkdir(parents=True)
        versioned_name = f"jcode-lite-free-{platform}-{release['version']}.zip"
        versioned = platform_dir / versioned_name
        latest = platform_dir / f"jcode-lite-free-{platform}-latest.zip"
        shutil.copy2(artifact, versioned)
        shutil.copy2(artifact, latest)
        manifest = {
            "schema_version": "jcode-lite-update/v1",
            "product": "jcode-lite-free",
            "platform": platform,
            "channel": release["channel"],
            "version": release["version"],
            "runtime_version": release["jcode_version"],
            "url": f"{args.base_url}/{platform}/{versioned_name}",
            "sha256": sha256(versioned),
            "size_bytes": versioned.stat().st_size,
            "compatibility": {
                "minimum_node_major": release["minimum_node_major"],
                "minimum_updater_schema": 1,
            },
        }
        manifest["signature"] = sign_manifest(manifest, args.signing_key)
        (platform_dir / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(output)


if __name__ == "__main__":
    main()
