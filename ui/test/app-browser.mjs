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
// and shows the review panel (specify-panel.md): each feature's requirements stated, a gap (with its
// suggestion) or decided none. Edits made in the panel (Add, None needed, Undo, Change, Remove) are
// listed in the bar and written together by Save (session API 2, `POST save {rev, edits}`), or dropped
// by Discard; `specify-core analyse` reads back what the saved file states. Save writes
// part.specified.step beside it even with questions open, saying which; the file reopens with what was
// saved stated; read-only has no Save; an assembly's part edits save together. With
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
const lengthIds = [
  'length:planes:1-9', 'length:planes:1-5', 'length:planes:1-3', 'length:planes:8-10',
  'length:planes:2.6-10', 'length:planes:4-12', 'length:planes:1-7.11',
];
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
// What each save wrote there, kept by the test for the checks below: nothing edited, the panel's
// first edits, then a change, a remove and an Undo.
const saves = join(dir, 'saves');
mkdirSync(saves);
const plainSave = join(saves, 'part.specified.step');
const edited = join(saves, 'part.specified-2.step');
const second = join(saves, 'part.specified-3.step');

/** `specify-core open` on ``files`` (one, or `--diff` and two): its address once it prints it, and a way to stop it. */
const servers = [];
async function open(file, ...flags) {
  const child = spawn(cli, ['open', ...assetArgs, '--no-browser', ...flags, ...[file].flat()], { env: assetEnv, stdio: ['ignore', 'pipe', 'pipe'] });
  servers.push(child);
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
/** The panel's row for feature-and-kind ``id`` (or an item of "Other PMI in the file", by its number). */
const row = (page, id) => page.locator(`[data-row="${id}"]`);
const material = page => row(page, 'part.material');
/** The bar's entry for row ``id``. */
const listedEdit = (page, id) => page.locator(`.spec-bar [data-pending="${id}"]`);
/** Add on gap ``id``: its line opens pre-filled with the suggestion; ``value`` typed over it, if given. */
async function add(page, id, value) {
  const r = row(page, id);
  await r.locator('.acts').getByRole('button', { name: 'Add', exact: true }).click();
  if (value !== undefined) await r.locator('.spec-edit input').first().fill(value);
  await r.locator('.spec-edit').getByRole('button', { name: 'Add', exact: true }).click();
  await check(r.locator('.spec-edit')).toHaveCount(0);
  await check(listedEdit(page, id)).toBeVisible();
}
/** None needed on gap ``id``. */
async function noneNeeded(page, id) {
  await row(page, id).getByRole('button', { name: 'None needed', exact: true }).click();
  await check(row(page, id)).toHaveAttribute('data-state', 'decided_none');
}
/** Every row's value fits its box and keeps clear of the row's icons and buttons. */
async function assertValuesFit(page) {
  const bad = await page.locator('.spec-rows .spec-row').evaluateAll(rows => rows.flatMap(row => {
    const v = row.querySelector(':scope > .v');
    const acts = row.querySelector(':scope > .acts');
    if (!v || !v.getClientRects().length) return [];
    const box = v.getBoundingClientRect();
    const out = [];
    if (v.scrollWidth > v.clientWidth + 1) out.push(`${row.dataset.row}: overflows (${v.scrollWidth} > ${v.clientWidth})`);
    if (acts) {
      const a = acts.getBoundingClientRect();
      const sameLine = box.top < a.bottom - 1 && a.top < box.bottom - 1;
      if (sameLine && box.right > a.left + 1) out.push(`${row.dataset.row}: under its icons`);
    }
    return out;
  }));
  assert.deepEqual(bad, [], 'every value wraps within its box');
}
/** No wording in ``box`` says who wrote a requirement (decision 6). */
async function assertNoOrigin(box) {
  const text = await box.innerText();
  assert.ok(text.trim().length > 0, 'there is text to check');
  assert.doesNotMatch(text, /from the file|answered|\bdefaults?\b|Specify|written by|\bby you\b|another tool/i, `no origin wording: ${text}`);
}
/** Save and check the acknowledgement and the page's confirmation. The summary can repeat. */
async function save(page, server, n) {
  const reply = page.waitForResponse(
    response => response.url() === `${server.url}save` && response.request().method() === 'POST',
    { timeout: 90000 },
  );
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  const response = await reply;
  const saved = await response.json();
  assert.equal(response.status(), 200, JSON.stringify(saved));
  await check(page.locator('.app-saved code')).toHaveText(saved.path);
  await check.poll(() => (server.output().match(/Saved /g) ?? []).length).toBe(n);
  await check(page.locator('.spec-bar')).toContainText('written, all together');
  const body = response.request().postDataJSON();
  return { rev: saved.rev, requestedRev: body.rev, body };
}
let first = null;
/** The feature-and-kinds the first two saves list: each is in the file after it, stated or decided none. */
const listed = ['part.heat_treatment', 'part.material', 'hole.position:holes:14.15', 'hole.position:holes:13', 'datum.A', 'part.coating'];
/** `specify-core analyse` of ``file``: what Specify reads from it (its PMI as characteristics, the answers stored). */
/** The answers a person gave: Specify's own `_` records left out. */
const given = answers => Object.fromEntries(Object.entries(answers ?? {}).filter(([id]) => !id.startsWith('_')));
const analyse = (file, part) => JSON.parse(execFileSync(cli, ['analyse', '--output', '-', ...(part === undefined ? [] : ['--part', String(part)]), file], { encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] }));
/** ADR P11: what the file states, by feature-and-kind (the analysis's table): each row's state and value. */
const table = (file, part) => Object.fromEntries((analyse(file, part).table?.rows ?? []).map(r => [r.id, r]));
/** The feature-and-kinds the file holds, stated or decided none, and the choices with no row it
 * stores (how holes are located, `part.locating`: it writes no PMI, so the file stores it). */
