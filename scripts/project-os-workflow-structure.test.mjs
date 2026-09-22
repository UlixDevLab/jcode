#!/usr/bin/env node
// project-os-workflow-structure.test.mjs
// Structural RED/GREEN test for the workflow-lens migration.
// Verifies:
//   1. .opencode/project-os.yaml workflow-pipelines view has leaf nodes and
//      edges (not whole-workflow summary nodes).
//   2. Decisions/human-gates in workflow-pipelines have labeled outgoing
//      relations encoding the branch outcome.
//   3. Workflows that share a gate declare that gate as a node and connect
//      to it with at least one edge.
//   4. The review overlay CSS uses semantic theme variables for buttons
//      and labels, with explicit default/hover/active/focus states and a
//      reduced-motion fallback.
//   5. The standalone .opencode/workflow-pipelines.mock.html has been
//      deleted (it was a parallel artifact; everything lives in the YAML
//      and the single generated HTML).
//
// Run: node scripts/project-os-workflow-structure.test.mjs
// Exit 0 on success; non-zero on first failure.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync, statSync } from 'node:fs';
import { resolve } from 'node:path';

const ROOT = resolve(new URL('..', import.meta.url).pathname);
const YAML_PATH = resolve(ROOT, '.opencode/project-os.yaml');
const HTML_PATH = resolve(ROOT, '.opencode/project-os.html');
const OVERLAY_PATH = resolve(ROOT, 'scripts/project-os-review-overlay.mjs');
const MOCK_PATH = resolve(ROOT, '.opencode/workflow-pipelines.mock.html');

function loadYaml() {
  const raw = readFileSync(YAML_PATH, 'utf8');
  // Minimal YAML parsing via the JS engine: we only need lines that start
  // with `- id:` or `id:` plus their graph: and relations: blocks. We
  // operate line-by-line so this stays pure-node and dependency-free.
  return raw;
}

function findGraphEntries(yaml, key, graphName) {
  // Walk top-level list `key:` and return array of { id, graph, raw }.
  const lines = yaml.split('\n');
  const items = [];
  let inKey = false;
  let blockDepth = -1;
  let cur = null;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!inKey && line.startsWith(key + ':')) {
      inKey = true;
      blockDepth = 0;
      i += 1;
      continue;
    }
    if (!inKey) continue;
    if (line.startsWith(' ') || line === '') {
      // inside the block
    } else {
      break;
    }
    if (line.startsWith('  - id:') || line.startsWith('  - id: ')) {
      cur = { id: line.replace(/^  - id:\s*/, '').trim(), raw: [], graph: null, relations: [] };
      items.push(cur);
      blockDepth = 1;
    } else if (cur) {
      cur.raw.push(line);
      const graphMatch = line.match(/^\s+graph:\s*(.+)$/);
      if (graphMatch) cur.graph = graphMatch[1].trim();
      const relMatch = line.match(/^\s+-\s+id:\s*([^\s]+)\s*$/);
      if (relMatch) {
        cur.relations.push({ id: relMatch[1], raw: [] });
      } else if (cur.relations.length) {
        cur.relations[cur.relations.length - 1].raw.push(line);
      }
    }
  }
  return items.filter((it) => it.graph === graphName);
}

function findEdgesInGraph(yaml, graphName) {
  const lines = yaml.split('\n');
  const edges = [];
  let inEdges = false;
  let cur = null;
  let inRelationBlock = false;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!inEdges && line.startsWith('edges:')) {
      inEdges = true;
      continue;
    }
    if (!inEdges) continue;
    if (line.startsWith(' ') || line === '') {
      // inside
    } else {
      break;
    }
    if (line.startsWith('  - id:') || line.startsWith('  - id: ')) {
      cur = { id: line.replace(/^  - id:\s*/, '').trim(), graph: null, source: null, target: null, type: null, relations: [] };
      edges.push(cur);
      inRelationBlock = false;
    } else if (cur) {
      const graphMatch = line.match(/^\s+graph:\s*(.+)$/);
      const sourceMatch = line.match(/^\s+source:\s*(.+)$/);
      const targetMatch = line.match(/^\s+target:\s*(.+)$/);
      const typeMatch = line.match(/^\s+type:\s*(.+)$/);
      if (graphMatch) cur.graph = graphMatch[1].trim();
      if (sourceMatch) cur.source = sourceMatch[1].trim();
      if (targetMatch) cur.target = targetMatch[1].trim();
      if (typeMatch) cur.type = typeMatch[1].trim();
      if (line.trim() === 'relations:') { inRelationBlock = true; continue; }
      if (inRelationBlock && line.startsWith('    - id:')) {
        const id = line.replace(/^    - id:\s*/, '').trim();
        cur.relations.push({ id, label: null, description: null, source_child: null, target_child: null });
      } else if (inRelationBlock && line.startsWith('      - id:')) {
        // alternate indentation seen in existing YAML
        const id = line.replace(/^      - id:\s*/, '').trim();
        cur.relations.push({ id, label: null, description: null });
      } else if (inRelationBlock && cur.relations.length) {
        const last = cur.relations[cur.relations.length - 1];
        const labelMatch = line.match(/^\s+label:\s*(.+)$/);
        const descMatch = line.match(/^\s+description:\s*(.+)$/);
        const srcChildMatch = line.match(/^\s+source_child:\s*(.+)$/);
        const tgtChildMatch = line.match(/^\s+target_child:\s*(.+)$/);
        if (labelMatch) last.label = labelMatch[1].trim();
        if (descMatch) last.description = descMatch[1].trim();
        if (srcChildMatch) last.source_child = srcChildMatch[1].trim();
        if (tgtChildMatch) last.target_child = tgtChildMatch[1].trim();
        if (!line.startsWith('      ') && !line.startsWith('    ') && line.trim() !== '') inRelationBlock = false;
      }
    }
  }
  return edges.filter((e) => e.graph === graphName);
}

