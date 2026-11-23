import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createSession, createSessionStore, reapply } from '../app/answers.ts';
import { getDiff, Refused, save as postSave } from '../app/local.ts';
import { EditsRefused, editsSaver, FileChanged, NotShown, Stale } from '../app/save.ts';
// The local app's answers store over the command's session (specify-core docs/open.md, session API 2):
// the review panel's edits stay in the page until Save sends them as an edit list (`POST save {rev,
// edits}`); events from others (an agent's answers) land in the session.

test('the second page save uses the HTTP response revision before any event arrives', async () => {
  const session = createSession(4, { 0: {} });
  const store = session.store(0);
  let serverRev = 4;
  const saver = editsSaver(store, session, () => store.get(), async (rev, edits) => {
    assert.equal(rev, serverRev, 'the next POST uses the committed revision');
    const parts = { 0: edits.none ? { 'part.coating': 'None' } : {} };
    return { path: 'p', bytes: 1, rev: ++serverRev, parts, report: {}, complete: true, missing: [] };
  });
  await saver.save({ none: ['part.coating'] });
  const first = { answers: store.get(), dirty: session.dirty() };
  await saver.save({ clear: ['part.coating'] });
  assert.deepEqual(first, { answers: { 'part.coating': 'None' }, dirty: false });
  assert.deepEqual(store.get(), {});
  assert.equal(session.dirty(), false);
  session.receive({ rev: 7, part: 0, answers: { 'part.material': 'Steel' }, source: 'mcp' });
  session.receiveSave({ rev: 5, parts: { 0: { 'part.coating': 'None' } } });
  assert.equal(await store.settled(), 7, 'a late response cannot roll back a newer event');
  assert.deepEqual(store.get(), { 'part.material': 'Steel' });
  assert.equal(session.dirty(), true);
});

test('a delayed save response records the exact disk answers after a newer agent event', () => {
  const session = createSession(4, { 0: {} });
  const agentAnswers = { 'part.material': 'Steel' };
  session.receive({ rev: 6, part: 0, answers: agentAnswers, source: 'mcp' });
  session.receiveSave({ rev: 5, parts: { 0: { 'part.coating': 'None' } } });
  assert.equal(session.rev(), 6);
  assert.deepEqual(session.answers(0), agentAnswers);
  assert.equal(session.dirty(), true);
});

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
  await assert.rejects(editsSaver(store, session, () => store.get(), async (...args) => posts.push(args)).save({ fill: { material: 'Steel' } }), { name: 'AbortError' });
  assert.deepEqual(posts, []);
});

test('reapply puts the page\'s change onto answers changed meanwhile', () => {
  assert.deepEqual(reapply({ a: 1, b: 2 }, { a: 1 }, { a: 3, c: 4 }), { a: 3, b: 2, c: 4 });
  assert.deepEqual(reapply({ a: 1, b: 2 }, { a: 1, b: 2 }, { b: 2 }), { b: 2 }, 'a cleared answer is cleared');
});

/** A write that records what Save sent: the rev and the edit list. */
const recorder = () => {
  const writes = [];
  const write = async (rev, edits) => { writes.push([rev, edits]); return { path: 'p', bytes: 1, rev, parts: {}, report: {}, complete: true, missing: [] }; };
  return { writes, write };
};

