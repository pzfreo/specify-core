import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import { SESSION_API } from '../app/session-api.ts';

const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));

test('the app build has index.html and the versioned session API manifest at its root', async (t) => {
  const temp = mkdtempSync(join(tmpdir(), 'specify-app-manifest-'));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const app = join(temp, 'app');
  await build({ configFile: fileURLToPath(new URL('../vite.app.config.ts', import.meta.url)),
    logLevel: 'silent', build: { outDir: app } });
  const expected = { name: 'specify-ui-app', version: pkg.version, api: SESSION_API };
  // specify-core's build.rs builds in a dist-app with exactly these at its root, of its own version.
  assert.deepEqual(JSON.parse(readFileSync(join(app, 'specify-ui-app.json'), 'utf8')), expected);
  assert.ok(existsSync(join(app, 'index.html')));
});

/** Run the built worker's message handler with the same WASM glue as the browser. */
async function workerAt(t) {
  const { gunzipSync } = await import('node:zlib');
  const { readdirSync } = await import('node:fs');
  const { pathToFileURL } = await import('node:url');
  const { requireEngine } = await import('./engine-path.mjs');
  const { modulePath, wasmPath } = requireEngine();
  const temp = mkdtempSync(join(tmpdir(), 'specify-app-worker-'));
  t.after(() => { delete globalThis.self; rmSync(temp, { recursive: true, force: true }); });
  await build({ configFile: fileURLToPath(new URL('../vite.app.config.ts', import.meta.url)),
    logLevel: 'silent', build: { outDir: temp } });
  const assets = join(temp, 'assets');
  const worker = readdirSync(assets).find(name => name.startsWith('intent.worker-') && name.endsWith('.js'));
  const wasmUrl = 'data:text/javascript,' + encodeURIComponent(`
    import init from ${JSON.stringify(pathToFileURL(modulePath).href)};
    import { readFileSync } from 'node:fs';
    export * from ${JSON.stringify(pathToFileURL(modulePath).href)};
    export default () => init({module_or_path: readFileSync(${JSON.stringify(wasmPath)})});
  `);
  globalThis.self = {};
  await import(pathToFileURL(join(assets, worker)).href);
  self.onmessage({ data: { wasmUrl } });
  const send = (operation, args) => new Promise((resolve, reject) => {
    self.postMessage = message => message.ok ? resolve(message.result) : reject(new Error(message.error));
    self.onmessage({ data: { id: 1, operation, args } });
  });
  const file = (path, sourceId = path) => ({
    name: 'part.step', sourceId, bytes: gunzipSync(readFileSync(new URL(path, import.meta.url))),
  });
  return { send, file };
}

test('review gates process-dependent gaps for a pending or stated FDM process', async t => {
  const { send, file } = await workerAt(t);
  const source = file('./fixtures/gramil_shaft_before.step.gz');
  const args = answers => ({ ...source, answers, without: [] });
  const process = { 'part.process': '3D printed: FDM' };
  const bare = await send('review-edits', args({}));
  assert.ok(bare.questions?.some(q => q.id === 'part.coating'), 'unanswered process initially asks for coating');
  const pending = await send('review-edits', args(process));
  const gated = ['part.coating', 'part.edges', 'part.surface_finish'];
  assert.deepEqual(pending.questions.filter(q => gated.includes(q.id)), [], 'pending FDM hides machining questions');

  const { IntentEngine } = await import('./engine-fixture.mjs');
  const engine = new IntentEngine(source.name, source.bytes);
  let written;
  try { written = JSON.parse(engine.edit(JSON.stringify({ fill: process }))); }
  finally { engine.free(); }
  assert.ok(written.written, written.message);
  const saved = { ...source, sourceId: 'fdm', bytes: Uint8Array.from(written.bytes) };
  const { analysis } = await send('open', saved);
  assert.equal(analysis.table.rows.find(row => row.id === 'part.process').value, process['part.process']);
  const reopened = await send('review-edits', { ...saved, answers: {}, without: [] });
  assert.deepEqual(reopened.questions.filter(q => gated.includes(q.id)), [], 'stated FDM hides machining questions');
});

test('browser edit returns STEP bytes with unaddressed foreign PMI preserved', async t => {
  const { send, file } = await workerAt(t);
  const source = file('../../tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz');
  const edits = { fill: { 'part.material': 'Steel' } };
  const result = await send('edit', { ...source, edits });
  assert.ok(result.written && result.bytes instanceof Uint8Array, 'worker returns the edited STEP bytes');
  const before = await send('open', source);
  const after = await send('open', { ...source, sourceId: 'written', bytes: result.bytes });
  assert.ok(before.analysis.existing.length, 'the source carries foreign PMI');
  assert.equal(after.analysis.table.rows.find(r => r.id === 'part.material').value, 'Steel');
  const stepLines = bytes => new TextDecoder().decode(bytes).split(/\r?\n/);
  const output = new Set(stepLines(result.bytes));
  assert.deepEqual(stepLines(source.bytes).filter(line => !output.has(line) && !/^FILE_(DESCRIPTION|NAME)\(/.test(line)), [],
    'every original entity, including foreign PMI, is byte-identical');
});
