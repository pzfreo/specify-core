import type { PmiAnswers } from '../src/types/pmi';
import type { AnswerDraft, AnswersEvent, SessionAnswers, Source } from './local.ts';

export type AnswersSource = Source;

/** One part's answers in the command's session (specify-core docs/open.md, "The session"): everyone
 * else's changes (an agent's) come down as `answers` events. The page's own changes stay in the page
 * until Save sends them as an edit list (session API 2: there is no `PUT answers`). What the page
 * shows is always the session's answers, with the page's own unsaved changes laid over them. */
export interface AnswersStore {
  readonly part: number;
  get(): PmiAnswers;
  /** A change made in this page: from ``base`` (the answers the page showed when it was made) to
   * ``answers``. Only that change is kept, laid onto whatever the session holds, so an answer
   * someone else gave meanwhile is never taken away; Save sends it. */
  set(answers: PmiAnswers, base: PmiAnswers): void;
  /** Each change to what the page should show. ``previous``: the answers before it, to show what
   * someone else (an agent) changed. */
  subscribe(listener: (answers: PmiAnswers, source: AnswersSource, previous: PmiAnswers) => void): () => void;
  /** The session's revision the page's answers are laid over. */
  settled(): Promise<number>;
  /** Kept for the page's wiring: the page's changes are no longer sent, so nothing fails to arrive. */
  onProblem(listener: (error: Error) => void): void;
  /** No more updates (another part is shown). */
  close(): void;
}

/** The command's session as this page knows it: every part's answers at ``rev()``, the page's own
 * unsaved changes, and what is on disk, so unsaved answers are judged for the whole file, whichever
 * part is shown. */
