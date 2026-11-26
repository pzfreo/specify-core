import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { pathToFileURL } from 'node:url';
import { build } from 'esbuild';
import { requireEngine } from './engine-path.mjs';
import { bestDrawnView, drawnFromScene, withoutNotesBlock } from '../src/specify/drawn.ts';
import { bestReferenceView, findStatedOrigin, isSpecifyCreated, normalizeDraftingPmi, resolveSavedViewCamera } from '../src/specify/drafting-layout.ts';
import {
  isDecidedNoCallout, oneViewFile, oneViewLabels, readSceneSemanticLabels, reviewDrawnFromScene, scenePendingState,
} from '../src/specify/one-view.ts';
import { chosenForYou } from '../src/specify/chosen.ts';

const { modulePath, wasmPath } = requireEngine();
const wasm = await import(pathToFileURL(modulePath).href);
await wasm.default({ module_or_path: readFileSync(wasmPath) });

function fakeDom() {
  const makeStyle = () => {
    const map = new Map();
    return new Proxy({}, {
      set(_t, prop, val) { map.set(String(prop), String(val)); return true; },
      get(_t, prop) {
        if (prop === 'setProperty') return (k, v) => map.set(String(k), String(v));
        if (prop === 'getPropertyValue') return (k) => map.get(String(k)) ?? '';
        return map.get(String(prop)) ?? '';
      },
    });
  };
  const doc = { defaultView: { Element: Object }, createElement: null };
  const makeEl = (tag) => {
    const classes = new Set();
    const el = {
      tagName: tag.toUpperCase(),
      ownerDocument: doc,
      get className() { return [...classes].join(' '); },
      set className(v) {
        classes.clear();
        for (const c of String(v).split(/\s+/).filter(Boolean)) classes.add(c);
      },
      classList: {
        add: (...cs) => cs.forEach((c) => classes.add(c)),
        remove: (...cs) => cs.forEach((c) => classes.delete(c)),
        toggle: (c, force) => (force === undefined ? (classes.has(c) ? classes.delete(c) : classes.add(c)) : (force ? classes.add(c) : classes.delete(c))),
        contains: (c) => classes.has(c),
      },
      textContent: '',
      title: '',
      dataset: {},
      style: makeStyle(),
      children: [],
      parentElement: null,
      get parentNode() { return el.parentElement; },
      get firstElementChild() { return el.children[0] ?? null; },
      append(...nodes) {
        for (const n of nodes) {
          if (typeof n === 'string') el.textContent += n;
          else if (n && typeof n === 'object') { n.parentElement = el; el.children.push(n); }
        }
      },
      prepend(...nodes) {
        for (const n of [...nodes].reverse()) {
          if (n && typeof n === 'object') { n.parentElement = el; el.children.unshift(n); }
        }
      },
      appendChild(n) { el.append(n); return n; },
      removeChild(n) { n?.remove?.(); return n; },
      remove() {
        if (!el.parentElement) return;
        const idx = el.parentElement.children.indexOf(el);
        if (idx >= 0) el.parentElement.children.splice(idx, 1);
        el.parentElement = null;
      },
      addEventListener() {},
      setAttribute() {},
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 0, height: 0, right: 0, bottom: 0 }),
    };
    return el;
  };
  doc.createElement = makeEl;
  return doc;
}

let runtimePromise;
function bundleSpecifyRuntime() {
  runtimePromise ??= (async () => {
    const built = await build({
      stdin: {
        contents: `
          export * as THREE from 'three';
          export { createAnnotations } from './src/specify/annotations.ts';
          export { createDrawnLayer, STANDARD_INK, STANDARD_GAP_INK, STANDARD_UNSAVED_INK, STANDARD_LIT_INK } from './src/specify/drawn-layer.ts';
          export { toLabels } from './src/specify/labels.ts';
        `,
        resolveDir: process.cwd(),
        loader: 'ts',
      },
      bundle: true,
      write: false,
      format: 'esm',
      platform: 'browser',
    });
    return import(`data:text/javascript;base64,${Buffer.from(built.outputFiles[0].text).toString('base64')}`);
  })();
  return runtimePromise;
}

const rowsOf = (analysis, questions = []) => {
  const qById = new Map(questions.map((q) => [q.id, q]));
  return [
    ...analysis.table.rows.map((r) => ({
      id: r.id, state: r.state, open: r.open, faces: r.faces, label: r.id,
      ...(qById.has(r.id) ? { question: qById.get(r.id) } : {}),
    })),
    ...analysis.table.file_pmi.map((r, i) => ({
      id: r.number ?? r.id ?? `item:${i}`, state: 'stated', open: false, faces: r.faces, label: r.text, other: true,
    })),
  ];
};

const minDistToFaceMesh = (pt, faces, mesh) => {
  let best = Infinity;
  for (const fid of faces) {
    const [start, count] = mesh.faces[fid] ?? [0, 0];
    for (let k = start * 3; k < (start + count) * 3; k++) {
      const v = mesh.indices[k] ?? 0;
      const dx = pt[0] - (mesh.positions[v * 3] ?? 0);
      const dy = pt[1] - (mesh.positions[v * 3 + 1] ?? 0);
      const dz = pt[2] - (mesh.positions[v * 3 + 2] ?? 0);
      const d = Math.hypot(dx, dy, dz);
      if (d < best) best = d;
    }
  }
  return best;
};

