// Save: the page sends the edits it has collected (`POST save {rev, edits}`, specify-core
// docs/open.md, session API 2) and the command applies them to the file, all or nothing, with the
// same writer and naming as the agent's `save`. Each answer the person gave is a fill (a value
// meaning none records "decided: none"); each default the person confirmed is a fill of the value
// they saw. Anything not sent is left as the file has it. It never refuses for open questions: many
// AP242 files have no complete answer to everything. It says what is still open.
import type { PmiAnswers } from '../src/types/pmi';
import type { AnswersSession, AnswersStore } from './answers.ts';
import { Refused, save as post, type EditFailure, type EditList, type Saved } from './local.ts';

/** A default a save with defaults would write: part ``part``'s question ``id`` set to ``value``
 * (``raw``, the default itself, is what is compared; ``value`` is how it reads). */
export interface Default { part: number; id: string; prompt: string; value: string; raw: string }

export interface SaveOptions {
  /** The defaults the person saw listed and confirmed, of every part the save writes (ADR P9: a
   * default is written only when approved); none for a plain save. */
  defaults: readonly Default[];
}

/** The session answers that went into a successful write, including the approvals sent with it. */
export interface SavedWithAnswers extends Saved { answers: Record<string, PmiAnswers> }
export interface Saver { save(options: SaveOptions): Promise<SavedWithAnswers> }

/** Nothing saved: the session holds answers the page has not shown yet (they are being shown). */
export class NotShown extends Error {}
/** Nothing saved: the session moved on since the page's rev (an agent answered); ``rev`` is the session's. */
export class Stale extends Error {}
/** Nothing saved: (`--in-place`) the file was changed on disk by another program since it was opened. */
export class FileChanged extends Error {}
/** Nothing saved: the defaults a save would write are no longer the ones the person confirmed. */
export class DefaultsChanged extends Error {}
/** Nothing saved: the command refused the edit list (all or nothing); ``failures`` names every entry
 * that cannot be applied, and why. */
export class EditsRefused extends Error {
  readonly failures: readonly EditFailure[];
  constructor(message: string, failures: readonly EditFailure[]) { super(message); this.failures = failures; }
}
/** Nothing saved: changes this page cannot send as an edit list yet: changes on several parts of an
 * assembly (a list applies to one part). ``ids``: the parts concerned. */
export class CannotSend extends Error {
  readonly ids: readonly string[];
  constructor(message: string, ids: readonly string[]) { super(message); this.ids = ids; }
}
const severalParts = (parts: readonly number[]) => new CannotSend(
  `changes on ${parts.length} parts of an assembly cannot be saved together yet: an edit list applies to one part`, parts.map(String));

/** The parts a save writes (specify-core docs/open.md): every part with at least one answer in the
 * session (approved defaults count), and a file's only part. A part with no answers is left as it is. */
export function writtenParts(parts: Record<string, PmiAnswers>, several: boolean): number[] {
  const out = new Set(Object.entries(parts).filter(([, answers]) => Object.keys(answers).some((id) => !id.startsWith('_'))).map(([k]) => Number(k)));
  if (!several) out.add(0);
  return [...out].sort((a, b) => a - b);
}

/** The parts a save with defaults lists the defaults of: every part of the file (``indexes``, and any
 * part the session has), so approving a part's defaults is enough for it to be written (the command
 * applies approvals as answers before it writes). A file of one part is part 0. */
export function defaultParts(parts: Record<string, PmiAnswers>, indexes: readonly number[], several: boolean): number[] {
  if (!several) return [0];
  return [...new Set([...indexes, ...Object.keys(parts).map(Number)])].sort((a, b) => a - b);
}

/** The edit list for a part: each answer the page shows (``after``) that differs from the session's
 * (``base``, what the file stores) is a fill, each default confirmed (``raw``, the value the person
 * saw, as JSON) is a fill of that value, and an answer the file stores that the page cleared is a
 * clear (the file stores only decisions that leave no PMI). Specify's own `_` records are never sent. */
export function editsFor(base: PmiAnswers, after: PmiAnswers, defaults: readonly Pick<Default, 'id' | 'raw'>[] = []): EditList {
  const fill: Record<string, unknown> = {};
  const clear: string[] = [];
  const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
  for (const id of new Set([...Object.keys(base), ...Object.keys(after)])) {
    if (id.startsWith('_') || same(base[id], after[id])) continue;
    if (id in after) fill[id] = after[id];
    else clear.push(id);
  }
  for (const d of defaults) if (!(d.id in fill)) fill[d.id] = JSON.parse(d.raw);
  return { ...(Object.keys(fill).length ? { fill } : {}), ...(clear.length ? { clear } : {}) };
}

