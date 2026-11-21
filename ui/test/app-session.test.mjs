import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createSession, createSessionStore, reapply } from '../app/answers.ts';
import { getDiff, Refused, save as postSave } from '../app/local.ts';
import { CannotSend, defaultParts, DefaultsChanged, editsFor, EditsRefused, FileChanged, matchMissing, NotShown, sessionSaver, Stale, writtenParts } from '../app/save.ts';
// The local app's answers store over the command's session (specify-core docs/open.md, session API 2):
// the page's changes stay in the page until Save sends them as an edit list (`POST save {rev, edits}`);
// events from others (an agent's answers) land under them.

const none = async () => [];

// Full answer replay history was removed with reconstruction; retain disk-baseline coverage.
test('a delayed saved revision still tracks disk answers after further edits', () => {
  const metadata = { schema: 2, high_water: 17, entries: [] };
  const saved = { 'part.material': 'Steel', 'part.surface_finish': 'Ra 3.2', _specify_characteristic_numbers: metadata };
  const session = createSession(0, { 0: {} });
  session.receive({ rev: 1, part: 0, answers: saved, source: 'page' });
  session.receive({ rev: 2, part: 0, answers: { 'part.material': 'Brass' }, source: 'page' });
  session.markSaved(1);
  assert.equal(session.dirty(), true);
  assert.deepEqual(session.parts(), { 0: { 'part.material': 'Brass' } });
  session.receive({ rev: 3, part: 0, answers: saved, source: 'page' });
  assert.equal(session.dirty(), false, 'the saved answers remain the disk baseline, independent of current answers');
});

// Session API 2: there are no answer PUTs to wait for. A page closed before Save posts never posts.
test('a page closed before Save posts its edits never posts them', async () => {
  const session = createSession(0, { 0: {} });
  const store = session.store(0);
  store.set({ material: 'Steel' }, {});
  session.close();
  const posts = [];
  await assert.rejects(sessionSaver(store, session, () => store.get(), none, async (...args) => posts.push(args)).save({ defaults: [] }), { name: 'AbortError' });
  assert.deepEqual(posts, []);
});

test('closing while Save reviews confirmed defaults prevents the deferred POST', async () => {
  const session = createSession(0, { 0: {} });
  const store = session.store(0);
  const defaults = [{ part: 0, id: 'edges', prompt: 'Edges?', value: '0.2', raw: '0.2' }];
  let finish;
  let started;
  const began = new Promise((resolve) => { started = resolve; });
  const review = () => new Promise((resolve) => { finish = resolve; started(); });
  const posts = [];
  const saving = sessionSaver(store, session, () => ({}), review, async (...args) => posts.push(args)).save({ defaults });
  const cancelled = assert.rejects(saving, { name: 'AbortError' });
  await began;
  session.close();
  finish(defaults);
  await cancelled;
  assert.deepEqual(posts, []);
});

test('assembly missing IDs retain part order, including a part that was cleared', () => {
  const empty = { open: ['material', 'finish'], labels: ['A material', 'A finish'], required: false };
  const answered = { open: ['finish'], labels: ['B finish'], required: true };
  assert.deepEqual(matchMissing(['material', 'finish', 'finish'], [empty, answered]), ['A material', 'A finish', 'B finish']);
  assert.deepEqual(matchMissing(['finish'], [empty, answered]), ['B finish'], 'a joined but untouched part is omitted');
  assert.equal(matchMissing(['material', 'finish', 'finish'], [empty, { ...empty, labels: ['C material', 'C finish'] }, answered]), null,
    'a flat ID list cannot distinguish two empty parts with the same questions');
});

test('reapply puts the page\'s change onto answers changed meanwhile', () => {
  assert.deepEqual(reapply({ a: 1, b: 2 }, { a: 1 }, { a: 3, c: 4 }), { a: 3, b: 2, c: 4 });
  assert.deepEqual(reapply({ a: 1, b: 2 }, { a: 1, b: 2 }, { b: 2 }), { b: 2 }, 'a cleared answer is cleared');
});

/** A write that records what Save sent: the rev and the edit list. */
const recorder = () => {
  const writes = [];
  const write = async (rev, edits) => { writes.push([rev, edits]); return { path: 'p', bytes: 1, rev, report: {}, complete: true, missing: [] }; };
  return { writes, write };
};