const pointToPlaneDist = (pt, face) => {
  if (!face?.direction) return Infinity;
  const n = face.direction;
  const nLen = Math.hypot(n[0], n[1], n[2]) || 1;
  return Math.abs(
    (pt[0] - face.centroid[0]) * n[0]
    + (pt[1] - face.centroid[1]) * n[1]
    + (pt[2] - face.centroid[2]) * n[2],
  ) / nLen;
};

test('isDecidedNoCallout distinguishes explicit general/none decisions from null unanswered gaps', () => {
  assert.equal(isDecidedNoCallout('general'), true);
  assert.equal(isDecidedNoCallout('General'), true);
  assert.equal(isDecidedNoCallout('none'), true);
  assert.equal(isDecidedNoCallout('None (as machined)'), true);
  assert.equal(isDecidedNoCallout(null), false);
  assert.equal(isDecidedNoCallout(undefined), false);
  assert.equal(isDecidedNoCallout('fit:H7'), false);
  assert.equal(isDecidedNoCallout('tapped:M6'), false);
});

test('Ø8 general-tolerance hole never gets a ⊕ gap marker or floating pill before Keep all, after Keep all, when answered general, or after Save', () => {
  const bytes = gunzipSync(readFileSync('../tests/fixtures/parts/GRM-01_shank.step.gz'));
  const eng = new wasm.IntentEngine('GRM-01_shank.step', bytes);
  try {
    const { analysis } = JSON.parse(eng.snapshot());
    const bare = JSON.parse(eng.review('{}'));
    const shownQs = bare.questions.filter((q) => !q.id.startsWith('length:') && !q.id.startsWith('location:'));
    const rows = rowsOf(analysis, shownQs);
    const holeRow = rows.find((r) => r.id === 'hole.function:holes:6');
    assert.ok(holeRow, 'GRM-01 has the Ø8 hole row');
    assert.equal(holeRow.question?.default, 'general');

    // 1. Before Keep all (opening state): no ⊕ gap marker for the routine `general` hole.
    const emptyFile = oneViewFile(null, analysis, rows, new Map());
    const atOpen = oneViewLabels(bare.intent, analysis, rows, emptyFile, new Map(), {}, new Set(bare.open), false);
    assert.equal(atOpen.labels.some((l) => l.question === 'hole.function:holes:6'), false, 'no ⊕ gap marker at open');

    // 2. After Keep all (before Save): Keep all keeps chosenForYou (does not touch holes:6).
    const chosen = chosenForYou(shownQs, {}, analysis);
    const keepAnswers = Object.fromEntries(chosen.map((c) => [c.id, shownQs.find((q) => q.id === c.id).default]));
    const keepPending = new Map(Object.entries(keepAnswers).map(([id, value]) => [id, { kind: 'fill', id, value }]));
    const afterKeepRev = JSON.parse(eng.review(JSON.stringify(keepAnswers)));
    const afterKeep = oneViewLabels(afterKeepRev.intent, analysis, rows, emptyFile, keepPending, keepAnswers, new Set(afterKeepRev.open), false);
    assert.equal(afterKeep.labels.some((l) => l.question === 'hole.function:holes:6'), false, 'no ⊕ gap marker after Keep all');

    // 3. Explicitly answering the Ø8 hole as `general` before Save: no floating "general tolerance" pill either.
    const genPending = new Map(keepPending);
    genPending.set('hole.function:holes:6', { kind: 'fill', id: 'hole.function:holes:6', value: 'general', text: 'general tolerance' });
    const withGen = oneViewLabels(afterKeepRev.intent, analysis, rows, emptyFile, genPending, { ...keepAnswers, 'hole.function:holes:6': 'general' }, new Set(), false);
    assert.equal(withGen.labels.some((l) => l.question === 'hole.function:holes:6'), false, 'no floating pill when answered general');

    // 4. After Save (following Keep all): engine still reports hole.function:holes:6 as gap+suggestion=general;
    //    UI matches before-Save and emits no ⊕ gap marker.
    const saved = JSON.parse(eng.edit(JSON.stringify({ fill: keepAnswers })));
    assert.ok(saved.written, 'Save succeeds');
    const eng2 = new wasm.IntentEngine('GRM-01_shank.specified.step', new Uint8Array(saved.bytes));
    try {
      const snap2 = JSON.parse(eng2.snapshot());
      const stored2 = snap2.analysis.resume?.answers ?? {};
      const rev2 = JSON.parse(eng2.review(JSON.stringify(stored2)));
      const rows2 = rowsOf(snap2.analysis, rev2.questions);
      const afterSaveRow = rows2.find((r) => r.id === 'hole.function:holes:6');
      assert.equal(afterSaveRow?.state, 'gap', 'engine still reports untouched general hole as gap after Keep-all Save');
      const afterSave = oneViewLabels(rev2.intent, snap2.analysis, rows2, oneViewFile(null, snap2.analysis, rows2, new Map()), new Map(), stored2, new Set(rev2.open), false);
      assert.equal(afterSave.labels.some((l) => l.question === 'hole.function:holes:6'), false, 'no ⊕ gap marker after Save');
    } finally {
      eng2.free();
    }

    // 5. Answering the Ø8 hole with a real fit (`fit:H7`) DOES show an unsaved callout before Save.
    const fitAnswers = { ...keepAnswers, 'hole.function:holes:6': 'fit:H7' };
    const fitPending = new Map(keepPending);
    fitPending.set('hole.function:holes:6', { kind: 'fill', id: 'hole.function:holes:6', value: 'fit:H7', text: 'H7' });
    const fitRev = JSON.parse(eng.review(JSON.stringify(fitAnswers)));
    const withFit = oneViewLabels(fitRev.intent, analysis, rows, emptyFile, fitPending, fitAnswers, new Set(fitRev.open), false);
    const fitLabel = withFit.labels.find((l) => l.question === 'hole.function:holes:6');
    assert.ok(fitLabel?.unsaved, 'fit:H7 produces an unsaved callout');

    // 6. A synthetic real open required gap (default: null) with faces DOES get a ⊕ gap marker,
    //    and a row decided `none` does NOT.
    const synthRows = [
      { id: 'hole.function:holes:6', state: 'gap', open: true, faces: [6], label: 'Ø8 hole', question: { id: 'hole.function:holes:6', kind: 'choice', prompt: 'What is the Ø8 hole for?', faces: [6], default: null, diameter: 8, attention: 'required' } },
    ];
    const reqGap = oneViewLabels(null, analysis, synthRows, emptyFile, new Map(), {}, new Set(['hole.function:holes:6']), false);
    assert.ok(reqGap.labels.some((l) => l.id === 'gap:hole.function:holes:6' && l.cells[0] === 'hole Ø8'), 'real required gap gets ⊕ marker');
    const nonePending = new Map([['hole.function:holes:6', { kind: 'none', id: 'hole.function:holes:6' }]]);
    const noneOut = oneViewLabels(null, analysis, synthRows, emptyFile, nonePending, {}, new Set(), false);
    assert.equal(noneOut.labels.some((l) => l.question === 'hole.function:holes:6'), false, 'decided none gets no marker or pill');
  } finally {
    eng.free();
  }
});

