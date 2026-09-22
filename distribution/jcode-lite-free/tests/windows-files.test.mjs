import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { verifyFiles, removeVersionTree } from '../windows/install-files.mjs';

test('long MCP paths verify and retire without touching recipient home', () => {
  const root = fs.mkdtempSync(path.join(process.env.JCODE_SCRATCH_DIR || os.tmpdir(), 'win-files-'));
  try {
    const target = path.join(root, '.staging-0.3.0-beta.1-test');
    const relative = Array(8).fill('long-node-module-package-name').join('/') + '/file.js';
    fs.mkdirSync(path.dirname(path.join(target, relative)), { recursive: true });
    fs.writeFileSync(path.join(target, relative), 'retained payload');
    fs.writeFileSync(path.join(target, 'allowlist.txt'), relative + '\n');
    assert.ok(path.join(target, relative).length > 260);
    verifyFiles(target);
    for (const invalid of ['../home/state.json', 'C:/outside', 'a\\b']) {
      fs.writeFileSync(path.join(target, 'allowlist.txt'), invalid + '\n');
      assert.throws(() => verifyFiles(target), /Unsafe/);
    }
    const home = path.join(root, 'home'); fs.mkdirSync(home);
    fs.writeFileSync(path.join(home, 'keep'), 'untouched');
    assert.throws(() => removeVersionTree(root, home), /Refusing/);
    assert.throws(() => removeVersionTree(root, root), /Refusing/);
    assert.throws(() => removeVersionTree(root, path.join(root, '../0.3.0')), /Refusing/);
    removeVersionTree(root, target);
    assert.equal(fs.existsSync(target), false);
    assert.equal(fs.readFileSync(path.join(home, 'keep'), 'utf8'), 'untouched');
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