// Session API 2: the page's change stays in the page (no PUT) over the agent's answers, and Save sends
// only it, as a fill, at the session's rev.
test('a page change stays in the page over an agent\'s answer, and Save sends it at the session\'s rev', async () => {
  const store = createSessionStore(0, 0, {}, {});
  const seen = [];
  store.subscribe((answers, source) => seen.push([answers, source]));
  store.set({ 'part.edges': 'Break 0.2' }, {});
  assert.deepEqual(store.session.answers(0), {}, 'nothing sent to the session');
  // The agent answered meanwhile.
  store.receive({ rev: 1, part: 0, answers: { 'part.material': 'Steel' }, source: 'mcp' });
  assert.deepEqual(store.get(), { 'part.material': 'Steel', 'part.edges': 'Break 0.2' }, 'the page shows both');
  assert.deepEqual(seen.at(-1), [store.get(), 'mcp']);
  assert.equal(await store.settled(), 1);
  const { writes, write } = recorder();
  await sessionSaver(store, store.session, () => store.get(), none, write).save({ defaults: [] });
  assert.deepEqual(writes, [[1, { fill: { 'part.edges': 'Break 0.2' } }]], 'the page\'s change only: the agent\'s is the session\'s');
  assert.deepEqual(store.session.changedParts(), [], 'saved: the page keeps no change');
});

test('events: an agent\'s change lands, one equal to the page\'s changes nothing, another part is ignored', async () => {
  const store = createSessionStore(0, 3, { a: 1 }, { a: 1 });
  const seen = [];
  store.subscribe((answers, source, previous) => seen.push({ answers, source, previous }));
  store.receive({ rev: 4, part: 0, answers: { a: 1, b: 2 }, source: 'mcp' });
  assert.deepEqual(seen, [{ answers: { a: 1, b: 2 }, source: 'mcp', previous: { a: 1 } }]);
  assert.equal(store.dirty(), true);
  store.receive({ rev: 5, part: 1, answers: { z: 1 }, source: 'mcp' });
  store.receive({ rev: 3, part: 0, answers: {}, source: 'page' });
  assert.equal(seen.length, 1, 'other parts and old revisions change nothing');
  store.set({ a: 1, b: 2, c: 3 }, { a: 1, b: 2 });
  store.receive({ rev: 6, part: 0, answers: { a: 1, b: 2, c: 3 }, source: 'mcp' });
  assert.equal(await store.settled(), 6);
  assert.equal(seen.length, 2, 'an answer equal to the page\'s own change is not a change');
  // A save of rev 6 (whoever asked): nothing unsaved.
  store.markSaved(6);
  assert.equal(store.dirty(), false);
});

test('unsaved: the session\'s answers against the file\'s, Specify\'s own records aside', () => {
  const store = createSessionStore(0, 0, { a: 1, _specify: { x: 1 } }, { a: 1 });
  assert.equal(store.dirty(), false);
  const agentFirst = createSessionStore(0, 2, { a: 1 }, {});
  assert.equal(agentFirst.dirty(), true, 'answers given before the page opened are not on disk');
});

// B1: the person's edit was made on {a}; while Specify reviewed it the agent's {a, c} arrived. The
// edit's change (b added) goes onto the agent's answers: c is kept, and Save sends b only.
test('a page edit made before an agent\'s answer arrived keeps the agent\'s answer', async () => {
  const store = createSessionStore(0, 0, { a: 1 }, { a: 1 });
  const base = store.get();
  store.receive({ rev: 1, part: 0, answers: { a: 1, c: 3 }, source: 'mcp' });
  store.set({ a: 1, b: 2 }, base);
  assert.deepEqual(store.get(), { a: 1, b: 2, c: 3 }, 'the page shows both at once');
  assert.equal(await store.settled(), 1);
  const { writes, write } = recorder();
  await sessionSaver(store, store.session, () => store.get(), none, write).save({ defaults: [] });
  assert.deepEqual(writes, [[1, { fill: { b: 2 } }]], 'sent once, at the agent\'s rev, with the page\'s change only');
});