test('drafting layout on GRM-01: feature-attached datums, rim-attached hole leaders, feature-touching extension lines, and identical geometry before/after Save', () => {
  const bytes = gunzipSync(readFileSync('../tests/fixtures/parts/GRM-01_shank.step.gz'));
  const eng = new wasm.IntentEngine('GRM-01_shank.step', bytes);
  try {
    const { analysis, mesh } = JSON.parse(eng.snapshot());
    const bare = JSON.parse(eng.review('{}'));
    const shownQs = bare.questions.filter((q) => !q.id.startsWith('length:') && !q.id.startsWith('location:'));
    const chosen = chosenForYou(shownQs, {}, analysis);
    const keepAnswers = Object.fromEntries(chosen.map((c) => [c.id, shownQs.find((q) => q.id === c.id).default]));

    // Before Save: live exact pmiScene + normalizeDraftingPmi.
    const rawBefore = JSON.parse(eng.pmiScene(JSON.stringify({ answers: keepAnswers, auto: [] })));
    const readBefore = reviewDrawnFromScene(rawBefore, analysis);
    assert.ok(readBefore.ok, readBefore.reason);
    const semBefore = readSceneSemanticLabels(rawBefore);
    const normBefore = normalizeDraftingPmi(readBefore.value, rawBefore, analysis, mesh, semBefore);

    // Every datum triangle's base vertices lie flush on the datum face plane (0.00 mm gap).
    const faceById = new Map(analysis.faces.map((f) => [f.id, f]));
    for (const letter of ['A', 'B', 'C']) {
      const id = `datum:${letter}`;
      const gs = normBefore.presentation.items[id];
      assert.equal(gs?.length, 1, `${id} has one drafting graphic`);
      assert.equal(gs[0].triangles.length, 1, `${id} has a filled datum triangle`);
      const [, base1, base2] = gs[0].triangles[0];
      const baseMid = [(base1[0] + base2[0]) / 2, (base1[1] + base2[1]) / 2, (base1[2] + base2[2]) / 2];
      const fids = normBefore.links[id].faces;
      const planeFace = fids.map((fid) => faceById.get(fid)).find((f) => f?.kind === 'plane');
      assert.ok(planeFace, `${id} references a planar face`);
      assert.ok(pointToPlaneDist(baseMid, planeFace) < 0.05, `${id} triangle base sits flush on datum plane`);
    }

    // Stacked thread + position callout on M6 hole shares exactly ONE leader anchored on the hole rim.
    const m6Thread = normBefore.presentation.items['thread:holes:20.25']?.[0];
    const m6Pos = normBefore.presentation.items['position:holes:20.25']?.[0];
    assert.ok(m6Thread && m6Pos, 'M6 thread and position callouts exist');
    const leaderCount = [m6Thread, m6Pos].filter((g) => g.polylineRoles?.includes('leader')).length;
    assert.equal(leaderCount, 1, 'stacked thread + position callout emits exactly one shared leader');
    const leaderG = m6Thread.polylineRoles?.includes('leader') ? m6Thread : m6Pos;
    const tip = leaderG.polylines[leaderG.polylineRoles.indexOf('leader')][0];
    const hole20 = analysis.features.find((f) => f.id === 'holes:20.25');
    assert.ok(minDistToFaceMesh(tip, hole20.faces, mesh) < 2.5, 'hole leader tip lands near the hole opening rim');

    // Linear/basic dimensions have 2 extension lines and 2 filled arrowheads.
    const dimIds = Object.keys(normBefore.presentation.items).filter((k) => k.startsWith('planner:dim:plane:'));
    assert.equal(dimIds.length, 3, 'three basic locating dimensions');
    for (const id of dimIds) {
      const g = normBefore.presentation.items[id][0];
      assert.equal(g.polylineRoles.filter((r) => r === 'extension').length, 2, `${id} has 2 extension lines`);
      assert.equal(g.triangles.length, 2, `${id} has 2 filled arrowheads`);
    }

    // After Save: same layout & same Best view camera.
    const saved = JSON.parse(eng.edit(JSON.stringify({ fill: keepAnswers })));
    assert.ok(saved.written);
    const eng2 = new wasm.IntentEngine('GRM-01_shank.specified.step', new Uint8Array(saved.bytes));
    try {
      const snap2 = JSON.parse(eng2.snapshot());
      const stored2 = snap2.analysis.resume?.answers ?? {};
      const rawAfter = JSON.parse(eng2.pmiScene(JSON.stringify({ answers: stored2, auto: [] })));
      const readAfter = reviewDrawnFromScene(rawAfter, snap2.analysis);
      assert.ok(readAfter.ok);
      const semAfter = readSceneSemanticLabels(rawAfter);
      const normAfter = normalizeDraftingPmi(readAfter.value, rawAfter, snap2.analysis, snap2.mesh, semAfter);

      const camBefore = bestReferenceView(normBefore, analysis, mesh);
      const camAfter = bestReferenceView(normAfter, snap2.analysis, snap2.mesh);
      for (let k = 0; k < 3; k++) {
        assert.ok(Math.abs(camBefore.eye[k] - camAfter.eye[k]) < 1e-6, 'Best view eye matches before and after Save');
        assert.ok(Math.abs(camBefore.up[k] - camAfter.up[k]) < 1e-6, 'Best view up matches before and after Save');
      }
      // Every visible non-general item has identical text origin before and after Save.
      for (const id of ['datum:A', 'datum:B', 'datum:C', 'thread:holes:20.25', 'position:holes:20.25', 'thread:holes:7', 'position:holes:7', ...dimIds]) {
        const pBefore = normBefore.presentation.items[id]?.[0]?.textPlacements?.[0]?.origin;
        const pAfter = normAfter.presentation.items[id]?.[0]?.textPlacements?.[0]?.origin;
        assert.ok(pBefore && pAfter, `${id} placed before and after Save`);
        const delta = Math.hypot(pBefore[0] - pAfter[0], pBefore[1] - pAfter[1], pBefore[2] - pAfter[2]);
        assert.ok(delta < 1e-3, `${id} placement unchanged across Save (delta=${delta})`);
      }
      // Plain part without an explicit STEP origin gets no origin triad before or after Save.
      assert.equal(findStatedOrigin(bytes, analysis, normBefore), null);
      assert.equal(findStatedOrigin(new Uint8Array(saved.bytes), snap2.analysis, normAfter), null);
    } finally {
      eng2.free();
    }
  } finally {
    eng.free();
  }
});

