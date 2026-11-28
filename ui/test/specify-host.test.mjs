// The host composition (useSpecifyHost's core, SpecifyHost): the review props built from the file's table, the one edit
// list a save composes, the save and the re-read, on the real worker (a fake Worker hands messages to its onmessage).
import { test, before } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { pathToFileURL } from 'node:url';
import { build } from 'esbuild';
import { requireEngine } from './engine-path.mjs';
import { IntentEngine } from './engine-fixture.mjs';
import { WorkerClient } from '../src/engine/rpc.ts';
import { SpecifyHost } from '../src/specify/host/specify-host.ts';
import { composeEditList } from '../src/specify/host/compose.ts';
import { PendingEdits } from '../src/specify/edits/pending.ts';

const { modulePath, wasmPath } = requireEngine();
const dir = mkdtempSync(join(tmpdir(), 'specify-host-'));
let client;

before(async () => {
  // The engine as the worker loads it from a host's wasmUrl: its glue, started on the .wasm read from disk.
  const shim = join(dir, 'engine.mjs');
  const glue = JSON.stringify(pathToFileURL(modulePath).href);
  writeFileSync(shim, `import init from ${glue};
import { readFileSync } from 'node:fs';
export * from ${glue};
export default () => init({ module_or_path: readFileSync(${JSON.stringify(wasmPath)}) });\n`);
  const out = join(dir, 'worker.mjs');
  await build({ entryPoints: [new URL('../src/engine/intent.worker.ts', import.meta.url).pathname], bundle: true, format: 'esm', platform: 'node',
    outfile: out, external: ['@draftwright/specify-pmi-wasm'], logLevel: 'silent' });
  const replies = [];
  globalThis.self = { postMessage: (m) => replies.shift()?.(m) };
  await import(pathToFileURL(out).href);
  const worker = {
    postMessage(data) {
      if (data.id !== undefined) replies.push((m) => worker.onmessage({ data: m }));
      globalThis.self.onmessage({ data });
    },
    terminate() {}, onmessage: null, onerror: null, onmessageerror: null,
  };
  client = new WorkerClient(() => worker);
  worker.postMessage({ wasmUrl: pathToFileURL(shim).href });
});


const plate = readFileSync(new URL('../../tests/fixtures/parts/plate.step', import.meta.url));
const pin = readFileSync(new URL('../../tests/fixtures/parts/pin.step', import.meta.url));
const assembly = readFileSync(new URL('../../tests/fixtures/parts/assembly.step', import.meta.url));
const file = (name, bytes, sourceId = name, part) => ({ name, bytes: new Uint8Array(bytes), sourceId, ...(part === undefined ? {} : { part }) });
const rowsOf = (bytes) => {
  const engine = new IntentEngine('written.step', bytes);
  try { return Object.fromEntries(JSON.parse(engine.table()).rows.map((r) => [r.id, r])); } finally { engine.free(); }
};
/** A host on ``f`` with the options a test names, its table read. */
async function hostOn(f, options = {}) {
  const host = new SpecifyHost({ client, file: f, answers: {}, ...options });
  await host.setFile(f);
  return host;
}
const dimension = { id: 'length:1', what: 'the 40 mm width', kind: 'size', faces: [6], reference: [] };

test('the review props come from the file table: stated rows, unit, one view, taken keys', async () => {
  const saved = await client.edit(file('plate.step', plate), { edits: { fill: { 'part.material': 'Aluminium 6061' } } });
  assert.equal(saved.written, true);
  const stated = await hostOn(file('stated.step', saved.bytes));
  const props = stated.reviewProps();
  assert.ok(props, 'props once the table is read');
  assert.match(String(props.stated.values['part.material']), /Aluminium 6061/);
  assert.ok(props.stated.texts['part.material']);
  assert.equal(props.lengthUnit, 'mm');
  assert.ok(props.oneView.rows.some((r) => r.id === 'part.material' && r.state === 'stated'));
  assert.deepEqual(props.takenKeys, []);
  const bare = await hostOn(file('plate.step', plate));
  assert.deepEqual(bare.reviewProps().stated.values, {}, 'a bare plate states nothing');
  assert.equal(bare.reviewProps().calledOutDimensions.size, 0);
});

