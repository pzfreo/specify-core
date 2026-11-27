// Save: the page sends the edits the review panel has collected (`POST save {rev, edits}` or `{rev, partEdits}`,
// specify-pmi docs/open.md, session API 2) and the command applies them to the file, all or
// nothing, with the same writer and naming as the agent's `save`. Anything not sent is left as the
// file has it. It never refuses for open questions: many AP242 files have no complete answer to
// everything. It says what is still open.
import type { PmiAnswers } from '../src/types/pmi';
import type { AnswersSession, AnswersStore } from './answers.ts';
import { Refused, save as post, type EditFailure, type SaveEdits, type Saved } from './local.ts';

export interface Saver { save(edits: SaveEdits, displayedRev?: number): Promise<Saved> }

/** Nothing saved: the session holds answers the page has not shown yet (they are being shown). */
export class NotShown extends Error {}
/** Nothing saved: the session moved on since the page's rev (an agent answered); ``rev`` is the session's. */
export class Stale extends Error {}
/** Nothing saved: (`--in-place`) the file was changed on disk by another program since it was opened. */
export class FileChanged extends Error {}
/** Nothing saved: the command refused the edit list (all or nothing); ``failures`` names every entry
 * that cannot be applied, and why. */
export class EditsRefused extends Error {
  readonly failures: readonly EditFailure[];
  constructor(message: string, failures: readonly EditFailure[]) { super(message); this.failures = failures; }
}

const given = (answers: PmiAnswers) => JSON.stringify(Object.entries(answers).filter(([id]) => !id.startsWith('_')).sort(([a], [b]) => (a < b ? -1 : 1)));

/** Saves the panel's edit list at the revision the page shows. ``shown()``: the session's answers as
 * the page shows them, so a save never goes ahead over answers the person has not seen. */
export function editsSaver(store: Pick<AnswersStore, 'settled' | 'get'>, session: Pick<AnswersSession, 'signal' | 'receiveSave'>, shown: () => PmiAnswers, write = post): Saver {
  return {
    async save(edits, displayedRev) {
      session.signal.throwIfAborted();
      const rev = await store.settled();
      session.signal.throwIfAborted();
      if ((displayedRev !== undefined && displayedRev !== rev) || given(store.get()) !== given(shown())) throw new NotShown('the answers changed just now');
      try {
        const saved = await write(rev, edits, session.signal);
        session.receiveSave(saved);
        return saved;
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