const given = (answers: PmiAnswers) => JSON.stringify(Object.entries(answers).filter(([id]) => !id.startsWith('_')).sort(([a], [b]) => (a < b ? -1 : 1)));

/** The same defaults, part, question and value, in whatever order. */
export function sameDefaults(a: readonly Default[], b: readonly Default[]): boolean {
  const key = (list: readonly Default[]) => list.map((d) => JSON.stringify([d.part, d.id, d.raw])).sort().join('\n');
  return key(a) === key(b);
}

/** Saves the page's edits at the revision the page shows. ``shown()``: the answers the page shows for
 * the part shown, so a save never writes answers the person has not seen. ``defaultsOf(parts)``: the
 * defaults a save with defaults would write for the answers ``parts`` (every part's, by index). The
 * edit list covers every part the page changed (the command applies a list to a file of one part). */
export function sessionSaver(store: AnswersStore, session: Pick<AnswersSession, 'parts' | 'signal' | 'answers' | 'shown' | 'changedParts' | 'commit'>, shown: () => PmiAnswers, defaultsOf: (parts: Record<string, PmiAnswers>) => Promise<Default[]>, write = post): Saver {
  return {
    async save({ defaults }) {
      session.signal.throwIfAborted();
      const rev = await store.settled();
      session.signal.throwIfAborted();
      const answers = store.get();
      if (given(answers) !== given(shown())) throw new NotShown('the answers changed just now');
      // The parts as the page shows them: the session's, with the page's unsaved changes.
      const parts = session.parts();
      for (const part of session.changedParts()) parts[String(part)] = session.shown(part);
      // The whole set, part by part, value by value: any default added, gone or changed (an agent
      // answered meanwhile) means a list the person has not confirmed.
      if (defaults.length && !sameDefaults(await defaultsOf(parts), defaults)) throw new DefaultsChanged('the defaults to write changed');
      // Pagehide may have run while the defaults review was pending. It cancels this signal before
      // closed, preventing a deferred POST and aborting an in-flight request.
      session.signal.throwIfAborted();
      const indexes = [...new Set([...session.changedParts(), ...defaults.map((d) => d.part)])].sort((a, b) => a - b);
      if (indexes.length > 1) throw severalParts(indexes);
      const edits: EditList = indexes.length
        ? editsFor(session.answers(indexes[0]!), session.shown(indexes[0]!), defaults)
        : {};
      try {
        const saved = await write(rev, edits, session.signal);
        // The edits are in the file now.
        session.commit();
        session.receiveSave(saved);
        const answers = { ...parts };
        for (const d of defaults) answers[String(d.part)] = { ...answers[String(d.part)], [d.id]: JSON.parse(d.raw) };
        return { ...saved, answers };
      } catch (error) {
        // Both are 409s with the session's rev: when the session is still at the page's rev, it is the file that changed.
        if (error instanceof Refused && error.status === 409 && typeof error.body.rev === 'number') {
          throw error.body.rev === rev ? new FileChanged(error.message) : new Stale(error.message);
        }
        if (error instanceof Refused && Array.isArray(error.body.failures)) {
          throw new EditsRefused(error.message, error.body.failures as EditFailure[]);
        }
        throw error;
      }
    },
  };
}

/** Match the save response's flat list in part order. A part whose answers were cleared can be
 * written too, but the contract does not expose which empty parts were touched. */
export function matchMissing(missing: readonly string[], parts: readonly {
  open: readonly string[]; labels: readonly string[] | null; required: boolean;
}[]): string[] | null {
  // Each offset is a way to consume that many question IDs. Null means the labels are ambiguous.
  let matches = new Map<number, string[] | null>([[0, []]]);
  for (const { open, labels, required } of parts) {
    if (!open.length) continue;
    const next = required ? new Map<number, string[] | null>() : new Map(matches);
    for (const [offset, previous] of matches) {
      if (!open.every((id, i) => missing[offset + i] === id)) continue;
      const end = offset + open.length;
      next.set(end, next.has(end) || !previous || !labels ? null : [...previous, ...labels]);
    }
    matches = next;
  }
  return matches.get(missing.length) ?? null;
}