// B2: an agent's change lands after the page's change. The rev Save gets is one whose answers the
// page has shown: a save of answers the page has not shown yet is refused.
test('an agent\'s change landing after a page change is shown before Save writes', async () => {
  const store = createSessionStore(0, 0, {}, {});
  const seen = [];
  store.subscribe((answers, source) => seen.push({ answers, source }));
  store.set({ b: 2 }, {});
  store.receive({ rev: 2, part: 0, answers: { c: 3 }, source: 'mcp' });
  assert.equal(await store.settled(), 2);
  assert.deepEqual(store.get(), { b: 2, c: 3 }, 'the page shows the agent\'s change');
  assert.ok(seen.some(e => e.source === 'mcp' && e.answers.c === 3), 'shown as the agent\'s');
  const { writes, write } = recorder();
  await assert.rejects(sessionSaver(store, store.session, () => ({ b: 2 }), none, write).save({ defaults: [] }), NotShown);
  assert.deepEqual(writes, [], 'nothing saved that the page has not shown');
  await sessionSaver(store, store.session, () => ({ c: 3, b: 2 }), none, write).save({ defaults: [] });
  assert.deepEqual(writes, [[2, { fill: { b: 2 } }]]);
});

test('a save refused because the file changed on disk is told apart from a stale rev', async () => {
  const store = createSessionStore(0, 4, { a: 1 }, { a: 1 });
  const refuse = rev => async () => { throw new Refused('changed', 409, { rev }); };
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 1 }), none, refuse(4)).save({ defaults: [] }), FileChanged);
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 1 }), none, refuse(6)).save({ defaults: [] }), Stale);
});

// S1: unsaved is the whole file's, whichever part is shown: changes left unsaved on a part not
// shown count (the page keeps them while another part is shown), and after a save the parts shown
// next are not unsaved for what was saved.
test('unsaved answers are the session\'s, across parts, against the last save', async () => {
  const s = createSession(0, { 0: {}, 1: { m: 'stored' } });
  const first = s.store(0, {});
  first.set({ a: 1 }, {});
  assert.equal(s.dirty(), true);
  // Another part shown: the first part's change is still unsaved, and still the page's.
  first.close();
  const second = s.store(1, { m: 'stored' });
  assert.equal(s.dirty(), true, 'part 0\'s change is still not on disk');
  assert.deepEqual(s.changedParts(), [0]);
  assert.deepEqual(s.shown(0), { a: 1 }, 'kept while another part is shown');
  // A save of it (rev 1: the session as the saved file has it): nothing unsaved; back on part 0, still nothing.
  s.commit();
  s.receive({ rev: 1, part: 0, answers: { a: 1 }, source: 'page' });
  s.markSaved(1);
  assert.equal(s.dirty(), false);
  second.close();
  const again = s.store(0, {});
  assert.equal(s.dirty(), false, 'part 0 as the file was opened is not what is on disk after the save');
  again.set({ a: 1, b: 2 }, { a: 1 });
  assert.equal(s.dirty(), true);
});

test('a save heard of before its revision is taken when the revision arrives', () => {
  const s = createSession(3, { 0: { a: 1 } });
  s.store(0, {});
  assert.equal(s.dirty(), true);
  s.markSaved(4);
  s.receive({ rev: 4, part: 0, answers: { a: 1, b: 2 }, source: 'mcp' });
  assert.equal(s.dirty(), false);
});

test('a page change that changes nothing (Specify reviewing the answers it opened with) is not kept', async () => {
  const store = createSessionStore(0, 0, { a: 1 }, { a: 1 });
  store.set({ a: 1 }, { a: 1 });
  assert.deepEqual(store.session.changedParts(), []);
  const { writes, write } = recorder();
  await sessionSaver(store, store.session, () => store.get(), none, write).save({ defaults: [] });
  assert.deepEqual(writes, [[0, {}]], 'Save sends an empty list: the file is saved as it is');
});

// ADR P9: defaults are written only when the person confirmed the list of them. A plain save never
// asks for defaults; a save with defaults asks only when the defaults it would write are the ones confirmed.
const d = (part, id, value) => ({ part, id, prompt: id, value: String(value), raw: JSON.stringify(value) });

