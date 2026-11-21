import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { appendFileSync, chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { chromium, expect } from '@playwright/test';
import { characteristicHistory, validateOpening, wasmDirectory } from './engine-fixture.mjs';
// The local app (dist-app, `npm run build:app`) served by the real `specify-core open`, the manual
// way in (no agent; test/app-agent.mjs is the agent's): on a part it loads, is titled for the file,
// takes an answer, has the command save part.specified.step beside it (whose PMI `specify-core
// analyse` reads back) even with questions open, saying which, saves with the defaults when asked
// (each confirmed default an edit list's fill of the value shown: session API 2, `POST save {rev, edits}`),
// resumes from that file with the open questions still open, and has no Save when read-only. With
// `open --diff OLD NEW` it shows what changed, lists what needs a decision, opens NEW's
// requirements in Graphical, and Save writes NEW with the carried answers. With `--in-place`, a
// save over a file another program changed is refused as that, not as answers that moved on. With
// the command's event streams all taken, the page says it gets no live updates, and catches up
// when it gets one. Nothing is requested from anywhere but 127.0.0.1. `npm run test:app`.
//
//   SPECIFY_CORE  the specify-core binary (default: specify-core on PATH); it needs the session
//                 contract, API 2 (`GET answers`, `POST save {rev, edits}`, `open --diff`).
//   SPECIFY_WASM_DIR  the engine's pkg/ (default: public/wasm/intent, from `npm run fetch:wasm`).
//   SPECIFY_APP_BUILT_IN=1  serve the app and engine built into SPECIFY_CORE (ADR P12) instead
//                 of passing --app dist-app --wasm SPECIFY_WASM_DIR (the release's check).
//   SHOTS         a directory to save screenshots in (optional).
const check = expect.configure({ timeout: 90000 });
const cli = process.env.SPECIFY_CORE ?? 'specify-core';
const app = resolve('dist-app');
const wasm = wasmDirectory;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const builtIn = process.env.SPECIFY_APP_BUILT_IN === '1';
if (!builtIn) assert.ok(existsSync(join(app, 'index.html')), 'dist-app is built (npm run build:app)');
assert.ok(existsSync(join(wasm, 'specify_core_wasm.js')), 'the engine is fetched (npm run fetch:wasm)');
/** The command's app and engine: its own, or dist-app and the engine directory. */
const assetArgs = builtIn ? [] : ['--app', app, '--wasm', wasm];
const assetEnv = builtIn ? { ...process.env, SPECIFY_APP_DIR: '', SPECIFY_WASM_DIR: '' } : process.env;

const dir = mkdtempSync(join(tmpdir(), "specify app's-"));
const part = join(dir, 'part.step');
// A part with no answers stored in it: gramil's shaft.
writeFileSync(part, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
// The copy the session's saves write: the first save creates it and later ones update it (#217).
const specified = join(dir, 'part.specified.step');
// What each save wrote there, kept by the test for the checks below: nothing answered, the
// defaults taken, one answer given.
const saves = join(dir, 'saves');
mkdirSync(saves);
const plainSave = join(saves, 'part.specified.step');
const withDefaults = join(saves, 'part.specified-2.step');
const second = join(saves, 'part.specified-3.step');

/** `specify-core open` on ``files`` (one, or `--diff` and two): its address once it prints it, and a way to stop it. */
async function open(file, ...flags) {
  const child = spawn(cli, ['open', ...assetArgs, '--no-browser', ...flags, ...[file].flat()], { env: assetEnv, stdio: ['ignore', 'pipe', 'pipe'] });
  let out = '';
  child.stdout.on('data', chunk => { out += chunk; });
  child.stderr.on('data', chunk => { out += chunk; });
  for (let n = 0; n < 200 && !/http:\/\/127\.0\.0\.1:\d+\/[0-9a-f]{32}\//.test(out); n++) {
    if (child.exitCode !== null) break;
    await new Promise(r => setTimeout(r, 50));
  }
  const url = out.match(/http:\/\/127\.0\.0\.1:\d+\/[0-9a-f]{32}\//)?.[0];
  assert.ok(url, `specify-core open printed its address: ${out}`);
  return { url, origin: new URL(url).origin, output: () => out, stop: () => child.kill() };
}

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader'] });
const problems = [];
/** A page on the served app, every request it (and its worker) makes recorded. */
async function load(server, ready = '.pmi-lab-panel') {
  const context = await browser.newContext({ viewport: { width: 1400, height: 950 }, acceptDownloads: true });
  const requests = [];
  context.on('request', request => requests.push({ url: request.url(), method: request.method() }));
  // Every message the page posts to a worker (the engine), by operation: what the engine was asked.
  await context.addInitScript(() => {
    window.__engineOps = [];
    const post = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (message, ...rest) {
      if (message && typeof message.operation === 'string') window.__engineOps.push(message.operation);
      return post.call(this, message, ...rest);
    };
  });
  const page = await context.newPage();
  page.on('pageerror', error => problems.push(String(error)));
  page.on('console', message => { if (message.type() === 'error') problems.push(`console: ${message.text()}`); });
  await page.goto(server.url);
  await check(page.locator(`${ready}, .app-open-failed`).first()).toBeVisible();
  if (ready !== '.app-open-failed' && await page.locator('.app-open-failed').isVisible()) {
    throw new Error(`The app could not open the part: ${await page.locator('.app-open-failed').textContent()}`);
  }
  return { context, page, requests };
}
/** Nothing but the page's own origin (and its blob:/data: URLs) was asked for. */
const onlyLocal = (requests, origin) => {
  const elsewhere = requests.filter(({ url }) => !url.startsWith(`${origin}/`) && !/^(blob|data):/.test(url));
  assert.deepEqual(elsewhere, [], 'no request leaves 127.0.0.1');
  assert.ok(requests.length > 0);
};
const material = page => page.locator('[data-question="part.material"]');
let first = null;
/** The question ids the defaults dialog listed and the person confirmed. */
let listed = [];
/** `specify-core analyse` of ``file``: what Specify reads from it (its PMI as characteristics, the answers stored). */
/** The answers a person gave: Specify's own `_` records left out. */
const given = answers => Object.fromEntries(Object.entries(answers ?? {}).filter(([id]) => !id.startsWith('_')));
const analyse = file => JSON.parse(execFileSync(cli, ['analyse', '--output', '-', file], { encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] }));
/** ADR P11: what the file states, by feature-and-kind (the analysis's table): each row's state and value. */
const table = file => Object.fromEntries((analyse(file).table?.rows ?? []).map(r => [r.id, r]));
/** The feature-and-kinds the file holds: stated, or decided none. */
const held = file => Object.entries(table(file)).filter(([, r]) => r.state === 'stated' || r.state === 'decided_none').map(([id]) => id);
/** The file holds exactly ``ids``: each is stated or decided none, and any other row it holds is stated
 * only by requirements of those (a tapped hole's thread also states its full-thread depth). */
function holdsExactly(file, ids, message) {
  const rows = table(file);
  assert.deepEqual(ids.filter(id => !held(file).includes(id)), [], `${message}: each is in the file`);
  const theirs = new Set(ids.flatMap(id => (rows[id]?.requirements ?? []).map(r => r.number ?? r.text)));
  const others = held(file).filter(id => !ids.includes(id) && !(rows[id].state === 'stated'
    && rows[id].requirements?.length && rows[id].requirements.every(r => theirs.has(r.number ?? r.text))));
  assert.deepEqual(others, [], `${message}: nothing else`);
}
/** A save from the page's origin with an edit list (session API 2). */
const post = (server, body) => fetch(`${server.url}save`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: server.origin }, body: JSON.stringify(body) });