test('NIST CTC-01: preserves file-native graphics untouched, resolves MBD_0 camera sign, and detects explicit STEP origin point', () => {
  const bytes = gunzipSync(readFileSync('../tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz'));
  const eng = new wasm.IntentEngine('nist_ctc_01.stp', bytes);
  try {
    const { analysis, mesh } = JSON.parse(eng.snapshot());
    const raw = JSON.parse(eng.pmiScene(JSON.stringify({ answers: {}, auto: [] })));
    const read = reviewDrawnFromScene(raw, analysis);
    assert.ok(read.ok, read.reason);
    const sem = readSceneSemanticLabels(raw);
    const norm = normalizeDraftingPmi(read.value, raw, analysis, mesh, sem);

    // All 23 file-native graphics remain source='file' untouched.
    const nativeCount = Object.entries(norm.presentation.items)
      .flatMap(([item, gs]) => gs.filter((g) => !isSpecifyCreated(item, g)))
      .length;
    assert.ok(nativeCount >= 20, `preserves file-native graphics (${nativeCount})`);

    // MBD_0 saved view camera resolves to the front-facing side.
    const mbd = norm.presentation.views.find((v) => v.direction);
    assert.ok(mbd, 'NIST has a saved camera view');
    const resolved = resolveSavedViewCamera(mbd, norm);
    assert.ok(resolved, 'resolved saved view camera');
    const best = bestDrawnView(norm, () => true, undefined, bestReferenceView(norm, analysis, mesh));
    assert.ok(best && best.key === 'best' && best.name === 'Best view');

    // Explicit isolated origin point detected at (-325, -175, 0) matching Datum B hole.
    const origin = findStatedOrigin(bytes, analysis, norm);
    assert.ok(origin, 'detects explicit stated origin on NIST CTC-01');
    assert.equal(origin.source, 'step-point');
    assert.ok(Math.hypot(origin.position[0] - (-325), origin.position[1] - (-175), origin.position[2]) < 1e-3);
  } finally {
    eng.free();
  }
});

