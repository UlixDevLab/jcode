#!/usr/bin/env node
// project-os-review-overlay.test.mjs
// Tests for scripts/project-os-review-overlay.mjs. Pure node:test + node:assert.
//
// Run: node scripts/project-os-review-overlay.test.mjs
// Exit 0 on success; non-zero on first failure.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import vm from 'node:vm';

import {
  inject,
  buildCss,
  buildJs,
  PALETTE,
  REVIEW_LABELS,
  STORAGE_PREFIX,
  MARKER_ID,
  STYLE_ID,
  SCRIPT_ID,
} from './project-os-review-overlay.mjs';

const SAMPLE_BODY = '<!doctype html><html><head><title>t</title></head><body><main>x</main></body>';

function freshHtml(extra) {
  return SAMPLE_BODY + (extra || '');
}

// ---------- inject() contract ----------

test('inject: adds both style and script markers', () => {
  const html = freshHtml();
  const out = inject(html);
  assert.equal(out.ok, true);
  assert.match(out.html, new RegExp('<style[^>]*id="' + STYLE_ID + '"'));
  assert.match(out.html, new RegExp('<script[^>]*id="' + SCRIPT_ID + '"'));
});

test('inject: idempotent on second call (byte-equal)', () => {
  const html = freshHtml();
  const a = inject(html);
  assert.equal(a.ok, true);
  const b = inject(a.html);
  assert.equal(b.ok, true);
  assert.equal(b.html, a.html);
  assert.equal(b.replaced, false, 'second call must report replaced:false');
});

test('inject: refused when </body> is missing', () => {
  const out = inject('<!doctype html><html><head></head><body>');
  assert.equal(out.ok, false);
  assert.match(out.reason, /body/i);
  assert.equal(out.html, undefined);
});

test('inject: refusal does not throw on empty input', () => {
  let threw = false;
  try { const out = inject(''); assert.equal(out.ok, false); }
  catch (_) { threw = true; }
  assert.equal(threw, false);
});

test('inject: never modifies raw YAML', () => {
  const yaml = 'schema_version: project-os-schema/v2.1\nnodes: []\n';
  const out = inject(yaml);
  assert.equal(out.ok, false);
});

test('inject: does not duplicate markers across multiple calls', () => {
  const html = freshHtml();
  const a = inject(html);
  assert.equal((a.html.match(new RegExp(STYLE_ID, 'g')) || []).length, 1);
  assert.equal((a.html.match(new RegExp(SCRIPT_ID, 'g')) || []).length, 1);
  const b = inject(a.html);
  assert.equal((b.html.match(new RegExp(STYLE_ID, 'g')) || []).length, 1);
  assert.equal((b.html.match(new RegExp(SCRIPT_ID, 'g')) || []).length, 1);
});

// ---------- palette classes ----------

test('palette: every palette key has a CSS rule with both c4-node and canvas-node selectors', () => {
  const css = buildCss();
  for (const type of Object.keys(PALETTE)) {
    const escaped = type.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const re = new RegExp('\\.c4-node\\[data-jcode-type="' + escaped + '"[\\s\\S]*?\\.canvas-node\\[data-jcode-type="' + escaped + '"');
    assert.match(css, re, 'palette rule for ' + type);
  }
});

test('palette: contains the spec custom types', () => {
  for (const t of ['core-module','core-modified','new-module','virtual-module','agent','external-service','decision','issue','human-gate']) {
    assert.ok(PALETTE[t], 'palette must include ' + t);
  }
});

test('palette: virtual-module uses dashed border', () => {
  const css = buildCss();
  const block = css.match(/data-jcode-type="virtual-module"[^}]*\}/);
  assert.ok(block, 'virtual-module rule');
  assert.match(block[0], /dashed/);
});

// ---------- review labels ----------

