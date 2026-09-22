#!/usr/bin/env node

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) throw new Error(`invalid argument ${key ?? ""}`);
    options[key.slice(2)] = value;
  }
  for (const key of [
    "manifest-url",
    "trust",
    "verifier",
    "product",
    "platform",
    "channel",
    "current-version",
    "state",
    "name",
    "update-command",
  ]) {
    if (!options[key]) throw new Error(`--${key} is required`);
  }
  return options;
}

function parseVersion(version) {
  const match = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(version);
  if (!match) throw new Error(`version is not semantic: ${version}`);
  return { core: match.slice(1, 4).map(Number), prerelease: match[4]?.split(".") ?? [] };
}

function compareIdentifier(left, right) {
  const leftNumber = /^\d+$/.test(left) ? Number(left) : null;
  const rightNumber = /^\d+$/.test(right) ? Number(right) : null;
  if (leftNumber !== null && rightNumber !== null) return Math.sign(leftNumber - rightNumber);
  if (leftNumber !== null) return -1;
  if (rightNumber !== null) return 1;
  return left.localeCompare(right);
}

function compareVersions(left, right) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  for (let index = 0; index < 3; index += 1) {
    if (a.core[index] !== b.core[index]) return Math.sign(a.core[index] - b.core[index]);
  }
  if (a.prerelease.length === 0 || b.prerelease.length === 0) {
    return a.prerelease.length === b.prerelease.length ? 0 : a.prerelease.length === 0 ? 1 : -1;
  }
  for (let index = 0; index < Math.max(a.prerelease.length, b.prerelease.length); index += 1) {
    if (a.prerelease[index] === undefined) return -1;
    if (b.prerelease[index] === undefined) return 1;
    const comparison = compareIdentifier(a.prerelease[index], b.prerelease[index]);
    if (comparison !== 0) return comparison;
  }
  return 0;
}

function readState(statePath) {
  try {
    return JSON.parse(fs.readFileSync(statePath, "utf8"));
  } catch {
    return null;
  }
}

function writeState(statePath, state) {
  fs.mkdirSync(path.dirname(statePath), { recursive: true, mode: 0o700 });
  const temporary = `${statePath}.new.${process.pid}`;
  fs.writeFileSync(temporary, `${JSON.stringify(state)}\n`, { mode: 0o600 });
  fs.renameSync(temporary, statePath);
}

function showAvailable(options, version) {
  console.error(`${options.name} ${version} is available. Update with: ${options["update-command"]}`);
}

let activeOptions;

async function main() {
  const options = parseArgs(process.argv.slice(2));
  activeOptions = options;
  const intervalHours = Number(options["interval-hours"] ?? "24");
  if (!Number.isFinite(intervalHours) || intervalHours < 0) throw new Error("--interval-hours must be non-negative");
  const now = Date.now();
  const cached = readState(options.state);
  if (cached?.checked_at && now - cached.checked_at < intervalHours * 60 * 60 * 1000) {
    if (cached.available && cached.latest_version) showAvailable(options, cached.latest_version);
    return;
  }

  const response = await fetch(options["manifest-url"], { signal: AbortSignal.timeout(4000) });
  if (!response.ok) throw new Error(`release metadata returned HTTP ${response.status}`);
  const manifestText = await response.text();
  const temporaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "jcode-lite-update-check."));
  const manifestPath = path.join(temporaryDirectory, "manifest.json");
  try {
    fs.writeFileSync(manifestPath, manifestText, { mode: 0o600 });
    const verification = spawnSync(
      process.execPath,
      [
        options.verifier,
        manifestPath,
        options.trust,
        "--product",
        options.product,
        "--platform",
        options.platform,
        "--channel",
        options.channel,
        "--current-version",
        options["current-version"],
      ],
      { encoding: "utf8" },
    );
    if (verification.status !== 0) throw new Error("release metadata signature or policy check failed");
    const manifest = JSON.parse(manifestText.replace(/^\uFEFF/, ""));
    const available = compareVersions(manifest.version, options["current-version"]) > 0;
    writeState(options.state, { checked_at: now, latest_version: manifest.version, available });
    if (available) showAvailable(options, manifest.version);
  } finally {
    fs.rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}

main().catch((error) => {
  if (activeOptions?.state) {
    try {
      writeState(activeOptions.state, { checked_at: Date.now(), available: false });
    } catch {
      // A read-only or unavailable state directory must not block Jcode startup.
    }
  }
  if (process.env.JCODE_LITE_UPDATE_CHECK_DEBUG === "1") {
    console.error(`Automatic update check deferred: ${error.message}`);
  }
});