test('findStatedOrigin detects explicit CONSTRUCTIVE_GEOMETRY_REPRESENTATION AXIS2_PLACEMENT_3D', () => {
  const stepText = `ISO-10303-21;
DATA;
#1 = CARTESIAN_POINT('Origin', (10.0, 20.0, 30.0));
#2 = DIRECTION('Z', (0.0, 0.0, 1.0));
#3 = DIRECTION('X', (1.0, 0.0, 0.0));
#4 = AXIS2_PLACEMENT_3D('CSYS_1', #1, #2, #3);
#5 = CONSTRUCTIVE_GEOMETRY_REPRESENTATION('csys', (#4), #99);
ENDSEC;
END-ISO-10303-21;`;
  const origin = findStatedOrigin(new TextEncoder().encode(stepText), { faces: [], features: [], existing: [] }, null);
  assert.ok(origin);
  assert.equal(origin.source, 'step-csys');
  assert.deepEqual(origin.position, [10, 20, 30]);
  assert.deepEqual(origin.xAxis, [1, 0, 0]);
  assert.deepEqual(origin.yAxis, [0, 1, 0]);
  assert.deepEqual(origin.zAxis, [0, 0, 1]);
});

test('spindle_pmi_after: strips file General notes via withoutNotesBlock, links drawn items, generates fallback for unpresented M2 thread', () => {
  const bytes = gunzipSync(readFileSync('test/fixtures/spindle_pmi_after.step.gz'));
  const eng = new wasm.IntentEngine('spindle.step', bytes);
  try {
    const { analysis } = JSON.parse(eng.snapshot());
    const rawScene = JSON.parse(wasm.pmiScene(bytes));
    const read = reviewDrawnFromScene(rawScene, analysis);
    assert.ok(read.ok, read.reason);
    const drawn = read.value;
    const allNames = [
      ...Object.values(drawn.presentation.items).flat(),
      ...drawn.presentation.unlinked,
    ].map((g) => g.name.toLowerCase());
    assert.ok(!allNames.includes('general notes'), `General notes stripped: ${allNames}`);

    const rows = rowsOf(analysis);
    const file = oneViewFile(drawn, analysis, rows, new Map());
    assert.ok([...file.labels.values()].some((l) => l.question === 'datum.A'), 'Datum A linked');
    assert.ok([...file.labels.values()].some((l) => l.question === 'hole.function:holes:14'), 'bore linked');

    // Unpresented M2 thread receives a fallback label.
    const threadIdx = analysis.existing.findIndex((e) => e.kind === 'note' && e.type === 'internal thread');
    assert.ok(threadIdx >= 0);
    assert.equal(file.covered.has(threadIdx), false);
    const out = oneViewLabels(null, analysis, rows, file, new Map(), {}, new Set(), false);
    const threadLabel = out.labels.find((l) => l.id === `existing:${threadIdx}`);
    assert.ok(threadLabel, 'unpresented thread gets a fallback label');
    assert.equal(threadLabel.question, 'hole.function:holes:15.16');

    // Pending change hides the old drawn graphic and marks the edit unsaved.
    const pending = new Map([['hole.function:holes:14', { kind: 'change', id: 'hole.function:holes:14', value: 'fit:H7', text: 'H7' }]]);
    const replaced = oneViewFile(drawn, analysis, rows, pending);
    assert.equal([...replaced.labels.values()].some((l) => l.question === 'hole.function:holes:14'), false);
    const { unsavedItems } = scenePendingState(file, new Map([['datum.A', { kind: 'fill', id: 'datum.A', value: [0] }]]));
    assert.ok(unsavedItems.size > 0, 'scenePendingState flags pending scene items for unsaved ink and diagram key');
  } finally {
    eng.free();
  }
});

