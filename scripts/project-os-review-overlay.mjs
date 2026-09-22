#!/usr/bin/env node
// project-os-review-overlay.mjs
// Idempotently injects a CSS+JS review overlay into a Knowledge OS exported HTML.
//
// Contract:
//  - Reads an exported .opencode/project-os.html (always contains </body>).
//  - Injects a single marked block (CSS + JS) just before </body>.
//  - Palettes map visible custom type text -> color (core-module blue-gray, etc.).
//  - Local-only review marks persisted under localStorage namespace
//    project-os-review:<board>:<nodeId>.
//  - Marks: keep | too-much | needs-more | clear
//  - Refuses to write when </body> is missing; never modifies the YAML.
//
// Usage:
//   node scripts/project-os-review-overlay.mjs [path/to/project-os.html]
//   default path is .opencode/project-os.html relative to cwd.

import { readFileSync, writeFileSync, statSync, existsSync, renameSync, unlinkSync } from 'node:fs';
import { resolve, basename } from 'node:path';

const MARKER_ID = 'project-os-review-overlay';
const STYLE_ID = 'project-os-review-overlay-style';
const SCRIPT_ID = 'project-os-review-overlay-script';
const STORAGE_PREFIX = 'project-os-review:';

const PALETTE = {
  'core-module':    { border: '#4a6c8f', bg: 'rgba(74,108,143,0.18)',  label: 'Core module', borderStyle: 'solid' },
  'core-modified':  { border: '#c08400', bg: 'rgba(192,132,0,0.20)',   label: 'Core modified', borderStyle: 'solid' },
  'new-module':     { border: '#1f7a3a', bg: 'rgba(31,122,58,0.18)',   label: 'New module', borderStyle: 'solid' },
  'virtual-module': { border: '#6b3fa0', bg: 'rgba(107,63,160,0.16)',  label: 'Virtual module', borderStyle: 'dashed' },
  'agent':          { border: '#0e8ea0', bg: 'rgba(14,142,160,0.16)',  label: 'Agent', borderStyle: 'solid' },
  'external-service': { border: '#0e8ea0', bg: 'rgba(14,142,160,0.16)', label: 'External service', borderStyle: 'solid' },
  'decision':       { border: '#a05a00', bg: 'rgba(160,90,0,0.16)',   label: 'Decision', borderStyle: 'solid' },
  'issue':          { border: '#b00020', bg: 'rgba(176,0,32,0.18)',   label: 'Issue', borderStyle: 'solid' },
  'human-gate':     { border: '#b8860b', bg: 'rgba(184,134,11,0.20)',  label: 'Human gate', borderStyle: 'solid' },
};

const REVIEW_LABELS = {
  'keep':        { text: 'Keep',        cls: 'is-keep' },
  'too-much':    { text: 'Too much',    cls: 'is-too-much' },
  'needs-more':  { text: 'Needs more',  cls: 'is-needs-more' },
  'clear':       { text: 'Clear',       cls: 'is-clear' },
};

const ALL_TYPES = Object.keys(PALETTE);
const ALL_REVIEWS = Object.keys(REVIEW_LABELS);