async function downloadFile(page, name, filename) {
  const [download] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name }).click()]);
  assert.equal(download.suggestedFilename(), filename);
  return download.path();
}

const exportAnswers = async (page, filename = 'part.answers.json') => JSON.parse(readFileSync(await downloadFile(page, 'Answers (JSON)', filename), 'utf8'));
const workbookSheet = (path, number) => execFileSync('unzip', ['-p', path, `xl/worksheets/sheet${number}.xml`], { encoding: 'utf8' });

/** The exports downloaded from the page, named for ``path``, against ``file`` (what the save wrote there) read back. */
async function inspectSaved(page, path, file = path) {
  const name = basename(path).replace(/\.gz$/i, '').replace(/\.(step|stp)$/i, '');
  await check(page.locator('.app-inspection-source')).toContainText(`from ${basename(path)}`);
  const csv = readFileSync(await downloadFile(page, 'Inspection list (CSV)', `${name}-characteristics.csv`), 'utf8');
  const book = await downloadFile(page, 'AS9102 FAI (Excel)', `${name}-AS9102.xlsx`);
  const form2 = workbookSheet(book, 2);
  const form3 = workbookSheet(book, 3);
  const numbers = (analyse(file).characteristics ?? []).map((item) => item.number).sort((a, b) => a - b);
  const csvNumbers = csv.trimEnd().split('\n').slice(1).map((line) => Number(line.split(',')[0])).filter((n) => n > 0);
  const faiNumbers = [...form3.matchAll(/<c r="A(\d+)"[^>]*><is><t [^>]*>([^<]+)<\/t>/g)]
    .filter((match) => Number(match[1]) >= 5).map((match) => Number(match[2]));
  assert.deepEqual(csvNumbers, numbers, 'CSV numbers are exactly those read back from the saved STEP');
  assert.deepEqual(faiNumbers, numbers, 'AS9102 numbers are exactly those read back from the saved STEP');
  assert.doesNotMatch(form3, /Material:/, 'Material belongs on Form 2 only');
  return { csv, form2, form3 };
}
const assertSaveFirst = async (page) => {
  for (const name of ['Inspection list (CSV)', 'AS9102 FAI (Excel)']) {
    const button = page.getByRole('button', { name, exact: true });
    await check(button).toBeDisabled();
    await check(button).toHaveAttribute('title', 'Save first: the inspection files describe the saved STEP');
  }
};