test('a save with defaults writes only the defaults confirmed', async () => {
  const store = createSessionStore(0, 2, { a: 1 }, { a: 1 });
  const { writes, write } = recorder();
  const defaultsOf = async () => [d(0, 'b', 1), d(0, 'c', 2)];
  await sessionSaver(store, store.session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [] });
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [d(0, 'b', 1)] }), DefaultsChanged);
  await sessionSaver(store, store.session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [d(0, 'c', 2), d(0, 'b', 1)] });
  // Session API 2: each confirmed default is a fill of the value the person saw; `{}` for a plain save.
  assert.deepEqual(writes, [[2, {}], [2, { fill: { c: 2, b: 1 } }]], 'exactly the confirmed ids with the values seen; nothing written for a list that changed');
});

// Review round 2, item 2: the same questions defaulted to another value (an agent's answer changed
// what a default is) is not what was confirmed.
test('a confirmed default whose value changed before the save is confirmed again', async () => {
  const store = createSessionStore(0, 2, { a: 1 }, { a: 1 });
  const { writes, write } = recorder();
  // The default of b follows a: 0.1 while a is 1, 0.2 once the agent answers a = 2.
  const defaultsOf = async (parts) => [d(0, 'b', parts['0'].a === 1 ? 0.1 : 0.2)];
  const confirmed = await defaultsOf(store.session.parts());
  store.receive({ rev: 3, part: 0, answers: { a: 2 }, source: 'mcp' });
  await assert.rejects(sessionSaver(store, store.session, () => ({ a: 2 }), defaultsOf, write).save({ defaults: confirmed }), DefaultsChanged);
  assert.deepEqual(writes, [], 'nothing written: the list is shown again');
  await sessionSaver(store, store.session, () => ({ a: 2 }), defaultsOf, write).save({ defaults: await defaultsOf(store.session.parts()) });
  assert.deepEqual(writes, [[3, { fill: { b: 0.2 } }]]);
});

// Review round 2, item 1 (ADR P9): the defaults confirmed are every part's, not only the part shown's.
// Session API 2: an edit list applies to one part, so defaults confirmed on several parts are not sent.
test('a save with defaults of an assembly checks the defaults of every part, and sends one part only', async () => {
  const session = createSession(5, { 0: { a: 1 }, 3: { m: 'Steel' } });
  const store = session.store(0, {});
  const { writes, write } = recorder();
  const asked = [];
  const defaultsOf = async (parts) => { asked.push(Object.keys(parts).sort()); return Object.keys(parts).map((k) => d(Number(k), 'edges', 0.5)); };
  // Only the part shown's default confirmed: refused, and nothing written.
  await assert.rejects(sessionSaver(store, session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [d(0, 'edges', 0.5)] }), DefaultsChanged);
  assert.deepEqual(asked, [['0', '3']], 'the defaults of every part in the session');
  // Both confirmed: two parts' edits, which one list cannot carry yet.
  await assert.rejects(sessionSaver(store, session, () => ({ a: 1 }), defaultsOf, write).save({ defaults: [d(3, 'edges', 0.5), d(0, 'edges', 0.5)] }),
    (e) => e instanceof CannotSend && e.ids.join() === '0,3');
  assert.deepEqual(writes, []);
  // One part's: sent as that part's fills.
  await sessionSaver(store, session, () => ({ a: 1 }), async () => [d(3, 'edges', 0.5)], write).save({ defaults: [d(3, 'edges', 0.5)] });
  assert.deepEqual(writes, [[5, { fill: { edges: 0.5 } }]]);
});

// Review round 2, nit: answers an agent gave a part before the page loaded are unsaved, even when
// the page never shows that part (the file stores none for it, or others).
test('answers given to a part not shown before the page loaded are unsaved', () => {
  const fresh = createSession(2, { 0: {}, 4: { 'part.material': 'Steel' } });
  fresh.store(0, {});
  fresh.fileAnswers(4, {});
  assert.equal(fresh.dirty(), true, 'the file stores no answers for part 4');
  const written = createSession(2, { 0: {}, 4: { 'part.material': 'Steel' } });
  written.store(0, {});
  written.fileAnswers(4, { 'part.material': 'Steel' });
  assert.equal(written.dirty(), false, 'what the file stores is not unsaved');
  written.markSaved(2);
  written.fileAnswers(4, {});
  assert.equal(written.dirty(), false, 'after a save, the save is what is on disk');
});

