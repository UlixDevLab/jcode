#!/usr/bin/env node

import fs from "node:fs";
import crypto from "node:crypto";

function fail(message) {
  console.error(`Invalid update manifest: ${message}`);
  process.exit(1);
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function parseArgs(argv) {
  if (argv.length < 2) fail("usage: verify-update-manifest.mjs MANIFEST TRUST [options]");
  const options = { manifest: argv[0], trust: argv[1], allowDowngrade: false };
  for (let index = 2; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--allow-downgrade") {
      options.allowDowngrade = true;
      continue;
    }
    const value = argv[index + 1];
    if (!value) fail(`${arg} requires a value`);
    if (arg === "--product") options.product = value;
    else if (arg === "--platform") options.platform = value;
    else if (arg === "--channel") options.channel = value;
    else if (arg === "--current-version") options.currentVersion = value;
    else fail(`unknown option ${arg}`);
    index += 1;
  }
  return options;
}

function parseVersion(version) {
  const match = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(version);
  if (!match) fail(`version is not semantic: ${version}`);
  return {
    core: match.slice(1, 4).map(Number),
    prerelease: match[4] ? match[4].split(".") : [],
  };
}

function compareIdentifiers(left, right) {
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
  if (a.prerelease.length === 0 && b.prerelease.length === 0) return 0;
  if (a.prerelease.length === 0) return 1;
  if (b.prerelease.length === 0) return -1;
  const count = Math.max(a.prerelease.length, b.prerelease.length);
  for (let index = 0; index < count; index += 1) {
    if (a.prerelease[index] === undefined) return -1;
    if (b.prerelease[index] === undefined) return 1;
    const comparison = compareIdentifiers(a.prerelease[index], b.prerelease[index]);
    if (comparison !== 0) return comparison;
  }
  return 0;
}

const options = parseArgs(process.argv.slice(2));
let manifest;
let trust;
try {
  manifest = JSON.parse(fs.readFileSync(options.manifest, "utf8").replace(/^\uFEFF/, ""));
  trust = JSON.parse(fs.readFileSync(options.trust, "utf8").replace(/^\uFEFF/, ""));
} catch (error) {
  fail(error.message);
}

if (manifest.schema_version !== "jcode-lite-update/v1") fail("unsupported schema_version");
for (const field of ["product", "platform", "channel", "version", "runtime_version", "url", "sha256", "size_bytes", "compatibility", "signature"]) {
  if (manifest[field] === undefined || manifest[field] === null) fail(`missing ${field}`);
}
if (options.product && manifest.product !== options.product) fail(`product mismatch: ${manifest.product}`);
if (options.platform && manifest.platform !== options.platform) fail(`platform mismatch: ${manifest.platform}`);
if (options.channel && manifest.channel !== options.channel) fail(`channel mismatch: ${manifest.channel}`);
if (!/^https:\/\//.test(manifest.url)) fail("url must use https");
if (!/^[a-f0-9]{64}$/.test(manifest.sha256)) fail("sha256 must be lowercase hexadecimal");
if (!Number.isSafeInteger(manifest.size_bytes) || manifest.size_bytes <= 0) fail("size_bytes must be positive");
if (manifest.compatibility.minimum_updater_schema > 1) fail("this updater is too old for the release");
const nodeMajor = Number(process.versions.node.split(".")[0]);
if (nodeMajor < manifest.compatibility.minimum_node_major) fail(`Node.js ${manifest.compatibility.minimum_node_major}+ is required`);
if (options.currentVersion && !options.allowDowngrade && compareVersions(manifest.version, options.currentVersion) < 0) {
  fail(`downgrade from ${options.currentVersion} to ${manifest.version} is not allowed`);
}

const signature = manifest.signature;
if (signature.algorithm !== "ed25519") fail(`unsupported signature algorithm ${signature.algorithm}`);
const key = trust.keys?.find((candidate) => candidate.key_id === signature.key_id && candidate.algorithm === signature.algorithm);
if (!key) fail(`untrusted key ${signature.key_id}`);
const payload = { ...manifest };
delete payload.signature;
let verified = false;
try {
  verified = crypto.verify(null, Buffer.from(canonical(payload)), key.public_key_pem, Buffer.from(signature.value, "base64"));
} catch (error) {
  fail(`signature check failed: ${error.message}`);
}
if (!verified) fail("signature does not match the manifest payload");
console.log(`Verified ${manifest.product} ${manifest.version} (${manifest.channel}/${manifest.platform}) with ${signature.key_id}.`);