test('the edit list composes answers, a removal and a call-out; the row\'s last word stands', async () => {
  const saved = await client.edit(file('plate.step', plate), { edits: { fill: { 'part.material': 'Aluminium 6061' } } });
  const host = await hostOn(file('stated.step', saved.bytes), { answers: { 'part.general_tolerance': 'ISO 2768-m', 'part.material': 'Aluminium 6061' } });
  const props = host.reviewProps();
  props.stated.onRemove('part.material');
  assert.equal(host.reviewProps().stated.removed.has('part.material'), true);
  assert.equal(host.reviewProps().callOutDimension(dimension, '0.1'), true);
  const list = await host.compose();
  assert.equal(list.fill['part.general_tolerance'], 'ISO 2768-m');
  assert.ok(list.items.remove.some((t) => typeof t === 'string' || t.id === 'part.material'), 'the bin is a removal');
  const [key] = Object.keys(list.fill).filter((k) => k.startsWith('extra.'));
  assert.ok(key, 'the call-out is an Added by you entry');
  assert.equal(list.fill[key].value, '±0.1');
  assert.deepEqual([...host.reviewProps().calledOutDimensions], [['length:1', '0.1']]);
  assert.deepEqual(host.reviewProps().takenKeys.includes(key), true);
});

test('a call-out reaches the host\'s answers, and taking it back removes it', async () => {
  let answers = { 'part.material': 'Steel' };
  const host = await hostOn(file('plate.step', plate), { answers, onAnswers: (a) => { answers = a; } });
  host.reviewProps().callOutDimension(dimension, '0.1');
  const key = Object.keys(answers).find((k) => k.startsWith('extra.'));
  assert.ok(key && answers['part.material'] === 'Steel');
  host.options.answers = answers;
  host.reviewProps().callOutDimension(dimension, '');
  assert.equal(Object.keys(answers).some((k) => k.startsWith('extra.')), false);
});

test('Save accepting defaults writes what the defaults answer; without it, only what was answered', async () => {
  const plain = await (await hostOn(file('plate.step', plate), { answers: { 'part.material': 'Steel' } })).compose();
  const defaulted = await (await hostOn(file('plate.step', plate), { answers: { 'part.material': 'Steel' }, acceptDefaults: true })).compose();
  assert.deepEqual(Object.keys(plain.fill), ['part.material']);
  assert.ok(Object.keys(defaulted.fill ?? {}).length > 1 || Object.keys(defaulted).some((k) => k !== 'fill'), 'defaults add entries');
  assert.equal(defaulted.fill['part.material'], 'Steel');
});

test('the BOM rule: bom.* entries only when the table has bom.* rows; the host can opt out', async () => {
  const answers = { 'part.material': 'Steel', 'bom.item': '12' };
  const ruled = await (await hostOn(file('plate.step', plate), { answers })).compose();
  assert.equal('bom.item' in ruled.fill, false);
  assert.equal(ruled.fill['part.material'], 'Steel');
  const optedOut = await (await hostOn(file('plate.step', plate), { answers, bom: false })).compose();
  assert.equal(optedOut.fill['bom.item'], '12');
});

test('beforeSave gets the composed list and returns the one sent: an entry it drops is not in the saved file', async () => {
  let seen;
  const answers = { 'part.material': 'Aluminium 6061', 'part.general_tolerance': 'ISO 2768-m' };
  const host = await hostOn(file('plate.step', plate), { answers, beforeSave: (edits) => {
    seen = structuredClone(edits);
    const { 'part.general_tolerance': _, ...fill } = edits.fill;
    return { ...edits, fill };
  } });
  const done = await host.save();
  assert.equal(done.written, true, done.message);
  assert.ok('part.general_tolerance' in seen.fill, 'the hook saw the composed list');
  const rows = rowsOf(done.bytes);
  assert.equal(rows['part.material'].state, 'stated');
  assert.notEqual(rows['part.general_tolerance'].state, 'stated', 'the dropped entry is not in the file');
});