// Session API 2: the panel's edits are sent as the edit list they are, at the session's rev, once the
// page shows the agent's answers that arrived meanwhile.
test('Save sends the panel\'s edit list at the session\'s rev, an agent\'s answer having arrived', async () => {
  const store = createSessionStore(0, 0, {}, {});
  const seen = [];
  store.subscribe((answers, source) => seen.push([answers, source]));
  // The agent answered.
  store.receive({ rev: 1, part: 0, answers: { 'part.material': 'Steel' }, source: 'mcp' });
  assert.deepEqual(store.get(), { 'part.material': 'Steel' });
  assert.deepEqual(seen.at(-1), [store.get(), 'mcp']);
  assert.equal(await store.settled(), 1);
  const { writes, write } = recorder();
  const edits = { fill: { 'part.edges': 'Break 0.2' }, none: ['part.coating'], items: { remove: ['#4'] } };
  await editsSaver(store, store.session, () => store.get(), write).save(edits);
  assert.deepEqual(writes, [[1, edits]], 'the list as the panel made it: the agent\'s answer is the session\'s');
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

// B2: an agent's change lands after the page's change. The rev Save gets is one whose answers the
// page has shown: a save of answers the page has not shown yet is refused.
test('an agent\'s change landing before Save is shown before Save writes', async () => {
  const store = createSessionStore(0, 0, {}, {});
  const seen = [];
  store.subscribe((answers, source) => seen.push({ answers, source }));
  store.receive({ rev: 2, part: 0, answers: { c: 3 }, source: 'mcp' });
  assert.equal(await store.settled(), 2);
  assert.deepEqual(store.get(), { c: 3 }, 'the session has the agent\'s change');
  assert.ok(seen.some(e => e.source === 'mcp' && e.answers.c === 3), 'shown as the agent\'s');
  const { writes, write } = recorder();
  await assert.rejects(editsSaver(store, store.session, () => ({}), write).save({ fill: { b: 2 } }), NotShown);
  assert.deepEqual(writes, [], 'nothing saved over answers the page has not shown');
  await editsSaver(store, store.session, () => ({ c: 3 }), write).save({ fill: { b: 2 } });
  assert.deepEqual(writes, [[2, { fill: { b: 2 } }]]);
});

test('a save refused because the file changed on disk is told apart from a stale rev', async () => {
  const store = createSessionStore(0, 4, { a: 1 }, { a: 1 });
  const refuse = rev => async () => { throw new Refused('changed', 409, { rev }); };
  await assert.rejects(editsSaver(store, store.session, () => ({ a: 1 }), refuse(4)).save({}), FileChanged);
  await assert.rejects(editsSaver(store, store.session, () => ({ a: 1 }), refuse(6)).save({}), Stale);
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
  await editsSaver(store, store.session, () => store.get(), write).save({});
  assert.deepEqual(writes, [[0, {}]], 'Save sends an empty list: the file is saved as it is');
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
test('an edit list the command refuses is reported entry by entry', async () => {
  const store = createSessionStore(0, 2, {}, {});
  const failures = [{ entry: 'fill["extra.1"]', reason: 'the file has no feature-and-kind "extra.1"' }];
  const write = async () => { throw new Refused('nothing was written', 422, { error: 'nothing was written', failures }); };
  await assert.rejects(editsSaver(store, store.session, () => store.get(), write).save({ fill: { 'extra.1': { kind: 'size', faces: [9], value: '±0.1' } } }),
    (e) => e instanceof EditsRefused && e.failures.length === 1 && e.failures[0].entry === failures[0].entry);
});

// A joining part event is a change (a new rev, answers possibly empty): the session holds it, and a
// save at the older rev is stale.
test('a part joining the session is held, with the answers it gets', () => {
  const session = createSession(2, { 0: { a: 1 } });
  const store = session.store(0, {});
  session.receive({ rev: 3, part: 1, answers: {}, source: 'mcp', joined: true });
  assert.equal(session.rev(), 3, 'the join is a new rev: a save at 2 gets a 409 and the page re-lists');
  assert.deepEqual(Object.keys(session.parts()), ['0', '1'], 'part 1 is in the page\'s model');
  session.receive({ rev: 4, part: 1, answers: { m: 'Steel' }, source: 'mcp' });
  assert.deepEqual(session.answers(1), { m: 'Steel' });
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


test('a batched commit clears all shown drafts on its first part event', () => {
  const initial = { '0': { fill: { m: 'Steel' } }, '1': { fill: { m: 'Brass' } } };
  const session = createSession(1, { '0': { m: 'Steel' }, '1': { m: 'Brass' } }, initial);
  assert.deepEqual(session.draftSnapshot(), { rev: 1, drafts: initial });
  session.receive({ rev: 2, part: 0, answers: {}, source: 'page', drafts: {} });
  assert.deepEqual(session.draftSnapshot(), { rev: 2, drafts: {} });
  session.receive({ rev: 3, part: 1, answers: { m: 'Bronze' }, source: 'mcp', draft: { fill: { m: 'Bronze' } } });
  session.receiveSave({ rev: 2, parts: { '0': {}, '1': {} } });
  assert.deepEqual(session.draftSnapshot(), { rev: 3, drafts: { '1': { fill: { m: 'Bronze' } } } });
});

test('an unchanged save acknowledgment keeps a refused answer until discard', () => {
  const drafts = { '0': { refused: { material: 'use an explicit change' } } };
  const session = createSession(3, { '0': {} }, drafts);
  session.markSaved(3);
  assert.deepEqual(session.draftSnapshot(), { rev: 3, drafts });
  session.receiveSave({ rev: 3, parts: { '0': {} } });
  assert.deepEqual(session.draftSnapshot(), { rev: 3, drafts });
  session.receive({ source: 'start', rev: 4, parts: { '0': {} } });
  assert.deepEqual(session.draftSnapshot(), { rev: 4, drafts: {} });
});

test('Save waits for a pending Undo review at the displayed draft revision', async () => {
  const material = { 'part.material': 'Brass CW614N' };
  const store = createSessionStore(1, 3, { ...material, 'part.general_tolerance': 'ISO 2768-f' });
  const { promise: reviewed, resolve: finishReview } = Promise.withResolvers();
  store.receive({ rev: 4, parts: { 0: { 'part.material': 'Steel' }, 1: material }, source: 'start' });
  const { writes, write } = recorder();
  const save = editsSaver(store, store.session, () => reviewed, write);
  const edits = [0, 1].map(part => ({ part, edits: { fill: { 'part.material': part === 0 ? 'Steel' : 'Brass CW614N' } } }));

  const saving = save.save(edits, 4);
  await Promise.resolve();
  assert.deepEqual(writes, [], 'Save has not sent the list while the Undo review is held');
  finishReview(material);
  await saving;
  assert.deepEqual(writes, [[4, edits]], 'the original Save sends the list after review, without another click');
});

test('Save at the displayed draft revision refuses a newer answer on another part', async () => {
  const session = createSession(1, { '0': {}, '1': {} });
  const store = session.store(0);
  session.receive({ rev: 2, part: 1, answers: { m: 'Steel' }, source: 'mcp', draft: { fill: { m: 'Steel' } } });
  let wrote = false;
  const saver = editsSaver(store, session, () => store.get(), async (rev) => {
    wrote = true;
    return { rev, path: 'part.step', bytes: 1, parts: {}, complete: true, missing: [], report: {} };
  });
  await assert.rejects(saver.save({ none: ['part.coating'] }, 1), NotShown);
  assert.equal(wrote, false);
});
