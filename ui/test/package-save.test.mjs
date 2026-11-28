// Saving and rematching from a host that embeds SpecifyReview: the package's `edits` entry builds an edit list,
// the engine's worker `edit` op applies it, the written file reads back with the PMI; a 0.2 answers file saves
// through the same op; `rematch` carries saved answers to a revision. The real worker, bundled and driven
// without a browser (a fake Worker hands messages to its onmessage).
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
import { editList } from '../src/specify/edits/pending.ts';

const { modulePath, wasmPath } = requireEngine();
const dir = mkdtempSync(join(tmpdir(), 'package-save-'));
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
const file = (name, bytes, sourceId = name) => ({ name, bytes: new Uint8Array(bytes), sourceId });
const rowsOf = (bytes) => {
  const engine = new IntentEngine('written.step', bytes);
  try { return Object.fromEntries(JSON.parse(engine.table()).rows.map((r) => [r.id, r])); } finally { engine.free(); }
};

test('an edit list from editList saves through the worker edit op and the written file reads back', async () => {
  const source = file('plate.step', plate);
  const table = await client.table(source);
  assert.ok(table.rows.some((r) => r.id === 'part.material' && r.state === 'gap'), 'the plate has no material yet');
  const list = editList([{ kind: 'fill', id: 'part.material', name: 'Material', value: 'Aluminium 6061', text: 'Aluminium 6061' }]);
  const done = await client.edit(source, { edits: list });
  assert.equal(done.written, true, done.message);
  assert.ok(done.bytes instanceof Uint8Array && done.bytes.length > 0);
  const row = rowsOf(done.bytes)['part.material'];
  assert.equal(row.state, 'stated');
  assert.match(JSON.stringify(row.value), /Aluminium 6061/);
});

test('an edit the file cannot take is refused whole, naming each entry', async () => {
  const refused = await client.edit(file('plate.step', plate), { edits: { fill: { 'part.material': 'Steel', 'no.such:row': 'x' } } });
  assert.equal(refused.written, false);
  assert.ok(refused.failures.length >= 1 && refused.failures.every((f) => f.entry && f.reason));
});

test('a 0.2 answers object through edit writes the PMI the equivalent edit list writes', async () => {
  const source = file('plate.step', plate);
  const answers = { 'part.material': 'Aluminium 6061', 'part.general_tolerance': 'ISO 2768-m' };
  const viaList = await client.edit(source, { edits: { fill: answers } });
  const viaAnswers = await client.edit(source, { edits: answers });
  assert.equal(viaList.written, true, viaList.message);
  assert.equal(viaAnswers.written, true, viaAnswers.message);
  const strip = (rows) => Object.fromEntries(Object.entries(rows).map(([id, r]) => [id, { state: r.state, value: r.value, requirements: (r.requirements ?? []).map((q) => q.text) }]));
  const written = rowsOf(viaAnswers.bytes);
  assert.equal(written['part.material'].state, 'stated');
  assert.equal(written['part.general_tolerance'].state, 'stated');
  assert.deepEqual(strip(rowsOf(viaAnswers.bytes)), strip(rowsOf(viaList.bytes)), 'answers lose nothing against the edit list');
});

test('rematch carries saved answers to a revision of the part', async () => {
  const old = gunzipSync(readFileSync(new URL('../../tests/fixtures/revisions/hole_added.old.step.gz', import.meta.url)));
  const next = gunzipSync(readFileSync(new URL('../../tests/fixtures/revisions/hole_added.new.step.gz', import.meta.url)));
  const { evidence } = await client.request('open', file('part.step', old, 'old'));
  const matched = await client.rematch(file('part.step', next, 'new'), { answers: { 'part.material': 'Steel' }, evidence });
  assert.equal(matched.answers['part.material'], 'Steel');
});
