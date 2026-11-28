// Inspection balloons on the drawn callouts: the numbers are the engine's (the file's own, or for an
// unsaved callout the one Save will write); the layout keeps a fixed slot for each balloon.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { pathToFileURL } from 'node:url';
import { requireEngine } from './engine-path.mjs';
import { drawnBalloons, readSceneSemanticLabels, reviewDrawnFromScene } from '../src/specify/one-view.ts';
import { normalizeDraftingPmi } from '../src/specify/drafting-layout.ts';

const { modulePath, wasmPath } = requireEngine();
const wasm = await import(pathToFileURL(modulePath).href);
await wasm.default({ module_or_path: readFileSync(wasmPath) });

test('a drawn item carries its link\'s number; items with none, and no scene, have no balloon', () => {
  const drawn = { links: { a: { faces: [], number: 4 }, b: { faces: [] }, c: { faces: [], number: 12 } } };
  assert.deepEqual([...drawnBalloons(drawn)], [['a', [4]], ['c', [12]]]);
  assert.equal(drawnBalloons(null).size, 0);
});

// The bracket with a datum frame and the five general dimensions called out, as the editor holds it.
const answers = {
  'part.general_tolerance': 'ISO 2768-m', 'part.edges': 'Deburr only', 'datum.A': [0], 'datum.B': [2], 'datum.C': [8],
  'part.process': 'sheet metal',
  'extra.1': { faces: [6], kind: 'distance', value: '±0.1', reference: [0] },
  'extra.2': { faces: [4], kind: 'distance', value: '±0.1', reference: [2] },
  'extra.3': { faces: [3], kind: 'distance', value: '±0.3', reference: [0] },
  'extra.4': { faces: [7], kind: 'distance', value: '±0.3', reference: [2] },
  'extra.5': { faces: [8], kind: 'distance', value: '±0.3', reference: [9] },
  'datum.A.flatness': 'none', 'datum.B.control': 'none', 'datum.C.control': 'none',
};
const bytes = gunzipSync(readFileSync('../tests/fixtures/parts/sheet-metal/bent-bracket.step.gz'));
const numbers = (scene) => Object.values(scene.specify).flatMap((v) => (typeof v.number === 'number' ? [v.number] : [])).sort((a, b) => a - b);

test('planned numbers: the engine\'s for the unsaved callouts are the numbers Save writes', () => {
  const eng = new wasm.IntentEngine('bent-bracket.step', bytes);
  try {
    const planned = JSON.parse(eng.pmiScene(JSON.stringify({ answers, auto: [] })));
    assert.deepEqual(numbers(planned), [1, 2, 3, 4, 5, 6, 7, 8]);
    const saved = JSON.parse(eng.edit(JSON.stringify({ fill: answers })));
    assert.ok(saved.written);
    const after = JSON.parse(wasm.pmiScene(new Uint8Array(saved.bytes), undefined));
    assert.deepEqual(numbers(after), numbers(planned));
    // Each datum keeps its number and the dimensions carry theirs, item by item.
    for (const letter of ['A', 'B', 'C']) assert.equal(after.specify[`datum:${letter}`].number, planned.specify[`datum:${letter}`].number);
    // The drawn links carry them, so the balloons do.
    const read = reviewDrawnFromScene(planned, JSON.parse(eng.snapshot()).analysis);
    assert.ok(read.ok, read.reason);
    assert.deepEqual([...drawnBalloons(read.value)].map(([, n]) => n[0]).sort((a, b) => a - b), [1, 2, 3, 4, 5, 6, 7, 8]);
  } finally { eng.free?.(); }
});