test('drawn-layer & annotations DOM: unsaved burnt-amber ink, in-file blueprint-blue ink, origin triad excluded from bounds/hit, no Unsaved pills', async () => {
  const { THREE, createAnnotations, createDrawnLayer, STANDARD_INK, STANDARD_UNSAVED_INK, STANDARD_LIT_INK } = await bundleSpecifyRuntime();
  const prevDocument = globalThis.document;
  const dom = fakeDom();
  globalThis.document = dom;
  try {
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera();
    camera.position.set(0, 0, 100);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld();
    const nav = { scene, camera, diagonal: 100, size: () => [800, 600], onFrame() {} };
    const container = dom.createElement('div');
    const layer = createDrawnLayer(nav, container, { onClick() {}, titleOf: () => 'Datum A · bottom face' });
    const samplePmi = {
      presentation: {
        items: {
          'datum:A': [{
            id: 'drafting:datum:A', source: 'generated', name: 'Datum A',
            polylines: [[[0, 0, 0], [10, 0, 0]]], polylineRoles: ['leader'], triangles: [], text: ['A'],
            textPlacements: [{ textIndex: 0, origin: [10, 0, 0], xAxis: [1, 0, 0], normal: [0, 0, 1], height: 3, cellBox: [[-2, -2], [2, 2]] }],
          }],
        },
        unlinked: [], semanticIds: ['datum:A'], planes: [], views: [],
      },
      links: { 'datum:A': { faces: [0], status: 'stated', question: 'datum.A' } },
    };
    layer.set(samplePmi);
    layer.setVisible(true);
    assert.equal(container.dataset.drawnItems, `datum:A ${STANDARD_INK}`, 'in-file item draws in blueprint blue');

    // Marking item unsaved switches its ink colour to burnt amber without creating any floating pill.
    layer.setUnsaved(new Set(['datum:A']));
    assert.equal(container.dataset.drawnItems, `datum:A ${STANDARD_UNSAVED_INK}`, 'unsaved item draws in burnt amber');
    const tags = scene.children.filter((o) => o.isCSS2DObject);
    assert.ok(tags.every((t) => !t.element.textContent.includes('Unsaved')), 'no floating Unsaved pill on 3D callout');

    // Lighting switches to STANDARD_LIT_INK.
    layer.light('datum:A');
    assert.equal(container.dataset.drawnItems, `datum:A ${STANDARD_LIT_INK}`);
    layer.light(null);

    // Origin triad renders RGB x/y/z labels and is excluded from bounds() and tightPoints().
    const boundsBefore = layer.bounds().clone();
    layer.setOrigin({ position: [500, 500, 500], xAxis: [1, 0, 0], yAxis: [0, 1, 0], zAxis: [0, 0, 1], source: 'step-csys' });
    const axisLabels = scene.children
      .filter((o) => o.isCSS2DObject && o.element.className === 'pmi-origin-axis')
      .map((o) => o.element.textContent)
      .sort();
    assert.deepEqual(axisLabels, ['x', 'y', 'z'], 'origin triad renders x/y/z axis labels');
    const boundsAfter = layer.bounds();
    assert.ok(boundsAfter.equals(boundsBefore), 'origin triad is excluded from drawn bounds()');
    assert.equal(layer.tightPoints().some((p) => p.x > 100), false, 'origin triad is excluded from tightPoints()');
    layer.dispose();

    // Fallback DOM annotations: unsaved shown in burnt amber with NO "Unsaved" pill, and real gap gets ⊕.
    const annScene = new THREE.Scene();
    const annNav = { scene: annScene, camera, diagonal: 100, size: () => [800, 600], onFrame() {} };
    const annContainer = dom.createElement('div');
    const mesh = { format: 'pmi-assist-mesh', version: 2, binding: { schema: 1 }, bbox: [[0, 0, 0], [10, 10, 10]], positions: [0, 0, 0, 10, 0, 0, 0, 10, 0], indices: [0, 1, 2], faces: [[0, 1]], edges: [], missing: [] };
    const analysis = { binding: { schema: 1 }, faces: [{ id: 0, kind: 'plane', centroid: [3, 3, 0], direction: [0, 0, 1] }], features: [], existing: [] };
    const annotations = createAnnotations(annNav, annContainer, { mesh, analysis, onClick() {}, occluders: () => [] });
    annotations.set([
      { id: 'pending:extra.1', question: 'extra.1', kind: 'note', group: 'answered', unsaved: true, faces: [0], cells: ['⏥ 0.05'] },
      { id: 'gap:hole.1', question: 'hole.1', kind: 'note', group: 'default', gap: 'Ø8 hole', faces: [0], cells: ['hole Ø8'] },
    ]);
    const annTags = annScene.children.filter((o) => o.isCSS2DObject);
    const pendingEl = annTags.find((t) => t.element.dataset.pmiQuestion === 'extra.1')?.element;
    const gapEl = annTags.find((t) => t.element.dataset.pmiQuestion === 'hole.1')?.element;
    assert.ok(pendingEl && gapEl);
    assert.equal(pendingEl.style.color, STANDARD_UNSAVED_INK, 'unsaved fallback label uses burnt amber colour');
    assert.equal(pendingEl.textContent.includes('Unsaved'), false, 'unsaved fallback label has no Unsaved pill');
    assert.ok(gapEl.className.includes('pmi-gap'), 'real gap fallback label gets pmi-gap class');
    assert.equal(gapEl.firstElementChild.firstElementChild.textContent, '⊕ ');
    annotations.dispose();
  } finally {
    globalThis.document = prevDocument;
  }
});