// Session API 2: Save sends `{rev, edits}`.
test('the page sends the edit list with its rev', async () => {
  const real = globalThis.fetch;
  const sent = [];
  globalThis.fetch = async (url, init) => { sent.push(JSON.parse(init.body)); return new Response('{"path":"p","bytes":1,"rev":4,"report":{},"complete":true,"missing":[]}', { status: 200 }); };
  try {
    await postSave(4, {});
    await postSave(4, { fill: { edges: 0.5 } });
  } finally { globalThis.fetch = real; }
  assert.deepEqual(sent, [{ rev: 4, edits: {} }, { rev: 4, edits: { fill: { edges: 0.5 } } }]);
});

// All or nothing: the command's 422 names every entry it cannot apply; the saver hands them on and
// keeps the page's changes (nothing was written).
test('an edit list the command refuses is reported entry by entry, and the changes are kept', async () => {
  const store = createSessionStore(0, 2, {}, {});
  store.set({ 'extra.1': { kind: 'size', faces: [9], value: '±0.1' } }, {});
  const failures = [{ entry: 'fill["extra.1"]', reason: 'the file has no feature-and-kind "extra.1"' }];
  const write = async () => { throw new Refused('nothing was written', 422, { error: 'nothing was written', failures }); };
  await assert.rejects(sessionSaver(store, store.session, () => store.get(), none, write).save({ defaults: [] }),
    (e) => e instanceof EditsRefused && e.failures.length === 1 && e.failures[0].entry === failures[0].entry);
  assert.deepEqual(store.session.changedParts(), [0], 'the page\'s change is still there');
});

// What the page sends: an answer is a fill (a none answer records decided none), and an answer the
// file stores (a decision that leaves no PMI) that the page cleared is a clear.
test('answers become fills; clearing a stored answer is a clear', async () => {
  assert.deepEqual(editsFor({ 'part.coating': 'None (as machined)' }, { 'part.coating': 'anodise', 'part.material': 'Steel', _specify: { x: 1 } }),
    { fill: { 'part.coating': 'anodise', 'part.material': 'Steel' } });
  assert.deepEqual(editsFor({}, { 'part.notes': [] }), { fill: { 'part.notes': [] } }, 'a none answer is a fill of it');
  assert.deepEqual(editsFor({ 'part.coating': 'None' }, {}), { clear: ['part.coating'] });
  assert.deepEqual(editsFor({}, { a: 1 }, [{ id: 'm', raw: '"ISO 2768-m"' }, { id: 'e', raw: '{"a":[1,2]}' }]),
    { fill: { a: 1, m: 'ISO 2768-m', e: { a: [1, 2] } } }, 'the value the person saw, structured values included');
  const store = createSessionStore(0, 2, { 'part.coating': 'None' }, { 'part.coating': 'None' });
  store.set({}, { 'part.coating': 'None' });
  const { writes, write } = recorder();
  await sessionSaver(store, store.session, () => store.get(), none, write).save({ defaults: [] });
  assert.deepEqual(writes, [[2, { clear: ['part.coating'] }]]);
});

// A save writes only parts with at least one answer, plus a single-part file's only part.
test('a save writes the parts with answers, and a file\'s only part', () => {
  assert.deepEqual(writtenParts({ 0: { a: 1 }, 1: {}, 2: { _specify: { x: 1 } }, 3: { m: 'Steel' } }, true), [0, 3], 'no answers (or only Specify\'s records): not written');
  assert.deepEqual(writtenParts({ 0: {} }, false), [0], 'a single part is always written');
  assert.deepEqual(writtenParts({}, false), [0]);
  assert.deepEqual(writtenParts({}, true), []);
});