test('the layout keeps a fixed balloon slot on every callout Specify lays out, whatever the numbers', () => {
  const eng = new wasm.IntentEngine('bent-bracket.step', bytes);
  const lay = (scene, analysis, mesh) => {
    const read = reviewDrawnFromScene(scene, analysis);
    assert.ok(read.ok, read.reason);
    return normalizeDraftingPmi(read.value, scene, analysis, mesh, readSceneSemanticLabels(scene));
  };
  try {
    const { analysis, mesh } = JSON.parse(eng.snapshot());
    const planned = JSON.parse(eng.pmiScene(JSON.stringify({ answers, auto: [] })));
    const before = lay(planned, analysis, mesh);
    const generated = Object.entries(before.presentation.items).filter(([, gs]) => gs.some((g) => g.source === 'generated'));
    assert.ok(generated.length >= 8, 'datums and dimensions are laid out');
    for (const [item, gs] of generated) {
      const slot = gs[0].balloonSlot;
      assert.ok(slot, `${item} has a balloon slot`);
      // The slot's size is the text's, not the number's.
      assert.equal(slot.height, gs[0].textPlacements[0].height);
    }
    // Numbers play no part: the same layout with every number removed from the scene.
    const stripped = JSON.parse(JSON.stringify(planned));
    for (const v of Object.values(stripped.specify)) delete v.number;
    assert.deepEqual(lay(stripped, analysis, mesh).presentation.items, before.presentation.items);
    // After Save the callouts Specify lays out still have their slot.
    const saved = JSON.parse(eng.edit(JSON.stringify({ fill: answers })));
    const eng2 = new wasm.IntentEngine('bent-bracket.specified.step', new Uint8Array(saved.bytes));
    try {
      const snap = JSON.parse(eng2.snapshot());
      const raw = JSON.parse(eng2.pmiScene(JSON.stringify({ answers: snap.analysis.resume?.answers ?? {}, auto: [] })));
      const after = lay(raw, snap.analysis, snap.mesh);
      const laid = Object.values(after.presentation.items).filter((gs) => gs.some((g) => g.source === 'generated'));
      assert.ok(laid.length >= 6 && laid.every((gs) => gs[0].balloonSlot), 'Specify-laid callouts after Save have their slot');
    } finally { eng2.free?.(); }
  } finally { eng.free?.(); }
});

test('every callout is drawn identically before and after Save, as the same Specify layout', () => {
  const eng = new wasm.IntentEngine('bent-bracket.step', bytes);
  const lay = (scene, analysis, mesh) => {
    const read = reviewDrawnFromScene(scene, analysis);
    assert.ok(read.ok, read.reason);
    return normalizeDraftingPmi(read.value, scene, analysis, mesh, readSceneSemanticLabels(scene));
  };
  const round = (v) => JSON.parse(JSON.stringify(v, (_, x) => (typeof x === 'number' ? Math.round(x * 1e6) / 1e6 : x)));
  // What a callout is drawn as: its strokes, fills, text and balloon slot, whatever it is named.
  const drawing = (pmi) => new Map(Object.entries(pmi.presentation.items).flatMap(([item, gs]) => {
    const number = pmi.links[item]?.number;
    return number ? [[number, round(gs.map(({ polylines, polylineRoles, triangles, text, textPlacements, balloonSlot }) => (
      { polylines, polylineRoles, triangles, text, textPlacements, balloonSlot })))]] : [];
  }));
  try {
    const { analysis, mesh } = JSON.parse(eng.snapshot());
    const before = lay(JSON.parse(eng.pmiScene(JSON.stringify({ answers, auto: [] }))), analysis, mesh);
    const saved = JSON.parse(eng.edit(JSON.stringify({ fill: answers })));
    const eng2 = new wasm.IntentEngine('bent-bracket.specified.step', new Uint8Array(saved.bytes));
    try {
      const snap = JSON.parse(eng2.snapshot());
      const raw = JSON.parse(eng2.pmiScene(JSON.stringify({ answers: snap.analysis.resume?.answers ?? {}, auto: [] })));
      const after = lay(raw, snap.analysis, snap.mesh);
      const [was, now] = [drawing(before), drawing(after)];
      assert.deepEqual([...now.keys()].sort(), [...was.keys()].sort(), 'the same numbered callouts');
      assert.equal(was.size, 8);
      for (const [number, geometry] of was) assert.deepEqual(now.get(number), geometry, `callout ${number} is drawn the same`);
      // Specify wrote them, and the file says so.
      for (const item of Object.keys(after.presentation.items)) {
        if (after.links[item]?.number) assert.equal(after.links[item].written, true, `${item} is marked as written by Specify`);
      }
    } finally { eng2.free?.(); }
  } finally { eng.free?.(); }
});