test('Save applies the list, carries project and payload, reads the saved file again and reports', async () => {
  const saved = [];
  const host = await hostOn(file('plate.step', plate), { answers: { 'part.material': 'Aluminium 6061' }, payload: { name: 'test-app', value: { n: 1 } }, onSaved: (o, f) => saved.push([o, f]) });
  assert.deepEqual(host.reviewProps().stated.values, {});
  const done = await host.save();
  assert.equal(done.written, true, done.message);
  assert.equal(saved.length, 1);
  assert.ok(host.getSnapshot().lastReport);
  assert.equal(host.getSnapshot().saving, false);
  assert.match(String(host.reviewProps().stated.values['part.material']), /Aluminium 6061/, 'new table, new props');
  assert.equal(host.pendingEdits().size, 0);
  assert.equal(await host.save(), null, 'answers equal to the file leave nothing to apply');
});

test('Save through a transport of the host\'s own sends the same list; a refusal is reported whole', async () => {
  let sent;
  const via = await hostOn(file('plate.step', plate), { answers: { 'part.material': 'Steel' }, saveVia: async (edits, f) => { sent = edits; return client.edit(f, { edits }); } });
  assert.equal((await via.save()).written, true);
  assert.equal(sent.fill['part.material'], 'Steel');
  const refused = await hostOn(file('plate.step', plate), { answers: { 'no.such:row': 'x' } });
  const outcome = await refused.save();
  assert.equal(outcome.written, false);
  assert.ok(refused.getSnapshot().refused.failures.length >= 1);
});

test('a new file re-reads the table and drops the held edits; the props match the new file', async () => {
  const saved = await client.edit(file('plate.step', plate), { edits: { fill: { 'part.material': 'Aluminium 6061' } } });
  const a = file('a.step', saved.bytes);
  const host = await hostOn(a);
  host.reviewProps().stated.onRemove('part.material');
  assert.equal(host.pendingEdits().size, 1);
  assert.ok(host.reviewProps().stated.values['part.material']);
  const b = file('b.step', plate);
  const reading = host.setFile(b);
  assert.equal(host.reviewProps(), null, 'no props for the old file while the new one is read');
  await reading;
  assert.deepEqual(host.reviewProps().stated.values, {}, 'the stated rows are B\'s');
  assert.equal(host.pendingEdits().size, 0, 'the old file\'s edits are gone');
});

test('a call-out and a bin outlast a part switch, per part, and still save', async () => {
  const parts = await client.request('parts', { name: 'assembly.step', bytes: new Uint8Array(assembly) });
  assert.ok(parts.length >= 2, 'the fixture has two parts');
  const bytes = new Uint8Array(assembly);
  const at = (part) => ({ name: 'assembly.step', bytes, sourceId: `assembly.step#${part}`, part });
  const host = await hostOn(at(0));
  assert.equal(host.reviewProps().callOutDimension(dimension, '0.1'), true);
  assert.equal(host.pendingEdits().size, 1);
  await host.setFile(at(1));
  assert.equal(host.pendingEdits().size, 0, 'part 1 has none of part 0\'s');
  assert.equal(host.reviewProps().calledOutDimensions.size, 0);
  await host.setFile(at(0));
  assert.deepEqual([...host.reviewProps().calledOutDimensions], [['length:1', '0.1']], 'back on part 0 the call-out is still there');
  const list = await host.compose();
  assert.ok(Object.keys(list.fill).some((k) => k.startsWith('extra.')), 'and still saves');
  const done = await host.save();
  assert.equal(done.written, true, done.message);
});

test('the BOM rule keeps bom.* entries when the table has bom.* rows', () => {
  const row = (id) => ({ id, kind: 'bom', feature: 'bom', faces: [], state: 'gap', open: false });
  const read = (rows) => ({ table: { rows, file_pmi: [], decided_none: [] }, bare: [], model: { part: null, features: [], datums: null, other: [] }, analysis: { existing: [] }, stated: { ids: new Set(), values: {}, questions: [], texts: {}, decided: new Set() } });
  const answers = { 'bom.make_buy': 'buy', 'part.material': 'Steel' };
  assert.deepEqual(composeEditList(read([row('bom.make_buy'), row('part.material')]), answers, new PendingEdits()).fill, answers);
  assert.deepEqual(composeEditList(read([row('part.material')]), answers, new PendingEdits()).fill, { 'part.material': 'Steel' });
});
