// Release-package refusals and recovery, alongside scripts/wasm-parity.mjs.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import init, { IntentEngine, review, acceptDefaults, interpret, answerEdit, tolerancingEdit, openedTolerancing } from '../crates/specify-pmi-wasm/pkg/specify_pmi_wasm.js';
await init({ module_or_path: readFileSync(new URL('../crates/specify-pmi-wasm/pkg/specify_pmi_wasm_bg.wasm', import.meta.url)) });
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
    refuses('review: malformed answers', () => engine.review(text), /Invalid answers/);
    refuses('free defaults: malformed answers', () => acceptDefaults(analysis, text), /Invalid answers/);
    // An edit list that is not one is refused as a result, never thrown, and writes nothing.
    for (const method of ['edit', 'checkEdits']) {
      const out = JSON.parse(engine[method](text));
      assert.equal(out.written, false, `${method}: malformed list writes nothing`);
      assert.ok(out.failures.length > 0, `${method}: malformed list is named`);
    }
  }
  for (const answers of [
    { missing: 'steel' },
    { 'datum.A.control': 'none' },
    { 'datum.D.flatness': 'none' },
    { [hole.id.replace('hole.function:', 'diameter.fit:')]: 'general' },
    { 'hole.function:missing': 'general' },
    { [hole.id]: 'tapped:clearance M6' },
    { [hole.id]: 'clearance tapped M6' },
    { [hole.id]: 42 },
    { 'part.notes': null },
    { 'part.notes': ['ok', 42] },
    { 'datum.A': [999999] },
    { 'extra.bad': { faces: [999999], kind: 'flatness', value: '0.05' } },
  ]) {
    refuses('review: hostile answer', () => engine.review(JSON.stringify(answers)), /answer|feature|face|text|read|tabulated|either|Choose|Type|datum/i);
    refuses('free defaults: hostile answer', () => acceptDefaults(analysis, JSON.stringify(answers)), /answer|feature|face|text|read|tabulated|either|Choose|Type|datum/i);
    // The same values as fills: every one refused, all or nothing, nothing written.
    const out = JSON.parse(engine.edit(JSON.stringify({ fill: answers })));
    assert.equal(out.written, false, `edit: hostile fill ${JSON.stringify(answers)} writes nothing`);
    assert.equal(out.bytes, undefined);
  }
  for (const bad of ['{', 'null', '[]', '{}', '{"answers":42}', '{"answers":{},"auto":[42]}', '{"answers":{},"auto":{}}', '{"answers":{},"gdt":"invalid"}']) {
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
  for (const method of ['review', 'edit', 'checkEdits', 'settle']) {
    refuses(`${method}: oversized JSON`, () => engine[method](large), /exceeds.*limit/);
  }
  refuses('free analysis: oversized JSON', () => review(large, '{}'), /exceeds.*limit/);
  refuses('oversized interpretation', () => interpret(analysis, '{}', hole.id, large), /exceeds.*limit/);
  refuses('free defaults: oversized answers', () => acceptDefaults(analysis, large), /exceeds.*limit/);
  refuses('oversized payload name', () => engine.payload(' '.repeat(64 * 1024 + 1)), /exceeds.*limit/);
  refuses('oversized project', () => engine.edit('{}', large), /exceeds.*limit/);
  refuses('oversized payload', () => engine.edit('{}', null, large), /exceeds.*limit/);
  refuses('oversized saved answers', () => engine.rematchAnswers(large), /exceeds.*limit/);
  refuses('oversized evidence', () => engine.rematchAnswers('{}', large), /exceeds.*limit/);
  refuses('oversized automatic answers', () => engine.rematchAnswers('{}', null, large), /exceeds.*limit/);
  refuses('oversized STEP', () => new IntentEngine('large.step', new Uint8Array(64 * 1024 * 1024 + 1)), /exceeds.*limit/);
  // These values are deliberately accepted by rules/py.rs, not malformed answers.
  let compatible = 0;
  for (const id of ['part.material', 'part.coating', 'part.heat_treatment']) {
    for (const value of [null, '', 42, false, 0, [], {}, ['legacy']]) {
      const answers = { [id]: value };
      const text = JSON.stringify(answers);
      const result = JSON.parse(engine.review(text));
      if (value === null || value === '' || value === false || value === 0 ||
          (typeof value === 'object' && Object.keys(value).length === 0)) {
        assert.ok(!Object.hasOwn(result.intent.part, id.slice(5)), `${id}: cleared answer`);
      } else assert.deepEqual(result.intent.part[id.slice(5)], value);
      assert.deepEqual(JSON.parse(acceptDefaults(analysis, text))[id], value);
      const legacy = { answers, auto: ['missing', 'missing', id], gdt: null };
      const legacyJson = JSON.stringify(legacy);
      // State's serde contract omits gdt when None, even when input explicitly says null.
      const expectedState = { answers, auto: legacy.auto };
      assert.deepEqual(JSON.parse(answerEdit(legacyJson, text)), expectedState);
      assert.deepEqual(JSON.parse(openedTolerancing(legacyJson)), expectedState);
      const edited = JSON.parse(tolerancingEdit(legacyJson, 'geometric', text));
      assert.deepEqual(edited, { answers: {}, auto: [], gdt: 'geometric' });
      const settled = JSON.parse(engine.settle(legacyJson));
      assert.deepEqual(settled.review, JSON.parse(engine.review(JSON.stringify(settled.state.answers))), 'settle reuses review with identical anchors');
      assert.deepEqual(settled.state.answers[id], value);
      assert.equal(engine.snapshot(), snapshot);
      compatible++;
    }
  }
  console.log(`${compatible} legacy text/state cases: review, defaults, settle and editor transforms passed`);
  assert.ok(JSON.parse(acceptDefaults(analysis, '{}')));
  assert.ok(JSON.parse(engine.edit('{"fill": {"part.material": "steel"}}')).bytes.length > 0);
  console.log(`${count} hostile-input refusals: thrown strings; next valid review and identical snapshot on the same engine`);
} finally { engine.free(); }
// Also exercise allocation, writer verification and disposal after the refusals.
const reopened = new IntentEngine('plate.step', bytes);
try { assert.ok(JSON.parse(reopened.edit('{"fill": {"part.material": "steel"}}')).bytes.length > 0); }
finally { reopened.free(); }
