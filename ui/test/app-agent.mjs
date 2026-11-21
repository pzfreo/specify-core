import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { chromium, expect } from '@playwright/test';
import { mcp } from './mcp-client.mjs';
// The local app as an agent uses it (specify-core docs/session.md): `specify-core mcp` over stdio,
// started as an AI client starts it. The agent opens a part and calls `show {browser: false}`; an
// answer the agent gives while the page is open lands with "Updated by your agent", on its row of the
// review panel, as unsaved. Saving is an edit list (session API 2): the agent's `save {doc, edits}`
// (refused while its own `answer`s are unsaved; `check` first) and the page's Save write the same
// naming, and an incomplete save says what is `missing`. Then an assembly (an agent's answer to a
// part not shown is said and unsaved; an edit list applies to one part, so an assembly's rows are read
// only and its save is refused until the assembly-edits update), a part opened through a symbolic link
// (the link's path is kept, Save writes beside the link), and a diff the agent opened, which `show`
// serves in diff mode.
// `npm run test:app` runs it after app-browser.mjs.
//
//   SPECIFY_CORE      the specify-core binary with `mcp`'s `show` (specify-core open-session).
//   SPECIFY_WASM_DIR  the engine's pkg/ (default: public/wasm/intent).
//   SPECIFY_APP_BUILT_IN=1  serve the app and engine built into SPECIFY_CORE (ADR P12) instead
//                     of setting SPECIFY_APP_DIR=dist-app and SPECIFY_WASM_DIR.
//   SHOTS             a directory to save screenshots in (optional).
const check = expect.configure({ timeout: 90000 });
const cli = process.env.SPECIFY_CORE ?? 'specify-core';
const app = resolve('dist-app');
const wasm = resolve(process.env.SPECIFY_WASM_DIR ?? 'public/wasm/intent');
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const builtIn = process.env.SPECIFY_APP_BUILT_IN === '1';
if (!builtIn) assert.ok(existsSync(join(app, 'index.html')), 'dist-app is built (npm run build:app)');

const dir = mkdtempSync(join(tmpdir(), 'specify-agent-'));
// Empty overrides are unset: the command's own app and engine.
const env = builtIn ? { SPECIFY_APP_DIR: '', SPECIFY_WASM_DIR: '' } : { SPECIFY_APP_DIR: app, SPECIFY_WASM_DIR: wasm };
const analyse = (file, part) => JSON.parse(execFileSync(cli, ['analyse', '--output', '-', ...(part === undefined ? [] : ['--part', String(part)]), file], { encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'ignore'] }));
/** ADR P11: what the file states, by feature-and-kind (the analysis's table). */
const table = (file, part) => Object.fromEntries((analyse(file, part).table?.rows ?? []).map(r => [r.id, r]));
/** A save from the page's origin with an edit list (session API 2). */
const post = (url, body) => fetch(`${url}save`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: new URL(url).origin }, body: JSON.stringify(body) });
/** The answers a person or agent gave: Specify's own `_` keys left out. */
const given = answers => Object.fromEntries(Object.entries(answers).filter(([id]) => !id.startsWith('_')));

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader'] });
const problems = [];
async function load(url, ready = '.pmi-lab-panel') {
  const context = await browser.newContext({ viewport: { width: 1400, height: 950 } });
  const requests = [];
  context.on('request', request => requests.push(request.url()));
  // Every message the page posts to a worker (the engine), by operation.
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
  await page.goto(url);
  await check(page.locator(ready).first()).toBeVisible();
  return { context, page, requests };
}
const onlyLocal = (requests, url) => {
  const origin = new URL(url).origin;
  assert.deepEqual(requests.filter(u => !u.startsWith(`${origin}/`) && !/^(blob|data):/.test(u)), [], 'no request leaves 127.0.0.1');
};
const question = (page, id) => page.locator(`[data-question="${id}"]`);
/** Add on the panel's gap ``id``, pre-filled with its suggestion. */
async function add(page, id) {
  const row = question(page, id);
  await row.locator('.acts').getByRole('button', { name: 'Add', exact: true }).click();
  await row.locator('.spec-edit').getByRole('button', { name: 'Add', exact: true }).click();
  await check(page.locator(`.spec-bar [data-pending="${id}"]`)).toBeVisible();
}
const agents = [];