function buildCss() {
  const paletteRules = ALL_TYPES.map((type) => {
    const p = PALETTE[type];
    return '.c4-node[data-jcode-type="' + type + '"],'
      + '.canvas-node[data-jcode-type="' + type + '"]'
      + '{border-color:' + p.border
      + ';background-color:' + p.bg
      + ';border-style:' + p.borderStyle + '}';
  }).join('\n');
  const reviewColors = { keep: '#1f7a3a', 'too-much': '#a05a00', 'needs-more': '#b00020', clear: '#4a6c8f' };
  const reviewRules = ALL_REVIEWS.map((r) => {
    return '.c4-node.' + REVIEW_LABELS[r].cls + ',.canvas-node.' + REVIEW_LABELS[r].cls
      + '{box-shadow:0 0 0 3px ' + reviewColors[r] + ' inset}';
  }).join('\n');
  const css = '<style id="' + STYLE_ID + '">\n'
    // Semantic theme variables on :root so every panel/button picks them up.
    + ':root{'
    + '--review-surface:rgba(20,28,40,0.96);'
    + '--review-ink:#eee;'
    + '--review-muted:#bdbdbd;'
    + '--review-line:#333;'
    + '--review-focus:#0e8ea0;'
    + '--review-button-border:#555;'
    + '--review-button-hover:#333;'
    + '--review-shadow:0 4px 14px rgba(0,0,0,0.4);'
    + '--review-swatch-border:rgba(0,0,0,0.4);'
    + '}\n'
    + '.c4-node,.canvas-node{transition:box-shadow .12s ease,border-color .12s ease}\n'
    + paletteRules + '\n'
    + reviewRules + '\n'
    + '#' + MARKER_ID + '-legend{position:fixed;bottom:12px;right:12px;z-index:9999;background:var(--review-surface);color:var(--review-ink);padding:8px 10px;border:1px solid var(--review-line);border-radius:6px;font:11px/1.4 system-ui,sans-serif;max-width:260px;box-shadow:var(--review-shadow)}\n'
    + '#' + MARKER_ID + '-legend h3{margin:0 0 6px;font-size:11px;letter-spacing:.04em;text-transform:uppercase;color:var(--review-muted)}\n'
    + '#' + MARKER_ID + '-legend ul{list-style:none;margin:0;padding:0;display:grid;grid-template-columns:1fr 1fr;gap:3px 8px}\n'
    + '#' + MARKER_ID + '-legend li{display:flex;align-items:center;gap:6px}\n'
    + '#' + MARKER_ID + '-legend .swatch{width:10px;height:10px;border-radius:2px;border:1px solid var(--review-swatch-border)}\n'
    + '#' + MARKER_ID + '-legend .review-counts{margin-top:6px;border-top:1px solid var(--review-line);padding-top:4px;font-size:10px;color:var(--review-muted)}\n'
    + '#' + MARKER_ID + '-panel{position:fixed;bottom:12px;left:12px;z-index:9999;background:var(--review-surface);color:var(--review-ink);padding:8px 10px;border:1px solid var(--review-line);border-radius:6px;font:11px/1.4 system-ui,sans-serif;max-width:280px;display:none}\n'
    + '#' + MARKER_ID + '-panel.is-open{display:block}\n'
    + '#' + MARKER_ID + '-panel h3{margin:0 0 6px;font-size:11px;letter-spacing:.04em;text-transform:uppercase;color:var(--review-muted)}\n'
    + '#' + MARKER_ID + '-panel .target{font-weight:bold;margin-bottom:6px;color:var(--review-focus)}\n'
    + '#' + MARKER_ID + '-panel .row{display:flex;flex-wrap:wrap;gap:4px;margin-top:4px}\n'
    + '#' + MARKER_ID + '-panel button{appearance:none;cursor:pointer;padding:3px 6px;border:1px solid var(--review-button-border);border-radius:3px;font:10px/1.4 system-ui,sans-serif;background:var(--review-surface);color:var(--review-ink)}\n'
    + '#' + MARKER_ID + '-panel button:hover{background:var(--review-button-hover)}\n'
    + '#' + MARKER_ID + '-panel button:focus-visible{outline:2px solid var(--review-focus);outline-offset:2px}\n'
    + '#' + MARKER_ID + '-panel button:active{background:var(--review-button-hover);transform:translateY(1px)}\n'
    + '#' + MARKER_ID + '-panel button.is-active{background:var(--review-focus);color:var(--review-ink);border-color:var(--review-focus)}\n'
    + '@media (prefers-reduced-motion:reduce){.c4-node,.canvas-node,#' + MARKER_ID + '-panel button{transition:none}}\n'
    + '</style>';
  return css;
}

