// For #87: exercise the shipped WASM rematchAnswers, including moved face indices.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import init, { IntentEngine, diff } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';

const root = new URL('../', import.meta.url);
await init({ module_or_path: readFileSync(new URL('crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', root)) });
const bytes = revision => gunzipSync(readFileSync(new URL(`tests/fixtures/revisions/named_plate.${revision}.step.gz`, root)));
const name = (feature, n) => `f:named_plate.${feature}.${n};M:named_plate.body`;
const hole = name('body_make_cylinder1', 1);
const bottom = name('body_make_box1', 5);
function face(engine, name) {
  return JSON.parse(engine.snapshot()).analysis.faces.find(f => f.name === name).id;
}
function answers(engine, unnamedBottom) {
  const top = face(engine, name('body_make_box1', 6));
  const base = unnamedBottom ?? face(engine, bottom);
  return {
    'part.material': 'steel',
    [`hole.function:holes:${face(engine, hole)}`]: 'fit:H7',
    'datum.A': [base], 'datum.A.flatness': '0.03',
    'extra.1': { kind: 'parallelism', faces: [top, base], datums: ['A'], value: '0.1' },
    'extra.2': { kind: 'distance', faces: [top], reference: [base], value: '±0.05' },
  };
}
function renamed(bytes, from, to) {
  const source = bytes.toString();
  const literal = `'${from}'`;
  assert.equal(source.split(literal).length, 2, 'one authored face name in the source');
  return Buffer.from(source.replace(literal, `'${to}'`));
}
const oldBytes = bytes('old'), newBytes = bytes('new');
const old = new IntentEngine('old.step', oldBytes), next = new IntentEngine('new.step', newBytes);
try {
  const given = answers(old), expected = answers(next);
  assert.notDeepEqual(given, expected);
  const evidence = old.evidence();
  const automatic = Object.keys(given);
  const rematched = JSON.parse(next.rematchAnswers(JSON.stringify(given), evidence, JSON.stringify(automatic)));
  assert.deepEqual(rematched.answers, expected);
  assert.equal(rematched.unresolved, undefined);
  assert.deepEqual(rematched.automatic, Object.keys(expected));
  const report = JSON.parse(diff('old.step', oldBytes, 'new.step', newBytes, JSON.stringify(given), 'Named rebuild')).report;
  assert.deepEqual(report.answers, expected);
  assert.deepEqual(report.unresolved, []);
  for (const [changed, reason] of [
    [renamed(oldBytes, hole, ''), /not in this revision/],
    [renamed(oldBytes, name('body_make_cylinder2', 1), hole), /ambiguous face name/],
  ]) {
    const engine = new IntentEngine('changed.step', changed);
    try {
      const out = JSON.parse(engine.rematchAnswers(JSON.stringify(given), evidence));
      assert.equal(out.unresolved.length, 1);
      assert.match(out.unresolved[0].reason, reason);
    } finally { engine.free(); }
  }
  // Shared source labels are not saved identities and never refuse a fresh write.
  const sharedBytes = renamed(renamed(oldBytes, hole, 'f:shared.class'), name('body_make_cylinder2', 1), 'f:shared.class');
  const shared = new IntentEngine('shared.step', sharedBytes);
  try {
    const evidence = JSON.parse(shared.evidence());
    assert.equal(evidence.schema, 3);
    assert.equal(evidence.face_names[face(old, hole)], null);
    assert.equal(evidence.face_names[face(old, name('body_make_cylinder2', 1))], null);
    assert.ok(evidence.geometry);
    const written = JSON.parse(shared.writePmi(JSON.stringify(given)));
    const back = new IntentEngine('shared-written.step', Uint8Array.from(written.bytes));
    try { assert.deepEqual(JSON.parse(back.snapshot()).analysis.resume.answers, given); }
    finally { back.free(); }
  } finally { shared.free(); }
  let genericBytes = oldBytes;
  for (const [i, f] of JSON.parse(old.snapshot()).analysis.faces.entries()) {
    genericBytes = renamed(genericBytes, f.name, i === 0 ? '24' : i === 1 ? 'Fill.1' : 'PartBody');
  }
  const generic = new IntentEngine('generic.step', genericBytes);
  try {
    const evidence = JSON.parse(generic.evidence());
    assert.equal(evidence.schema, 2);
    assert.equal(evidence.face_names, undefined);
    assert.ok(JSON.parse(generic.snapshot()).analysis.faces.every(f => f.name === undefined));
    const written = JSON.parse(generic.writePmi(JSON.stringify(given)));
    const back = new IntentEngine('generic-written.step', Uint8Array.from(written.bytes));
    try { assert.deepEqual(JSON.parse(back.snapshot()).analysis.resume.answers, given); }
    finally { back.free(); }
  } finally { generic.free(); }
  const mixedOld = new IntentEngine('mixed-old.step', renamed(oldBytes, bottom, ''));
  const mixedNew = new IntentEngine('mixed-new.step', renamed(newBytes, bottom, ''));
  try {
    const a = answers(mixedOld, face(old, bottom));
    const expected = answers(mixedNew, face(next, bottom));
    const out = JSON.parse(mixedNew.rematchAnswers(JSON.stringify(a), mixedOld.evidence()));
    assert.deepEqual(out.answers, expected);
    assert.equal(out.unresolved, undefined);
  } finally { mixedOld.free(); mixedNew.free(); }
} finally { old.free(); next.free(); }
console.log('STEP face names: WASM rematchAnswers/diff, shifted indices, automatic answers, refusals, shared/generic labels and mixed support passed');