// A joining part event is a change (a new rev, answers possibly empty): the session holds it, a save
// at the older rev is stale, and the defaults a save lists include the part once it has an answer.
test('a part joining the session is held, and counts for a save once it has answers', () => {
  const session = createSession(2, { 0: { a: 1 } });
  const store = session.store(0, {});
  session.receive({ rev: 3, part: 1, answers: {}, source: 'mcp', joined: true });
  assert.equal(session.rev(), 3, 'the join is a new rev: a save at 2 gets a 409 and the page re-lists');
  assert.deepEqual(Object.keys(session.parts()), ['0', '1'], 'part 1 is in the page\'s model');
  assert.deepEqual(writtenParts(session.parts(), true), [0], 'with no answers it is not written');
  session.receive({ rev: 4, part: 1, answers: { m: 'Steel' }, source: 'mcp' });
  assert.deepEqual(writtenParts(session.parts(), true), [0, 1]);
  store.close();
});

test('GET diff is read as WASM diff() gives it, with its scenes', async () => {
  const real = globalThis.fetch;
  const body = { report: { verdict: 'ok', pmi: { scenes: { old: {}, new: {} } } }, markdown: '# x', view: { faces: {} } };
  globalThis.fetch = async () => new Response(JSON.stringify(body), { status: 200 });
  try { assert.deepEqual(await getDiff(), body, 'handed on whole: StepDiff draws from it'); } finally { globalThis.fetch = real; }
});

// One rev can carry several `answers` events (one per part); a repeat of (rev, part) is ignored.
test('events are deduped by (rev, part): one rev may carry one event per part', () => {
  const session = createSession(2, { 0: { a: 1 }, 1: { m: 'Steel' } });
  const store0 = session.store(0, {});
  const store1 = session.store(1, {});
  const seen = [];
  store0.subscribe((answers) => seen.push(['0', answers]));
  store1.subscribe((answers) => seen.push(['1', answers]));
  assert.equal(session.receive({ rev: 3, part: 0, answers: { a: 1, e: 0.5 }, source: 'mcp' }), true);
  assert.equal(session.receive({ rev: 3, part: 1, answers: { m: 'Steel', e: 0.5 }, source: 'mcp' }), true, 'the same rev, another part: heard');
  assert.equal(session.receive({ rev: 3, part: 0, answers: { a: 1, e: 0.5 }, source: 'mcp' }), false, 'the same rev and part again: ignored');
  assert.equal(session.rev(), 3);
  assert.deepEqual(store0.get(), { a: 1, e: 0.5 });
  assert.deepEqual(store1.get(), { m: 'Steel', e: 0.5 });
  assert.equal(seen.length, 2, 'each part told once');
});

test('a repeated event does not undo what a later resync holds', () => {
  const session = createSession(2, { 0: { a: 1 } });
  session.receive({ rev: 3, part: 0, answers: { a: 2 }, source: 'mcp' });
  session.receive({ rev: 4, parts: { 0: { a: 3 } }, source: 'start' });
  assert.equal(session.receive({ rev: 3, part: 0, answers: { a: 2 }, source: 'mcp' }), false);
  assert.deepEqual(session.answers(0), { a: 3 });
});

// Save with defaults lists every part of the file, answered or not. Plain save is unchanged: an
// untouched assembly sends an empty list.
test('save with defaults lists every part of an assembly, even where no part has an answer', async () => {
  const session = createSession(5, {});
  const store = session.store(0, {});
  assert.deepEqual(defaultParts(session.parts(), [0, 1, 2], true), [0, 1, 2], 'every part of the file, though the session holds no answers');
  assert.deepEqual(defaultParts({ 3: { a: 1 } }, [0, 1], true), [0, 1, 3], 'and a part only the session has');
  assert.deepEqual(defaultParts({}, [0], false), [0], 'a single part still works');
  const { writes, write } = recorder();
  const defaultsOf = async (parts) => defaultParts(parts, [0, 1, 2], true).map((k) => d(k, 'edges', 0.5));
  const listed = await defaultsOf(session.parts());
  // Left one part's default out of what was confirmed: it is not the list that would be written.
  await assert.rejects(sessionSaver(store, session, () => ({}), defaultsOf, write).save({ defaults: listed.slice(0, 2) }), DefaultsChanged);
  assert.equal(writes.length, 0);
  // A plain save of the same untouched assembly sends nothing for any part (so none is written).
  await sessionSaver(store, session, () => ({}), defaultsOf, write).save({ defaults: [] });
  assert.deepEqual(writes[0], [5, {}]);
  assert.deepEqual(writtenParts(session.parts(), true), [], 'plain save: no part has answers, so none is written');
  store.close();
});