function buildJs() {
  const safeJson = (v) => JSON.stringify(v);
  const typesJson = safeJson(ALL_TYPES);
  const reviewsJson = safeJson(ALL_REVIEWS);
  const labelsJson = safeJson(Object.fromEntries(Object.entries(REVIEW_LABELS).map(([k, v]) => [k, v.text])));
  const clsJson = safeJson(Object.fromEntries(Object.entries(REVIEW_LABELS).map(([k, v]) => [k, v.cls])));
  const storageJson = safeJson(STORAGE_PREFIX);
  const markerJson = safeJson(MARKER_ID);
  // Indented runtime: keeps nested single quotes only.
  const lines = [];
  lines.push('(function () {');
  lines.push('  var TYPES = ' + typesJson + ';');
  lines.push('  var REVIEWS = ' + reviewsJson + ';');
  lines.push('  var LABELS = ' + labelsJson + ';');
  lines.push('  var REVIEW_CLS = ' + clsJson + ';');
  lines.push('  var STORAGE_PREFIX = ' + storageJson + ';');
  lines.push('  var MARKER_ID = ' + markerJson + ';');
  lines.push('  function decorateNode(nodeEl) {');
  lines.push('    if (!nodeEl || nodeEl.dataset.jcodeTyped === "1") return null;');
  lines.push('    var c4Kind = nodeEl.querySelector(".kind");');
  lines.push('    var popupKind = nodeEl.querySelector(".popup-kind");');
  lines.push('    var summary = nodeEl.querySelector(".node-summary");');
  lines.push('    var candidate = ((c4Kind && c4Kind.textContent) || (popupKind && popupKind.textContent) || "").trim();');
  lines.push('    var resolved = null;');
  lines.push('    for (var i = 0; i < TYPES.length; i++) if (candidate.toLowerCase() === TYPES[i]) { resolved = TYPES[i]; break; }');
  lines.push('    if (!resolved) {');
  lines.push('      var text = ((summary && summary.textContent) || "").toLowerCase();');
  lines.push('      for (var j = 0; j < TYPES.length; j++) if (text.indexOf(TYPES[j]) !== -1) { resolved = TYPES[j]; break; }');
  lines.push('    }');
  lines.push('    if (resolved) nodeEl.setAttribute("data-jcode-type", resolved);');
  lines.push('    nodeEl.dataset.jcodeTyped = "1";');
  lines.push('    return resolved;');
  lines.push('  }');
  lines.push('  function findNodeId(nodeEl) {');
  lines.push('    if (nodeEl.dataset) {');
  lines.push('      if (nodeEl.dataset.nodeId) return nodeEl.dataset.nodeId;');
  lines.push('      if (nodeEl.dataset.id) return nodeEl.dataset.id;');
  lines.push('    }');
  lines.push('    var cls = nodeEl.className || "";');
  lines.push('    var m = cls.match(/(?:^|\\s)node-([\\w-]+)/);');
  lines.push('    return m ? m[1] : null;');
  lines.push('  }');
  lines.push('  function paintReview(nodeEl, value) {');
  lines.push('    for (var i = 0; i < REVIEWS.length; i++) nodeEl.classList.remove(REVIEW_CLS[REVIEWS[i]]);');
  lines.push('    if (value && REVIEW_CLS[value]) nodeEl.classList.add(REVIEW_CLS[value]);');
  lines.push('  }');
  lines.push('  function storageKey(b, n) { return STORAGE_PREFIX + b + ":" + n; }');
  lines.push('  function loadMark(b, n) { try { var v = localStorage.getItem(storageKey(b, n)); return REVIEWS.indexOf(v) >= 0 ? v : null; } catch (_) { return null; } }');
  lines.push('  function setMark(b, n, v) { try { if (!v) localStorage.removeItem(storageKey(b, n)); else localStorage.setItem(storageKey(b, n), v); return true; } catch (_) { return false; } }');
  lines.push('  function storedReviewCounts(prefix) { var counts = { keep: 0, "too-much": 0, "needs-more": 0, clear: 0 }; try { for (var i = 0; i < localStorage.length; i++) { var k = localStorage.key(i); if (k && k.indexOf(prefix) === 0) { var v = localStorage.getItem(k); if (counts[v] !== undefined) counts[v] += 1; } } } catch (_) {} return counts; }');
  lines.push('  function getBoardId() {');
  lines.push('    var a = document.querySelector("[data-board-id]");');
  lines.push('    if (a) return a.getAttribute("data-board-id");');
  lines.push('    var hash = location.hash.replace(/^#/, "");');
  lines.push('    if (hash) return hash;');
  lines.push('    return "default";');
  lines.push('  }');
  lines.push('  function paintAllReviews(boardId) {');
  lines.push('    var nodes = document.querySelectorAll(".c4-node[data-jcode-type],.canvas-node[data-jcode-type]");');
  lines.push('    for (var i = 0; i < nodes.length; i++) {');
  lines.push('      var el = nodes[i];');
  lines.push('      var id = findNodeId(el);');
  lines.push('      if (!id) continue;');
  lines.push('      paintReview(el, loadMark(boardId, id));');
  lines.push('    }');
  lines.push('  }');
  lines.push('  function renderLegend(boardId) {');
  lines.push('    var legend = document.getElementById(MARKER_ID + "-legend");');
  lines.push('    if (!legend) { legend = document.createElement("div"); legend.id = MARKER_ID + "-legend"; document.body.appendChild(legend); }');
  lines.push('    var prefix = STORAGE_PREFIX + boardId + ":";');
  lines.push('    var counts = storedReviewCounts(prefix);');
  lines.push('    var html = "<h3>JCode review overlay</h3><ul>";');
  lines.push('    for (var t = 0; t < TYPES.length; t++) { var name = TYPES[t]; html += "<li><span class=\\"swatch\\" data-type=\\"" + name + "\\"></span>" + name + "</li>"; }');
  lines.push('    html += "</ul><div class=\\"review-counts\\">";');
  lines.push('    for (var r = 0; r < REVIEWS.length; r++) { var key = REVIEWS[r]; html += LABELS[key] + ": " + counts[key] + " | "; }');
  lines.push('    html += "</div>";');
  lines.push('    legend.innerHTML = html;');
  lines.push('    var swatches = legend.querySelectorAll(".swatch");');
  lines.push('    for (var s = 0; s < swatches.length; s++) { var sp = swatches[s]; var tn = sp.getAttribute("data-type"); sp.style.background = nodePaletteBg(tn); sp.style.borderColor = nodePaletteBorder(tn); }');
  lines.push('  }');
  lines.push('  function nodePaletteBorder(name) { if (name === "core-module") return "#4a6c8f"; if (name === "core-modified") return "#c08400"; if (name === "new-module") return "#1f7a3a"; if (name === "virtual-module") return "#6b3fa0"; if (name === "agent" || name === "external-service") return "#0e8ea0"; if (name === "decision") return "#a05a00"; if (name === "issue") return "#b00020"; if (name === "human-gate") return "#b8860b"; return "#888"; }');
  lines.push('  function nodePaletteBg(name) { if (name === "core-module") return "rgba(74,108,143,0.4)"; if (name === "core-modified") return "rgba(192,132,0,0.4)"; if (name === "new-module") return "rgba(31,122,58,0.4)"; if (name === "virtual-module") return "rgba(107,63,160,0.35)"; if (name === "agent" || name === "external-service") return "rgba(14,142,160,0.35)"; if (name === "decision") return "rgba(160,90,0,0.35)"; if (name === "issue") return "rgba(176,0,32,0.35)"; if (name === "human-gate") return "rgba(184,134,11,0.4)"; return "rgba(128,128,128,0.35)"; }');
  lines.push('  function ensurePanel() {');
  lines.push('    var panel = document.getElementById(MARKER_ID + "-panel");');
  lines.push('    if (panel) return panel;');
  lines.push('    panel = document.createElement("aside");');
  lines.push('    panel.id = MARKER_ID + "-panel";');
  lines.push('    panel.setAttribute("aria-label", "Review this node");');
  lines.push('    var target = document.createElement("div"); target.className = "target";');
  lines.push('    var row = document.createElement("div"); row.className = "row";');
  lines.push('    var clearRow = document.createElement("div"); clearRow.className = "clear-row";');
  lines.push('    var clearBtn = document.createElement("button"); clearBtn.setAttribute("data-clear", ""); clearBtn.textContent = "Clear mark";');
  lines.push('    var h = document.createElement("h3"); h.textContent = "Review";');
  lines.push('    clearRow.appendChild(clearBtn);');
  lines.push('    panel.appendChild(h); panel.appendChild(target); panel.appendChild(row); panel.appendChild(clearRow);');
  lines.push('    document.body.appendChild(panel);');
  lines.push('    return panel;');
  lines.push('  }');
  lines.push('  function findActiveNode() {');
  lines.push('    var c4Selected = document.querySelector(".c4-node.selected");');
  lines.push('    if (c4Selected) return c4Selected;');
  lines.push('    var detail = document.querySelector(".detail-panel");');
  lines.push('    if (detail) {');
  lines.push('      var heading = detail.querySelector("h2");');
  lines.push('      if (heading) {');
  lines.push('        var wanted = heading.textContent.trim();');
  lines.push('        var candidates = document.querySelectorAll(".canvas-node");');
  lines.push('        for (var i = 0; i < candidates.length; i++) { var el = candidates[i]; var t = el.querySelector(".node-title"); if (t && t.textContent.trim() === wanted) return el; }');
  lines.push('      }');
  lines.push('    }');
  lines.push('    var rf = document.querySelector(".react-flow__node.is-active, .react-flow__node.selected");');
  lines.push('    if (rf) { var inner = rf.querySelector(".c4-node, .canvas-node"); if (inner) return inner; }');
  lines.push('    return document.querySelector(".canvas-node.is-active");');
  lines.push('  }');
  lines.push('  function renderReviewPanel() {');
  lines.push('    var panel = ensurePanel();');
  lines.push('    var boardId = getBoardId();');
  lines.push('    var target = findActiveNode();');
  lines.push('    if (!target) { panel.classList.remove("is-open"); return; }');
  lines.push('    var id = findNodeId(target);');
  lines.push('    if (!id) { panel.classList.remove("is-open"); return; }');
  lines.push('    panel.classList.add("is-open");');
  lines.push('    panel.querySelector(".target").textContent = "Node: " + id;');
  lines.push('    var row = panel.querySelector(".row");');
  lines.push('    row.innerHTML = "";');
  lines.push('    var current = loadMark(boardId, id);');
  lines.push('    for (var i = 0; i < REVIEWS.length; i++) {');
  lines.push('      (function () {');
  lines.push('        var r = REVIEWS[i];');
  lines.push('        var btn = document.createElement("button");');
  lines.push('        btn.textContent = LABELS[r];');
  lines.push('        btn.setAttribute("data-review", r);');
  lines.push('        if (current === r) btn.classList.add("is-active");');
  lines.push('        btn.addEventListener("click", function () {');
  lines.push('          setMark(boardId, id, r);');
  lines.push('          paintReview(target, r);');
  lines.push('          paintAllReviews(boardId);');
  lines.push('          renderLegend(boardId);');
  lines.push('          renderReviewPanel();');
  lines.push('        });');
  lines.push('        row.appendChild(btn);');
  lines.push('      })();');
  lines.push('    }');
  lines.push('    panel.querySelector("[data-clear]").onclick = function () {');
  lines.push('      setMark(boardId, id, null);');
  lines.push('      paintReview(target, null);');
  lines.push('      paintAllReviews(boardId);');
  lines.push('      renderLegend(boardId);');
  lines.push('      renderReviewPanel();');
  lines.push('    };');
  lines.push('  }');
  lines.push('  function paintAll() {');
  lines.push('    var nodes = document.querySelectorAll(".c4-node, .canvas-node");');
  lines.push('    for (var i = 0; i < nodes.length; i++) decorateNode(nodes[i]);');
  lines.push('    var boardId = getBoardId();');
  lines.push('    paintAllReviews(boardId);');
  lines.push('    renderLegend(boardId);');
  lines.push('    renderReviewPanel();');
  lines.push('  }');
  lines.push('  var observer = new MutationObserver(function () {');
  lines.push('    if (window.__jcodeReviewTick) clearTimeout(window.__jcodeReviewTick);');
  lines.push('    window.__jcodeReviewTick = setTimeout(paintAll, 60);');
  lines.push('  });');
  lines.push('  function start() {');
  lines.push('    paintAll();');
  lines.push('    observer.observe(document.body, { subtree: true, childList: true, attributes: true });');
  lines.push('  }');
  lines.push('  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start); else start();');
  lines.push('  window.__jcodeReviewOverlay = { palette: TYPES, reviews: REVIEWS, storage: STORAGE_PREFIX, paintAll: paintAll, findNodeId: findNodeId, findActiveNode: findActiveNode, getBoardId: getBoardId, renderReviewPanel: renderReviewPanel, decorateNode: decorateNode };');
  lines.push('})();');
  return '<script id="' + SCRIPT_ID + '">\n' + lines.join('\n') + '\n</script>';
}