const held = file => {
  const rows = table(file);
  const choices = Object.keys(given(analyse(file).resume?.answers)).filter(id => !(id in rows));
  return [...Object.entries(rows).filter(([, r]) => r.state === 'stated' || r.state === 'decided_none').map(([id]) => id), ...choices];
};
/** The file holds exactly ``ids``: each is stated or decided none, and any other row it holds is stated
 * only by requirements of those (a tapped hole's thread also states its full-thread depth). */
function holdsExactly(file, ids, message) {
  const rows = table(file);
  assert.deepEqual(ids.filter(id => !held(file).includes(id)), [], `${message}: each is in the file`);
  const theirs = new Set(ids.flatMap(id => (rows[id]?.requirements ?? []).map(r => r.number ?? r.text)));
  const others = held(file).filter(id => !ids.includes(id) && !(rows[id]?.state === 'stated'
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
  // A real assembly: edit the plate in place and compare the original assembly records and the
  // pin's PMI entity text. Seed the pin first so those records can be identified without pins
  // on entity numbers. The plate is seeded separately with the two requirements to edit.
  {
    const fixture = '../tests/fixtures/parts/assembly.step';
    const asm = join(dir, 'plate-and-pin.step');
    const pinOnly = join(dir, 'pin-pmi.step');
    const list = join(dir, 'assembly-seed.json');
    const seed = (input, part, fill, output) => {
      writeFileSync(list, JSON.stringify({ fill }));
      execFileSync(cli, ['write', input, list, '--part', String(part), '-o', output]);
    };
    seed(fixture, 1, { 'part.material': 'Bronze', 'part.general_tolerance': 'ISO 2768-f' }, pinOnly);
    seed(pinOnly, 0, { 'part.material': 'Steel', 'part.general_tolerance': 'ISO 2768-m' }, asm);
    const entities = file => new Map(Array.from(readFileSync(file, 'utf8')
      .matchAll(/^#(\d+)\s*=\s*(?:'(?:[^']|'')*'|[^';])*;/gm), m => [m[1], m[0]]));
    const original = entities(asm);
    const pinRecords = entities(pinOnly);
    assert.ok(pinRecords.size > entities(fixture).size, 'the unedited pin carries PMI');
    const pinTable = table(asm, 1);
    const choose = async (page, part, value) => {
      await page.locator('.app-part select').selectOption(String(part));
      await check(material(page).locator('.v')).toHaveText(value);
    };
    const change = async (page, value) => {
      const r = material(page);
      await r.getByRole('button', { name: 'Change Material', exact: true }).click();
      await r.getByRole('textbox', { name: 'New value' }).fill(value);
      await r.getByRole('button', { name: 'Apply', exact: true }).click();
      await check(r.locator('.spec-edit')).toHaveCount(0);
      await check(r.locator('.v')).toHaveText(value);
    };
    const pending = (page, part) => page.locator(`.spec-bar [data-part="${part}"][data-pending]`);
    const allPending = page => page.locator('.spec-bar [data-pending]');
    const discard = page => page.getByRole('button', { name: 'Discard', exact: true }).click();

    const server = await open(asm, '--in-place');
    const { context, page, requests } = await load(server);
    await choose(page, 1, 'Bronze');
    await choose(page, 0, 'Steel');
    await check(material(page).getByRole('button', { name: 'Change Material', exact: true })).toBeEnabled({ timeout: 5000 });
    await change(page, 'Aluminium 6082-T6');
    const gt = row(page, 'part.general_tolerance');
    await gt.getByRole('button', { name: 'Remove General tolerance', exact: true }).click();
    await gt.locator('.spec-edit.rm').getByRole('button', { name: 'Remove', exact: true }).click();
    await check(gt).toHaveClass(/removed/);
    await gt.getByRole('button', { name: 'Undo removal', exact: true }).click();
    await check(gt).not.toHaveClass(/removed/);
    await check(listedEdit(page, 'part.general_tolerance')).toHaveCount(0);
    await gt.getByRole('button', { name: 'Remove General tolerance', exact: true }).click();
    await gt.locator('.spec-edit.rm').getByRole('button', { name: 'Remove', exact: true }).click();
    await check(gt).toHaveClass(/removed/);
    await add(page, 'part.coating', 'Anodised');
    await check(pending(page, 0)).toHaveCount(3);
    const single = await save(page, server, 1);
    assert.deepEqual(single.body.partEdits.map(p => p.part), [0]);
    const saved = entities(asm);
    for (const [id, record] of pinRecords) {
      assert.equal(original.get(id), record, `the plate seed preserved #${id}`);
      assert.equal(saved.get(id), record, `the panel save preserved the original assembly and pin entity #${id}`);
    }
    assert.deepEqual(table(asm, 1), pinTable, 'the pin reads the same after the plate save');
    const plate = table(asm, 0);
    assert.equal(plate['part.material'].value, 'Aluminium 6082-T6');
    assert.equal(plate['part.general_tolerance'].state, 'gap');
    assert.equal(plate['part.coating'].value, 'Anodised');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();

    const reopened = await open(asm, '--in-place');
    const review = await load(reopened);
    const main = review.page;
    await check(material(main).locator('.v')).toHaveText('Aluminium 6082-T6');
    await check(row(main, 'part.general_tolerance')).toHaveAttribute('data-state', 'gap');
    await check(row(main, 'part.coating').locator('.v')).toHaveText('Anodised');
    await check(main.locator('.spec-row.changed')).toHaveCount(0);
    await noneNeeded(main, 'part.heat_treatment');
    await check(pending(main, 0)).toHaveText('plate · None needed: Heat treatment');
    await row(main, 'part.heat_treatment').getByRole('button', { name: 'Undo', exact: true }).click();
    await check(allPending(main)).toHaveCount(0);
    await main.getByRole('button', { name: 'Add all suggestions', exact: true }).click();
    await check(pending(main, 0).filter({ hasText: 'plate · Add General tolerance' })).toBeVisible();
    await check(pending(main, 1)).toHaveCount(0);
    await discard(main);

    // The same row id in two parts must have independent values and validation. Switch back and
    // forth with both edits present, then send one POST with both part lists.
    await change(main, 'Steel 1.4301');
    await choose(main, 1, 'Bronze');
    await change(main, 'Brass CW614N');
    await choose(main, 0, 'Steel 1.4301');
    await check(material(main)).toHaveClass(/changed/);
    await check(pending(main, 0)).toHaveText('plate · Change Material: Aluminium 6082-T6 → Steel 1.4301');
    await check(pending(main, 1)).toHaveText('pin · Change Material: Bronze → Brass CW614N');
    await choose(main, 1, 'Brass CW614N');
    const both = await save(main, reopened, 1);
    assert.deepEqual(both.body.partEdits.map(p => p.part).sort(), [0, 1]);
    assert.equal(table(asm, 0)['part.material'].value, 'Steel 1.4301');
    assert.equal(table(asm, 1)['part.material'].value, 'Brass CW614N');
    await check(material(main)).not.toHaveClass(/changed/);

    // A save-time refusal can still happen after WASM checks. Change the second part's outgoing
    // entry so the real server refuses it after staging the first, and check disk and session.
    await choose(main, 0, 'Steel 1.4301');
    await change(main, 'Steel');
    await choose(main, 1, 'Brass CW614N');
    await change(main, 'Bronze');
    const beforeRefusal = readFileSync(asm);
    const failed = main.waitForResponse(response => response.url() === `${reopened.url}save`);
    await main.route('**/save', route => {
      const body = route.request().postDataJSON();
      const second = body.partEdits.find(p => p.part === 1);
      second.edits = { fill: { 'unknown.pin': 'invalid' } };
      return route.continue({ postData: JSON.stringify(body) });
    });
    const beforeErrors = problems.length;
    await main.getByRole('button', { name: 'Save', exact: true }).click();
    const refusal = await failed;
    assert.equal(refusal.status(), 422);
    const refused = await refusal.json();
    assert.deepEqual(refused.failures.map(f => f.entry), ['Part "pin" (1): fill["unknown.pin"]']);
    await check(main.getByRole('alert').first()).toContainText('nothing was written');
    await check(main.getByRole('alert').first()).toContainText(refused.failures[0].entry);
    assert.deepEqual(readFileSync(asm), beforeRefusal, 'one refused entry prevents both writes');
    assert.equal((await (await fetch(`${reopened.url}answers`)).json()).rev, both.rev);
    assert.equal((reopened.output().match(/Saved /g) ?? []).length, 1);
    await check(allPending(main)).toHaveCount(2);
    await discard(main);
    await check(allPending(main)).toHaveCount(0);
    await check(material(main).locator('.v')).toHaveText('Brass CW614N');
    await choose(main, 0, 'Steel 1.4301');
    problems.splice(beforeErrors, problems.length - beforeErrors,
      ...problems.slice(beforeErrors).filter(p => !/^console: .*422/.test(p)));
    onlyLocal(review.requests, reopened.origin);
    await review.context.close();
    reopened.stop();
  }
  // Specify: the panel's edits, listed in the bar, written together by Save, read back.
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
    // A bare part: every feature-and-kind is a gap, with Specify's suggestion as a chip (decision 5);
    // nothing is highlighted before an edit (decision 6), and the bar says nothing is written until Save.
    await check(material(page)).toHaveAttribute('data-state', 'gap');
    await check(row(page, 'part.general_tolerance').locator('.sugg')).toHaveText('suggested: ISO 2768-m');
    await check(page.locator('.spec-row.changed')).toHaveCount(0);
    await check(page.locator('.spec-bar')).toContainText('No changes yet. Nothing is written until Save.');
    // Save with nothing edited writes nothing, and says so (the mock-up's "Nothing to save.").
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.spec-bar')).toContainText('Nothing to save.');
    await page.waitForTimeout(500);
    assert.ok(!existsSync(specified), 'nothing written');
    assert.equal((server.output().match(/Saved /g) ?? []).length, 0);
    // A save with the material (required) still open is not refused: it writes the edit, and says
    // what is still open. Many STEP files have no complete answer to everything.
    await noneNeeded(page, 'part.heat_treatment');
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
    assert.deepEqual(Object.keys(given(analyse(specified).resume?.answers)), ['part.heat_treatment'], 'the decision listed is all it stores');
    copyFileSync(specified, plainSave);
    if (shots) await page.screenshot({ path: join(shots, 'save-partial.png') });
    // Discard drops every edit in the bar: nothing is written, and the rows read as the file has them.
    await add(page, 'part.edges');
    await add(page, 'part.surface_finish');
    await check(page.locator('.spec-bar li[data-pending]')).toHaveCount(2);
    await check(page.locator('.app-dirty')).toBeVisible();
    await page.getByRole('button', { name: 'Discard', exact: true }).click();
    await check(page.locator('.spec-bar li[data-pending]')).toHaveCount(0);
    await check(row(page, 'part.edges')).toHaveAttribute('data-state', 'gap');
    await check(row(page, 'part.surface_finish')).toHaveAttribute('data-state', 'gap');
    await check(page.locator('.spec-row.changed')).toHaveCount(0);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    assert.deepEqual(readFileSync(specified), readFileSync(plainSave), 'Discard writes nothing');
    // Add, pre-filled with the suggestion (decision 5): the general tolerance in its editor, as a class.
    // (It is saved in the refusal case below: on this base it also writes the dimensions it covers.)
    await row(page, 'part.general_tolerance').locator('.acts').getByRole('button', { name: 'Add', exact: true }).click();
    const gt = row(page, 'part.general_tolerance');
    await check(gt.getByRole('combobox', { name: 'General tolerance class' })).toHaveValue('ISO 2768-m');
    // Or a block, row by row (#228 decision 2): the half-shell's title block, entered line by line,
    // is the one line of text the edit list carries.
    await gt.getByRole('radio', { name: 'A block, row by row' }).check();
    await gt.getByRole('combobox', { name: 'Units' }).selectOption('in');
    await gt.getByRole('combobox', { name: 'Row 1 decimal places' }).selectOption('2');
    await gt.getByRole('textbox', { name: 'Row 1 tolerance' }).fill('±.01');
    await gt.getByRole('combobox', { name: 'Row 2 decimal places' }).selectOption('3');
    await gt.getByRole('textbox', { name: 'Row 2 tolerance' }).fill('±.005');
    await gt.getByRole('button', { name: 'Add row' }).click();
    await gt.getByRole('button', { name: 'Remove row 3' }).click();
    await gt.getByRole('textbox', { name: 'Note 1', exact: true }).fill('fractions ±1/64');
    await gt.getByRole('button', { name: 'Add note', exact: true }).click();
    await gt.getByRole('textbox', { name: 'Note 2', exact: true }).fill('break all edges');
    await check(gt.locator('.spec-gt-text code')).toContainText('fractions ±1/64, break all edges');
    await gt.getByRole('button', { name: 'Remove note 2', exact: true }).click();
    await gt.getByRole('textbox', { name: 'Angles' }).fill('±1°');
    const block = '.XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°';
    await check(gt.locator('.spec-gt-text code')).toHaveText(block);
    await gt.locator('.spec-edit').getByRole('button', { name: 'Add', exact: true }).click();
    // The engine takes the block (#228): its check passes and the bar shows it as entered.
    await check(gt.locator('.spec-problem')).toHaveCount(0);
    await check(listedEdit(page, 'part.general_tolerance')).toHaveText(`Add General tolerance: ${block}`);
    await page.getByRole('button', { name: 'Discard', exact: true }).click();
    const notes = row(page, 'part.notes');
    await notes.locator('.acts').getByRole('button', { name: 'Add', exact: true }).click();
    await notes.locator('.spec-edit textarea').fill('Deburr all edges\nMark lot');
    await notes.locator('.spec-edit').getByRole('button', { name: 'Add', exact: true }).click();
    await check(notes.locator('.spec-note')).toHaveText(['Deburr all edges', 'Mark lot']);
    await page.getByRole('button', { name: 'Discard', exact: true }).click();
    await add(page, 'part.material', 'Aluminium 6082-T6');
    // Each edit joins the bar only if the file takes it with those listed (the bar is always a list
    // Save can write): a hole's position before the datum it cites is refused, saying so.
    await row(page, 'hole.position:holes:13').locator('.acts').getByRole('button', { name: 'Add', exact: true }).click();
    await row(page, 'hole.position:holes:13').locator('.spec-edit').getByRole('button', { name: 'Add', exact: true }).click();
    await check(row(page, 'hole.position:holes:13').locator('.spec-problem')).toContainText('fill datum.A');
    await row(page, 'hole.position:holes:13').getByRole('button', { name: 'Cancel', exact: true }).click();
    await add(page, 'datum.A');
    await add(page, 'hole.position:holes:14.15');
    await add(page, 'hole.position:holes:13');
    // A choice with no none option stays a gap and explains the refusal on its own row.
    const processRow = row(page, 'part.process');
    await processRow.getByRole('button', { name: 'None needed', exact: true }).click();
    await check(processRow.getByRole('alert')).toContainText("'none' is not an answer");
    await check(processRow).toHaveAttribute('data-state', 'gap');
    await check(listedEdit(page, 'part.process')).toHaveCount(0);
    // None needed records the decision; Undo of one made in this session drops it; None needed again.
    await noneNeeded(page, 'part.coating');
    await check(row(page, 'part.coating')).toContainText('none needed');
    await row(page, 'part.coating').getByRole('button', { name: 'Undo', exact: true }).click();
    await check(row(page, 'part.coating')).toHaveAttribute('data-state', 'gap');
    await check(listedEdit(page, 'part.coating')).toHaveCount(0);
    await noneNeeded(page, 'part.coating');
    await check(listedEdit(page, 'part.coating')).toHaveText('None needed: Coating');
    // If a checked None needed is later refused at Save, the row and the bar both stay visible.
    const beforeSave = problems.length;
    await page.route('**/save', route => route.fulfill({ status: 422, contentType: 'application/json',
      body: JSON.stringify({ error: 'nothing was written', failures: [{ entry: 'none["part.coating"]', reason: 'Coating is required here' }] }) }));
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(row(page, 'part.coating').getByRole('alert')).toHaveText('Coating is required here');
    await check(listedEdit(page, 'part.coating')).toBeVisible();
    await page.unroute('**/save');
    problems.splice(beforeSave, problems.length - beforeSave,
      ...problems.slice(beforeSave).filter(p => !/^console: .*422/.test(p)));
    // A value the file cannot take is refused on its line, by the engine, and never joins the bar.
    await row(page, 'part.surface_finish').locator('.acts').getByRole('button', { name: 'Add', exact: true }).click();
    await row(page, 'part.surface_finish').locator('.spec-edit input').fill('shiny');
    await row(page, 'part.surface_finish').locator('.spec-edit').getByRole('button', { name: 'Add', exact: true }).click();
    await check(row(page, 'part.surface_finish').locator('.spec-problem')).toContainText('not an answer');
    await row(page, 'part.surface_finish').getByRole('button', { name: 'Cancel', exact: true }).click();
    await check(listedEdit(page, 'part.surface_finish')).toHaveCount(0);
    // Only the unsaved edits are highlighted (decision 6).
    await check(page.locator('.spec-row.changed')).toHaveCount(5);
    await check(page.locator('.app-dirty')).toBeVisible();
    if (shots) await page.screenshot({ path: join(shots, 'panel-edits.png') });
    // The exports stay those of the file as saved, not the unsaved edits.
    await inspectSaved(page, specified, plainSave);
    await save(page, server, 2);
    copyFileSync(specified, edited);
    // Save wrote exactly the edits listed, all together (ADR P11: the file states them, or holds the decision).
    holdsExactly(edited, listed, 'Save writes the edits listed');
    const states = table(edited);
    assert.match(JSON.stringify(states['part.material'].value), /Aluminium 6082-T6/);
    assert.equal(states['part.coating'].state, 'decided_none');
    assert.equal(states['part.edges'].state, 'gap', 'the discarded edit is not in the file');
    // After Save every requirement looks the same: nothing highlighted (decision 6), and the rows read the file.
    await check(page.locator('.spec-row.changed')).toHaveCount(0);
    await check(material(page)).toHaveAttribute('data-state', 'stated');
    await check(material(page)).toContainText('Aluminium 6082-T6');
    await check(page.locator('.app-dirty')).toHaveCount(0);
    await assertExportsFromSaved(page, edited);
    // Change shows the current value first, in a line under the row (decision 2); a change of limits keeps its number.
    const position = 'hole.position:holes:14.15';
    const number = states[position].requirements[0].number;
    const pos = row(page, position);
    // Values read as a drawing writes them, not as the engine stores them.
    await check(pos).toContainText('⌖ ⌀0.2 A');
    // The pencil and the bin match, and neither is a warning colour (decision 1).
    const icons = pos.locator('.acts button.spec-icon');
    await check(icons).toHaveCount(2);
    const looks = await icons.evaluateAll(buttons => buttons.map(b => {
      const c = getComputedStyle(b); const box = b.getBoundingClientRect();
      return JSON.stringify([c.color, c.borderTopColor, c.backgroundColor, Math.round(box.width), Math.round(box.height)]);
    }));
    assert.equal(new Set(looks).size, 1, `the pencil and the bin look the same: ${looks}`);
    assert.doesNotMatch(looks[0], /168, 50, 45|179, 38, 30/, 'not red');
    await pos.getByRole('button', { name: /^Change / }).click();
    await check(pos.locator('.spec-edit .now')).toHaveText('Now: ⌖ ⌀0.2 A');
    await check(pos.locator('.spec-edit input')).toHaveValue('0.2');
    await check(pos.locator('.spec-edit')).toContainText(`Keeps ${number} if only the limits change. A different measurement gets a new number and ${number} is retired.`);
    await pos.locator('.spec-edit input').fill('0.1');
    await pos.getByRole('button', { name: 'Apply', exact: true }).click();
    await check(listedEdit(page, position)).toHaveText('Change Ø1.6 hole position: ⌖ ⌀0.2 A → ⌖ ⌀0.1 A');
    await check(pos).toContainText('⌖ ⌀0.1 A');
    // Remove: the bin asks once, in a line, the only red in the panel (decision 1); its number is retired.
    const removed = 'hole.position:holes:13';
    const retired = states[removed].requirements[0].number;
    const gone = row(page, removed);
    await gone.getByRole('button', { name: /^Remove / }).click();
    await check(gone.locator('.spec-edit.rm')).toContainText(`Number ${retired} is retired and never reused.`);
    const red = await gone.locator('.spec-edit.rm').getByRole('button', { name: 'Remove', exact: true }).evaluate(b => getComputedStyle(b).color);
    assert.equal(red, 'rgb(168, 50, 45)', 'the confirmation is red');
    if (shots) await page.screenshot({ path: join(shots, 'panel-remove.png') });
    await gone.locator('.spec-edit.rm').getByRole('button', { name: 'Remove', exact: true }).click();
    await check(gone).toHaveClass(/removed/);
    // Every stated row has the pencil and the bin (decisions 1 and 2), the material too. Without a
    // number its removal names the row (`items.remove: [{"id": "part.material"}]`; saved below, on the
    // reopened file); here it is asked and cancelled, and nothing joins the bar.
    await check(material(page).locator('.acts button.spec-icon')).toHaveCount(2);
    await material(page).getByRole('button', { name: /^Remove / }).click();
    await check(material(page).locator('.spec-edit.rm .now')).toHaveText('Remove Aluminium 6082-T6?');
    await material(page).getByRole('button', { name: 'Cancel', exact: true }).click();
    await check(listedEdit(page, 'part.material')).toHaveCount(0);
    await check(material(page)).not.toHaveClass(/removed/);
    // Undo on a decision the file stores takes it back (the edit list's clear): a gap again.
    await row(page, 'part.coating').getByRole('button', { name: 'Undo', exact: true }).click();
    await check(row(page, 'part.coating')).toHaveAttribute('data-state', 'gap');
    await check(listedEdit(page, 'part.coating')).toHaveText('Undo none needed: Coating');
    if (shots) await page.screenshot({ path: join(shots, 'panel-pending.png') });
    await save(page, server, 3);
    copyFileSync(specified, second);
    const after = table(second);
    assert.equal(after[position].state, 'stated');
    assert.match(JSON.stringify(after[position].value), /0\.1/, 'the change is in the file');
    assert.equal(after[position].requirements[0].number, number, 'a change of limits keeps its number');
    assert.equal(after[removed].state, 'gap', 'the removed requirement is gone: a gap again');
    assert.ok(!(analyse(second).characteristics ?? []).some(c => `#${c.number}` === retired), `${retired} is retired`);
    assert.equal(after['part.coating'].state, 'gap', 'Undo took the stored decision back');
    assert.equal(after['part.material'].state, 'stated', 'what the list did not name is left as it is');
    await check(page.locator('.spec-row.changed')).toHaveCount(0);
    if (shots) await page.screenshot({ path: join(shots, 'panel-saved.png') });
    // Origin is never shown (decision 6): not in the panel, nor in its Characteristics tab.
    await assertNoOrigin(page.locator('.spec-rows'));
    await page.getByRole('tab', { name: /^Characteristics/ }).click();
    await check(page.locator('.spec-chars')).toBeVisible();
    await assertNoOrigin(page.locator('.spec-chars'));
    await page.getByRole('tab', { name: 'Review / Edit' }).click();
    onlyLocal(requests, server.origin);
    // Leaving the page (pagehide, not into the back/forward cache) sends `closed`: the command then
    // stops 90 s later (without it, after 30 minutes), checked at the end.
    await page.goto('about:blank');
    first = server;
    await context.close();
  }
  // The PMI Specify wrote reads back.
  {
    const analysis = analyse(edited);
    const plain = (analyse(plainSave).characteristics ?? []).length;
    assert.ok((analysis.characteristics ?? []).length > plain, 'the save has the edits\' PMI');
    const kinds = new Set((analysis.characteristics ?? []).map(c => c.kind));
    assert.ok(kinds.has('datum') && kinds.has('geometric_tolerance'), `the PMI written reads back: ${[...kinds]}`);
  }
  // Reopening: the file Specify wrote opens with what it states.
  {
    const server = await open(second);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.specified-3.step — Specify');
    // ADR P11: the file states what was saved, and the panel shows it as stated, like any requirement.
    assert.equal(table(second)['part.material'].state, 'stated');
    await check(material(page)).toHaveAttribute('data-state', 'stated');
    await check(page.locator('.spec-panel')).toContainText('6082');
    await check(page.locator('.spec-row.changed')).toHaveCount(0);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    // The heartbeat: `GET config` again within 30 s of the first.
    const configs = () => requests.filter(({ url, method }) => method === 'GET' && url === `${server.url}config`).length;
    assert.equal(configs(), 1);
    await check.poll(configs, { timeout: 40000 }).toBe(2);
    // A stated row with no number is removed by its id (`items.remove: [{"id": "part.material"}]`):
    // the engine takes it, the bar lists it, and the saved file has the material as a gap.
    await material(page).getByRole('button', { name: /^Remove / }).click();
    await material(page).locator('.spec-edit.rm').getByRole('button', { name: 'Remove', exact: true }).click();
    await check(material(page).locator('.spec-problem')).toHaveCount(0);
    await check(listedEdit(page, 'part.material')).toHaveText('Remove Material: Aluminium 6082-T6');
    await save(page, server, 1);
    const withoutMaterial = (await page.locator('.app-saved code').textContent()).trim();
    assert.equal(table(withoutMaterial)['part.material'].state, 'gap', 'the material removed by its id');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // The partial save reopens with nothing stated: the questions left open are gaps.
  {
    const server = await open(plainSave);
    const { context, page, requests } = await load(server);
    await check(page).toHaveTitle('part.specified.step — Specify');
    await check(material(page)).toHaveAttribute('data-state', 'gap');
    await check(material(page)).toContainText('not stated');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // An edit list the command refuses (all or nothing: 422, every failing entry named) writes nothing,
  // and the page says which change; a failed write (the folder read-only) is said, nothing is
  // written and the edits stay listed; writable again, the same Save writes them.
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
    await add(page, 'part.general_tolerance');
    await check(listedEdit(page, 'part.general_tolerance')).toHaveText('Add General tolerance: ISO 2768-m');
    for (const id of lengthIds) await add(page, id);
    for (const id of lengthIds) await check(listedEdit(page, id)).toContainText('Add Length');
    // The page, meeting a refusal: says which change cannot be applied, and that nothing was written.
    await page.route('**/save', route => route.fulfill({ status: 422, contentType: 'application/json', body: JSON.stringify(body) }));
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.getByRole('alert').first()).toContainText('nothing was written');
    await check(page.getByRole('alert').first()).toContainText('nope.unknown');
    await page.unroute('**/save');
    await check(listedEdit(page, 'part.general_tolerance')).toBeVisible();
    for (const id of lengthIds) await check(listedEdit(page, id)).toContainText('Add Length');
    problems.splice(0, problems.length, ...problems.filter(p => !/40\d|422/.test(p)));
    assert.ok(!existsSync(join(copy, 'shaft.specified.step')), 'nothing written');
    // The write fails (the folder is read-only): Not saved, the edits still listed.
    if (process.getuid?.() !== 0) {
      chmodSync(copy, 0o555);
      try {
        await page.getByRole('button', { name: 'Save', exact: true }).click();
        await check(page.getByRole('alert').first()).toContainText('Not saved');
      } finally { chmodSync(copy, 0o755); }
      await check(page.locator('.app-saved')).toHaveCount(0);
      await check(listedEdit(page, 'part.general_tolerance')).toBeVisible();
      await check(row(page, 'part.general_tolerance')).toHaveClass(/changed/);
      problems.splice(0, problems.length, ...problems.filter(p => !/50\d/.test(p)));
      // The folder writable again, the same Save writes them.
      await page.getByRole('button', { name: 'Save', exact: true }).click();
      await check(page.locator('.app-saved')).toContainText('shaft.specified');
      // The general tolerance added in its editor, as listed, is the one the file states.
      assert.match(JSON.stringify(table(join(copy, 'shaft.specified.step'))['part.general_tolerance'].value), /ISO 2768-m/);
    }
    await context.close();
    server.stop();
  }
  // "Add all suggestions" (Paul, 2026-11-21): every gap's suggestion queued as pending edits, a
  // suggestion of none as None needed; a gap with none suggested is left alone. Discard writes nothing;
  // Save writes them as one list.
  {
    const copy = join(dir, 'all');
    mkdirSync(copy);
    const file = join(copy, 'shaft.step');
    writeFileSync(file, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const original = readFileSync(file);
    const before = table(file);
    assert.deepEqual(Object.values(before).filter(r => r.kind === 'length').map(r => r.id), lengthIds);
    const partial = table(plainSave);
    for (const id of lengthIds) {
      assert.equal(partial[id]?.open, true, `${id} stays open after a plain save`);
    }
    const suggested = Object.values(before).filter(r => r.state === 'gap' && r.suggestion !== null && r.suggestion !== undefined);
    const none = v => (Array.isArray(v) ? v.length === 0 : typeof v === 'string' && /^none\b/i.test(v.trim()));
    assert.ok(suggested.length > 5 && suggested.some(r => none(r.suggestion)) && Object.values(before).some(r => r.state === 'gap' && r.suggestion == null),
      'the part has value suggestions, none suggestions and a gap with no suggestion');
    const server = await open(file);
    const { context, page } = await load(server);
    const addAll = page.getByRole('button', { name: 'Add all suggestions', exact: true });
    await addAll.click();
    // 1. Every suggested gap is pending with its suggestion, highlighted as unsaved.
    await check(page.locator('.spec-bar li[data-pending]')).toHaveCount(suggested.length);
    await check(listedEdit(page, 'part.general_tolerance')).toHaveText('Add General tolerance: ISO 2768-m');
    await check(row(page, 'hole.position:holes:13')).toHaveClass(/changed/);
    // 5. A suggestion of none queues None needed.
    await check(listedEdit(page, 'part.coating')).toHaveText('None needed: Coating');
    await check(row(page, 'part.coating')).toHaveAttribute('data-state', 'decided_none');
    // 4. A gap with no suggestion (the material) is left alone.
    await check(listedEdit(page, 'part.material')).toHaveCount(0);
    await check(material(page)).toHaveAttribute('data-state', 'gap');
    await check(page.getByRole('alert')).toHaveCount(0);
    // Long values wrap, never clipped or under the icons (specify-ui #14), at the panel's width and
    // at a phone's.
    await assertValuesFit(page);
    if (shots) await page.screenshot({ path: join(shots, 'add-all.png') });
    const wide = page.viewportSize();
    await page.setViewportSize({ width: 360, height: 800 });
    await assertValuesFit(page);
    await page.setViewportSize(wide);
    // 3. Discard after it writes nothing: the file is byte-identical, and no copy is made.
    await page.getByRole('button', { name: 'Discard', exact: true }).click();
    await check(page.locator('.spec-bar li[data-pending]')).toHaveCount(0);
    await check(page.locator('.spec-row.changed')).toHaveCount(0);
    await page.waitForTimeout(500);
    assert.deepEqual(readFileSync(file), original, 'Discard leaves the file byte-identical');
    assert.ok(!existsSync(join(copy, 'shaft.specified.step')), 'nothing written');
    // 2. Again, and Save: one list, read back from the file.
    await addAll.click();
    await check(page.locator('.spec-bar li[data-pending]')).toHaveCount(suggested.length);
    await save(page, server, 1);
    const written = analyse(join(copy, 'shaft.specified.step'));
    const kinds = [...new Set(written.characteristics.map(c => c.kind))].sort();
    assert.deepEqual(kinds, ['datum', 'dimension', 'geometric_tolerance', 'surface_texture', 'thread'], 'the approved PMI reads back');
    const after = Object.fromEntries(written.table.rows.map(r => [r.id, r]));
    for (const r of suggested) {
      assert.equal(after[r.id].state, none(r.suggestion) ? 'decided_none' : 'stated', `${r.id} is in the file as suggested`);
    }
    assert.match(JSON.stringify(after['part.general_tolerance'].value), /ISO 2768-m/);
    assert.equal(after['part.material'].state, 'gap', 'the gap with no suggestion is still a gap');
    await context.close();
    server.stop();
  }
  // GRM-05 offers three plane lengths under The part. Bulk approval writes those rows.
  const lengths = [['length:planes:2-3', 14], ['length:planes:2-5', 5], ['length:planes:2-8', 18]];
  for (const declined of [null, lengths[0][0]]) {
    const copy = join(dir, declined ? 'grm05-decline' : 'grm05-all');
    mkdirSync(copy);
    const file = join(copy, 'grm05.step');
    writeFileSync(file, gunzipSync(readFileSync('test/fixtures/grm05_depth_lock_bolt.step.gz')));
    const before = table(file);
    for (const [id] of lengths) {
      assert.equal(before[id]?.state, 'gap', id);
      assert.equal(before[id]?.open, true, id);
    }
    const server = await open(file);
    try {
      const { context, page } = await load(server);
      try {
        const partGroup = page.locator('[data-group="part"]');
        await check(partGroup.locator('[data-row^="length:planes:"]')).toHaveCount(3);
        for (const [id, nominal] of lengths) {
          await check(partGroup.locator(`[data-row="${id}"]`)).toBeVisible();
          await check(row(page, id)).toContainText(`${nominal} ±`);
        }
        if (declined) {
          await noneNeeded(page, declined);
          await check(row(page, declined)).toHaveAttribute('data-state', 'decided_none');
        }
        await page.getByRole('button', { name: 'Add all suggestions', exact: true }).click();
        for (const [id] of lengths) {
          await check(listedEdit(page, id)).toContainText(id === declined ? 'None needed' : 'Add Length');
        }
        await save(page, server, 1);
        const written = analyse(join(copy, 'grm05.specified.step'));
        for (const [id] of lengths) {
          assert.equal(written.table.rows.find(r => r.id === id)?.state, id === declined ? 'decided_none' : 'stated', id);
        }
        const writtenLengths = written.existing.filter(item => item.type === 'Location_LinearDistance')
          .map(item => item.value).sort((a, b) => a - b);
        assert.deepEqual(writtenLengths, lengths.filter(([id]) => id !== declined).map(([, value]) => value).sort((a, b) => a - b));
      } finally { await context.close(); }
    } finally { server.stop(); }
  }
  // Read only: a viewer, no Save, and the command refuses one.
  {
    const server = await open(edited, '--read-only');
    const { context, page, requests } = await load(server);
    await check(page.locator('.pmi-lab-panel')).toContainText('PMI (view only)');
    await assertNoOrigin(page.locator('.pmi-lab-panel'));
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
    await add(page, 'part.material', 'Aluminium 6082-T6');
    await add(page, 'part.general_tolerance');
    await check(page.locator('.app-dirty')).toBeVisible();
    await assertSaveFirst(page);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(saved);
    const original = await inspectSaved(page, saved);
    assert.match(original.form2, /MATERIAL:? Aluminium 6082-T6/);
    assert.ok(existsSync(`${saved}.bak`), 'successful in-place save retained the original backup');
    // A further edit, not saved: the saved exports do not change.
    await add(page, 'part.surface_finish');
    await check(page.locator('.app-dirty')).toBeVisible();
    assert.deepEqual(await inspectSaved(page, saved), original, 'further edits leave both saved inspection downloads unchanged');
    // There is no Answers export in 0.3.0 (D4): the file is the record.
    await check(page.getByRole('button', { name: 'Answers (JSON)', exact: true })).toHaveCount(0);
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
  // "Other PMI in the file" (requirements on no recognised feature): remove only (decision 7). The
  // removed item's number is retired; every other requirement keeps its number and text.
  {
    const spindle = join(dir, 'other-pmi.step');
    writeFileSync(spindle, gunzipSync(readFileSync('test/fixtures/spindle_pmi_after.step.gz')));
    const before = analyse(spindle);
    const item = before.table.file_pmi.find(r => r.number)?.number;
    assert.ok(item, 'the spindle has numbered PMI on no recognised feature');
    const server = await open(spindle);
    const { context, page, requests } = await load(server);
    const other = row(page, item);
    await check(page.locator('[data-group="other"]')).toBeVisible();
    await check(other.locator('.acts button')).toHaveCount(1);
    await check(other.getByRole('button', { name: /^Remove / })).toHaveCount(1);
    await check(other.getByRole('button', { name: /^Change / })).toHaveCount(0);
    await other.getByRole('button', { name: /^Remove / }).click();
    await check(other.locator('.spec-edit.rm')).toContainText(`Number ${item} is retired and never reused.`);
    await other.locator('.spec-edit.rm').getByRole('button', { name: 'Remove', exact: true }).click();
    await check(listedEdit(page, item)).toContainText(`(${item})`);
    await save(page, server, 1);
    const written = join(dir, 'other-pmi.specified.step');
    const kept = c => JSON.stringify([c.number, c.kind, c.text, c.faces]);
    const after = analyse(written).characteristics.map(kept).sort();
    const expected = before.characteristics.filter(c => `#${c.number}` !== item).map(kept).sort();
    assert.deepEqual(after, expected, `only ${item} is gone; every other requirement keeps its number`);
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // Another tool's PMI (NIST CTC-01, from the NIST MBE PMI project): a panel edit leaves every
  // entity it did not name byte for byte as it was (checklist 8). Its requirements carry no
  // characteristic number, so they can be changed but not removed by number.
  {
    const nist = join(dir, 'nist-ctc-01.stp');
    writeFileSync(nist, gunzipSync(readFileSync('../tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz')));
    const server = await open(nist);
    const { context, page, requests } = await load(server);
    await check(page.locator('[data-group="other"] .spec-row').first()).toBeVisible();
    // Every "Other PMI" row has the bin (decision 7), numbered or not.
    const others = await page.locator('[data-group="other"] .spec-row').count();
    await check(page.locator('[data-group="other"] .acts button.spec-icon')).toHaveCount(others);
    await add(page, 'part.material', 'Steel');
    const firstSave = await save(page, server, 1);
    const written = readFileSync(join(dir, 'nist-ctc-01.specified.step'), 'utf8').split(/\r?\n/);
    const has = new Set(written);
    const lost = readFileSync(nist, 'utf8').split(/\r?\n/).filter(line => !has.has(line) && !/^FILE_(DESCRIPTION|NAME)\(/.test(line));
    assert.deepEqual(lost, [], 'every entity of the original is in the saved file, byte for byte');
    assert.equal(table(join(dir, 'nist-ctc-01.specified.step'))['part.material'].state, 'stated');
    // Remove an unnumbered item on the same page, following the first save immediately.
    await check(material(page)).toHaveAttribute('data-state', 'stated');
    const saved = join(dir, 'nist-ctc-01.specified.step');
    const before = analyse(saved).table.file_pmi;
    const target = before.find(r => !r.number && r.id);
    assert.ok(target, 'NIST CTC-01 has unnumbered PMI the engine names');
    const gone = row(page, target.id);
    await gone.getByRole('button', { name: /^Remove / }).click();
    await gone.locator('.spec-edit.rm').getByRole('button', { name: 'Remove', exact: true }).click();
    await check(listedEdit(page, target.id)).toBeVisible();
    await gone.getByRole('button', { name: 'Undo removal', exact: true }).click();
    await check(gone).not.toHaveClass(/removed/);
    await check(listedEdit(page, target.id)).toHaveCount(0);
    await gone.getByRole('button', { name: /^Remove / }).click();
    await gone.locator('.spec-edit.rm').getByRole('button', { name: 'Remove', exact: true }).click();
    await check(listedEdit(page, target.id)).toBeVisible();
    const secondSave = await save(page, server, 2);
    assert.equal(secondSave.requestedRev, firstSave.rev);
    assert.equal(secondSave.rev, firstSave.rev + 1);
    const left = analyse(saved).table.file_pmi;
    assert.ok(!left.some(r => r.id === target.id), `${target.id} (${target.text}) removed`);
    assert.equal(left.length, before.length - 1, 'only that one');
    onlyLocal(requests, server.origin);
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
    await check(page.locator('.spec-panel')).toBeAttached();
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
    // NEW decided coating none; Undo takes that back (the edit list's clear), and Save writes it.
    await row(page, 'part.coating').getByRole('button', { name: 'Undo', exact: true }).click();
    await check(listedEdit(page, 'part.coating')).toBeVisible();
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    const target = join(dir, `${basename(newFile).replace(/\.gz$/, '').replace(/\.(step|stp)$/, '')}.specified.step`);
    await check(page.locator('.app-saved')).toContainText(target);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    const { 'part.coating': _undone, ...kept } = own;
    assert.deepEqual(given(analyse(target).resume?.answers ?? {}), kept, 'Save writes NEW with the session\'s answers, less the one taken back');
    assert.equal(table(target)['part.coating'].state, 'gap', 'the decision taken back is a gap');
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
    await add(page, 'part.material', 'Aluminium 6082-T6');
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
  for (const child of servers) child.kill();
  await browser.close();
}