try {
  // One part: the agent and the person on one session.
  {
    const part = join(dir, 'part.step');
    writeFileSync(part, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const opened = await agent.call('open', { path: part });
    const doc = opened.doc;
    const shown = await agent.call('show', { doc, browser: false });
    assert.equal(shown.mode, 'specify');
    assert.match(shown.url, /^http:\/\/127\.0\.0\.1:\d+\/[0-9a-f]{32}\/$/);
    const { context, page, requests } = await load(shown.url);
    await check(page).toHaveTitle('part.step — Specify');
    await check(page.locator('.app-path')).toHaveText(part);
    await check(page.locator('.pmi-lab-resumed')).toHaveCount(0);
    // An answer the agent gives now lands in the open page: said (by the question's short label,
    // #217), its question opened and lit. The note and the light stay until dismissed or the next change.
    await agent.call('answer', { doc, set: { 'part.general_tolerance': 'ISO 2768-f' } });
    await check(page.locator('.app-agent')).toContainText('Updated by your agent');
    await check(page.locator('.app-agent')).toContainText('General tolerance');
    await check(page.locator('.app-agent')).not.toContainText('Which general tolerance applies');
    await check(question(page, 'part.general_tolerance')).toBeVisible();
    await check(question(page, 'part.general_tolerance')).toHaveClass(/app-agent-lit/);
    await check(question(page, 'part.general_tolerance')).toContainText('2768-f');
    await check(page.locator('.app-dirty')).toBeVisible();
    if (shots) await page.screenshot({ path: join(shots, 'agent-update.png') });
    await page.locator('.app-agent').getByRole('button', { name: 'Dismiss' }).click();
    await check(page.locator('.app-agent')).toHaveCount(0);
    await check(page.locator('.app-agent-lit')).toHaveCount(0);
    // Saving is an edit list: while the agent's own `answer`s are unsaved, a save is refused
    // (they would be lost) and nothing is written.
    const edits = { fill: { 'part.material': 'Aluminium 6061-T6', 'part.general_tolerance': 'ISO 2768-f' } };
    const unsaved = await agent.call('save', { doc, edits }, { allowError: true });
    assert.match(JSON.stringify(unsaved), /unsaved answers/);
    assert.ok(!existsSync(join(dir, 'part.specified.step')), 'nothing written while answers are unsaved');
    await agent.call('answer', { doc, clear: ['part.general_tolerance'] });
    // `check` first: applicable, nothing written. Then the agent saves: part.specified.step beside the
    // part, incomplete, saying what is missing.
    const checked = await agent.call('check', { doc, edits });
    assert.equal(checked.applicable, true, JSON.stringify(checked));
    assert.ok(!existsSync(join(dir, 'part.specified.step')), 'check writes nothing');
    const byAgent = await agent.call('save', { doc, edits });
    assert.equal(byAgent.path, join(dir, 'part.specified.step'));
    assert.equal(byAgent.complete, false);
    assert.ok(byAgent.missing.includes('part.surface_finish'), `missing: ${byAgent.missing}`);
    assert.ok(!byAgent.missing.includes('part.material'));
    // ADR P11: the file states what the list filled (was: the answers stored, an inherited failure).
    const states = table(byAgent.path);
    assert.equal(states['part.material'].state, 'stated');
    assert.match(JSON.stringify(states['part.material'].value), /6061-T6/);
    assert.equal(states['part.general_tolerance'].state, 'stated');
    // The page hears of it, whoever saved, and says what is still unanswered.
    await check(page.locator('.app-saved')).toContainText('Your agent saved');
    await check(page.locator('.app-saved')).toContainText(byAgent.path);
    await check(page.locator('.app-saved')).toContainText('still unanswered');
    // By the question's short label, not its prompt (#217).
    await check(page.locator('.app-saved')).toContainText('Surface finish');
    await check(page.locator('.app-saved')).not.toContainText('What surface finish applies');
    await check(page.locator('.app-dirty')).toHaveCount(0);
    // The session's copy: later saves update it (#217).
    await check(page.locator('.app-target')).toHaveText(byAgent.path);
    // The page's Save updates it (an empty list: the page changed nothing), and says what is still open.
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).not.toContainText('Your agent saved');
    await check(page.locator('.app-saved')).toContainText(byAgent.path);
    await check(page.locator('.app-saved')).toContainText('still open');
    await check(page.locator('.app-saved')).toContainText('Surface finish');
    if (shots) await page.screenshot({ path: join(shots, 'save-partial.png') });
    assert.equal(table(byAgent.path)['part.material'].state, 'stated', 'the page\'s save keeps what the agent\'s wrote');
    assert.ok(!existsSync(join(dir, 'part.specified-2.step')), 'no numbered copy');
    // A save of a revision the page has not seen is refused, with the current one.
    const stale = await post(shown.url, { rev: 0, edits: {} });
    assert.equal(stale.status, 409);
    assert.equal(typeof (await stale.json()).rev, 'number');
    assert.ok(!existsSync(join(dir, 'part.specified-2.step')), 'nothing written for a stale save');
    // Killing the MCP command drops SSE. The page briefly retries, then keeps the review and
    // identifies the last file actually saved rather than the next target.
    const beforeStop = problems.length;
    agent.stop();
    await check(page.locator('.app-offline')).toContainText('reconnecting', { timeout: 10_000 });
    const stopped = page.locator('.app-stopped');
    await check(stopped).toHaveText(`Specify has stopped. Run specify-core open … again (or ask your agent to show it). Your saved file is at ${byAgent.path}.`, { timeout: 30_000 });
    await check(page.getByRole('button', { name: 'Save', exact: true })).toBeDisabled();
    await check(page.getByRole('button', { name: 'Answers (JSON)', exact: true })).toBeEnabled();
    await check(page.locator('.spec-panel')).toContainText('6061-T6');
    problems.splice(beforeStop, problems.length - beforeStop,
      ...problems.slice(beforeStop).filter(p => !/^console: .*net::ERR_CONNECTION_REFUSED$/.test(p)));
    onlyLocal(requests, shown.url);
    await context.close();
  }

  // An assembly: the agent's answers on two parts are shown, a part at a time, and an agent's answer
  // to the part not shown is said, by name, and unsaved. An edit list applies to one part, so an
  // assembly's save is refused (the assembly-edits update brings edits on parts of an assembly) and
  // nothing is written: the panel shows the rows read only, saying so, and Save is off.
  {
    const asm = join(dir, 'asm.step');
    writeFileSync(asm, gunzipSync(readFileSync('test/fixtures/gramil_assembly_before.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const listed = await agent.call('open', { path: asm });
    assert.equal(listed.needs_part, true);
    const [a, b] = listed.parts.map(p => p.part);
    const docA = (await agent.call('open', { path: asm, part: a })).doc;
    const docB = (await agent.call('open', { path: asm, part: b })).doc;
    const refused = await agent.call('save', { doc: docA, edits: { fill: { 'part.material': 'Steel 1.4301' } } }, { allowError: true });
    assert.match(JSON.stringify(refused), /assembly/);
    assert.ok(!existsSync(join(dir, 'asm.specified.step')), 'nothing written for a part of an assembly');
    await agent.call('answer', { doc: docA, set: { 'part.material': 'Steel 1.4301' } });
    await agent.call('answer', { doc: docB, set: { 'part.material': 'Brass CW614N' } });
    const shown = await agent.call('show', { doc: docB, browser: false });
    // `show` names the part: the page starts on it (not on the first part with answers).
    assert.equal((await (await fetch(`${shown.url}config`)).json()).part, b);
    const { context, page, requests } = await load(shown.url);
    await check(page.locator('.app-part select')).toBeVisible();
    await check(page.locator('.app-part select')).toHaveValue(String(b));
    await check(page.locator('.spec-panel')).toContainText('CW614N');
    await page.locator('.app-part select').selectOption(String(a));
    await check(page.locator('.spec-panel')).toContainText('1.4301');
    // The agent answers the part not shown now: the page says so, by name, and offers to show it.
    await agent.call('answer', { doc: docB, set: { 'part.general_tolerance': 'ISO 2768-f' } });
    const nameB = listed.parts.find(p => p.part === b).name;
    await check(page.locator('.app-agent')).toContainText(`Updated by your agent: ${nameB}`);
    // Unsaved is the file's, not the part shown's: the agent's answer to the other part counts.
    await check(page.locator('.app-dirty')).toBeVisible();
    await page.getByRole('button', { name: 'Show it' }).click();
    await check(page.locator('.app-part select')).toHaveValue(String(b));
    await check(page.locator('.spec-panel')).toContainText('2768-f');
    // The agent's answer is on its row, as unsaved; the rows are read only on this version, said once.
    await check(question(page, 'part.general_tolerance')).toHaveClass(/changed/);
    await check(page.locator('.spec-readonly')).toContainText('This part is in an assembly');
    await check(page.locator('.spec-panel .acts')).toHaveCount(0);
    await check(page.getByRole('button', { name: 'Save', exact: true })).toBeDisabled();
    if (shots) await page.screenshot({ path: join(shots, 'assembly.png') });
    assert.ok(!existsSync(join(dir, 'asm.specified.step')), 'nothing written');
    onlyLocal(requests, shown.url);
    await context.close();
    agent.stop();
  }

  // A part the agent opens while the page is open joins the session (`joined`): the page takes it
  // in (said, listed in the picker), and a save asked for at the rev before it is stale (409).
  {
    const asm = join(dir, 'asm-join.step');
    writeFileSync(asm, gunzipSync(readFileSync('test/fixtures/gramil_assembly_before.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const listed = await agent.call('open', { path: asm });
    const [a, b] = listed.parts.map(p => p.part);
    const nameB = listed.parts.find(p => p.part === b).name;
    const docA = (await agent.call('open', { path: asm, part: a })).doc;
    await agent.call('answer', { doc: docA, set: { 'part.material': 'Steel 1.4301' } });
    const shown = await agent.call('show', { doc: docA, browser: false });
    const origin = new URL(shown.url).origin;
    const { context, page } = await load(shown.url);
    await check(page.locator('.app-part select')).toHaveValue(String(a));
    await check(page.locator(`.app-part option[value="${b}"]`)).toHaveText(nameB);
    const before = (await (await fetch(`${shown.url}answers`)).json());
    assert.equal(String(b) in before.parts, false, 'the session holds part a only');
    // The agent opens part b: a join, with no answers.
    const docB = (await agent.call('open', { path: asm, part: b })).doc;
    await check(page.locator('.app-agent')).toContainText(`Your agent opened ${nameB}`);
    const after = (await (await fetch(`${shown.url}answers`)).json());
    assert.ok(after.rev > before.rev, 'the join is a new rev');
    assert.deepEqual(after.parts[String(b)], {}, 'part b is in the session with no answers');
    // A save at the rev before the join is refused (409), and writes nothing.
    const stale = await post(shown.url, { rev: before.rev, edits: {} });
    assert.equal(stale.status, 409);
    assert.ok(!existsSync(join(dir, 'asm-join.specified.step')), 'nothing written for the stale save');
    // The part joined is shown like the others: its rows, read only.
    await page.getByRole('button', { name: 'Show it' }).click();
    await check(page.locator('.app-part select')).toHaveValue(String(b));
    await check(page.locator('.spec-readonly')).toContainText('This part is in an assembly');
    void docB;
    await context.close();
    agent.stop();
  }

  // An edit list the command refuses (all or nothing): 422 naming every failing entry, nothing
  // written, and the page says which change; a malformed list or the API 1 body is 400.
  {
    const part = join(dir, 'refused.step');
    writeFileSync(part, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const { doc } = await agent.call('open', { path: part });
    const checked = await agent.call('check', { doc, edits: { fill: { 'part.material': 'Steel', 'nope.unknown': 1 }, items: { remove: ['#404'] } } });
    assert.equal(checked.applicable, false);
    assert.deepEqual(checked.failures.map(f => f.entry).sort(), ['fill["nope.unknown"]', 'items.remove["#404"]']);
    const shown = await agent.call('show', { doc, browser: false });
    const { context, page } = await load(shown.url);
    const { rev } = await (await fetch(`${shown.url}answers`)).json();
    const bad = await post(shown.url, { rev, edits: { fill: { 'nope.unknown': 1 } } });
    assert.equal(bad.status, 422);
    const body = await bad.json();
    assert.deepEqual(body.failures.map(f => f.entry), ['fill["nope.unknown"]']);
    assert.equal((await post(shown.url, { rev, edits: true })).status, 400, 'a boolean is no list');
    assert.equal((await post(shown.url, { rev, acceptDefaults: {} })).status, 400, 'the API 1 body is no list');
    assert.ok(!existsSync(join(dir, 'refused.specified.step')), 'nothing written');
    await add(page, 'part.general_tolerance');
    await page.route('**/save', route => route.fulfill({ status: 422, contentType: 'application/json', body: JSON.stringify(body) }));
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.getByRole('alert').first()).toContainText('nothing was written');
    await check(page.getByRole('alert').first()).toContainText('nope.unknown');
    await page.unroute('**/save');
    problems.splice(0, problems.length, ...problems.filter(p => !/40\d|422/.test(p)));
    assert.ok(!existsSync(join(dir, 'refused.specified.step')), 'nothing written');
    await context.close();
    agent.stop();
  }

  // A failed write (the folder is read-only): the page says Not saved, nothing is written, the
  // session is as it was (session API 2: the panel's edits are an edit list, never answers in it),
  // and the edit is still listed.
  if (process.getuid?.() !== 0) {
    const locked = join(dir, 'locked');
    mkdirSync(locked);
    const file = join(locked, 'shaft.step');
    writeFileSync(file, gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const agent = await mcp(cli, { cwd: locked, env }); agents.push(agent);
    const { doc } = await agent.call('open', { path: file });
    const shown = await agent.call('show', { doc, browser: false });
    const { context, page } = await load(shown.url);
    const answersBefore = (await (await fetch(`${shown.url}answers`)).json()).parts['0'] ?? {};
    await add(page, 'part.general_tolerance');
    chmodSync(locked, 0o555);
    try {
      await page.getByRole('button', { name: 'Save', exact: true }).click();
      await check(page.getByRole('alert').first()).toContainText('Not saved');
    } finally { chmodSync(locked, 0o755); }
    assert.ok(!existsSync(join(locked, 'shaft.specified.step')), 'nothing written');
    await check(page.locator('.app-saved')).toHaveCount(0);
    await check(page.locator('.spec-bar [data-pending="part.general_tolerance"]')).toBeVisible();
    const session = await (await fetch(`${shown.url}answers`)).json();
    assert.deepEqual(given(session.parts['0'] ?? {}), given(answersBefore), 'the session is as it was');
    const open = await agent.call('questions', { doc, state: 'open' });
    assert.ok(open.questions.some(q => q.id === 'part.general_tolerance'), 'the edit not saved leaves it an open question');
    // The row still shows the edit, highlighted as not saved.
    await check(question(page, 'part.general_tolerance')).toHaveClass(/changed/);
    problems.splice(0, problems.length, ...problems.filter(p => !/50\d/.test(p)));
    await context.close();
    agent.stop();
  }

  // A part opened through a symbolic link: the link's path is shown and Save writes beside it.
  {
    const real = join(dir, 'real');
    mkdirSync(real);
    writeFileSync(join(real, 'shaft.step'), gunzipSync(readFileSync('test/fixtures/gramil_shaft_before.step.gz')));
    const link = join(dir, 'linked.step');
    symlinkSync(join(real, 'shaft.step'), link);
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const { doc } = await agent.call('open', { path: link });
    const shown = await agent.call('show', { doc, browser: false });
    const config = await (await fetch(`${shown.url}config`)).json();
    assert.equal(config.path, link, 'config.path is the link as given');
    assert.equal(config.saveTarget, join(dir, 'linked.specified.step'));
    const { context, page } = await load(shown.url);
    await check(page.locator('.app-path')).toHaveText(link);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await check(page.locator('.app-saved')).toContainText(join(dir, 'linked.specified.step'));
    assert.ok(existsSync(join(dir, 'linked.specified.step')) && !existsSync(join(real, 'shaft.specified.step')), 'saved beside the link');
    await context.close();
    agent.stop();
  }
  // A diff the agent opened (`open` with `old`): `show` serves diff mode beside the new file's own
  // session (ADR P11: nothing is carried; the comparison reads the two files' PMI).
  {
    const oldFile = join(dir, 'enclosure.step');
    const newFile = join(dir, 'enclosure-rev2.step');
    writeFileSync(oldFile, gunzipSync(readFileSync('test/fixtures/enclosure_base_pmi_before.step.gz')));
    writeFileSync(newFile, gunzipSync(readFileSync('test/fixtures/enclosure_base_pmi_after.step.gz')));
    const agent = await mcp(cli, { cwd: dir, env }); agents.push(agent);
    const opened = await agent.call('open', { path: newFile, old: oldFile });
    assert.ok(opened.verdict, 'open with old compares');
    const shown = await agent.call('show', { doc: opened.doc, browser: false });
    assert.equal(shown.mode, 'diff');
    const { context, page, requests } = await load(shown.url, '.diff-summary');
    await check(page.getByRole('tab', { name: 'What changed' })).toHaveAttribute('aria-selected', 'true');
    await check(page.locator('.app-path')).toHaveText(`${oldFile} → ${newFile}`);
    // What to check: the new file's requirements on the four holes resized Ø3 → Ø3.4. (Before
    // ADR P11 a third decision asked the new Ø8 entry hole's question; diff asks no questions.)
    assert.deepEqual(opened.report.check.map(c => c.requirement), ['⌀3.4 H11', '⌖ ⌀0.2Ⓜ A B C']);
    await check(page.locator('.app-decision')).toHaveText(['Check: ⌀3.4 H11', 'Check: ⌖ ⌀0.2Ⓜ A B C']);
    // The new Ø8 entry hole has ⌀8 and its locations stated in the new file, so no feature is
    // listed as new with nothing stated.
    assert.equal(opened.report.new_unstated, undefined, 'the Ø8 hole has requirements stated');
    await check(page.locator('.diff-report')).toContainText('Ø8 × 3 through hole');
    await check(page.locator('.diff-report')).not.toContainText('nothing stated');
    // Drawn from GET diff (the session's comparison): no engine diff call.
    const ops = await page.evaluate(() => window.__engineOps);
    assert.ok(ops.includes('open') && !ops.includes('diff'), `no engine diff call in diff mode: ${ops}`);
    assert.ok(requests.some(u => u === `${shown.url}diff`), 'the comparison came from GET diff');
    onlyLocal(requests, shown.url);
    await context.close();
    agent.stop();
  }
  assert.deepEqual(problems, [], 'no page errors');
  console.log('app-agent passed');
} finally {
  for (const agent of agents) agent.stop();
  await browser.close();
}