/** After a save to a copy, the exports are enabled from the file as saved (`file?rev=saved`,
 * specify-ui#14), and the 3D view draws that file's PMI. ``before``: the save line shown before it. */
const assertExportsFromSaved = async (page, file, before) => {
  if (before !== undefined) await check(page.locator('.app-saved')).not.toHaveText(before);
  for (const name of ['Inspection list (CSV)', 'AS9102 FAI (Excel)']) {
    await check(page.getByRole('button', { name, exact: true })).toBeEnabled();
  }
  const exported = await inspectSaved(page, specified, file);
  const ops = await page.evaluate(() => window.__engineOps);
  assert.ok(!ops.includes('saved-copy'), 'a separate copy is never reconstructed');
  await check.poll(async () => (await page.evaluate(() => window.__engineOps)).includes('file-scene')).toBe(true);
  return exported;
};
/** The text of the save line, or null before one. */
const savedLine = page => page.locator('.app-saved').textContent({ timeout: 1000 }).catch(() => null);

try {
  // Specify: answer, save, read the PMI back.
  {
    const server = await open(part);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.step — Specify');
    await check(page.locator('.app-brand')).toHaveText('Specify');
    await check(page.getByText('Draftwright')).toHaveCount(0);
    // A file of one part: no part named by the command.
    assert.equal((await (await fetch(`${server.url}config`)).json()).part, null);
    await check(page.locator('.app-target')).toHaveText(specified);
    await assertSaveFirst(page);
    // Save with the material, which is required, still open is not refused: it writes what is answered
    // (nothing, here), and says what is still open. Many STEP files have no complete answer to everything.
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(specified);
    await assertExportsFromSaved(page, specified);
    await check(page.locator('.app-saved')).toContainText('still open');
    // Open questions by their short labels: not their ids, nor the questions with their hints (#217).
    await check(page.locator('.app-saved')).toContainText('Material');
    await check(page.locator('.app-saved')).not.toContainText('What material is the part made from?');
    await check(page.locator('.app-saved')).not.toContainText('part.material');
    await check(page.getByRole('alert')).toHaveCount(0);
    assert.ok(existsSync(specified), 'part.specified.step written with a required question open');
    assert.deepEqual(given(analyse(specified).resume?.answers), {}, 'a plain Save with nothing answered stores no answers');
    copyFileSync(specified, plainSave);
    if (shots) await page.screenshot({ path: join(shots, 'save-partial.png') });
    // The person chooses the defaults, which is its own button, always there. It first lists every
    // default it would write (ADR P9: written only when approved): cancelled, nothing is written.
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await check(list).toContainText('Which general tolerance applies');
    await check(list).toContainText('2768-m');
    await check(list.locator('[data-default="part.material"]')).toHaveCount(0);
    assert.ok(await list.locator('li').count() > 3, 'the full list of defaults');
    await list.getByRole('button', { name: 'Cancel' }).click();
    await check(list).toHaveCount(0);
    await page.waitForTimeout(500);
    assert.deepEqual(readFileSync(specified), readFileSync(plainSave), 'nothing written when the list is cancelled');
    assert.deepEqual(await exportAnswers(page), {}, 'JSON after cancellation contains no unapproved defaults');
    // Confirmed: Save writes beside the file and says where.
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    await check(list.locator('[data-default]').first()).toBeVisible();
    listed = await list.locator('[data-default]').evaluateAll(items => items.map(item => item.getAttribute('data-default')).sort());
    const shownTolerance = (await list.locator('[data-default="part.general_tolerance"] strong').textContent()).trim();
    assert.equal(shownTolerance, 'ISO 2768-m', 'the dialog lists the default the person sees');
    assert.deepEqual(await exportAnswers(page), {}, 'listing defaults does not make them exportable');
    const beforeDefaults = await savedLine(page);
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    // The same copy, updated: the session wrote it (#217).
    await check(page.locator('.app-saved')).not.toHaveText(beforeDefaults);
    await check(page.locator('.app-saved')).toContainText(specified);
    await check.poll(() => (server.output().match(/Saved /g) ?? []).length).toBe(2);
    copyFileSync(specified, withDefaults);
    // The value the person confirmed is exactly what lands in the file. ADR P11: the file states it
    // (its table row), it is not a stored answer (was: `resume.answers`, an inherited failure since P11).
    const tolerance = table(withDefaults)['part.general_tolerance'];
    assert.equal(tolerance.state, 'stated', 'the confirmed general tolerance is stated in the file');
    assert.match(JSON.stringify(tolerance.value), new RegExp(shownTolerance.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), 'the value stated is the value listed');
    holdsExactly(withDefaults, listed, 'the confirmed defaults are in the file, stated or decided none');
    // Inspect the export itself: filtering internal keys here would conceal an export leak. After the
    // save the page works on the saved file: what is stated is the file's, not an answer to export.
    const approved = await exportAnswers(page);
    assert.deepEqual(Object.keys(approved).filter(id => !listed.includes(id)), [], 'JSON holds only confirmed defaults (those the file stores as decisions)');
    assert.equal(approved['part.general_tolerance'], undefined, 'the stated tolerance is the file\'s, not an exported answer');
    await check(page.getByRole('alert')).toHaveCount(0);
    assert.ok(existsSync(withDefaults), 'the defaults save written');
    assert.match(server.output(), /Saved .*part\.specified\.step[^\n]*\n[^]*Saved .*part\.specified\.step/);
    assert.ok(!existsSync(join(dir, 'part.specified-2.step')), 'no numbered copy');
    // The next save updates the same copy, and the header says so.
    await check(page.locator('.app-target')).toHaveText(specified);
    await assertExportsFromSaved(page, withDefaults);
    // An answer: the material. Unsaved until Save.
    const box = material(page).locator('input');
    await box.fill('Aluminium 6082-T6');
    await box.press('Enter');
    await check(page.locator('.app-dirty')).toBeVisible();
    if (shots) await page.screenshot({ path: join(shots, 'specify.png') });
    // The exports stay those of the file as saved, not the unsaved answer.
    await inspectSaved(page, specified, withDefaults);
    for (const [name, file] of [['Answers (JSON)', 'part.answers.json']]) {
      const path = await downloadFile(page, name, file);
      if (file.endsWith('.json')) {
        const answers = JSON.parse(readFileSync(path, 'utf8'));
        assert.equal(answers['part.material'], 'Aluminium 6082-T6', 'the answers file has the answer given');
      }
    }
    // A plain Save writes the answer given (a fill) and assumes no new defaults. The defaults the
    // person confirmed earlier are in the file now, so with the material given nothing is left open.
    const beforeAnswer = await savedLine(page);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(specified);
    await check(page.locator('.app-saved')).toContainText('complete');
    await check.poll(() => (server.output().match(/Saved /g) ?? []).length).toBe(3);
    copyFileSync(specified, second);
    await assertExportsFromSaved(page, second, beforeAnswer);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    // ADR P11: the file holds the answer given and the defaults confirmed (stated, or decided none),
    // and nothing else of the suggestions (was: the stored answers, an inherited failure since P11).
    holdsExactly(second, [...listed, 'part.material'], 'Save writes the answer given and the defaults confirmed');
    assert.match(JSON.stringify(table(second)['part.material'].value), /Aluminium 6082-T6/);
    onlyLocal(requests, server.origin);
    // Leaving the page (pagehide, not into the back/forward cache) sends `closed`: the command then
    // stops 90 s later (without it, after 30 minutes), checked at the end.
    await page.goto('about:blank');
    first = server;
    await context.close();
  }
  // The PMI Specify wrote reads back.
  {
    const analysis = analyse(withDefaults);
    assert.ok(!('part.material' in (analysis.resume?.answers ?? {})), 'the defaults save had no material');
    // The page sends the ids it listed and the person confirmed, each a fill of the value seen
    // (`edits: {fill: {id: value}}`): the file holds exactly those and nothing else (ADR P11: stated or
    // decided none; was the stored answers, an inherited failure since P11).
    holdsExactly(withDefaults, listed, 'the file holds the defaults confirmed');
    const plain = (analyse(plainSave).characteristics ?? []).length;
    assert.ok((analysis.characteristics ?? []).length > plain, 'the defaults save has the defaults\' PMI');
    const kinds = new Set((analysis.characteristics ?? []).map(c => c.kind));
    assert.ok(kinds.has('datum') && kinds.has('dimension'), `the PMI written reads back: ${[...kinds]}`);
  }
  // Resume: the file Specify wrote opens with its answers.
  {
    const server = await open(second);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.specified-3.step — Specify');
    // ADR P11: the file states what was saved (nothing here is a stored decision), and the page shows
    // it as the file's (was: the "opened with the answers" note, which needs stored answers).
    assert.equal(table(second)['part.material'].state, 'stated');
    await check(page.locator('.pmi-lab-panel')).toContainText('6082');
    await check(page.locator('.app-dirty')).toHaveCount(0);
    // The heartbeat: `GET config` again within 30 s of the first.
    const configs = () => requests.filter(({ url, method }) => method === 'GET' && url === `${server.url}config`).length;
    assert.equal(configs(), 1);
    await check.poll(configs, { timeout: 40000 }).toBe(2);
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // An assembly: Save with defaults lists every part's, but an edit list applies to one part (the
  // piece-2 follow-on brings edits on parts of an assembly): the page says so and writes nothing.
  {
    const asmDir = join(dir, 'drawn');
    mkdirSync(asmDir);
    const asm = join(asmDir, 'asm.step');
    writeFileSync(asm, gunzipSync(readFileSync('test/fixtures/gramil_assembly_before.step.gz')));
    const server = await open(asm);
    const { context, page, requests } = await load(server);
    await check(page.locator('.app-part select')).toBeVisible();
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await check(list.locator('[data-part]').first()).toBeVisible();
    assert.ok(await list.locator('[data-part]').count() > 1, 'defaults listed for several parts');
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    await check(page.getByRole('alert').first()).toContainText('cannot be saved together yet');
    assert.ok(!existsSync(join(asmDir, 'asm.specified.step')), 'nothing written');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // The partial save reopens with what was answered, and the questions left open are still open.
  {
    const server = await open(plainSave);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.specified.step — Specify');
    await check(material(page)).toHaveClass(/required/);
    await check(page.locator('.pmi-lab-panel')).toContainText('required');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // An edit list the command refuses (all or nothing: 422, every failing entry named) writes nothing,
  // and the page says which change; a failed write (the folder read-only) is said, and nothing is
  // written; writable again, the same confirmation saves.
  {
    const copy = join(dir, 'fresh');
    mkdirSync(copy);
    const file = join(copy, 'shaft.step');
    writeFileSync(file, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const server = await open(file);
    const { context, page } = await load(server);
    const { rev } = await (await fetch(`${server.url}answers`)).json();
    // The command's own refusals: a list with an unknown id (422, naming it), a malformed list (400),
    // and no list at all (400).
    const refused = await post(server, { rev, edits: { fill: { 'part.material': 'Steel', 'nope.unknown': 1 } } });
    assert.equal(refused.status, 422);
    const body = await refused.json();
    assert.deepEqual(body.failures.map(f => f.entry), ['fill["nope.unknown"]']);
    assert.equal((await post(server, { rev, edits: { fill: [] } })).status, 400, 'a malformed list');
    assert.equal((await post(server, { rev, acceptDefaults: {} })).status, 400, 'the API 1 body is not a list');
    assert.ok(!existsSync(join(copy, 'shaft.specified.step')), 'nothing written');
    await page.getByRole('button', { name: 'Save with defaults' }).click();
    const list = page.getByRole('dialog', { name: 'Save with defaults' });
    await check(list.locator('[data-default]').first()).toBeVisible();
    // The page, meeting a refusal: says which change cannot be applied, and that nothing was written.
    await page.route('**/save', route => route.fulfill({ status: 422, contentType: 'application/json', body: JSON.stringify(body) }));
    await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
    await check(page.getByRole('alert').first()).toContainText('nothing was written');
    await check(page.getByRole('alert').first()).toContainText('nope.unknown');
    await page.unroute('**/save');
    problems.splice(0, problems.length, ...problems.filter(p => !/40\d|422/.test(p)));
    assert.ok(!existsSync(join(copy, 'shaft.specified.step')), 'nothing written');
    // The write fails (the folder is read-only): Not saved, nothing changed in the session.
    if (process.getuid?.() !== 0) {
      await page.getByRole('button', { name: 'Save with defaults' }).click();
      chmodSync(copy, 0o555);
      try {
        await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
        await check(page.getByRole('alert').first()).toContainText('Not saved');
      } finally { chmodSync(copy, 0o755); }
      await check(page.locator('.app-saved')).toHaveCount(0);
      await check(page.locator('.app-dirty')).toHaveCount(0);
      await check(page.locator('.pmi-lab-panel')).toContainText('Suggested: ISO 2768-m');
      problems.splice(0, problems.length, ...problems.filter(p => !/50\d/.test(p)));
      // The folder writable again, the same confirmation saves.
      await page.getByRole('button', { name: 'Save with defaults' }).click();
      await list.getByRole('button', { name: /^Save with these \d+ defaults$/ }).click();
      await check(page.locator('.app-saved')).toContainText('shaft.specified');
    }
    await context.close();
    server.stop();
  }
  // Read only: a viewer, no Save, and the command refuses one.
  {
    const server = await open(withDefaults, '--read-only');
    const { context, page, requests } = await load(server);
    await check(page.locator('.pmi-lab-panel')).toContainText('PMI (view only)');
    await check(page.getByRole('button', { name: 'Save', exact: true })).toHaveCount(0);
    await check(page.locator('.app-target')).toHaveCount(0);
    await check(page.getByRole('button', { name: 'Inspection list (CSV)' })).toBeEnabled();
    if (shots) await page.screenshot({ path: join(shots, 'read-only.png') });
    // The command refuses a save as the page would send it (its Origin, the session's rev).
    const { rev } = await (await fetch(`${server.url}answers`)).json();
    const refused = await post(server, { rev, edits: {} });
    assert.equal(refused.status, 409, 'a save is refused when read-only');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // Decision 2026-11-19: local inspection downloads describe the last saved STEP, never live answers.
  {
    const saved = join(dir, "inspection's in-place.step");
    writeFileSync(saved, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const server = await open(saved, '--in-place');
    const { context, page } = await load(server);
    await assertSaveFirst(page);
    const box = material(page).locator('input');
    await box.fill('Aluminium 6082-T6');
    await box.press('Enter');
    await page.locator('.pmi-lab-suggestion:has([data-suggestion="part.general_tolerance"])').getByRole('button', { name: 'Keep', exact: true }).click();
    await check(page.locator('.app-dirty')).toBeVisible();
    await assertSaveFirst(page);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(saved);
    const original = await inspectSaved(page, saved);
    assert.match(original.form2, /MATERIAL:? Aluminium 6082-T6/);
    assert.ok(existsSync(`${saved}.bak`), 'successful in-place save retained the original backup');
    // A further edit (after the save the material is the file's, which this page cannot change yet:
    // piece 7; so another question's suggestion is kept): the saved exports do not change.
    await page.locator('.pmi-lab-suggestion:has([data-suggestion="part.surface_finish"])').getByRole('button', { name: 'Keep', exact: true }).click();
    await check(page.locator('.app-dirty')).toBeVisible();
    assert.deepEqual(await inspectSaved(page, saved), original, 'further edits leave both saved inspection downloads unchanged');
    const json = await exportAnswers(page, "inspection's in-place.answers.json");
    assert.equal(json['part.surface_finish'], 'Ra 3.2', 'Answers JSON still exports the current answers');
    await context.close();
    server.stop();
    const readOnly = await open(saved, '--read-only');
    const viewer = await load(readOnly);
    assert.deepEqual(await inspectSaved(viewer.page, saved), original, 'read-only exports the actual opened file');
    await viewer.context.close();
    readOnly.stop();
  }
  // Stored characteristics absent from an interview reconstruction must remain in the exports.
  {
    const spindle = join(dir, 'stored-spindle.step');
    writeFileSync(spindle, gunzipSync(readFileSync('test/fixtures/spindle_pmi_after.step.gz')));
    const server = await open(spindle, '--read-only');
    const { context, page } = await load(server);
    const stored = await inspectSaved(page, spindle);
    const numbers = stored.csv.trimEnd().split('\n').slice(1).map((row) => Number(row.split(',')[0]));
    for (const number of [9, 16, 17]) assert.ok(numbers.includes(number), `stored spindle characteristic #${number}`);
    await context.close();
    server.stop();
  }
  // Diff: what changed from OLD to NEW, what needs checking, and Save writes NEW with its own answers.
  {
    // The enclosure: NEW's requirements on the resized holes to check (test/fixtures/README.md).
    const oldFile = join(dir, 'enclosure.step');
    const newFile = join(dir, 'enclosure-rev2.step');
    writeFileSync(oldFile, gunzipSync(readFileSync('test/fixtures/enclosure_base_pmi_before.step.gz')));
    writeFileSync(newFile, gunzipSync(readFileSync('test/fixtures/enclosure_base_pmi_after.step.gz')));
    const server = await open([oldFile, newFile], '--diff');
    const config = await (await fetch(`${server.url}config`)).json();
    assert.equal(config.mode, 'diff');
    // GET diff: exactly WASM diff()'s {report, markdown, view}, with the PMI scenes in report.pmi.scenes.
    const body = await (await fetch(`${server.url}diff`)).json();
    assert.deepEqual(Object.keys(body).sort(), ['markdown', 'report', 'view']);
    assert.ok(body.report.pmi?.scenes && 'old' in body.report.pmi.scenes && 'new' in body.report.pmi.scenes, 'GET diff carries the PMI scenes');
    const report = body.report;
    // Diff reads the two files' PMI only (ADR P11): nothing is carried; the session is NEW's own.
    assert.equal(report.answers, undefined);
    const { parts } = await (await fetch(`${server.url}answers`)).json();
    const own = given(parts['0'] ?? {});
    assert.deepEqual(own, given(analyse(newFile).resume?.answers ?? {}), 'the session starts from NEW\'s own answers');
    // It opens on what changed; the requirements are behind their tab.
    const { context, page, requests } = await load(server, '.diff-summary');
    await check(page.locator('.pmi-lab-panel')).toBeAttached();
    await check(page).toHaveTitle(`${basename(newFile)} — Specify`);
    await check(page.locator('.app-path')).toHaveText(`${oldFile} → ${newFile}`);
    // What changed, first: the comparison drawn, its summary.
    await check(page.getByRole('tab', { name: 'What changed' })).toHaveAttribute('aria-selected', 'true');
    await check(page.locator('.diff-summary').first()).toBeVisible();
    // The page drew it from GET diff and compared nothing itself: no `diff` request reached its engine
    // (it still asks the engine to open NEW and for the parts' meshes).
    const ops = await page.evaluate(() => window.__engineOps);
    assert.ok(ops.length > 0 && ops.includes('open'), `the engine was used: ${ops}`);
    assert.ok(!ops.includes('diff'), `no engine diff call in diff mode: ${ops}`);
    assert.ok(requests.some(({ url, method }) => method === 'GET' && url === `${server.url}diff`), 'the comparison came from GET diff');
    // What needs checking, from the command's report: NEW's requirements on the resized holes.
    assert.equal(report.unresolved, undefined);
    assert.equal(report.questions, undefined);
    assert.ok((report.check ?? []).length > 0, 'requirements to check');
    await check(page.locator('.app-decision')).toHaveCount(report.check.length);
    if (shots) await page.screenshot({ path: join(shots, 'diff.png') });
    await page.getByRole('tab', { name: /^Requirements for/ }).click();
    // NEW carries PMI (its own, or Specify's): View PMI opens in Graphical.
    await check(page.getByRole('radio', { name: 'Graphical' })).toBeChecked();
    if (shots) await page.screenshot({ path: join(shots, 'diff-decide.png') });
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    const target = join(dir, `${basename(newFile).replace(/\.gz$/, '').replace(/\.(step|stp)$/, '')}.specified.step`);
    await check(page.locator('.app-saved')).toContainText(target);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    assert.deepEqual(given(analyse(target).resume?.answers ?? {}), own, 'Save writes NEW with the session\'s answers');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // --in-place: another program changes the file after it was opened. Save is refused as that,
  // its own error, with no "Save again" (it cannot succeed), and nothing is written over the file.
  {
    const file = join(dir, 'in-place.step');
    writeFileSync(file, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const server = await open(file, '--in-place');
    const { context, page, requests } = await load(server);
    const box = material(page).locator('input');
    await box.fill('Aluminium 6082-T6');
    await box.press('Enter');
    await check(page.locator('.app-dirty')).toBeVisible();
    appendFileSync(file, '/* written by another program */\n');
    const changed = readFileSync(file);
    const before = problems.length;
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.getByRole('alert')).toContainText('the file changed on disk since it was opened');
    await check(page.getByRole('alert')).not.toContainText('Save again');
    await check(page.getByRole('button', { name: 'Save', exact: true })).toBeDisabled();
    assert.deepEqual(readFileSync(file), changed, 'nothing written over the changed file');
    assert.ok(!existsSync(`${file}.bak`), 'no backup made for a save that did not happen');
    // The refused save's 409 is the browser's console error, expected here.
    problems.splice(before, problems.length - before, ...problems.slice(before).filter(p => !/409/.test(p)));
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // The event stream refused (the command serves at most 4 at once: here all taken): the page says
  // it gets no live updates and keeps trying; once it gets a stream it shows what changed meanwhile.
  {
    const server = await open(part);
    const taken = [];
    for (let n = 0; n < 4; n++) {
      const stop = new AbortController();
      const response = await fetch(`${server.url}events`, { signal: stop.signal });
      assert.equal(response.status, 200);
      // Read (and so held) until aborted: an unread response the collector frees closes its stream.
      response.body.pipeTo(new WritableStream()).catch(() => {});
      taken.push({ stop, response });
    }
    const before = problems.length;
    const { context, page, requests } = await load(server);
    await check(page.locator('.app-offline')).toContainText('Not receiving live updates');
    // (Session API 2: there is no `PUT answers`, so no other tab can change the session meanwhile in
    // `open`; app-agent.mjs covers an agent's answers. Here: the page catches up once it gets a stream.)
    for (const { stop } of taken) stop.abort();
    await check(page.locator('.app-offline')).toHaveCount(0);
    // The refused stream's 503s are the browser's console errors, expected here.
    problems.splice(before, problems.length - before, ...problems.slice(before).filter(p => !/503|EventSource|events/.test(p)));
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // The first page's `closed` arrived: the command said so and stopped.
  await check.poll(() => first.output().includes('Closed.'), { timeout: 150000 }).toBe(true);
  assert.deepEqual(problems, [], 'no page errors');
  console.log('app-browser passed');
} finally {
  first?.stop();
  await browser.close();
}
