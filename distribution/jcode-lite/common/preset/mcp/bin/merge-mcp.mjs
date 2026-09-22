#!/usr/bin/env node
/**
 * Node-backed, idempotent MCP and config merge for the self-contained macOS
 * Lite package. It mirrors merge-mcp.py so the installer does not require the
 * macOS Command Line Tools or a separately installed Python runtime.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(scriptDir, "../../..");
const nonSharedServers = new Set(["playwright"]);

function usage() {
  console.error("Usage: merge-mcp.mjs [--user-mcp PATH] [--user-config PATH] [--bundled-config PATH] [--manifest PATH] [--dry-run] [--migrate SECTION.KEY=OLD_VALUE]");
}

function parseArgs(argv) {
  const args = { migrate: [], dryRun: false };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === "--dry-run") {
      args.dryRun = true;
    } else if (["--user-mcp", "--user-config", "--bundled-config", "--manifest", "--migrate"].includes(token)) {
      const value = argv[++index];
      if (!value) throw new Error(`${token} requires a value`);
      if (token === "--migrate") {
        const equal = value.indexOf("=");
        const dot = value.indexOf(".");
        if (equal <= 0 || dot <= 0 || dot > equal - 1) {
          throw new Error(`--migrate must be SECTION.KEY=OLD_VALUE, got: ${JSON.stringify(value)}`);
        }
        args.migrate.push({ section: value.slice(0, dot), key: value.slice(dot + 1, equal), oldValues: [value.slice(equal + 1)] });
      } else {
        args[token.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = value;
      }
    } else if (token === "--help" || token === "-h") {
      usage();
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${token}`);
    }
  }
  return args;
}

function loadJson(filePath, fallback) {
  if (!fs.existsSync(filePath)) return fallback;
  const text = fs.readFileSync(filePath, "utf8");
  return text.trim() ? JSON.parse(text) : fallback;
}

function bundledEntry(server, manifestRoot) {
  const entry = {
    command: "node",
    args: [path.resolve(manifestRoot, server.wrapper || `bin/${server.name}`)],
    type: "stdio",
    shared: !nonSharedServers.has(server.name),
  };
  const env = {};
  for (const key of server.env_keys_optional || []) env[key] = `\${${key}:-}`;
  for (const key of server.auth_modes?.authenticated?.env_keys_required || []) {
    if (!(key in env)) env[key] = `\${${key}:-}`;
  }
  if (Object.keys(env).length) entry.env = env;
  return entry;
}

function mergeMcp(userPath, manifest, dryRun) {
  const data = loadJson(userPath, { servers: {} });
  if (!data.servers || typeof data.servers !== "object" || Array.isArray(data.servers)) data.servers = {};
  const added = [];
  const preserved = [];
  const manifestRoot = path.dirname(manifest.__source);
  for (const server of manifest.servers || []) {
    if (Object.hasOwn(data.servers, server.name)) {
      preserved.push(server.name);
    } else {
      data.servers[server.name] = bundledEntry(server, manifestRoot);
      added.push(server.name);
    }
  }
  if (!dryRun) {
    fs.mkdirSync(path.dirname(userPath), { recursive: true });
    fs.writeFileSync(userPath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
  }
  return { added, preserved, path: userPath };
}

function sectionSpan(text, section) {
  const escaped = section.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`^\\[${escaped}\\]\\s*$`, "m").exec(text);
  if (!match) return null;
  const start = match.index + match[0].length;
  const next = /^\[/m.exec(text.slice(start));
  return [start, next ? start + next.index : text.length];
}

function bundledValue(text, section, key) {
  const span = sectionSpan(text, section);
  if (!span) return null;
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`^${escaped}\\s*=\\s*"([^"]*)"`, "m").exec(text.slice(...span));
  return match ? match[1] : null;
}

function migrateDefaults(text, bundledText, rules) {
  const migrated = [];
  let result = text;
  for (const rule of rules) {
    const nextValue = bundledValue(bundledText, rule.section, rule.key);
    const span = sectionSpan(result, rule.section);
    if (nextValue === null || !span) continue;
    const body = result.slice(...span);
    const escaped = rule.key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const keyPattern = new RegExp(`^(${escaped}\\s*=\\s*")([^"]*)(")`, "m");
    const match = keyPattern.exec(body);
    if (!match || !rule.oldValues.includes(match[2]) || match[2] === nextValue) continue;
    const updated = `${body.slice(0, match.index)}${match[1]}${nextValue}${match[3]}${body.slice(match.index + match[0].length)}`;
    result = `${result.slice(0, span[0])}${updated}${result.slice(span[1])}`;
    migrated.push(`${rule.section}.${rule.key}`);
  }
  return { text: result, migrated };
}

function sectionNames(text) {
  return new Set([...text.matchAll(/^\[([^\]]+)\]/gm)].map((match) => match[1]));
}

function sectionText(text, section) {
  const span = sectionSpan(text, section);
  if (!span) return "";
  const headerStart = text.lastIndexOf("[", span[0]);
  return text.slice(headerStart, span[1]).trimEnd();
}

function mergeConfig(userPath, bundledPath, dryRun, rules) {
  if (!fs.existsSync(bundledPath) || !fs.existsSync(userPath)) return { appended: [], path: userPath };
  const original = fs.readFileSync(userPath, "utf8");
  const bundled = fs.readFileSync(bundledPath, "utf8");
  const existingSections = sectionNames(original);
  const appended = [...sectionNames(bundled)].filter((section) => !existingSections.has(section)).sort();
  const migratedResult = migrateDefaults(original, bundled, rules);
  let merged = migratedResult.text;
  for (const section of appended) merged += `\n${sectionText(bundled, section)}\n`;
  if (!dryRun && (migratedResult.migrated.length || appended.length)) fs.writeFileSync(userPath, merged, "utf8");
  return { appended, migrated: migratedResult.migrated, path: userPath };
}

try {
  const args = parseArgs(process.argv.slice(2));
  const home = process.env.JCODE_HOME || path.join(os.homedir(), ".jcode");
  const manifestPath = path.resolve(args.manifest || path.join(root, "preset", "mcp", "manifest.json"));
  const userMcp = path.resolve(args.userMcp || path.join(home, "mcp.json"));
  const userConfig = path.resolve(args.userConfig || path.join(home, "config.toml"));
  const bundledConfig = path.resolve(args.bundledConfig || path.join(root, "preset", "config.toml"));
  const manifest = loadJson(manifestPath, { servers: [] });
  manifest.__source = manifestPath;
  console.log(JSON.stringify({
    "mcp.json": mergeMcp(userMcp, manifest, args.dryRun),
    "config.toml": mergeConfig(userConfig, bundledConfig, args.dryRun, args.migrate),
  }, null, 2));
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