function isAlreadyInjected(html) {
  // Idempotence: a freshly injected file contains both markers. Return that state.
  const styleRe = new RegExp('<style[^>]*id=["\']' + STYLE_ID + '["\']');
  const scriptRe = new RegExp('<script[^>]*id=["\']' + SCRIPT_ID + '["\']');
  return styleRe.test(html) && scriptRe.test(html);
}

function stripInjected(html) {
  const stripStyle = new RegExp('<style[^>]*id=["\']' + STYLE_ID + '["\'][^>]*>[\\s\\S]*?</style>\\s*', 'g');
  const stripScript = new RegExp('<script[^>]*id=["\']' + SCRIPT_ID + '["\'][^>]*>[\\s\\S]*?</script>\\s*', 'g');
  return html.replace(stripStyle, '').replace(stripScript, '');
}

function inject(html) {
  if (typeof html !== 'string') return { ok: false, reason: 'input-not-string', html: undefined };
  if (!html.includes('</body>')) return { ok: false, reason: 'missing-</body>-tag', html: undefined };
  if (isAlreadyInjected(html)) return { ok: true, html: html, replaced: false };
  const cleaned = stripInjected(html);
  const block = buildCss() + '\n' + buildJs();
  const next = cleaned.replace('</body>', block + '\n</body>');
  return { ok: true, html: next, replaced: true };
}