test('review labels: keep / too-much / needs-more / clear all defined', () => {
  for (const k of ['keep','too-much','needs-more','clear']) {
    assert.ok(REVIEW_LABELS[k], 'missing review label: ' + k);
    assert.ok(REVIEW_LABELS[k].text && REVIEW_LABELS[k].cls);
  }
});

test('review panel JS exposes all four labels in source', () => {
  const js = buildJs();
  assert.match(js, /var REVIEWS = \[[^\]]*"keep"[^\]]*"too-much"[^\]]*"needs-more"[^\]]*"clear"/);
});

test('review labels wired into CSS via per-label rules', () => {
  const css = buildCss();
  for (const k of Object.keys(REVIEW_LABELS)) {
    const cls = REVIEW_LABELS[k].cls;
    assert.match(css, new RegExp('\\.' + cls));
  }
});

// ---------- localStorage key namespace ----------

test('storage prefix is "project-os-review:"', () => {
  assert.equal(STORAGE_PREFIX, 'project-os-review:');
});

test('storage prefix is referenced both in source and runtime JS', () => {
  const js = buildJs();
  assert.match(js, /STORAGE_PREFIX = "project-os-review:"/);
  assert.match(js, /storageKey\([^)]*\) \{[^}]*STORAGE_PREFIX \+/);
  assert.match(js, /getItem\(storageKey/);
});

test('runtime starts when file-origin localStorage throws SecurityError', () => {
  const elements = new Map();
  const makeElement = () => ({
    id: '', className: '', textContent: '', innerHTML: '', style: {}, dataset: {},
    children: [],
    classList: { add() {}, remove() {} },
    appendChild(child) { this.children.push(child); if (child.id) elements.set(child.id, child); },
    setAttribute() {}, addEventListener() {},
    querySelector() { return null; }, querySelectorAll() { return []; },
  });
  const body = makeElement();
  const document = {
    readyState: 'complete', body,
    createElement: makeElement,
    getElementById(id) { return elements.get(id) || null; },
    querySelector() { return null; }, querySelectorAll() { return []; },
    addEventListener() {},
  };
  const context = {
    document, location: { hash: '' }, console,
    window: {}, setTimeout() { return 1; }, clearTimeout() {},
    MutationObserver: class { observe() {} },
  };
  Object.defineProperty(context, 'localStorage', {
    get() { throw new DOMException('blocked', 'SecurityError'); },
  });
  const runtime = buildJs().replace(/^<script[^>]*>\n?|\n?<\/script>$/g, '');
  assert.doesNotThrow(() => vm.runInNewContext(runtime, context));
  assert.ok(context.window.__jcodeReviewOverlay, 'overlay must still initialize');
});

test('runtime validates persisted review values before applying them', () => {
  const js = buildJs();
  assert.match(js, /REVIEWS\.indexOf\(v\) >= 0 \? v : null/);
  assert.match(js, /function storedReviewCounts\(prefix\)[\s\S]*catch \(_\) \{\}/);
});

// ---------- marker constants ----------

test('marker id is stable', () => {
  assert.equal(MARKER_ID, 'project-os-review-overlay');
  assert.equal(STYLE_ID, 'project-os-review-overlay-style');
  assert.equal(SCRIPT_ID, 'project-os-review-overlay-script');
});

// ---------- end-to-end filesystem smoke ----------

test('end-to-end: idempotent two-pass injection through filesystem', () => {
  const tmp = mkdtempSync(join(tmpdir(), 'project-os-review-'));
  const file = join(tmp, 'project-os.html');
  const html = '<!doctype html><html><body><div id="x"/></body></html>';
  writeFileSync(file, html);
  const a = inject(readFileSync(file, 'utf8'));
  assert.equal(a.ok, true);
  writeFileSync(file, a.html);
  const b = inject(readFileSync(file, 'utf8'));
  assert.equal(b.ok, true);
  writeFileSync(file, b.html);
  assert.equal(readFileSync(file, 'utf8'), a.html);
  rmSync(tmp, { recursive: true, force: true });
});
