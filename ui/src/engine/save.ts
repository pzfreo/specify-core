// The save side of the engine (specify-pmi 0.3): a file changes only by an edit list. These are the typed
// operations a host calls on the engine from createIntentEngine; see ui/README.md "Embedding and saving".
import type { PmiAnswers } from '../types/pmi';
import type { EditList } from '../specify/edits/types';

/** The file an operation is on: as `open` takes it. `sourceId` names this revision (a new one reopens the engine);
 * `part` picks one part of an assembly. */
export interface EngineFile { name: string; bytes: Uint8Array; sourceId: string; part?: number }

/** What the writer reports of a save (specify-pmi WriteReport; further members are kept as they come). */
export interface WriteReport {
  written: string[]; not_written: string[]; warnings: string[];
  /** The characteristic numbers the edit retired (never reused). */
  retired: number[];
  /** True when the list changed nothing: the bytes are the file's own. */
  unchanged: boolean;
  [member: string]: unknown;
}

/** An edit applied: the new file. */
export interface Edited { written: true; bytes: Uint8Array; report: WriteReport; source_sha256: string }
/** An entry of an edit list that cannot be applied, and why. */
export interface EditEntryFailure { entry: string; reason: string }
/** An edit refused, all or nothing: every entry that cannot be applied. */
export interface EditRefused { written: false; failures: EditEntryFailure[]; message: string }
export type EditOutcome = Edited | EditRefused;

/** What `edit` takes: a 0.3 edit list, or a 0.2 answers file. `project` is the host's base64 project, stored in the
 * file (`draftwright editor project`); `payload` is any application's `{name, value}`; at most one of the two. */
export interface EditRequest { edits: EditList | PmiAnswers; project?: string; payload?: { name: string; value: unknown } }

/** Saved answers matched to a file (specify-pmi `rematchAnswers`). */
export interface Rematched {
  answers: PmiAnswers;
  unresolved?: Record<string, string>;
  migrated?: unknown;
  unverified?: unknown;
  automatic?: string[];
}
/** What `rematch` takes: the answers saved outside the STEP, the evidence they were saved with (`analysis.evidence`
 * of the revision they were made on), and the ids the default path answered. */
export interface RematchRequest { answers: PmiAnswers; evidence?: unknown; automatic?: readonly string[] }
