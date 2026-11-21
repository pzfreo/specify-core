// For #87: STEP face names through the shipped WASM: rematchAnswers (including moved face
// indices), evidence, diff, and edits on shared and generic labels.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import init, { IntentEngine, diff, acceptDefaults } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';

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
// The answers an edit list can fill: those on the feature table's rows (added requirements,
// extra.*, are not feature-and-kinds).
const fills = given => Object.fromEntries(Object.entries(given).filter(([id]) => !id.startsWith('extra.')));
function stated(engine, filled) {
  const rows = JSON.parse(engine.table()).rows;
  for (const id of Object.keys(filled)) assert.equal(rows.find(r => r.id === id)?.state, 'stated', id);
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
  // Diff the files' PMI only and carries no answers (ADR P11).
  const report = JSON.parse(diff('old.step', oldBytes, 'new.step', newBytes, 'Named rebuild')).report;
  assert.equal(report.answers, undefined);
  assert.equal(report.unresolved, undefined);
  // Shared source labels are not saved identities and never refuse an edit.
  const sharedBytes = renamed(renamed(oldBytes, hole, 'f:shared.class'), name('body_make_cylinder2', 1), 'f:shared.class');
  const shared = new IntentEngine('shared.step', sharedBytes);
  try {
    const evidence = JSON.parse(shared.evidence());
    assert.equal(evidence.schema, 3);
    assert.equal(evidence.face_names[face(old, hole)], null);
    assert.equal(evidence.face_names[face(old, name('body_make_cylinder2', 1))], null);
    assert.ok(evidence.geometry);
    const written = JSON.parse(shared.edit(JSON.stringify({ fill: fills(given) })));
    assert.ok(written.written, written.message);
    const back = new IntentEngine('shared-written.step', Uint8Array.from(written.bytes));
    try { stated(back, fills(given)); }
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
    const written = JSON.parse(generic.edit(JSON.stringify({ fill: fills(given) })));
    assert.ok(written.written, written.message);
    const back = new IntentEngine('generic-written.step', Uint8Array.from(written.bytes));
    try { stated(back, fills(given)); }
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
console.log('STEP face names: WASM rematchAnswers/diff, shifted indices, automatic answers, refusals, evidence, and edits on shared/generic labels passed');


// The split-bottom workflow must stay safe through the actual JS/WASM boundary too.
const split = which => gunzipSync(readFileSync(new URL(`tests/fixtures/revisions/plate_split.${which}.step.gz`, root)));
const beforeBytes = split('old'), afterBytes = split('new');
const before = new IntentEngine('plate_v1.step', beforeBytes);
const after = new IntentEngine('plate_v2.step', afterBytes);
try {
  const given = { 'part.material': '6061-T6', 'datum.A': [4], 'datum.B': [1], 'datum.C': [0] };
  // An answers file carried with its datum dropped, as an earlier diff returned one.
  const carried = { 'part.material': given['part.material'],
    _specify: { evidence: JSON.parse(after.evidence()), unresolved: { 'datum.A': 'face 4 has no match in the new part' } } };
  const blocked = JSON.stringify(carried);
  const question = JSON.parse(after.review(blocked)).questions.find(q => q.id === 'datum.A');
  assert.equal(question.default, null);
  assert.equal(question.attention, 'required');
  assert.equal(question.reason, `carried answer could not be applied: ${carried._specify.unresolved['datum.A']}`);
  const incomplete = e => typeof e === 'string' && /datum\.A \(carried answer could not be applied: face 4 has no match/.test(e);
  const analysis = JSON.stringify(JSON.parse(after.snapshot()).analysis);
  assert.throws(() => acceptDefaults(analysis, blocked), incomplete);
  carried['datum.A'] = [4, 8];
  assert.deepEqual(JSON.parse(acceptDefaults(analysis, JSON.stringify(carried)))['datum.A'], [4, 8]);
  // The explicit replacement applied as an edit (a 0.2 answers file reads as fills): datum A on
  // both faces, and the transport not stored.
  const resolved = JSON.parse(after.edit(JSON.stringify(carried)));
  assert.ok(resolved.written, resolved.message);
  const back = new IntentEngine('resolved.step', Uint8Array.from(resolved.bytes));
  try {
    const datum = JSON.parse(back.table()).rows.find(r => r.id === 'datum.A');
    assert.equal(datum.state, 'stated');
    assert.equal(JSON.parse(back.snapshot()).analysis.resume?.answers?._specify, undefined, 'transport is not stored');
  } finally { back.free(); }
  // Answers saved with the old geometry's evidence: applied to their own revision, refused as
  // stale on the new one, by edit and by checkEdits alike.
  const saved = { ...given, _specify: { evidence: JSON.parse(before.evidence()), unresolved: {} } };
  assert.ok(JSON.parse(before.edit(JSON.stringify(saved))).written, 'their own revision takes them');
  const stale = /stale answers.*another revision/;
  const refused = JSON.parse(after.edit(JSON.stringify(saved)));
  assert.equal(refused.written, false);
  assert.match(refused.message, stale);
  assert.ok(refused.failures.some(f => f.entry === '_specify.evidence' && stale.test(f.reason)), refused.message);
  const checked = JSON.parse(after.checkEdits(JSON.stringify(saved)));
  assert.equal(checked.applicable, false);
  assert.ok(checked.failures.some(f => f.entry === '_specify.evidence' && stale.test(f.reason)), checked.message);
  // Rematched from their embedded evidence, the answers on the dropped face are blocked, so
  // taking the defaults for the rest is refused.
  const rematched = JSON.parse(after.rematchAnswers(JSON.stringify(saved))).answers;
  assert.throws(() => acceptDefaults(analysis, JSON.stringify(rematched)), e => typeof e === 'string' && /carried answer could not be applied/.test(e));
} finally { before.free(); after.free(); }
console.log('Carried answers: WASM blocked defaults, explicit replacement, stale evidence and embedded-evidence rematching passed');
