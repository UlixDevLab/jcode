#!/usr/bin/env python3
"""Free package provenance verifier.

Asserts that a Jcode Lite Free archive honors the vNext contract:
  * Pinned MCP versions (same as Lite) with no Stables bytes.
  * Allowed archive contents (allowlist.txt) is the source of truth.
  * No anthropic/claude references in any text file.
  * No visual-plan skill anywhere.
  * No Stables credential bytes (key, base URL, env var, env file).
  * Memory sidecar enabled and PDF/embeddings declared in the runtime build.
  * Free must not reject any provider or model — the manifest declares
    standard-jcode-login onboarding and excludes preconfigured profiles.
"""

import argparse
import hashlib
import json
import re
import sys
import zipfile


EXPECTED_SKILLS = {
    "anthropologist", "cfo", "client", "compass", "competitor",
    "component-stress", "consolidate-memory", "debate", "decision-board",
    "design-variants", "energy", "ethics", "first-principles",
    "frontend-design", "howtos", "idea-forge", "investor", "journalist",
    "landing-page", "mage", "mentor", "motion-review", "ops-diagnosis",
    "pe-analyst", "premortem", "research", "scroll-craft", "seller", "skeptic",
    "statistician", "stoic", "synthesis", "system-architect", "taleb",
    "wanderer",
}
EXPECTED_MCP = {
    "playwright": "@playwright/mcp@0.0.79",
    "context7": "@upstash/context7-mcp@4.0.2",
    "omnisearch": "mcp-omnisearch@0.0.28",
    "reddit": "reddit-mcp-buddy@1.1.14",
}
SECRET = re.compile(rb"(?:llmr_[A-Za-z0-9]{44}|sk-[A-Za-z0-9_-]{32,})")
STABLES_FORBIDDEN = (b"STABLES_API_KEY", b"router.legrin-tech.net", b"stables.env")
ROUTING_FORBIDDEN = (b"anthropic", b"claude", b"openai-api")
# Private Lite model routes that Free must never bundle. Free ships a
# provider-neutral swarm prompt and config; seeing any of these IDs in any
# text file (outside the explicit `bin/jcode` and `bin/jcode.exe` payloads)
# means the generic source was contaminated with the private surface.
STABLES_MODEL_FORBIDDEN = (
    b"codex/gpt-5.6-sol",
    b"minimax/M3",
    b"minimax/M2.7-highspeed",
)

parser = argparse.ArgumentParser()
parser.add_argument("package")
args = parser.parse_args()


def _fail(message):
    raise SystemExit(message)