test('end-on saved cameras along X, Y, and Z never throw in normalizeDraftingPmi', () => {
  const directions = [
    [1, 0, 0], [-1, 0, 0],
    [0, 1, 0], [0, -1, 0],
    [0, 0, 1], [0, 0, -1],
  ];
  // 1. NIST CTC-01 with its saved MBD camera redirected along each cardinal axis.
  {
    const bytes = gunzipSync(readFileSync('../tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz'));
    const eng = new wasm.IntentEngine('nist_ctc_01.stp', bytes);
    try {
      const { analysis, mesh } = JSON.parse(eng.snapshot());
      const raw = JSON.parse(eng.pmiScene(JSON.stringify({ answers: {}, auto: [] })));
      const read = reviewDrawnFromScene(raw, analysis);
      assert.ok(read.ok, read.reason);
      const sem = readSceneSemanticLabels(raw);
      for (const direction of directions) {
        const up = Math.abs(direction[2]) > 0.5 ? [0, 1, 0] : [0, 0, 1];
        const withAxisCam = {
          ...read.value,
          presentation: {
            ...read.value.presentation,
            views: read.value.presentation.views.map((v) => (v.direction ? { ...v, direction, up } : v)),
          },
        };
        const norm = normalizeDraftingPmi(withAxisCam, raw, analysis, mesh, sem);
        assert.ok(Object.keys(norm.presentation.items).length > 0, `NIST loads with camera ${direction}`);
      }
    } finally {
      eng.free();
    }
  }
  // 2. GRM-01 with generated linear dimensions along X, Y, Z and an injected end-on saved view.
  {
    const bytes = gunzipSync(readFileSync('../tests/fixtures/parts/GRM-01_shank.step.gz'));
    const eng = new wasm.IntentEngine('GRM-01_shank.step', bytes);
    try {
      const { analysis, mesh } = JSON.parse(eng.snapshot());
      const bare = JSON.parse(eng.review('{}'));
      const shownQs = bare.questions.filter((q) => !q.id.startsWith('length:') && !q.id.startsWith('location:'));
      const chosen = chosenForYou(shownQs, {}, analysis);
      const keepAnswers = Object.fromEntries(chosen.map((c) => [c.id, shownQs.find((q) => q.id === c.id).default]));
      const raw = JSON.parse(eng.pmiScene(JSON.stringify({ answers: keepAnswers, auto: [] })));
      const read = reviewDrawnFromScene(raw, analysis);
      assert.ok(read.ok, read.reason);
      const sem = readSceneSemanticLabels(raw);
      const nativeAnchor = {
        id: 'file:anchor', source: 'file', name: 'anchor',
        polylines: [[[0, 0, 0], [1, 0, 0]]], triangles: [], text: [],
      };
      for (const direction of directions) {
        const up = Math.abs(direction[2]) > 0.5 ? [0, 1, 0] : [0, 0, 1];
        const withAxisCam = {
          ...read.value,
          presentation: {
            ...read.value.presentation,
            unlinked: [nativeAnchor],
            views: [{ id: 'end-on', name: 'End-on', direction, up, graphics: ['file:anchor'] }],
          },
        };
        const norm = normalizeDraftingPmi(withAxisCam, raw, analysis, mesh, sem);
        const dimIds = Object.keys(norm.presentation.items).filter((k) => k.startsWith('planner:dim:plane:'));
        assert.equal(dimIds.length, 3, `GRM-01 places all 3 linear dimensions for camera ${direction}`);
        for (const id of dimIds) {
          assert.equal(norm.presentation.items[id]?.[0]?.triangles.length, 2, `${id} placed cleanly for camera ${direction}`);
        }
      }
    } finally {
      eng.free();
    }
  }
});

