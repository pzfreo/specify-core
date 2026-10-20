// Release-package refusals and recovery, alongside scripts/wasm-parity.mjs.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import init, { IntentEngine, review, acceptDefaults, interpret, answerEdit, tolerancingEdit, openedTolerancing } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';
await init({ module_or_path: readFileSync(new URL('../crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', import.meta.url)) });
const bytes = readFileSync(new URL('../tests/fixtures/parts/plate.step', import.meta.url));
const engine = new IntentEngine('plate.step', bytes);
let count = 0;
try {
  const snapshot = engine.snapshot();
  const analysis = JSON.stringify(JSON.parse(snapshot).analysis);
  const valid = engine.review('{}');
  const state = JSON.stringify({ answers: {}, auto: [], gdt: null });
  const hole = JSON.parse(valid).questions.find(q => q.id.startsWith('hole.function:'));
  assert.ok(hole);
  function refuses(label, call, pattern) {
    assert.throws(call, error => typeof error === 'string' && pattern.test(error), label);
    assert.equal(engine.review('{}'), valid, `${label}: next valid review on same engine`);
    assert.equal(engine.snapshot(), snapshot, `${label}: session unchanged`);
    count++;
  }
  for (const text of ['', 'not STEP', 'ISO-10303-21;HEADER;ENDSEC;DATA;#1 = ;']) {
    refuses('malformed STEP', () => new IntentEngine('bad.step', Buffer.from(text)), /STEP|part|parse|expected|end|entity|token/i);
  }
  for (const text of ['{', '{"part.material":', '[]', 'null', '42', '"steel"']) {
    for (const method of ['review', 'acceptDefaults', 'writePmi']) {
      refuses(`${method}: malformed answers`, () => engine[method](text), /Invalid answers/);
    }
  }
  for (const answers of [
    { missing: 'steel' },
    { 'hole.function:missing': 'general' },
    { [hole.id]: 'tapped:clearance M6' },
    { [hole.id]: 'clearance tapped M6' },
    { [hole.id]: 42 },
    { 'part.material': {} },
    { 'part.notes': ['ok', 42] },
    { 'datum.A': [999999] },
    { 'extra.bad': { faces: [999999], kind: 'flatness', value: '0.05' } },
  ]) {
    for (const method of ['review', 'acceptDefaults', 'writePmi']) {
      refuses(`${method}: hostile answer`, () => engine[method](JSON.stringify(answers)), /answer|feature|face|text|read|tabulated|either|Choose|Type|datum/i);
    }
  }
  for (const bad of ['{', 'null', '[]', '{}', '{"answers":42}', '{"answers":{},"auto":[42]}', '{"answers":{},"auto":["missing"]}', '{"answers":{},"gdt":"invalid"}']) {
    refuses('settle: invalid state', () => engine.settle(bad), /Invalid state|answer/);
    refuses('answerEdit: invalid state', () => answerEdit(bad, '{}'), /Invalid state|answer/);
    refuses('tolerancingEdit: invalid state', () => tolerancingEdit(bad, undefined, '{}'), /Invalid state|answer/);
    refuses('openedTolerancing: invalid state', () => openedTolerancing(bad), /Invalid state|answer/);
  }
  refuses('oversized mode', () => tolerancingEdit(state, 'x'.repeat(65537), '{}'), /exceeds.*limit/);
  refuses('oversized name', () => new IntentEngine('x'.repeat(65537), bytes), /exceeds.*limit/);
  refuses('unknown mode', () => tolerancingEdit(state, 'invalid', '{}'), /Invalid mode/);
  refuses('answerEdit: unknown answer', () => answerEdit(state, '{"missing":true}'), /answer/);
  refuses('openedTolerancing: invalid resume', () => openedTolerancing(state, '[]'), /Invalid answers/);
  refuses('unknown question', () => interpret(analysis, '{}', 'missing', 'H7'), /No choice question/);
  for (const bad of ['{', 'null', '[]', '{}', '{"faces":42}', '{"faces":[],"features":[42]}']) {
    refuses('free review: invalid analysis', () => review(bad, '{}'), /Invalid analysis|analysis|face|feature/);
    refuses('free defaults: invalid analysis', () => acceptDefaults(bad, '{}'), /Invalid analysis|analysis|face|feature/);
  }
  // A finite JSON representation may still contain Python-compatible non-finite text.
  const nonfinite = JSON.parse(analysis);
  const feature = nonfinite.features.find(f => f.family === 'holes');
  assert.ok(feature);
  for (const number of ['inf', 'nan', '1e999']) {
    const bad = structuredClone(nonfinite);
    bad.features.find(f => f.id === feature.id).record.diameter = number;
    refuses('non-finite feature', () => review(JSON.stringify(bad), '{}'), /finite|infinity|NaN/);
  }
  // Limits are checked before parsing/cloning the payload inside Rust.
  const large = ' '.repeat(8 * 1024 * 1024 + 1);
  for (const method of ['review', 'acceptDefaults', 'writePmi', 'settle']) {
    refuses(`${method}: oversized JSON`, () => engine[method](large), /exceeds.*limit/);
  }
  refuses('free analysis: oversized JSON', () => review(large, '{}'), /exceeds.*limit/);
  refuses('oversized interpretation', () => interpret(analysis, '{}', hole.id, large), /exceeds.*limit/);
  refuses('oversized project', () => engine.writePmi('{}', large), /exceeds.*limit/);
  refuses('oversized STEP', () => new IntentEngine('large.step', new Uint8Array(64 * 1024 * 1024 + 1)), /exceeds.*limit/);
  assert.ok(JSON.parse(engine.acceptDefaults('{}')));
  assert.ok(JSON.parse(engine.writePmi('{}')).bytes.length > 0);
  console.log(`${count} hostile-input refusals: thrown strings; next valid review and identical snapshot on the same engine`);
} finally { engine.free(); }
// Also exercise allocation, writer verification and disposal after the refusals.
const reopened = new IntentEngine('plate.step', bytes);
try { assert.ok(JSON.parse(reopened.writePmi('{}')).bytes.length > 0); }
finally { reopened.free(); }