export interface AnswersSession {
  /** Cancelled before the close beacon, shared by Save POSTs. */
  readonly signal: AbortSignal;
  rev(): number;
  /** Part ``part``'s answers in the session (without the page's unsaved changes). */
  answers(part: number): PmiAnswers;
  /** Part ``part``'s answers as the page shows them: the session's, with the page's unsaved changes. */
  shown(part: number): PmiAnswers;
  /** The parts the page has unsaved changes on. */
  changedParts(): number[];
  /** The page's changes are in the file now (a save of them succeeded): they are no longer kept. */
  commit(): void;
  /** Every part's answers at ``rev()``, by part index: the parts a save writes (with a file's only part). */
  parts(): Record<string, PmiAnswers>;
  /** Working answers as edit lists at the rendered revision. */
  draftSnapshot(): { rev: number; drafts: Record<string, AnswerDraft> };
  /** An `answers` event (or a `GET answers` resync, as a "start" event). One rev can carry several
   * events (one per part): an event for a (rev, part) already received is ignored, and false is returned. */
  receive(event: AnswersEvent): boolean;
  /** The store for part ``part``. ``onDisk``: its answers stored in the file as opened, so answers
   * an agent gave before the page opened show as unsaved (until a save). */
  store(part: number, onDisk?: PmiAnswers): AnswersStore;
  /** Part ``part``'s answers stored in the file as opened (none for a part Specify did not write),
   * so answers an agent gave it before the page opened count as unsaved, whether or not the part
   * is ever shown. Ignored once the page has saved. */
  fileAnswers(part: number, answers: PmiAnswers): void;
  /** The answers of session revision ``rev`` are on disk (a save's answer, or a `saved` event). */
  markSaved(rev: number): void;
  /** A successful page save carries the answers at the revision it committed. */
  receiveSave(saved: SessionAnswers): void;
  /** Answers given, in any part, that are not on disk: since the last save, or since the file was opened. */
  dirty(): boolean;
  /** Called on every change that can change ``dirty()``. */
  subscribe(listener: () => void): () => void;
  /** The page is leaving. Abort an in-flight Save and keep no more changes. */
  close(): void;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const sorted = (answers: PmiAnswers) => JSON.stringify(Object.entries(answers).sort(([a], [b]) => (a < b ? -1 : 1)));
/** The same answers, in whatever order. */
export const sameAnswers = (a: PmiAnswers, b: PmiAnswers) => sorted(a) === sorted(b);

/** ``base`` with the change from ``before`` to ``after`` applied: what the page changed, put onto
 * answers someone else has changed meanwhile. */
export function reapply(base: PmiAnswers, before: PmiAnswers, after: PmiAnswers): PmiAnswers {
  const next: Record<string, unknown> = { ...base };
  for (const id of new Set([...Object.keys(before), ...Object.keys(after)])) {
    if (same(before[id], after[id])) continue;
    if (id in after) next[id] = after[id]; else delete next[id];
  }
  return next as PmiAnswers;
}

/** What is compared for unsaved changes: the answers given, without Specify's own `_` records. */
const asGiven = (answers: PmiAnswers | undefined) => JSON.stringify(Object.fromEntries(Object.entries(answers ?? {}).filter(([id]) => !id.startsWith('_')).sort(([a], [b]) => (a < b ? -1 : 1))));
const NONE = asGiven({});

/** How many revisions' answers are remembered, to know what a save of a past revision wrote. */
const HISTORY = 500;

export function createSession(rev: number, parts: Record<string, PmiAnswers>, initialDrafts: Record<string, AnswerDraft> = {}): AnswersSession {
  const requests = new AbortController();
  let state = { rev, parts: { ...parts } };
  let drafts = { ...initialDrafts };
  // Each revision's answers (as given) by part: what a save of it put on disk.
  const history = new Map<number, Record<string, string>>();
  const remember = () => {
    history.set(state.rev, Object.fromEntries(Object.entries(state.parts).map(([k, a]) => [k, asGiven(a)])));
    for (const old of history.keys()) {
      if (history.size <= HISTORY) break;
      history.delete(old);
    }
  };
  remember();
  // On disk, by part: the file as opened (the session's start, or the part's stored answers when
  // the page opens it) until a save, then that save's answers.
  let onDisk: Record<string, string> = history.get(rev)!;
  let savedRev: number | null = null;
  // A save of a revision not heard of yet: taken as on disk when the session reaches it.
  let awaited: number | null = null;
  // The page's own changes, by part, in order: each from ``before`` to ``after``. Kept in the page
  // (not sent) until a save of them succeeds, whichever part is shown meanwhile.
  const pending = new Map<string, { before: PmiAnswers; after: PmiAnswers }[]>();
  const shownOf = (part: number) => (pending.get(String(part)) ?? [])
    .reduce((answers, c) => reapply(answers, c.before, c.after), state.parts[String(part)] ?? {});
  const stores = new Set<{ store: AnswersStore; update(source: AnswersSource): void }>();
  const watchers = new Set<() => void>();
  const changed = () => { for (const watcher of watchers) watcher(); };
  // The (rev, part) of the part events received.
  const heard = new Set<string>();

  /** The session at ``at`` holds ``changes``: every part's answers (``all``), or the parts changed. */
  const known = (at: number, changes: Record<string, PmiAnswers>, all: boolean, source: AnswersSource) => {
    if (requests.signal.aborted || at < state.rev) return;
    state = { rev: at, parts: all ? { ...changes } : { ...state.parts, ...changes } };
    remember();
    if (awaited !== null && at >= awaited) { onDisk = history.get(at)!; awaited = null; }
    for (const s of stores) s.update(source);
    changed();
  };

  const markSaved = (at: number, parts?: SessionAnswers['parts']) => {
    if (savedRev !== null && at < savedRev) return;
    savedRev = at;
    if (at >= state.rev) drafts = {};
    awaited = null;
    const then = parts
      ? Object.fromEntries(Object.entries(parts).map(([k, a]) => [k, asGiven(a)]))
      : history.get(at);
    if (then) onDisk = then;
    // Older than the history: without a snapshot, the latest known is the best guess.
    else if (at < state.rev) onDisk = history.get(state.rev)!;
    else awaited = at;
    changed();
  };

  const session: AnswersSession = {
    signal: requests.signal,
    rev: () => state.rev,
    answers: (part) => state.parts[String(part)] ?? {},
    shown: shownOf,
    changedParts: () => [...pending.entries()].filter(([k]) => !sameAnswers(shownOf(Number(k)), session.answers(Number(k)))).map(([k]) => Number(k)),
    commit() {
      pending.clear();
      for (const s of stores) s.update('start');
      changed();
    },
    parts: () => ({ ...state.parts }),
    draftSnapshot: () => ({ rev: state.rev, drafts }),
    receive(event) {
      if (requests.signal.aborted || event.rev < state.rev) return false;
      if ('parts' in event) {
        drafts = { ...event.drafts };
        known(event.rev, event.parts, true, event.source);
        return true;
      }
      // Several events share a rev (one per part), so a repeat is told apart by (rev, part).
      const key = `${event.rev}:${event.part}`;
      if (event.rev < state.rev || heard.has(key)) return false;
      heard.add(key);
      for (const old of heard) { if (heard.size <= HISTORY) break; heard.delete(old); }
      drafts = { ...(event.drafts ?? drafts) };
      if (event.draft) drafts[String(event.part)] = event.draft;
      else delete drafts[String(event.part)];
      known(event.rev, { [String(event.part)]: event.answers }, false, event.source);
      return true;
    },
    store(part, fileAnswers) {
      const key = String(part);
      if (fileAnswers) session.fileAnswers(part, fileAnswers);
      let current = shownOf(part);
      const listeners = new Set<(answers: PmiAnswers, source: AnswersSource, previous: PmiAnswers) => void>();
      const update = (source: AnswersSource) => {
        const previous = current;
        current = shownOf(part);
        if (sameAnswers(previous, current)) return;
        for (const listener of listeners) listener(current, source, previous);
      };
      const store: AnswersStore = {
        part,
        get: () => current,
        set(answers, base) {
          if (requests.signal.aborted || sameAnswers(answers, base)) return;
          const list = pending.get(key) ?? [];
          list.push({ before: base, after: answers });
          pending.set(key, list);
          update('page');
          changed();
        },
        subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
        async settled() { return state.rev; },
        onProblem() {},
        close() { stores.delete(entry); listeners.clear(); },
      };
      const entry = { store, update };
      stores.add(entry);
      return store;
    },
    fileAnswers(part, answers) {
      if (savedRev !== null) return;
      onDisk = { ...onDisk, [String(part)]: asGiven(answers) };
      queueMicrotask(changed);
    },
    markSaved,
    receiveSave(saved) {
      if (saved.rev >= state.rev) drafts = { ...saved.drafts };
      known(saved.rev, saved.parts, true, 'start');
      markSaved(saved.rev, saved.parts);
    },
    dirty() {
      const now: Record<string, string> = Object.fromEntries(Object.entries(state.parts).map(([k, a]) => [k, asGiven(a)]));
      for (const k of pending.keys()) now[k] = asGiven(shownOf(Number(k)));
      return [...new Set([...Object.keys(now), ...Object.keys(onDisk)])].some((k) => (now[k] ?? NONE) !== (onDisk[k] ?? NONE));
    },
    subscribe(listener) { watchers.add(listener); return () => { watchers.delete(listener); }; },
    close() { requests.abort(); },
  };
  return session;
}

/** A session holding part ``part`` only, and that part's store, with the session's calls on it: for tests. */
export function createSessionStore(part: number, rev: number, initial: PmiAnswers, onDisk: PmiAnswers = initial) {
  const session = createSession(rev, { [String(part)]: initial });
  const store = session.store(part, onDisk);
  return { ...store, get: store.get, session, receive: session.receive, markSaved: session.markSaved, dirty: session.dirty };
}

/** The questions whose answers differ between ``a`` and ``b``. */
export function changedIds(a: PmiAnswers, b: PmiAnswers): string[] {
  return [...new Set([...Object.keys(a), ...Object.keys(b)])]
    .filter((id) => !id.startsWith('_') && JSON.stringify(a[id]) !== JSON.stringify(b[id]));
}
