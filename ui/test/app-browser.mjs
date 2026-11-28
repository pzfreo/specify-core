import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { appendFileSync, chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { gunzipSync, gzipSync } from 'node:zlib';
import { chromium, expect } from '@playwright/test';
import { characteristicHistory, validateOpening, wasmDirectory } from './engine-fixture.mjs';
// The local app (dist-app, `npm run build:app`) served by the real `specify-pmi open`, the manual
// way in (no agent; test/app-agent.mjs is the agent's): on a part it loads, is titled for the file,
// and shows the editor's review screens (SpecifyReview, on the 0.3 engine through app/adapter.ts):
// what the file states as answered items "(from the file)" with a pencil and a bin, a gap as a
// suggestion card or a field. Answers given there (a suggestion kept, a value chosen or typed, a
// stated item changed or removed) are listed in the bar and written together by Save (session API 2,
// `POST save {rev, edits}`), all or nothing, or dropped by Discard; `specify-pmi analyse` reads back
// what the saved file states. Save writes
// part.specified.step beside it even with questions open, saying which; the file reopens with what was
// saved stated; read-only has no Save; an assembly's part edits save together. With
// `open --diff OLD NEW` it shows what changed, lists what needs a decision, opens NEW's
// requirements in Graphical, and Save writes NEW with the carried answers. With `--in-place`, a
// save over a file another program changed is refused as that, not as answers that moved on. With
// the command's event streams all taken, the page says it gets no live updates, and catches up
// when it gets one. Nothing is requested from anywhere but 127.0.0.1. `npm run test:app`.
//
//   SPECIFY_PMI  the specify-pmi binary (default: specify-pmi on PATH); it needs the session
//                 contract, API 2 (`GET answers`, `POST save {rev, edits}`, `open --diff`).
//   SPECIFY_WASM_DIR  the engine's pkg/ (default: public/wasm/intent, from `npm run fetch:wasm`).
//   SPECIFY_APP_BUILT_IN=1  serve the app and engine built into SPECIFY_PMI (ADR P12) instead
//                 of passing --app dist-app --wasm SPECIFY_WASM_DIR (the release's check).
//   SHOTS         a directory to save screenshots in (optional).
const check = expect.configure({ timeout: 90000 });
const cli = process.env.SPECIFY_PMI ?? process.env.SPECIFY_CORE ?? 'specify-pmi';
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
assert.ok(existsSync(join(wasm, 'specify_pmi_wasm.js')), 'the engine is fetched (npm run fetch:wasm)');
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

/** `specify-pmi open` on ``files`` (one, `--diff` and two, or `[]` for empty): its address once it prints it, and a way to stop it. */
const servers = [];
async function open(file, ...flags) {
  const opts = flags.length && typeof flags.at(-1) === 'object' ? flags.pop() : {};
  const child = spawn(cli, ['open', ...assetArgs, '--no-browser', ...flags, ...[file].flat()], { env: assetEnv, stdio: ['ignore', 'pipe', 'pipe'], ...opts });
  servers.push(child);
  let out = '';
  child.stdout.on('data', chunk => { out += chunk; });
  child.stderr.on('data', chunk => { out += chunk; });
  for (let n = 0; n < 200 && !/http:\/\/127\.0\.0\.1:\d+\/[0-9a-f]{32}\//.test(out); n++) {
    if (child.exitCode !== null) break;
    await new Promise(r => setTimeout(r, 50));
  }
  const url = out.match(/http:\/\/127\.0\.0\.1:\d+\/[0-9a-f]{32}\//)?.[0];
  assert.ok(url, `specify-pmi open printed its address: ${out}`);
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
    window.__sceneSources = [];
    const post = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (message, ...rest) {
      if (message && typeof message.operation === 'string') {
        window.__engineOps.push(message.operation);
        if (message.operation === 'scene' && typeof message.args?.sourceId === 'string') {
          window.__sceneSources.push(message.args.sourceId);
        }
      }
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
/** Question ``id`` in the review: a part setting's line or field, a feature's question, or an item
 * the file states (`.pmi-lab-stated`, its `data-state` stated, changed or removed). */
const question = (page, id) => page.locator(`[data-question="${id}"]`);
const material = page => question(page, 'part.material');
/** The suggestion card for gap ``id`` (its first button keeps the suggestion). */
const card = (page, id) => page.locator(`.pmi-lab-suggestion[data-suggestion="${id}"]`);
/** The bar's entry for question ``id``. */
const listedEdit = (page, id) => page.locator(`.spec-bar [data-pending="${id}"]`);
const allListed = page => page.locator('.spec-bar li[data-pending]');
/** The part settings folded under "Finish and notes". */
const NOTE_FIELDS = new Set(['part.surface_finish', 'part.coating', 'part.heat_treatment', 'part.edges']);
/** A value as a part setting's line shows it: from the file, or changed and not saved. */
const shows = value => new RegExp(`^${value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} \\((from the file|changed)\\)$`);
const panelReady = page => check(page.locator('.pmi-lab-panel .pmi-lab-rows')).toBeVisible();
/** Part setting ``id``'s field, opened where the review offers it: its suggestion's More…, its
 * line (under "Finish and notes" for the notes), or a stated item's pencil. */
async function partField(page, id) {
  await panelReady(page);
  const field = page.locator(`.pmi-lab-field[data-question="${id}"]`);
  if (await field.count()) return field;
  if (await card(page, id).count()) await card(page, id).getByRole('button', { name: 'More…', exact: true }).click();
  else {
    if (NOTE_FIELDS.has(id) && !await question(page, id).count()) {
      await page.locator(`[data-questions~="${id}"]`).click();
      await check(question(page, id)).toBeVisible();
    }
    const change = question(page, id).getByRole('button', { name: /^Change / });
    if (await change.count()) await change.click(); else await question(page, id).click();
  }
  await check(field).toBeVisible();
  return field;
}
/** Part setting ``id`` answered ``value`` in its field (a list, or a box typed and Enter): listed in the bar. */
/** A stored "none needed" taken back with its bin: the decision is listed to undo. */
async function takeBack(page, id) {
  await panelReady(page);
  if (!await question(page, id).count()) await page.locator(`[data-questions~="${id}"]`).click();
  const item = question(page, id);
  await check(item).toHaveAttribute('data-state', 'decided_none');
  await item.getByRole('button', { name: /^Remove / }).click();
  await check(item.locator('.pmi-lab-stated-tools.asking')).toContainText('Take back "none needed"?');
  await item.locator('.pmi-lab-stated-tools.asking button.remove').click();
  await check(item).toHaveAttribute('data-state', 'removed');
}
async function answerPart(page, id, value) {
  const field = await partField(page, id);
  const select = field.locator('select').first();
  if (await select.count()) await select.selectOption(value);
  else {
    const input = field.locator('input').first();
    await input.fill(value);
    await input.press('Enter');
  }
  await check(listedEdit(page, id)).toBeVisible();
}
/** None needed on gap ``id``: answered none in its field. */
async function noneNeeded(page, id) {
  await answerPart(page, id, 'None');
  await check(listedEdit(page, id)).toContainText('None needed');
}
/** Feature question ``id``, its row opened (the row head lists its questions). */
async function openQuestion(page, id) {
  await panelReady(page);
  const item = question(page, id);
  if (!await item.count()) await page.locator(`.pmi-lab-row-head[data-questions~="${id}"]`).click();
  await check(item).toBeVisible();
  return item;
}
/** Gap ``id``'s suggestion kept: its card's ✓, or, with its row open, its suggested option. */
async function keep(page, id) {
  await panelReady(page);
  if (await card(page, id).count()) await card(page, id).locator('.btns button').first().click();
  else {
    const item = await openQuestion(page, id);
    if (!await item.locator('.opts').count()) await item.click();
    await item.locator('.opts button.default').click();
  }
  await check(listedEdit(page, id)).toBeVisible();
}
/** The stated item ``id`` removed: its bin, then Remove in its confirmation. */
async function removeStated(page, id) {
  const item = question(page, id);
  await item.getByRole('button', { name: /^Remove / }).click();
  await item.locator('.pmi-lab-stated-tools.asking button.remove').click();
  await check(item).toHaveAttribute('data-state', 'removed');
}
/** The material the file states changed to ``value``: its pencil, the box, Enter. */
async function changeMaterial(page, value) {
  await material(page).getByRole('button', { name: 'Change Material', exact: true }).click();
  const box = material(page).getByRole('combobox', { name: 'Material' });
  await box.fill(value);
  await box.press('Enter');
  await check(material(page)).toHaveAttribute('data-state', 'changed');
  await check(material(page)).toContainText(`${value} (changed)`);
}
/** The material set back to ``value``, the file's: as stated again, no change. */
async function materialBack(page, value) {
  await material(page).getByRole('button', { name: 'Change Material', exact: true }).click();
  const box = material(page).getByRole('combobox', { name: 'Material' });
  await box.fill(value);
  await box.press('Enter');
  await check(material(page)).toHaveAttribute('data-state', 'stated');
}
/** The "Other PMI in the file" row, unfolded; its item ``key`` (a number, else the engine's id). */
const otherRow = page => page.locator('[data-row="other-pmi"]');
const otherItem = (page, key) => otherRow(page).locator(`[data-other="${key}"]`);
async function openOther(page) {
  const head = otherRow(page).locator('.pmi-lab-row-head');
  await check(head).toBeVisible();
  if (await head.getAttribute('aria-expanded') !== 'true') await head.click();
  await check(head).toHaveAttribute('aria-expanded', 'true');
}
/** Remove item ``key`` of "Other PMI in the file": its bin, then Remove in its confirmation. */
async function removeOther(page, key) {
  const item = otherItem(page, key);
  await item.getByRole('button', { name: /^Remove / }).click();
  await item.locator('.pmi-lab-stated-tools.asking button.remove').click();
  await check(item).toHaveClass(/removed/);
}
/** The general dimensions (the review's one row, "N dimensions under <general tolerance>"), each
 * whose text ``which`` takes called out at the tolerance the row shows for it: as the suggested
 * length was added, the file then states it. The bar ids of the call-outs ("Added by you", extra.N). */
async function callOutDimensions(page, which = () => true) {
  await panelReady(page);
  const head = page.locator('[data-row="dimensions"] .pmi-lab-row-head');
  if (await head.getAttribute('aria-expanded') !== 'true') await head.click();
  const lines = page.locator('[data-row="dimensions"] .pmi-lab-dim');
  await check(lines.first()).toBeVisible();
  const before = await allListed(page).evaluateAll(items => items.map(li => li.dataset.pending));
  for (let i = 0, n = await lines.count(); i < n; i++) {
    const line = lines.nth(i);
    if (!which(await line.locator('.text').textContent())) continue;
    const tolerance = (await line.locator('.tol').textContent()).replace(/^±/, '').replace(/ mm$/, '');
    await line.getByRole('button', { name: 'Call Out', exact: true }).click();
    await line.getByRole('textbox').fill(tolerance);
    await line.getByRole('button', { name: 'Use', exact: true }).click();
    await check(line).toHaveClass(/changed/);
  }
  const after = await allListed(page).evaluateAll(items => items.map(li => li.dataset.pending));
  return after.filter(id => !before.includes(id));
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
  // The part opens again from the file as saved (the review then reads what it states).
  await check(page.getByRole('button', { name: 'Save', exact: true })).toBeEnabled();
  const body = response.request().postDataJSON();
  return { rev: saved.rev, requestedRev: body.rev, body };
}
let first = null;
/** The feature-and-kinds the first two saves list: each is in the file after it, stated or decided none. */
const listed = ['part.heat_treatment', 'part.material', 'hole.position:holes:14.15', 'hole.position:holes:13', 'datum.A', 'part.coating'];
/** `specify-pmi analyse` of ``file``: what Specify reads from it (its PMI as characteristics, the answers stored). */
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
  const savedName = basename(specified);
  await check.poll(async () => (await page.evaluate(() => window.__sceneSources)).some((id) => id.startsWith(savedName) && id.includes('@'))).toBe(true);
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
      await check(material(page).locator('.value')).toHaveText(shows(value));
    };
    const change = changeMaterial;
    const switchPart = async (page, part) => {
      await page.locator('.app-part select').selectOption(String(part));
      await check(page.locator('.app-part select')).toBeEnabled();
      await check(page.locator('.app-part select')).toHaveValue(String(part));
      await panelReady(page);
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
    const gt = question(page, 'part.general_tolerance');
    await removeStated(page, 'part.general_tolerance');
    await gt.getByRole('button', { name: 'Undo', exact: true }).click();
    await check(gt).toHaveAttribute('data-state', 'stated');
    await check(listedEdit(page, 'part.general_tolerance')).toHaveCount(0);
    await removeStated(page, 'part.general_tolerance');
    await answerPart(page, 'part.coating', 'Anodised');
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
    await check(material(main)).toHaveAttribute('data-state', 'stated');
    await check(material(main).locator('.value')).toHaveText('Aluminium 6082-T6 (from the file)');
    // The removed general tolerance is a gap again: offered as a suggestion, not stated.
    await check(card(main, 'part.general_tolerance')).toBeVisible();
    await check(question(main, 'part.general_tolerance')).toHaveCount(0);
    await main.locator('[data-questions~="part.coating"]').click();
    await check(question(main, 'part.coating')).toHaveAttribute('data-state', 'stated');
    await check(question(main, 'part.coating').locator('.value')).toHaveText('Anodised (from the file)');
    await check(allPending(main)).toHaveCount(0);
    await noneNeeded(main, 'part.heat_treatment');
    await check(pending(main, 0)).toHaveText('plate · None needed: Heat treatment');
    // (rc.1 had Undo on the row; the review's part settings have none: Discard drops it.)
    await discard(main);
    await check(allPending(main)).toHaveCount(0);
    await main.getByRole('button', { name: '✓ Keep all', exact: true }).click();
    await check(pending(main, 0).filter({ hasText: 'plate · Add General tolerance' })).toBeVisible();
    await check(pending(main, 1)).toHaveCount(0);
    await discard(main);

    // The same row id in two parts must have independent values and validation. Switch back and
    // forth with both edits present, then send one POST with both part lists.
    await change(main, 'Steel 1.4301');
    await choose(main, 1, 'Bronze');
    await change(main, 'Brass CW614N');
    // Back on the plate, its change is still held and shown on its row; the pin's is beside it in the bar.
    await switchPart(main, 0);
    await check(material(main)).toHaveAttribute('data-state', 'changed');
    await check(material(main)).toContainText('Steel 1.4301');
    await check(pending(main, 0)).toHaveText('plate · Change Material: Aluminium 6082-T6 → Steel 1.4301');
    await check(pending(main, 1)).toHaveText('pin · Change Material: Bronze → Brass CW614N');
    await switchPart(main, 1);
    const both = await save(main, reopened, 1);
    assert.deepEqual(both.body.partEdits.map(p => p.part).sort(), [0, 1]);
    assert.equal(table(asm, 0)['part.material'].value, 'Steel 1.4301');
    assert.equal(table(asm, 1)['part.material'].value, 'Brass CW614N');
    await check(material(main)).toHaveAttribute('data-state', 'stated');

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
    await check(material(main).locator('.value')).toHaveText('Brass CW614N (from the file)');
    await choose(main, 0, 'Steel 1.4301');
    problems.splice(beforeErrors, problems.length - beforeErrors,
      ...problems.slice(beforeErrors).filter(p => !/^console: .*422/.test(p)));
    onlyLocal(review.requests, reopened.origin);
    await review.context.close();
    reopened.stop();
  }
  // Specify: the review's edits, listed in the bar, written together by Save, read back.
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
    // A bare part: nothing is stated; the material (no suggestion) is asked, the general tolerance
    // is offered as its suggestion; nothing is listed before an edit, and the bar says nothing is
    // written until Save.
    await check(material(page)).toHaveClass(/required/);
    await check(page.locator('.pmi-lab-stated')).toHaveCount(0);
    await check(card(page, 'part.general_tolerance').locator('.btns button').first()).toHaveText('✓ 2768-m');
    await check(allListed(page)).toHaveCount(0);
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
    // Discard drops every edit in the bar: nothing is written, and the review reads as the file has it.
    await keep(page, 'part.edges');
    await keep(page, 'part.surface_finish');
    await check(allListed(page)).toHaveCount(2);
    await check(page.locator('.app-dirty')).toBeVisible();
    await page.getByRole('button', { name: 'Discard', exact: true }).click();
    await check(allListed(page)).toHaveCount(0);
    await check(card(page, 'part.edges')).toBeVisible();
    await check(card(page, 'part.surface_finish')).toBeVisible();
    await check(page.locator('.app-dirty')).toHaveCount(0);
    assert.deepEqual(readFileSync(specified), readFileSync(plainSave), 'Discard writes nothing');
    // The general tolerance in its field, the suggestion chosen, as a class.
    // (It is saved in the refusal case below: on this base it also writes the dimensions it covers.)
    const gt = await partField(page, 'part.general_tolerance');
    await check(gt.getByRole('combobox', { name: 'General tolerance' })).toHaveValue('ISO 2768-m');
    // Or a block, row by row (#228 decision 2): the half-shell's title block, entered line by line,
    // is the one line of text the edit list carries.
    await gt.getByRole('combobox', { name: 'General tolerance' }).selectOption('By decimal places…');
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
    await check(gt.locator('.pmi-lab-gt-text code')).toContainText('fractions ±1/64, break all edges');
    await gt.getByRole('button', { name: 'Remove note 2', exact: true }).click();
    await gt.getByRole('textbox', { name: 'Angles' }).fill('±1°');
    const block = '.XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°';
    await check(gt.locator('.pmi-lab-gt-text code')).toHaveText(block);
    await gt.getByRole('button', { name: 'Use', exact: true }).click();
    // The engine takes the block (#228): its check passes and the bar shows it as entered.
    await check(listedEdit(page, 'part.general_tolerance')).toHaveText(`Add General tolerance: ${block}`);
    await check(page.getByRole('alert')).toHaveCount(0);
    await page.getByRole('button', { name: 'Discard', exact: true }).click();
    await check(allListed(page)).toHaveCount(0);
    // The part's notes: a note typed in the field is listed (the 0.3 engine takes notes as a list).
    await answerPart(page, 'part.notes', 'Deburr all edges');
    await check(listedEdit(page, 'part.notes')).toContainText('Deburr all edges');
    await page.getByRole('button', { name: 'Discard', exact: true }).click();
    await check(allListed(page)).toHaveCount(0);
    await answerPart(page, 'part.material', 'Aluminium 6082-T6');
    // The bar is not checked edit by edit; Save checks the whole list, all or nothing. A hole's
    // position before the datum it cites only waits (rule C: not a refusal, see tests/waiting_answers.rs);
    // a genuine error still refuses the whole list, writes nothing, and is the only entry named.
    await keep(page, 'hole.position:holes:13');
    const { rev: waitRev } = await (await fetch(`${server.url}answers`)).json();
    const early = await post(server, { rev: waitRev, edits: { fill: { 'part.material': 'Aluminium 6082-T6', 'nope.unknown': 1 } } });
    assert.equal(early.status, 422);
    assert.deepEqual((await early.json()).failures.map(f => f.entry), ['fill["nope.unknown"]'], 'only the genuine error refuses');
    await check(allListed(page)).toHaveCount(2);
    assert.deepEqual(readFileSync(specified), readFileSync(plainSave), 'a refused list writes nothing');
    assert.equal((server.output().match(/Saved /g) ?? []).length, 1);
    await keep(page, 'datum.A');
    await keep(page, 'hole.position:holes:14.15');
    // A choice with no none option offers none: the process's list has no such entry.
    const processField = await partField(page, 'part.process');
    await check(processField.locator('select option')).not.toHaveCount(0);
    assert.ok((await processField.locator('select option').allTextContents()).every(o => !/^none\b/i.test(o)), 'no none offered for the process');
    await check(listedEdit(page, 'part.process')).toHaveCount(0);
    // None needed records the decision.
    await noneNeeded(page, 'part.coating');
    await check(listedEdit(page, 'part.coating')).toHaveText('None needed: Coating');
    // If a None needed is refused at Save, the page says why and the bar still lists it.
    // The view's own controls are beside the panel, not under it: what is at each one's middle is it.
    const covered = await page.locator('.app-view .pmi-lab-pad button:visible, .app-view .pmi-lab-view-bar button:visible, .app-view .pmi-lab-show-bar label:visible').evaluateAll(es => es.filter(e => {
      const r = e.getBoundingClientRect();
      const at = document.elementFromPoint((r.left + r.right) / 2, (r.top + r.bottom) / 2);
      return !(at === e || e.contains(at));
    }).map(e => e.textContent));
    assert.deepEqual(covered, [], 'no view control is under the panel');
    assert.ok(await page.locator('.app-view .pmi-lab-pad button:visible').count() >= 6, 'the view has its controls');
    await check(page.locator('.app-view .pmi-lab-show')).toHaveCount(0);
    await check(page.locator('.app-view').getByLabel('Labels', { exact: true })).toHaveCount(0);
    for (const label of ['Suggested by Specify', 'In the file', 'General dimensions', 'Hide callouts facing away']) {
      await check(page.locator('.app-view .pmi-lab-show-bar').getByLabel(label, { exact: true })).toBeVisible();
    }
    await check(page.locator('.app-view .pmi-lab-view-bar').getByRole('button', { name: 'Best view', exact: true })).toBeVisible();
    assert.ok((await page.locator('.app-view .pmi-lab-view-bar button').allTextContents()).every((t) => !/^Plane \d+/.test(t)), 'views are named by direction, not Plane N');
    const beforeSave = problems.length;
    await page.route('**/save', route => route.fulfill({ status: 422, contentType: 'application/json',
      body: JSON.stringify({ error: 'nothing was written', failures: [{ entry: 'none["part.coating"]', reason: 'Coating is required here' }] }) }));
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.getByRole('alert').first()).toContainText('none["part.coating"]: Coating is required here');
    await check(listedEdit(page, 'part.coating')).toBeVisible();
    await page.unroute('**/save');
    problems.splice(beforeSave, problems.length - beforeSave,
      ...problems.slice(beforeSave).filter(p => !/^console: .*422/.test(p)));
    // A value the file cannot take is refused where it was typed, by the engine, and never joins the bar.
    const typedAt = await openQuestion(page, 'hole.function:holes:13');
    if (!await typedAt.locator('.pmi-lab-spec').count()) await typedAt.click();
    await typedAt.locator('.pmi-lab-spec input').fill('shiny');
    await typedAt.getByRole('button', { name: 'Use', exact: true }).click();
    await check(typedAt.locator('.pmi-lab-spec').getByRole('alert')).toBeVisible();
    // The panel never scrolls sideways: not with a question open, nor with 0.3's long notes title.
    const sideways = await page.locator('.pmi-lab-rows').evaluate(el => el.scrollWidth - el.clientWidth);
    assert.ok(sideways <= 0, `the panel scrolls sideways by ${sideways}px`);
    await check(listedEdit(page, 'hole.function:holes:13')).toHaveCount(0);
    // Only the unsaved edits are listed.
    await check(allListed(page)).toHaveCount(5);
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
    // After Save nothing is listed, and the review reads the file: what it states, "(from the file)".
    await check(allListed(page)).toHaveCount(0);
    await check(material(page)).toHaveAttribute('data-state', 'stated');
    await check(material(page)).toContainText('Aluminium 6082-T6 (from the file)');
    await check(page.locator('.app-dirty')).toHaveCount(0);
    await assertExportsFromSaved(page, edited);
    // Change: a change of limits keeps its number.
    const position = 'hole.position:holes:14.15';
    const number = states[position].requirements[0].number;
    const pos = await openQuestion(page, position);
    await check(pos).toHaveAttribute('data-state', 'stated');
    // Values read as a drawing writes them, not as the engine stores them.
    await check(pos).toContainText('⌖ ⌀0.2 mm A');
    // Every stated item has its pencil and its bin.
    await check(pos.getByRole('button', { name: /^Change / })).toHaveCount(1);
    await check(pos.getByRole('button', { name: /^Remove / })).toHaveCount(1);
    await pos.getByRole('button', { name: /^Change / }).click();
    await pos.locator('.opts').getByRole('button', { name: '0.1 mm', exact: true }).click();
    await check(listedEdit(page, position)).toHaveText(/^Change Ø1\.6 hole position: .*0\.2.* → .*0\.1/);
    await check(pos).toHaveAttribute('data-state', 'changed');
    // Remove: the bin asks once; its number is retired (the bar names it).
    const removed = 'hole.position:holes:13';
    const retired = states[removed].requirements[0].number;
    await openQuestion(page, removed);
    if (shots) await page.screenshot({ path: join(shots, 'panel-remove.png') });
    await removeStated(page, removed);
    await check(listedEdit(page, removed)).toContainText(`(${retired})`);
    // Every stated item has the pencil and the bin, the material too. Without a number its removal
    // names the row (`items.remove: [{"id": "part.material"}]`; saved below, on the reopened file);
    // here it is asked and cancelled, and nothing joins the bar.
    await check(material(page).getByRole('button', { name: 'Change Material', exact: true })).toHaveCount(1);
    await check(material(page).getByRole('button', { name: 'Remove Material', exact: true })).toHaveCount(1);
    await material(page).getByRole('button', { name: 'Remove Material', exact: true }).click();
    await check(material(page).getByRole('group', { name: 'Remove Material?' })).toBeVisible();
    await material(page).getByRole('button', { name: 'Cancel', exact: true }).click();
    await check(listedEdit(page, 'part.material')).toHaveCount(0);
    await check(material(page)).toHaveAttribute('data-state', 'stated');
    // The pencil and the bin match and neither is a warning colour; only the confirmation is red
    // (change list item 2: a pencil and a bin on each item, red only on the remove confirmation).
    const icons = material(page).locator('.pmi-lab-stated-tools button.icon');
    await check(icons).toHaveCount(2);
    const looks = await icons.evaluateAll(buttons => buttons.map(b => {
      const c = getComputedStyle(b); const box = b.getBoundingClientRect();
      return JSON.stringify([c.color, c.borderTopColor, c.backgroundColor, Math.round(box.width), Math.round(box.height)]);
    }));
    assert.equal(new Set(looks).size, 1, `the pencil and the bin look the same: ${looks}`);
    assert.doesNotMatch(looks[0], /192, 57, 43/, 'not red');
    await material(page).getByRole('button', { name: 'Remove Material', exact: true }).click();
    const confirm = await material(page).locator('.pmi-lab-stated-tools.asking button.remove').evaluate(b => getComputedStyle(b).backgroundColor);
    assert.equal(confirm, 'rgb(192, 57, 43)', 'the confirmation is red');
    await material(page).getByRole('button', { name: 'Cancel', exact: true }).click();
    // A change set back to the file's value is no change: its edit goes from the bar.
    const fileMaterial = (await material(page).innerText()).match(/Aluminium 6082-T6/)?.[0];
    assert.ok(fileMaterial, 'the material reads from the file');
    await changeMaterial(page, 'Steel 1.4301');
    await check(listedEdit(page, 'part.material')).toContainText('Steel 1.4301');
    await materialBack(page, fileMaterial);
    await check(listedEdit(page, 'part.material')).toHaveCount(0);
    await check(material(page)).toHaveAttribute('data-state', 'stated');
    // Removed after a change, then Undo: the change comes back, listed as it was.
    await changeMaterial(page, 'Steel 1.4301');
    await material(page).getByRole('button', { name: 'Remove Material', exact: true }).click();
    await material(page).locator('.pmi-lab-stated-tools.asking button.remove').click();
    await check(listedEdit(page, 'part.material')).toContainText('Remove Material');
    await material(page).getByRole('button', { name: 'Undo', exact: true }).click();
    await check(listedEdit(page, 'part.material')).toContainText('Steel 1.4301');
    await materialBack(page, fileMaterial);
    await check(listedEdit(page, 'part.material')).toHaveCount(0);
    // The decision the file stores (coating: none needed) is an item with its bin, which takes it
    // back (the edit list's clear): the item is open again.
    await takeBack(page, 'part.coating');
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
    await check(allListed(page)).toHaveCount(0);
    if (shots) await page.screenshot({ path: join(shots, 'panel-saved.png') });
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
    await check(page.locator('.pmi-lab-panel')).toContainText('6082');
    await check(allListed(page)).toHaveCount(0);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    // The heartbeat: `GET config` again within 30 s of the first.
    const configs = () => requests.filter(({ url, method }) => method === 'GET' && url === `${server.url}config`).length;
    assert.equal(configs(), 1);
    await check.poll(configs, { timeout: 40000 }).toBe(2);
    // A stated row with no number is removed by its id (`items.remove: [{"id": "part.material"}]`):
    // the engine takes it, the bar lists it, and the saved file has the material as a gap.
    await removeStated(page, 'part.material');
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
    await check(page.locator('.pmi-lab-stated')).toHaveCount(0);
    await check(material(page)).toHaveClass(/required/);
    await check(material(page)).toContainText('required');
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
    await keep(page, 'part.general_tolerance');
    await check(listedEdit(page, 'part.general_tolerance')).toHaveText('Add General tolerance: ISO 2768-m');
    // The general dimensions are one row (change list item 1): each called out, as the lengths were added.
    const lengthEdits = await callOutDimensions(page);
    assert.equal(lengthEdits.length, lengthIds.length, 'a call-out per general dimension');
    for (const id of lengthEdits) await check(listedEdit(page, id)).toContainText('Add Call out');
    // The page, meeting a refusal: says which change cannot be applied, and that nothing was written.
    await page.route('**/save', route => route.fulfill({ status: 422, contentType: 'application/json', body: JSON.stringify(body) }));
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.getByRole('alert').first()).toContainText('nothing was written');
    await check(page.getByRole('alert').first()).toContainText('nope.unknown');
    await page.unroute('**/save');
    await check(listedEdit(page, 'part.general_tolerance')).toBeVisible();
    for (const id of lengthEdits) await check(listedEdit(page, id)).toContainText('Add Call out');
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
      for (const id of lengthEdits) await check(listedEdit(page, id)).toContainText('Add Call out');
      problems.splice(0, problems.length, ...problems.filter(p => !/50\d/.test(p)));
      // The folder writable again, the same Save writes them.
      await page.getByRole('button', { name: 'Save', exact: true }).click();
      await check(page.locator('.app-saved')).toContainText('shaft.specified');
      // The general tolerance kept, as listed, is the one the file states.
      assert.match(JSON.stringify(table(join(copy, 'shaft.specified.step'))['part.general_tolerance'].value), /ISO 2768-m/);
    }
    await context.close();
    server.stop();
  }
  // "✓ Keep all" (the review's; rc.1's "Add all suggestions", Paul, 2026-11-21): every suggestion the
  // review offers queued as pending edits; a gap with none suggested is left alone. The general
  // dimensions, one row, are each called out at the tolerance it shows. Discard writes nothing;
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
    assert.ok(suggested.length > 5 && Object.values(before).some(r => r.state === 'gap' && r.suggestion == null),
      'the part has value suggestions and a gap with no suggestion');
    const server = await open(file);
    const { context, page } = await load(server);
    const keepAll = page.getByRole('button', { name: '✓ Keep all', exact: true });
    const offered = Number((await page.locator('.pmi-lab-review').textContent()).match(/(\d+) suggestions?/)?.[1]);
    assert.ok(offered > 5, `the review offers its suggestions: ${offered}`);
    const lengthEdits = await callOutDimensions(page);
    assert.equal(lengthEdits.length, lengthIds.length);
    await keepAll.click();
    // 1. Every suggestion is pending with its value.
    await check(allListed(page)).toHaveCount(offered + lengthIds.length);
    await check(listedEdit(page, 'part.general_tolerance')).toHaveText('Add General tolerance: ISO 2768-m');
    await check(listedEdit(page, 'hole.position:holes:13')).toBeVisible();
    // 4. A gap with no suggestion (the material) is left alone.
    await check(listedEdit(page, 'part.material')).toHaveCount(0);
    await check(material(page)).toHaveClass(/required/);
    await check(page.getByRole('alert')).toHaveCount(0);
    if (shots) await page.screenshot({ path: join(shots, 'add-all.png') });
    // 3. Discard after it writes nothing: the file is byte-identical, and no copy is made.
    await page.getByRole('button', { name: 'Discard', exact: true }).click();
    await check(allListed(page)).toHaveCount(0);
    await page.waitForTimeout(500);
    assert.deepEqual(readFileSync(file), original, 'Discard leaves the file byte-identical');
    assert.ok(!existsSync(join(copy, 'shaft.specified.step')), 'nothing written');
    // 2. Again, and Save: one list, read back from the file. (Fails today: after Discard the
    // adapter's baseline still holds the discarded answers, so keeping the same suggestions again
    // lists nothing; main.tsx onDiscard does not reset `shown.answers`. A product bug, reported.)
    await callOutDimensions(page);
    await keepAll.click();
    await check(allListed(page)).toHaveCount(offered + lengthIds.length);
    const { body } = await save(page, server, 1);
    const written = analyse(join(copy, 'shaft.specified.step'));
    const kinds = [...new Set(written.characteristics.map(c => c.kind))].sort();
    assert.deepEqual(kinds, ['datum', 'dimension', 'geometric_tolerance', 'surface_texture', 'thread'], 'the approved PMI reads back');
    const after = Object.fromEntries(written.table.rows.map(r => [r.id, r]));
    const kept = [...Object.keys(body.edits.fill ?? {}), ...(body.edits.none ?? [])].filter(id => id in before);
    assert.ok(kept.length >= offered - 1, `the suggestions kept are rows of the file: ${kept}`);
    for (const id of kept) {
      assert.equal(after[id].state, none(before[id].suggestion) ? 'decided_none' : 'stated', `${id} is in the file as suggested`);
    }
    for (const id of lengthIds) assert.equal(after[id].state, 'stated', `${id} is in the file, called out`);
    assert.match(JSON.stringify(after['part.general_tolerance'].value), /ISO 2768-m/);
    assert.equal(after['part.material'].state, 'gap', 'the gap with no suggestion is still a gap');
    await context.close();
    server.stop();
  }
  // GRM-05 has three plane lengths among its general dimensions (one row, change list item 1).
  // Called out, the file states those rows; one not called out stays a gap and is not written.
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
        const head = page.locator('[data-row="dimensions"] .pmi-lab-row-head');
        await check(head).toContainText('dimensions under ISO 2768-m');
        await head.click();
        const lines = page.locator('[data-row="dimensions"] .pmi-lab-dim');
        await check(lines.filter({ hasText: /\d mm from / })).toHaveCount(3);
        for (const [, nominal] of lengths) {
          await check(lines.filter({ has: page.locator('.text', { hasText: new RegExp(`^${nominal} mm from `) }) })).toHaveCount(1);
          await check(lines.filter({ has: page.locator('.text', { hasText: new RegExp(`^${nominal} mm from `) }) }).locator('.tol')).toHaveText(/^±/);
        }
        const declinedNominal = lengths.find(([id]) => id === declined)?.[1];
        const called = await callOutDimensions(page, text => / from /.test(text) && !text.startsWith(`${declinedNominal} mm from `));
        assert.equal(called.length, declined ? 2 : 3);
        await page.getByRole('button', { name: '✓ Keep all', exact: true }).click();
        await check(listedEdit(page, 'part.general_tolerance')).toBeVisible();
        for (const id of called) await check(listedEdit(page, id)).toContainText('Add Call out');
        await save(page, server, 1);
        const written = analyse(join(copy, 'grm05.specified.step'));
        for (const [id] of lengths) {
          assert.equal(written.table.rows.find(r => r.id === id)?.state, id === declined ? 'gap' : 'stated', id);
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
    await answerPart(page, 'part.material', 'Aluminium 6082-T6');
    await keep(page, 'part.general_tolerance');
    await check(page.locator('.app-dirty')).toBeVisible();
    await assertSaveFirst(page);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(saved);
    const original = await inspectSaved(page, saved);
    assert.match(original.form2, /MATERIAL:? Aluminium 6082-T6/);
    assert.ok(existsSync(`${saved}.bak`), 'successful in-place save retained the original backup');
    // A further edit, not saved: the saved exports do not change.
    await keep(page, 'part.surface_finish');
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
  // "Other PMI in the file" (requirements on no recognised feature): remove only, a bin per item. The
  // removed item's number is retired; every other requirement keeps its number and text.
  {
    const spindle = join(dir, 'other-pmi.step');
    writeFileSync(spindle, gunzipSync(readFileSync('test/fixtures/spindle_pmi_after.step.gz')));
    const before = analyse(spindle);
    const item = before.table.file_pmi.find(r => r.number)?.number;
    assert.ok(item, 'the spindle has numbered PMI on no recognised feature');
    const server = await open(spindle);
    const { context, page, requests } = await load(server);
    await openOther(page);
    const other = otherItem(page, item);
    await check(other).toBeVisible();
    await check(other.getByRole('button', { name: /^Remove / })).toHaveCount(1);
    await check(other.getByRole('button', { name: /^Change / })).toHaveCount(0);
    await removeOther(page, item);
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
  // An inch file: each length says its own unit. What the file states is in inches; suggestions, geometry
  // and what is typed, in the millimetres Specify works in; and what Specify then writes reads back in mm.
  {
    const inch = join(dir, 'inch.stp');
    writeFileSync(inch, gunzipSync(readFileSync('../tests/fixtures/nist/nist_ctc_03_asme1_ap242.stp.gz')));
    const server = await open(inch);
    const { context, page, requests } = await load(server);
    await panelReady(page);
    await check(page.locator('.pmi-lab-unit')).toHaveText('File in inches · Specify works in mm');
    await check(page.locator('.pmi-lab-row-head .sum').filter({ hasText: '⌀1.065 ±0.003' })).toContainText('⌀1.065 ±0.003 in');
    await check(page.locator('.pmi-lab-suggestion .why').filter({ hasText: 'this ⌀27.051 mm hole' }).first()).toBeVisible();
    await check(page.locator('.pmi-lab-row-head b').filter({ hasText: /^Ø50\.8 mm hole$/ })).toHaveCount(1);
    const [first] = await callOutDimensions(page, text => text.startsWith('57.2795 mm from '));
    await check(listedEdit(page, first)).toContainText(/±0\.3 mm$/);
    const lengths = await page.locator('.pmi-lab-dim .text').allTextContents();
    assert.ok(lengths.length > 5 && lengths.every(t => /^[^ ]+ mm from |^⌀[\d.]+ mm/.test(t)), `every general dimension says mm: ${lengths}`);
    await save(page, server, 1);
    const wrote = analyse(join(dir, 'inch.specified.stp').replace(/\.stp$/, existsSync(join(dir, 'inch.specified.stp')) ? '.stp' : '.step'));
    const made = wrote.existing.find(e => e.value && Math.abs(e.value - 57.2795) < 1e-3 && e.upper);
    assert.deepEqual([made?.upper, made?.lower], [0.3, -0.3], 'typed in mm, written in mm');
    // Its inspection files say which is which: the text in the unit it was written in, the numbers in mm.
    const list = readFileSync(await downloadFile(page, 'Inspection list (CSV)', 'inch.specified-characteristics.csv'), 'utf8').trimEnd().split('\n');
    assert.ok(list[0].endsWith(',Measured,Unit'), list[0]);
    assert.ok(list.some(l => l.includes('(0.75) in from left face,19.05,')), `stated in inches, its number in mm: ${list}`);
    assert.ok(list.some(l => /57\.27954 ±0\.3 mm from .*,57\.27954,0\.3,-0\.3,/.test(l) && l.endsWith(',mm')), 'called out in mm');
    assert.ok(list.every(l => !/\d\.\d{9,}/.test(l)), 'no float noise');
    assert.ok(list.some(l => l.includes('⌀11.125 mm hole')), 'a feature\'s name is in mm, and says so');
    assert.ok(list.some(l => l.includes('2 bottom faces')) && !list.some(l => / mm (?:\w+ )?faces/.test(l)), 'a count of faces is no length');
    assert.ok(!(await page.locator('.pmi-lab-panel').textContent()).match(/\d mm (?:top|bottom|left|right|front|back) faces/), 'nor on the panel');
    const form3 = workbookSheet(await downloadFile(page, 'AS9102 FAI (Excel)', 'inch.specified-AS9102.xlsx'), 3);
    assert.match(form3, /56\.97954–57\.57954 mm</);
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  {
    const metric = join(dir, 'metric.step');
    writeFileSync(metric, gunzipSync(readFileSync('test/fixtures/spindle_pmi_after.step.gz')));
    const server = await open(metric, '--read-only');
    const { context, page } = await load(server);
    await check(page.locator('.pmi-lab-unit')).toHaveText('Units: mm');
    await context.close();
    server.stop();
  }
  // Saved to a copy, the page shows the copy: its name, its path and its notes once; and shows it again reloaded.
  // Removed by someone else, a reload says so and opens the file as it was opened.
  {
    const named = join(dir, 'named.step');
    writeFileSync(named, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const copy = join(dir, 'named.specified.step');
    const server = await open(named);
    const { context, page, requests } = await load(server);
    const shows = async (name, path) => {
      await check(page).toHaveTitle(`${name} — Specify`);
      await check(page.locator('.app-file')).toHaveText(name);
      await check(page.locator('.app-path')).toHaveText(path);
      await check(page.locator('.pmi-lab-panel .title strong')).toHaveText(name);
    };
    await shows('named.step', named);
    await page.locator('[data-suggestion="part.general_tolerance"] .btns button').first().click();
    await check(listedEdit(page, 'part.general_tolerance')).toBeVisible();
    await save(page, server, 1);
    await shows('named.specified.step', copy);
    await check(page.locator('.pmi-lab-stated[data-question="part.general_tolerance"]')).toHaveCount(1);
    await page.reload();
    await panelReady(page);
    await shows('named.specified.step', copy);
    await check(page.locator('.pmi-lab-stated[data-question="part.general_tolerance"]')).toHaveCount(1);
    await check(page.locator('.app-inspection-source')).toContainText('from named.specified.step');
    // The next save, after the reload, updates the copy, with both.
    await page.locator('[data-suggestion="part.process"] .btns button').first().click();
    await check(listedEdit(page, 'part.process')).toBeVisible();
    await save(page, server, 2);
    assert.ok(!existsSync(join(dir, 'named.specified-2.step')), 'the copy is updated, not numbered');
    for (const id of ['part.general_tolerance', 'part.process']) assert.equal(table(copy)[id].state, 'stated', id);
    const beforeGone = problems.length;
    renameSync(copy, join(dir, 'moved.step'));
    await page.reload();
    await panelReady(page);
    await check(page.locator('.app-problem').first()).toContainText('The file saved earlier could not be shown, so this is the file as opened');
    await shows('named.step', named);
    problems.splice(beforeGone, problems.length - beforeGone, ...problems.slice(beforeGone).filter(p => !/^console: .*404/.test(p)));
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // An edit landing while Save runs is not lost when the save reopens the file: only the edits sent
  // are cleared, and the next Save writes the remaining one too.
  {
    const concurrent = join(dir, 'concurrent.step');
    writeFileSync(concurrent, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const copy = join(dir, 'concurrent.specified.step');
    const server = await open(concurrent);
    const { context, page, requests } = await load(server);
    await keep(page, 'part.general_tolerance');
    await check(listedEdit(page, 'part.general_tolerance')).toBeVisible();
    // Hold the next worker `review` request so a second edit is in flight when Save posts, then
    // release it while POST /save is in flight so it lands in localPending before Save finishes.
    await page.evaluate(() => {
      const post = Worker.prototype.postMessage;
      window.__heldReview = false;
      Worker.prototype.postMessage = function (message, ...rest) {
        if (message && message.operation === 'review') {
          Worker.prototype.postMessage = post;
          window.__heldReview = true;
          window.__releaseReview = () => post.call(this, message, ...rest);
          return;
        }
        return post.call(this, message, ...rest);
      };
    });
    await card(page, 'part.process').locator('.btns button').first().click();
    await check.poll(() => page.evaluate(() => window.__heldReview)).toBe(true);
    await check(listedEdit(page, 'part.process')).toHaveCount(0);
    let sentBody = null;
    await page.route('**/save', async route => {
      sentBody = route.request().postDataJSON();
      await page.evaluate(() => window.__releaseReview());
      await check(listedEdit(page, 'part.process')).toBeVisible();
      await route.continue();
    });
    const firstReply = page.waitForResponse(
      response => response.url() === `${server.url}save` && response.request().method() === 'POST',
      { timeout: 90000 },
    );
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    const response = await firstReply;
    assert.equal(response.status(), 200);
    await page.unroute('**/save');
    assert.deepEqual(Object.keys(sentBody.edits.fill ?? {}), ['part.general_tolerance'], 'the first save sent only the first edit');
    await check(page.locator('.app-saved code')).toHaveText(copy);
    await check(page.getByRole('button', { name: 'Save', exact: true })).toBeEnabled();
    // The sent edit is cleared and in the file; the edit made while Save ran stays listed and is not yet in the file.
    await check(listedEdit(page, 'part.general_tolerance')).toHaveCount(0);
    await check(listedEdit(page, 'part.process')).toBeVisible();
    assert.equal(table(copy)['part.general_tolerance'].state, 'stated');
    assert.equal(table(copy)['part.process'].state, 'gap');
    // A second Save writes the remaining edit too.
    await save(page, server, 2);
    await check(allListed(page)).toHaveCount(0);
    assert.equal(table(copy)['part.general_tolerance'].state, 'stated');
    assert.equal(table(copy)['part.process'].state, 'stated');
    // Same-row edit landing while Save runs: a retained fill on a newly filled row is rebuilt as a
    // change expecting the saved value, and an in-flight edit back to the pre-save value is kept as a change.
    const holdNextReview = () => page.evaluate(() => {
      const post = Worker.prototype.postMessage;
      window.__heldReview = false;
      Worker.prototype.postMessage = function (message, ...rest) {
        if (message && message.operation === 'review') {
          Worker.prototype.postMessage = post;
          window.__heldReview = true;
          window.__releaseReview = () => post.call(this, message, ...rest);
          return;
        }
        return post.call(this, message, ...rest);
      };
    });
    await answerPart(page, 'part.material', 'Steel');
    await check(listedEdit(page, 'part.material')).toContainText('Steel');
    const matField = await partField(page, 'part.material');
    await holdNextReview();
    const matInput = matField.locator('input').first();
    await matInput.fill('Brass');
    await matInput.press('Enter');
    await check.poll(() => page.evaluate(() => window.__heldReview)).toBe(true);
    let sameRowBody = null;
    await page.route('**/save', async route => {
      sameRowBody = route.request().postDataJSON();
      await page.evaluate(() => window.__releaseReview());
      await check(listedEdit(page, 'part.material')).toContainText('Brass');
      await route.continue();
    });
    const sameRowReply = page.waitForResponse(
      r => r.url() === `${server.url}save` && r.request().method() === 'POST',
      { timeout: 90000 },
    );
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    assert.equal((await sameRowReply).status(), 200);
    await page.unroute('**/save');
    assert.equal(sameRowBody.edits.fill?.['part.material'], 'Steel', 'first save wrote Steel');
    await check(page.getByRole('button', { name: 'Save', exact: true })).toBeEnabled();
    await check(listedEdit(page, 'part.material')).toContainText('Change Material: Steel → Brass');
    assert.equal(table(copy)['part.material'].value, 'Steel');
    // Next Save applies the rebuilt change cleanly.
    await save(page, server, 4);
    await check(allListed(page)).toHaveCount(0);
    assert.equal(table(copy)['part.material'].value, 'Brass');
    // Stated Brass changed to Aluminium, then an in-flight edit back to Brass lands while Save runs:
    // it must not vanish, and the next Save restores Brass.
    await changeMaterial(page, 'Aluminium 6082-T6');
    await check(listedEdit(page, 'part.material')).toContainText('Brass → Aluminium 6082-T6');
    await material(page).getByRole('button', { name: 'Change Material', exact: true }).click();
    const revertBox = material(page).getByRole('combobox', { name: 'Material' });
    await holdNextReview();
    await revertBox.fill('Brass');
    await revertBox.press('Enter');
    await check.poll(() => page.evaluate(() => window.__heldReview)).toBe(true);
    let revertBody = null;
    await page.route('**/save', async route => {
      revertBody = route.request().postDataJSON();
      await page.evaluate(() => window.__releaseReview());
      await check(listedEdit(page, 'part.material')).toHaveCount(0);
      await route.continue();
    });
    const revertReply = page.waitForResponse(
      r => r.url() === `${server.url}save` && r.request().method() === 'POST',
      { timeout: 90000 },
    );
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    assert.equal((await revertReply).status(), 200);
    await page.unroute('**/save');
    assert.deepEqual(revertBody.edits.change?.['part.material'], { to: 'Aluminium 6082-T6', expect: 'Brass' });
    await check(page.getByRole('button', { name: 'Save', exact: true })).toBeEnabled();
    await check(listedEdit(page, 'part.material')).toContainText('Change Material: Aluminium 6082-T6 → Brass');
    assert.equal(table(copy)['part.material'].value, 'Aluminium 6082-T6');
    await save(page, server, 6);
    await check(allListed(page)).toHaveCount(0);
    assert.equal(table(copy)['part.material'].value, 'Brass');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // A dimension the file states with its nominal alone (under the general tolerance) is called out
  // in place: its feature-and-kind changed to take the tolerance; nothing is added beside it, and
  // every other requirement keeps its number and text.
  {
    const spindle = join(dir, 'stated-call-out.step');
    writeFileSync(spindle, gunzipSync(readFileSync('test/fixtures/spindle_pmi_after.step.gz')));
    const before = analyse(spindle);
    const bare = before.existing.find(e => e.kind === 'dimension' && e.value != null && e.upper == null && e.lower == null && !e.basic && e.number);
    assert.ok(bare, 'the spindle states a dimension with its nominal alone');
    const row = before.table.rows.find(r => r.requirements?.some(q => q.number === `#${bare.number}`));
    const server = await open(spindle);
    const { context, page, requests } = await load(server);
    await panelReady(page);
    const head = page.locator('[data-row="dimensions"] .pmi-lab-row-head');
    if (await head.getAttribute('aria-expanded') !== 'true') await head.click();
    const line = page.locator(`[data-row="dimensions"] .pmi-lab-dim[data-characteristic="${bare.number}"]`);
    await line.getByRole('button', { name: 'Call Out', exact: true }).click();
    await line.getByRole('textbox').fill('0.05');
    await line.getByRole('button', { name: 'Use', exact: true }).click();
    await check(line).toHaveClass(/changed/);
    await check(listedEdit(page, row.id)).toContainText(`${row.value} ±0.05 mm`);
    await save(page, server, 1);
    const after = analyse(join(dir, 'stated-call-out.specified.step'));
    const now = after.existing.find(e => e.number === bare.number);
    assert.deepEqual([now?.value, now?.upper, now?.lower], [bare.value, 0.05, -0.05], `#${bare.number} now carries ±0.05`);
    assert.equal(after.characteristics.length, before.characteristics.length, 'nothing added beside it');
    const kept = c => JSON.stringify([c.number, c.kind, c.text, c.faces]);
    assert.deepEqual(after.characteristics.filter(c => c.number !== bare.number).map(kept).sort(),
      before.characteristics.filter(c => c.number !== bare.number).map(kept).sort(), 'every other requirement as it was');
    onlyLocal(requests, server.origin);
    await context.close();
    server.stop();
  }
  // Another tool's PMI (NIST CTC-01, from the NIST MBE PMI project): an edit in the review leaves every
  // entity it did not name byte for byte as it was (checklist 8). Its requirements carry no
  // characteristic number, so they can be changed but not removed by number.
  {
    const nist = join(dir, 'nist-ctc-01.stp');
    writeFileSync(nist, gunzipSync(readFileSync('../tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz')));
    const server = await open(nist);
    const { context, page, requests } = await load(server);
    await openOther(page);
    await check(otherRow(page).locator('[data-other]').first()).toBeVisible();
    // Every "Other PMI" item has the bin, numbered or not.
    const others = await otherRow(page).locator('[data-other]').count();
    await check(otherRow(page).locator('[data-other] button[aria-label^="Remove "]')).toHaveCount(others);
    await answerPart(page, 'part.material', 'Steel');
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
    await openOther(page);
    const gone = otherItem(page, target.id);
    await removeOther(page, target.id);
    await check(listedEdit(page, target.id)).toBeVisible();
    await gone.getByRole('button', { name: 'Undo', exact: true }).click();
    await check(gone).not.toHaveClass(/removed/);
    await check(listedEdit(page, target.id)).toHaveCount(0);
    await removeOther(page, target.id);
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
    // NEW carries PMI: the single PMI view draws it directly on 3D annotation planes with no View PMI mode switch.
    await check(page.getByRole('radiogroup', { name: 'View PMI' })).toHaveCount(0);
    await check.poll(async () => Number(await page.locator('.app-view [data-drawn-graphics]').getAttribute('data-drawn-graphics'))).toBeGreaterThan(0);
    if (shots) await page.screenshot({ path: join(shots, 'diff-decide.png') });
    // NEW decided coating none; given a value that is taken back (the edit list's clear, with the
    // fill: one entry), and Save writes it. (rc.1's Undo left it a gap; the review has no such Undo.)
    await answerPart(page, 'part.coating', 'Anodised');
    await check(listedEdit(page, 'part.coating')).toHaveText('Add Coating: Anodised');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    const target = join(dir, `${basename(newFile).replace(/\.gz$/, '').replace(/\.(step|stp)$/, '')}.specified.step`);
    await check(page.locator('.app-saved')).toContainText(target);
    await check(page.locator('.app-dirty')).toHaveCount(0);
    const { 'part.coating': _undone, ...kept } = own;
    assert.deepEqual(given(analyse(target).resume?.answers ?? {}), kept, 'Save writes NEW with the session\'s answers, less the one taken back');
    assert.equal(table(target)['part.coating'].state, 'stated', 'the decision taken back, the value given in its place');
    assert.match(JSON.stringify(table(target)['part.coating'].value), /Anodised/);
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
    await answerPart(page, 'part.material', 'Aluminium 6082-T6');
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
  // `specify-pmi open` without a file: opens the empty viewer with a drop target / file picker,
  // refuses a .step.gz expanding past 64 MiB without consuming the upload, then picks the tracked
  // GRM-01 fixture (.step.gz) on the same page, shows the review, and saves into the working dir.
  {
    const grm01 = resolve('../tests/fixtures/parts/GRM-01_shank.step.gz');
    assert.ok(existsSync(grm01), 'tests/fixtures/parts/GRM-01_shank.step.gz is present');
    const emptyDir = join(dir, 'empty-open');
    mkdirSync(emptyDir);
    const oversizedGz = join(dir, 'too-big.step.gz');
    writeFileSync(oversizedGz, gzipSync(Buffer.alloc(64 * 1024 * 1024 + 1, 0x20), { level: 1 }));
    const server = await open([], { cwd: emptyDir });
    assert.match(server.output(), /^Open: http:\/\/127\.0\.0\.1:\d+\/[0-9a-f]{32}\//);
    const { context, page, requests } = await load(server, '.app-drop-area');
    await check(page).toHaveTitle('Specify');
    await check(page.locator('.app-drop-area')).toContainText('Drop a STEP file here');
    await check(page.getByRole('button', { name: 'Choose file…' })).toBeVisible();
    await check(page.locator('input[type="file"]')).toHaveAttribute('accept', '.step,.stp,.step.gz,.stp.gz');
    const before = problems.length;
    const [refusedChooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.getByRole('button', { name: 'Choose file…' }).click(),
    ]);
    await refusedChooser.setFiles(oversizedGz);
    await check(page.getByRole('alert')).toContainText('67108864');
    await check(page.locator('.app-drop-area')).toBeVisible();
    await check(page.getByRole('button', { name: 'Choose file…' })).toBeEnabled();
    // The refused upload's 413 is the browser's console error, expected here.
    problems.splice(before, problems.length - before, ...problems.slice(before).filter(p => !/413/.test(p)));
    const [chooser] = await Promise.all([
      page.waitForEvent('filechooser'),
      page.getByRole('button', { name: 'Choose file…' }).click(),
    ]);
    await chooser.setFiles(grm01);
    await panelReady(page);
    await check(page.locator('.app-drop-area')).toHaveCount(0);
    await check(page).toHaveTitle('GRM-01_shank.step.gz — Specify');
    const expectedSaved = join(realpathSync(emptyDir), 'GRM-01_shank.specified.step');
    await check(page.locator('.app-target')).toHaveText(expectedSaved);
    await answerPart(page, 'part.material', 'Steel');
    await save(page, server, 1);
    assert.ok(existsSync(expectedSaved), 'Save wrote GRM-01_shank.specified.step in working directory');
    assert.equal(table(expectedSaved)['part.material'].value, 'Steel');
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