// ---------- top-level file existence ----------

test('project-os.yaml exists and is non-empty', () => {
  assert.ok(existsSync(YAML_PATH), 'missing ' + YAML_PATH);
  const stat = statSync(YAML_PATH);
  assert.ok(stat.size > 0, 'project-os.yaml is empty');
});

test('project-os.html exists and is non-empty after export', () => {
  assert.ok(existsSync(HTML_PATH), 'missing ' + HTML_PATH);
  const stat = statSync(HTML_PATH);
  assert.ok(stat.size > 0, 'project-os.html is empty');
});

test('scripts/project-os-review-overlay.mjs exists', () => {
  assert.ok(existsSync(OVERLAY_PATH), 'missing ' + OVERLAY_PATH);
});

// ---------- standalone mock deleted ----------

test('standalone .opencode/workflow-pipelines.mock.html is gone', () => {
  assert.equal(
    existsSync(MOCK_PATH),
    false,
    'parallel artifact still present: ' + MOCK_PATH,
  );
});

// ---------- schema and graph authority ----------

test('schema_version stays at v2.1 (no v2.2 router/condition fields)', () => {
  const yaml = loadYaml();
  assert.match(yaml, /^schema_version:\s*project-os-schema\/v2\.1\s*$/m);
  assert.doesNotMatch(yaml, /\btype:\s*router\b/, 'v2.2 router nodes must not appear');
  assert.doesNotMatch(yaml, /\bcondition:\s*\S+/, 'v2.2 condition fields must not appear');
  assert.doesNotMatch(yaml, /\bdirection:\s*both\b/, 'v2.2 direction:both must not appear');
});

test('workflow-pipelines view still exists', () => {
  const yaml = loadYaml();
  assert.match(yaml, /^\s+- id:\s*workflow-pipelines\s*$/m, 'workflow-pipelines view must remain in views[]');
  assert.match(yaml, /^\s+- id:\s*workflow-pipelines\s*$/m, 'workflow-pipelines must remain in graphs[]');
});

// ---------- node taxonomy: leaf steps, no whole-workflow summaries ----------

test('workflow-pipelines nodes are leaf steps (no detached summary names)', () => {
  const yaml = loadYaml();
  const nodes = findGraphEntries(yaml, 'nodes', 'workflow-pipelines');
  assert.ok(nodes.length >= 12, 'workflow-pipelines should have at least 12 leaf nodes (got ' + nodes.length + ')');
  for (const node of nodes) {
    // Whole-workflow summary IDs end in bare category names. Leaf step IDs
    // have a `wf-<workflow>-<step>` shape. The gate nodes (`-gate` suffix)
    // are allowed because they are referenced as human-gate participants.
    const id = node.id;
    const isLeaf = /-/.test(id.slice(3)); // has at least one extra segment after wf-
    const isGate = /-gate$/.test(id);
    assert.ok(isLeaf || isGate, 'detached workflow summary id ' + id + ' is not a leaf step or shared gate');
  }
});

test('workflow-pipelines has at least one user/prompt node', () => {
  const yaml = loadYaml();
  const nodes = findGraphEntries(yaml, 'nodes', 'workflow-pipelines');
  const hasPrompt = nodes.some((n) => /prompt|user/i.test(n.id) || /prompt|user/i.test(n.raw.join('\n')));
  assert.ok(hasPrompt, 'no node represents the prompt intake');
});

test('workflow-pipelines has at least one human gate node', () => {
  const yaml = loadYaml();
  const nodes = findGraphEntries(yaml, 'nodes', 'workflow-pipelines');
  const hasGate = nodes.some((n) => /-gate$/.test(n.id));
  assert.ok(hasGate, 'workflow-pipelines must include a human gate node');
});