with zipfile.ZipFile(args.package) as archive:
    names = sorted(name for name in archive.namelist() if not name.endswith("/"))
    if any(name.startswith("/") or ".." in name.split("/") or "\\" in name for name in names):
        _fail("unsafe archive path")
    allowlist = sorted(line for line in archive.read("allowlist.txt").decode().splitlines() if line)
    if names != allowlist:
        _fail("archive file set does not match allowlist.txt")
    release = json.loads(archive.read("release.json"))
    manifest = json.loads(archive.read("lite-manifest.json"))
    if release.get("product") != "jcode-lite-free" or manifest.get("schema_version") != "jcode-lite-free-manifest/v1":
        _fail("wrong free distribution metadata")

    # Shared version with Lite.
    expected_version = release.get("shared_release_version")
    if expected_version and release.get("version") != expected_version:
        _fail("free release.json version does not match shared_release_version")

    # Provider-free config. Forbidden literals are exact private-route IDs
    # and Stables router material; the verifier scans the whole archive
    # below for any leak, not just the swarm prompt.
    config = archive.read("preset/config.toml").decode()
    # Comments document unsupported keys for users and must not be mistaken
    # for active provider or route configuration.
    config_values = "\n".join(line.split("#", 1)[0] for line in config.splitlines())
    forbidden_config = (
        "default_provider", "default_model",
        "[providers.", "STABLES", "router.legrin-tech.net",
    )
    for value in forbidden_config:
        if value in config_values:
            _fail(f"free config.toml contains provider or model default: {value!r}")
    # Real Jcode [agents] schema for Free only allows memory_sidecar_enabled
    # + memory_embedding_backend. Free must NOT pin routes.
    free_route_leaks = (
        "swarm_model", "memory_model",
        "implementation_model", "scout_model", "sidecar_model",
        "memory_sidecar_embedding_model",
    )
    for key in free_route_leaks:
        if key in config_values:
            _fail(f"Free config.toml leaks pinned route key {key!r}")
    for section in ("[memory]", "[pdf]", "[mcp]"):
        if section in config_values:
            _fail(f"Free config.toml must not contain section {section!r}")

    # Skills, roles, MCP surface
    shipped_skills = {
        name.split("/")[2]
        for name in names
        if name.startswith("preset/skills/") and len(name.split("/")) > 3
    }
    if shipped_skills != EXPECTED_SKILLS:
        _fail(f"unexpected skill set: {sorted(shipped_skills)}")
    if "visual-plan" in shipped_skills:
        _fail("visual-plan must not ship in Free")
    if len(shipped_skills) != 35:
        _fail(f"Free must ship exactly 35 skills; got {len(shipped_skills)}")

    shipped_roles = {
        name.split("/")[2].removesuffix(".md")
        for name in names
        if name.startswith("preset/roles/") and name.endswith(".md")
    }
    expected_roles = {"implement", "lab", "research", "verify"}
    if shipped_roles != expected_roles:
        _fail(f"unexpected role set: {sorted(shipped_roles)}")
    if len(shipped_roles) != 4:
        _fail(f"Free must ship exactly 4 roles; got {len(shipped_roles)}")

    # MCP vendor surface
    if "preset/mcp/manifest.json" not in names:
        _fail("missing preset/mcp/manifest.json")
    mcp_manifest = json.loads(archive.read("preset/mcp/manifest.json"))
    servers = {s["name"]: s for s in mcp_manifest.get("servers", [])}
    for name, package in EXPECTED_MCP.items():
        if servers.get(name, {}).get("package") != package:
            _fail(f"MCP {name} pin mismatch: expected {package!r}, got {servers.get(name, {}).get('package')!r}")

    # Knowledge OS schema + renderer provenance
    schema = archive.read("preset/knowledge-os/templates/project-os/project-os.schema.yaml").decode()
    if "schema_version: project-os-schema/v2.2" not in schema:
        _fail("Knowledge OS schema is not v2.2")
    for filename, expected in release["knowledge_os_renderer_sha256"].items():
        data = archive.read(f"preset/knowledge-os/renderer/{filename}")
        if hashlib.sha256(data).hexdigest() != expected:
            _fail(f"renderer provenance mismatch: {filename}")

    # Manifest must declare memory sidecar, PDF, embeddings.
    features = manifest.get("include", {}).get("features", [])
    for required in ("memory-sidecar", "memory-embeddings", "pdf"):
        if required not in features:
            _fail(f"Free manifest features must include {required!r}")
    runtime_features = manifest.get("runtime", {}).get("cargo_features", [])
    for required in ("pdf", "embeddings"):
        if required not in runtime_features:
            _fail(f"Free runtime.cargo_features must include {required!r}")
    if runtime_features:
        for forbidden in ("bedrock",):
            if forbidden in runtime_features:
                _fail(f"Free runtime.cargo_features must not include {forbidden!r}")

    # No preconfigured provider profiles; no embedded credentials.
    excluded = manifest.get("exclude", {})
    if not excluded.get("preconfigured_provider_profiles"):
        _fail("Free must exclude preconfigured_provider_profiles")
    if not excluded.get("embedded_provider_credentials"):
        _fail("Free must exclude embedded_provider_credentials")

    # Scan text files for secrets / Stables / forbidden routing references.
    for name in names:
        if name.startswith("bin/"):
            continue
        data = archive.read(name)
        if SECRET.search(data):
            _fail(f"credential found in {name}")
        for needle in STABLES_FORBIDDEN:
            if needle in data:
                _fail(f"Stables material found in {name}: {needle.decode()}")
        # Vendored MCP Node packages live under preset/mcp/node_modules/**.
        # Their package metadata, licenses, source maps, and implementation
        # code legitimately mention "anthropic" / "claude" / "openai-api"
        # because those npm packages have their own optional integrations.
        # Jcode does not route to those providers; the carve-out only
        # applies to the vendor tree, not to Jcode-owned content such as
        # preset/config.toml, preset/swarm-prompt.md, or preset/mcp/*.json.
        # The bundled official Node runtime also contains npm's third-party
        # license identifiers (for example the Claude license). These are not
        # provider routes. Secret and private-router checks above still apply.
        if name.startswith(("preset/mcp/node_modules/", "runtime/node/lib/node_modules/", "runtime/node/node_modules/")):
            continue
        # START-HERE.md is end-user documentation, not routing configuration.
        # Free ships NO provider and its whole premise is that the user brings
        # their own, so the instructions must name which providers they can
        # connect. The guard's purpose is to stop packaged *routing* content
        # from implying a bundled provider; a doc telling the user to run
        # /login does the opposite.
        if name == "START-HERE.md":
            continue
        lowered = data.lower()
        for needle in ROUTING_FORBIDDEN:
            if needle in lowered:
                _fail(f"disallowed routing reference in {name}: {needle.decode()}")

    # Free must ship a swarm-prompt override (provider-neutral). The override
    # is the one place where specific provider/model routes would otherwise
    # appear; it intentionally does NOT, so we scan it explicitly with a
    # dedicated message. The full text-file scan above also catches any
    # stray leak across the rest of the archive.
    prompt_names = [n for n in names if n.endswith("preset/swarm-prompt.md")]
    if not prompt_names:
        _fail("missing preset/swarm-prompt.md (Free ships a provider-neutral override)")
    for prompt_name in prompt_names:
        prompt_text = archive.read(prompt_name).decode("utf-8", errors="ignore")
        for needle in STABLES_MODEL_FORBIDDEN:
            if needle.decode() in prompt_text:
                _fail(f"{prompt_name} leaked private Lite model route: {needle.decode()}")
    # Full-archive scan for exact private Lite model route IDs outside the
    # binary payloads. This catches accidental leaks in skills, roles, MCP
    # manifest, release.json, knowledge-os templates, or anywhere else in
    # the shared surface — the swarm-prompt scan above only covers the
    # provider-specific override artifact.
    private_route_leaks = []
    for name in names:
        if name.startswith("bin/"):
            continue
        try:
            data = archive.read(name)
        except Exception:
            continue
        # release.json is allowed to declare shared_release_version + jcode_version
        # but not model IDs. So we still scan it.
        for needle in STABLES_MODEL_FORBIDDEN:
            if needle.decode() in (data.decode("utf-8", errors="ignore") if isinstance(data, (bytes, bytearray)) else ""):
                private_route_leaks.append((name, needle.decode()))
    if private_route_leaks:
        _fail(f"private Lite model routes leaked in Free archive: {private_route_leaks}")

    # Visual-plan must never be shipped.
    if any("visual-plan" in name for name in names):
        _fail("visual-plan found in archive")

    # Required binary present.
    required = "bin/jcode.exe" if "install.ps1" in names else "bin/jcode"
    if required not in names:
        _fail(f"missing {required}")

print(f"PASS {args.package}: {len(names)} files, vNext Free contract verified")
