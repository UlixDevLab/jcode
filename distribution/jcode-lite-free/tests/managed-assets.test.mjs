import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { installAssets } from '../preset/install-assets.mjs';

function fixture(t) {
  const base = process.env.JCODE_SCRATCH_DIR || path.join(process.env.HOME, '.jcode/scratch');
  fs.mkdirSync(base, { recursive: true });
  const root = fs.mkdtempSync(path.join(base, 'free-assets-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const preset = path.join(root, 'preset'), home = path.join(root, 'home');
  fs.mkdirSync(preset); fs.mkdirSync(home);
  const put = (base, name, value) => {
    const file = path.join(base, name);
    fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, value);
  };
  put(preset, 'roles/implement.md', 'release one');
  put(preset, 'swarm-prompt.md', 'route by task');
  return { root, preset, home, put };
}

test('known unmodified assets advance while user edits and custom skills survive', t => {
  const { preset, home, put } = fixture(t);
  installAssets(preset, home);
  const state = JSON.parse(fs.readFileSync(path.join(home, '.managed-assets.json'), 'utf8'));
  assert.deepEqual(Object.keys(state.files).sort(), ['roles/implement.md', 'swarm-prompt.md']);
  put(home, 'roles/implement.md', 'my customization');
  put(home, 'skills/custom/SKILL.md', 'mine');
  put(preset, 'roles/implement.md', 'release two');
  put(preset, 'swarm-prompt.md', 'new routing');
  const report = installAssets(preset, home);
  assert.deepEqual(report.preserved, ['roles/implement.md']);
  assert.equal(fs.readFileSync(path.join(home, 'roles/implement.md'), 'utf8'), 'my customization');
  assert.equal(fs.readFileSync(path.join(home, '.asset-updates/roles/implement.md'), 'utf8'), 'release two');
  assert.equal(fs.readFileSync(path.join(home, 'skills/custom/SKILL.md'), 'utf8'), 'mine');
  assert.equal(fs.readFileSync(path.join(home, 'swarm-prompt.md'), 'utf8'), 'new routing');
});

test('removal is restricted to unchanged formerly managed assets', t => {
  const { preset, home, put } = fixture(t);
  put(preset, 'skills/old/SKILL.md', 'old release');
  installAssets(preset, home);
  put(home, 'roles/implement.md', 'user edited');
  fs.rmSync(path.join(preset, 'skills'), { recursive: true });
  fs.rmSync(path.join(preset, 'roles'), { recursive: true });
  const report = installAssets(preset, home);
  assert.deepEqual(report.removed, ['skills/old/SKILL.md']);
  assert.equal(fs.readFileSync(path.join(home, 'roles/implement.md'), 'utf8'), 'user edited');
});

for (const target of ['config.toml', 'skills/../../config.toml', '/absolute', 'roles\\x']) {
  test(`invalid managed state cannot delete or modify recipient files: ${target}`, t => {
    const { preset, home, put } = fixture(t);
    put(home, 'config.toml', 'private settings');
    const hash = crypto.createHash('sha256').update('private settings').digest('hex');
    put(home, '.managed-assets.json', JSON.stringify({ schema: 1, files: { [target]: hash } }));
    assert.throws(() => installAssets(preset, home), /Invalid managed asset record/);
    assert.equal(fs.readFileSync(path.join(home, 'config.toml'), 'utf8'), 'private settings');
    assert.equal(fs.existsSync(path.join(home, 'roles/implement.md')), false);
    assert.equal(fs.existsSync(path.join(home, '.managed-assets.lock')), false);
  });
}

for (const target of ['roles', '.managed-assets-report.json', '.asset-updates']) {
  test(`linked destination is rejected before managed asset changes: ${target}`, t => {
    const { root, preset, home } = fixture(t);
    const outside = path.join(root, 'outside'); fs.mkdirSync(outside);
    fs.symlinkSync(outside, path.join(home, target));
    assert.throws(() => installAssets(preset, home), /linked asset path/);
    assert.equal(fs.existsSync(path.join(home, 'swarm-prompt.md')), false);
    assert.deepEqual(fs.readdirSync(outside), []);
  });
}

test('linked package source is rejected before installed state changes', t => {
  const { root, preset, home } = fixture(t);
  fs.writeFileSync(path.join(root, 'outside.md'), 'outside');
  fs.symlinkSync(path.join(root, 'outside.md'), path.join(preset, 'roles/linked.md'));
  assert.throws(() => installAssets(preset, home), /linked asset/);
  assert.equal(fs.existsSync(path.join(home, 'roles/implement.md')), false);
});

test('existing install lock is not removed or bypassed', t => {
  const { preset, home } = fixture(t);
  fs.mkdirSync(path.join(home, '.managed-assets.lock'));
  assert.throws(() => installAssets(preset, home), /EEXIST/);
  assert.equal(fs.existsSync(path.join(home, '.managed-assets.lock')), true);
  assert.equal(fs.existsSync(path.join(home, 'roles/implement.md')), false);
});
