#!/usr/bin/env node
// Provider-neutral package assets. User edits are never replaced by a release.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const exists = file => { try { fs.lstatSync(file); return true; } catch (e) { if (e.code === 'ENOENT') return false; throw e; } };

function managed(relative) {
  return typeof relative === 'string' && !relative.includes('\\') &&
    relative.split('/').every(part => part !== '' && part !== '.' && part !== '..') &&
    (relative === 'swarm-prompt.md' || ['skills/', 'roles/', 'templates/knowledge-os/'].some(prefix => relative.startsWith(prefix)));
}

function checkedFile(root, relative) {
  const file = checked(root, relative);
  if (exists(file) && !fs.lstatSync(file).isFile()) throw new Error(`Asset target is not a file: ${relative}`);
  return file;
}

function checked(root, relative) {
  const target = path.resolve(root, relative);
  if (target !== root && !target.startsWith(root + path.sep)) throw new Error('Asset path escapes home');
  for (let item = target; ; item = path.dirname(item)) {
    if (exists(item) && fs.lstatSync(item).isSymbolicLink()) throw new Error(`Refusing linked asset path: ${relative}`);
    if (item === root) break;
  }
  return target;
}

function atomicWrite(file, bytes, mode = 0o600) {
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  const temporary = `${file}.new-${crypto.randomUUID()}`;
  try {
    fs.writeFileSync(temporary, bytes, { flag: 'wx', mode });
    fs.renameSync(temporary, file);
  } finally {
    if (exists(temporary)) fs.unlinkSync(temporary);
  }
}

function collect(source, target, entries) {
  const stat = fs.lstatSync(source);
  if (stat.isSymbolicLink()) throw new Error(`Package contains linked asset: ${target}`);
  if (stat.isDirectory()) {
    for (const name of fs.readdirSync(source).sort()) collect(path.join(source, name), path.posix.join(target, name), entries);
  } else if (stat.isFile()) {
    entries.push({ source, relative: target, mode: stat.mode & 0o777 });
  } else throw new Error(`Unsupported packaged asset: ${target}`);
}

export function installAssets(preset, home) {
  preset = path.resolve(preset);
  home = path.resolve(home);
  fs.mkdirSync(home, { recursive: true, mode: 0o700 });
  checked(home, '.');
  const lock = checked(home, '.managed-assets.lock');
  fs.mkdirSync(lock, { mode: 0o700 });
  try {
    const stateFile = checkedFile(home, '.managed-assets.json');
    const reportFile = checkedFile(home, '.managed-assets-report.json');
    const previous = exists(stateFile) ? JSON.parse(fs.readFileSync(stateFile, 'utf8')) : { schema: 1, files: {} };
    if (previous.schema !== 1 || !previous.files || Array.isArray(previous.files) || typeof previous.files !== 'object') {
      throw new Error('Unsupported managed asset state. User assets were not changed.');
    }
    const entries = [];
    for (const [source, target] of [['skills', 'skills'], ['roles', 'roles'], ['swarm-prompt.md', 'swarm-prompt.md'], ['knowledge-os/templates', 'templates/knowledge-os']]) {
      const sourcePath = path.join(preset, source);
      if (exists(sourcePath)) collect(sourcePath, target, entries);
    }
    // Reject invalid paths before replacing any asset.
    for (const { relative } of entries) { checkedFile(home, relative); checkedFile(home, path.join('.asset-updates', relative)); }
    for (const [relative, hash] of Object.entries(previous.files)) {
      if (!managed(relative) || typeof hash !== 'string' || !/^[a-f0-9]{64}$/.test(hash)) {
        throw new Error('Invalid managed asset record. User assets were not changed.');
      }
      checkedFile(home, relative);
    }
    const next = { schema: 1, files: {} };
    const report = { installed: [], updated: [], unchanged: [], preserved: [], removed: [] };
    for (const { source, relative, mode } of entries) {
      const file = checked(home, relative);
      const bytes = fs.readFileSync(source);
      const incoming = digest(bytes);
      const current = exists(file) ? digest(fs.readFileSync(file)) : null;
      if (current === incoming) {
        report.unchanged.push(relative);
      } else if (current === null || current === previous.files[relative]) {
        atomicWrite(file, bytes, mode);
        report[current === null ? 'installed' : 'updated'].push(relative);
      } else {
        atomicWrite(checked(home, path.join('.asset-updates', relative)), bytes, mode);
        report.preserved.push(relative);
        if (previous.files[relative]) next.files[relative] = previous.files[relative];
        continue;
      }
      next.files[relative] = incoming;
    }
    const shipped = new Set(entries.map(entry => entry.relative));
    for (const [relative, hash] of Object.entries(previous.files)) {
      if (shipped.has(relative)) continue;
      const file = checked(home, relative);
      if (exists(file) && fs.lstatSync(file).isFile() && digest(fs.readFileSync(file)) === hash) {
        fs.unlinkSync(file);
        report.removed.push(relative);
      }
    }
    atomicWrite(stateFile, JSON.stringify(next, null, 2) + '\n');
    atomicWrite(reportFile, JSON.stringify(report, null, 2) + '\n');
    return report;
  } finally {
    fs.rmdirSync(lock);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 4) throw new Error('Usage: install-assets.mjs PRESET HOME');
    const report = installAssets(process.argv[2], process.argv[3]);
    console.log(`[assets] installed=${report.installed.length} updated=${report.updated.length} preserved=${report.preserved.length}`);
    if (report.preserved.length) console.log('[assets] Local edits preserved. Incoming versions: home/.asset-updates; details: home/.managed-assets-report.json');
  } catch (error) {
    console.error(`[assets] FAILED: ${error.message}`);
    process.exitCode = 1;
  }
}