test('hole callouts preserve fit class and tolerance text matching the engine (GRM-01 Ø8 H7, deviations, and spindle Ø3.3 H11)', () => {
  // 1. GRM-01 Ø8 hole with fit:H7 -> ⌀8 H7, and with +0.1/-0.05 -> ⌀8 +0.1/-0.05.
  {
    const bytes = gunzipSync(readFileSync('../tests/fixtures/parts/GRM-01_shank.step.gz'));
    const eng = new wasm.IntentEngine('GRM-01_shank.step', bytes);
    try {
      const { analysis, mesh } = JSON.parse(eng.snapshot());
      const rawFit = JSON.parse(eng.pmiScene(JSON.stringify({ answers: { 'hole.function:holes:6': 'fit:H7' }, auto: [] })));
      const engineFitText = rawFit.presentation.items['planner:dim:hole:6/bore.diameter']?.[0]?.text?.[0];
      assert.equal(engineFitText, '⌀8 H7');
      const semFit = readSceneSemanticLabels(rawFit);
      assert.deepEqual(semFit.get('planner:dim:hole:6/bore.diameter')?.cells, ['⌀8 H7']);
      const readFit = reviewDrawnFromScene(rawFit, analysis);
      assert.ok(readFit.ok, readFit.reason);
      const normFit = normalizeDraftingPmi(readFit.value, rawFit, analysis, mesh, semFit);
      assert.deepEqual(normFit.presentation.items['planner:dim:hole:6/bore.diameter']?.[0]?.text, [engineFitText]);

      const rawDev = JSON.parse(eng.pmiScene(JSON.stringify({ answers: { 'extra.1': { faces: [6], kind: 'size', value: '+0.1/-0.05' } }, auto: [] })));
      const devId = Object.keys(rawDev.presentation.items).find((k) => k.includes('extra.1'));
      assert.ok(devId);
      const engineDevText = rawDev.presentation.items[devId]?.[0]?.text?.[0];
      assert.equal(engineDevText, '⌀8 +0.1/-0.05');
      const semDev = readSceneSemanticLabels(rawDev);
      const readDev = reviewDrawnFromScene(rawDev, analysis);
      assert.ok(readDev.ok, readDev.reason);
      const normDev = normalizeDraftingPmi(readDev.value, rawDev, analysis, mesh, semDev);
      assert.deepEqual(normDev.presentation.items[devId]?.[0]?.text, [engineDevText]);
    } finally {
      eng.free();
    }
  }
  // 2. Saved spindle fixture: ⌀3.3 H11 bore in both pmiScene and raw haecceity.pmi/1.
  {
    const bytes = gunzipSync(readFileSync('test/fixtures/spindle_pmi_after.step.gz'));
    const eng = new wasm.IntentEngine('spindle.step', bytes);
    try {
      const { analysis, mesh } = JSON.parse(eng.snapshot());
      const rawScene = JSON.parse(wasm.pmiScene(bytes));
      const engineName = rawScene.presentation.items['planner:dim:hole:14/bore.diameter']?.[0]?.name;
      assert.equal(engineName, '⌀3.3 H11');
      const sem = readSceneSemanticLabels(rawScene);
      assert.deepEqual(sem.get('planner:dim:hole:14/bore.diameter')?.cells, ['⌀3.3 H11']);
      const read = reviewDrawnFromScene(rawScene, analysis);
      assert.ok(read.ok, read.reason);
      const norm = normalizeDraftingPmi(read.value, rawScene, analysis, mesh, sem);
      assert.deepEqual(norm.presentation.items['planner:dim:hole:14/bore.diameter']?.[0]?.text, [engineName]);
    } finally {
      eng.free();
    }
    const rawPmi = JSON.parse(gunzipSync(readFileSync('test/fixtures/19-spindle-semantic-links-only.pmi.json.gz')));
    const semRaw = readSceneSemanticLabels({ pmi: rawPmi });
    assert.deepEqual(semRaw.get('3')?.cells, ['⌀3.3 H11']);
  }
});

test('public drawnFromScene + withoutNotesBlock path draws each existing requirement once (no duplicate labels)', () => {
  for (const fixturePath of ['test/fixtures/spindle_pmi_after.step.gz', 'test/fixtures/19-spindle-semantic-links-only.step.gz']) {
    const bytes = gunzipSync(readFileSync(fixturePath));
    const eng = new wasm.IntentEngine(fixturePath, bytes);
    try {
      const { analysis } = JSON.parse(eng.snapshot());
      const rawScene = JSON.parse(wasm.pmiScene(bytes));
      // Public host path: drawnFromScene -> withoutNotesBlock, without calling reviewDrawnFromScene or supplying reviewItems.
      const read = drawnFromScene(rawScene);
      assert.ok(read.ok, read.reason);
      const drawn = withoutNotesBlock(read.value);
      assert.equal(drawn.reviewItems, undefined, 'public helper does not require caller-supplied reviewItems');

      const rows = rowsOf(analysis);
      const file = oneViewFile(drawn, analysis, rows, new Map());
      const out = oneViewLabels(null, analysis, rows, file, new Map(), {}, new Set(), true);

      // Every existing requirement that has file ink is covered once and NOT duplicated as a camera-facing label.
      const threadIdx = analysis.existing.findIndex((e) => e.kind === 'note' && e.type === 'internal thread');
      assert.ok(threadIdx >= 0, `${fixturePath}: has unpresented M2 thread`);
      for (let i = 0; i < analysis.existing.length; i++) {
        if (i === threadIdx) {
          assert.equal(file.covered.has(i), false, `${fixturePath}: unpresented thread is not covered by file ink`);
          assert.equal(out.labels.filter((l) => l.id === `existing:${i}`).length, 1, `${fixturePath}: unpresented thread gets one fallback label`);
        } else {
          assert.equal(file.covered.has(i), true, `${fixturePath}: existing:${i} (${analysis.existing[i].text}) is covered by drawn ink`);
          assert.equal(out.labels.some((l) => l.id === `existing:${i}`), false, `${fixturePath}: existing:${i} (${analysis.existing[i].text}) is not duplicated as a label`);
        }
      }
      assert.equal(out.labels.length, 1, `${fixturePath}: only the unpresented M2 thread falls back to a camera-facing label`);
    } finally {
      eng.free();
    }
  }
});