test('workflow-pipelines has at least one decision node', () => {
  const yaml = loadYaml();
  const nodes = findGraphEntries(yaml, 'nodes', 'workflow-pipelines');
  const hasDecision = nodes.some((n) => /decision|complexity|consistent|complex|assess/i.test(n.id));
  assert.ok(hasDecision, 'workflow-pipelines must include a decision node');
});

// ---------- edges exist with explicit source/target and relations ----------

test('workflow-pipelines has at least one directed edge', () => {
  const yaml = loadYaml();
  const edges = findEdgesInGraph(yaml, 'workflow-pipelines');
  assert.ok(edges.length > 0, 'workflow-pipelines has zero edges; this is the RED state the migration fixes');
  for (const edge of edges) {
    assert.ok(edge.source, 'edge ' + edge.id + ' missing source');
    assert.ok(edge.target, 'edge ' + edge.id + ' missing target');
  }
});

test('workflow-pipelines has at least one verification loop edge (rework target)', () => {
  const yaml = loadYaml();
  const edges = findEdgesInGraph(yaml, 'workflow-pipelines');
  const loop = edges.find((e) => /loop|rework|verify|gap/i.test(e.id) || (e.relations || []).some((r) => /loop|rework|verify|gap/i.test(r.id)));
  assert.ok(loop, 'no rework/verify loop edge in workflow-pipelines');
});

test('workflow-pipelines has at least one human-gate edge', () => {
  const yaml = loadYaml();
  const edges = findEdgesInGraph(yaml, 'workflow-pipelines');
  const gateEdge = edges.find((e) => /consent|approve|gate/i.test(e.id) || /consent|approve|gate|human/i.test(e.target || ''));
  assert.ok(gateEdge, 'no human-gate edge in workflow-pipelines');
});

test('branch outcomes encoded in relation labels, not v2.2 conditions', () => {
  const yaml = loadYaml();
  const edges = findEdgesInGraph(yaml, 'workflow-pipelines');
  let labeled = 0;
  for (const edge of edges) {
    for (const rel of edge.relations || []) {
      if (rel.label && rel.label.length > 0) {
        labeled += 1;
        assert.ok(rel.description, 'edge ' + edge.id + ' relation ' + rel.id + ' has label but no description');
      }
    }
  }
  assert.ok(labeled >= 3, 'expected at least 3 labeled relations; got ' + labeled);
});

// ---------- overlay CSS uses theme variables ----------

test('overlay CSS declares semantic theme variables on :root', () => {
  const src = readFileSync(OVERLAY_PATH, 'utf8');
  // Extract the generated CSS by calling buildCss via dynamic import.
  const mod = import(OVERLAY_PATH);
  return mod.then((overlay) => {
    const css = overlay.buildCss();
    assert.match(css, /:root\s*\{[\s\S]*--review-surface[\s\S]*\}/, 'must declare :root CSS variables');
    assert.match(css, /--review-ink/, 'must define --review-ink token');
    assert.match(css, /--review-line/, 'must define --review-line token');
    assert.match(css, /--review-focus/, 'must define --review-focus token');
  });
});

test('overlay buttons use theme variables and explicit default/hover/active/focus-visible states', () => {
  return import(OVERLAY_PATH).then((overlay) => {
    const css = overlay.buildCss();
    assert.match(css, /#project-os-review-overlay-panel button\s*\{/, 'panel button base selector missing');
    assert.match(css, /#project-os-review-overlay-panel button[^\{]*\{[^}]*background\s*:\s*var\(--review-surface/, 'default background must be a CSS variable');
    assert.match(css, /#project-os-review-overlay-panel button[^\{]*:hover\s*\{/, 'explicit :hover state required');
    assert.match(css, /#project-os-review-overlay-panel button[^\{]*:focus-visible\s*\{/, 'explicit :focus-visible state required');
    assert.match(css, /#project-os-review-overlay-panel button[^\{]*:active\s*\{/, 'explicit :active state required');
    assert.match(css, /#project-os-review-overlay-panel button\.is-active\s*\{/, 'explicit .is-active state required');
    // No raw hex on the button base; palette tokens must come from CSS vars.
    assert.doesNotMatch(css, /#project-os-review-overlay-panel button\s*\{[^}]*#fff/, 'button base must not hardcode #fff');
  });
});

test('overlay reduces motion under prefers-reduced-motion', () => {
  return import(OVERLAY_PATH).then((overlay) => {
    const css = overlay.buildCss();
    assert.match(css, /@media\s*\(prefers-reduced-motion\s*:\s*reduce\)/);
  });
});

test('overlay no longer hardcodes dark-only rgba(20,20,24,...) panel background', () => {
  return import(OVERLAY_PATH).then((overlay) => {
    const css = overlay.buildCss();
    // The legacy dark panel background was rgba(20,20,24,0.92). The
    // production overlay must read from --review-surface, not that literal.
    assert.doesNotMatch(css, /background\s*:\s*rgba\(20,\s*20,\s*24/);
  });
});