function main() {
  const target = resolve(process.argv[2] || '.opencode/project-os.html');
  if (!existsSync(target)) {
    process.stderr.write('project-os-review-overlay: file not found: ' + target + '\n');
    process.exitCode = 3;
    return;
  }
  const before = statSync(target);
  if (!before.isFile()) {
    process.stderr.write('project-os-review-overlay: not a file: ' + target + '\n');
    process.exitCode = 3;
    return;
  }
  const html = readFileSync(target, 'utf8');
  const result = inject(html);
  if (!result.ok) {
    process.stderr.write('project-os-review-overlay: refused: ' + result.reason + '\n');
    process.exitCode = 4;
    return;
  }
  const second = inject(result.html);
  if (!second.ok || second.html !== result.html) {
    process.stderr.write('project-os-review-overlay: idempotence check failed\n');
    process.exitCode = 2;
    return;
  }
  const temporary = target + '.review-tmp-' + process.pid;
  try {
    writeFileSync(temporary, result.html);
    renameSync(temporary, target);
  } finally {
    if (existsSync(temporary)) unlinkSync(temporary);
  }
  const after = statSync(target);
  process.stdout.write('project-os-review-overlay: ' + basename(target) + ' (' + before.size + '->' + after.size + ' bytes) ok\n');
}

if (import.meta.url === 'file://' + process.argv[1]) {
  main();
}

export { inject, buildCss, buildJs, PALETTE, REVIEW_LABELS, STORAGE_PREFIX, MARKER_ID, STYLE_ID, SCRIPT_ID };
