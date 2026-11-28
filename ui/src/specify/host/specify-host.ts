// The host's side of SpecifyReview as one piece (the useSpecifyHost hook wraps this; the app and the Draftwright
// editor use it, so neither re-writes it): on open it reads the file's table and builds the review's props; on Save
// it composes answers, removals and call-outs into one edit list, applies it, and reads the saved file again.
// No React here: a host that is not React can subscribe to it directly.
import type { WorkerClient } from '../../engine/rpc.ts';
import type { Edited, EditOutcome, EditRefused, EngineFile, WriteReport } from '../../engine/save.ts';
import type { PmiAnswers } from '../../types/pmi.ts';
import type { DrawnPmi } from '../drawn.ts';
import { PendingEdits, type Pending } from '../edits/pending.ts';
import type { EditList } from '../edits/types.ts';
import { composeEditList, editingProps, oneViewOf, readHost, type EditingProps, type HostRead } from './compose.ts';

export interface SpecifyHostOptions {
  client: Pick<WorkerClient, 'request' | 'edit' | 'acceptDefaults'>;
  /** The file (and, for an assembly, the part: the host draws its own picker). A change of it re-reads the table. */
  file: EngineFile;
  /** The host's own answers for the part shown (the host keeps them; call-outs are reported through ``onAnswers``). */
  answers: PmiAnswers;
  /** A call-out is an answer (extra.N): the host's answers with it added, or with it taken back. */
  onAnswers?: (answers: PmiAnswers) => void;
  /** Written into the file by Save (the engine's `edit` `project`, base64) or as a `payload`. */
  project?: string;
  payload?: { name: string; value: unknown };
  /** Save writes with Specify's defaults accepted (`WorkerClient.acceptDefaults`) for the questions the answers leave open. */
  acceptDefaults?: boolean;
  /** The bom.* rows are written only when the file's table has them (default true; false: written as given). */
  bom?: boolean;
  /** The composed edit list (answers, removals, call-outs, defaults, the BOM rule) before it is sent; return the list to send. */
  beforeSave?: (edits: EditList) => EditList | Promise<EditList>;
  /** Sends the edit list somewhere other than the worker (the app's HTTP session); it must apply it to ``file`` all or nothing. */
  saveVia?: (edits: EditList, file: EngineFile) => Promise<EditOutcome>;
  /** The edit was applied: the outcome, and the file as saved (the host stores its bytes). The hook has already re-read it. */
  onSaved?: (outcome: Edited, saved: EngineFile) => void;
  /** The scene as drawn, for the number shown on each call-out. */
  drawn?: DrawnPmi | null;
}

export interface SpecifyHostState {
  read: HostRead | null;
  loading: boolean;
  error: string | null;
  saving: boolean;
  lastReport: WriteReport | null;
  /** The last save's refusal, naming each entry (null once a save is written). */
  refused: EditRefused | null;
  /** Bumps on every change, for subscribers. */
  version: number;
}

export type ReviewPropsFromHost = EditingProps & {
  oneView: NonNullable<ReturnType<typeof oneViewOf>>;
  lengthUnit: 'in' | 'mm';
};

const partOf = (file: EngineFile) => file.part ?? 0;

export class SpecifyHost {
  options: SpecifyHostOptions;
  private state: SpecifyHostState = { read: null, loading: false, error: null, saving: false, lastReport: null, refused: null, version: 0 };
  private listeners = new Set<() => void>();
  /** The file as the hook has it: the host's, or the one a save wrote (until the host gives another). */
  private file: EngineFile | null = null;
  private given: EngineFile | null = null;
  private saves = 0;
  private token = 0;
  /** Held edits by part: they outlast a part switch, and go with a new file. */
  private held = new Map<number, PendingEdits>();
  private readonly callOutKeys = new Map<string, string>();
  private readonly displaced = new Map<string, Pending>();
  /** Settles when the file last given has been read. */
  ready: Promise<void> = Promise.resolve();

  constructor(options: SpecifyHostOptions) { this.options = options; }

  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  getSnapshot = () => this.state;
  private set(patch: Partial<SpecifyHostState>) {
    this.state = { ...this.state, ...patch, version: this.state.version + 1 };
    for (const listener of [...this.listeners]) listener();
  }

  /** The file the host shows now. A new file (other bytes or name, or a new revision of the same part) drops the
   * held edits; another part of the same file keeps each part's. Reads the table. */
  setFile(file: EngineFile): Promise<void> {
    const before = this.given;
    if (before && before.bytes === file.bytes && before.name === file.name && before.sourceId === file.sourceId && partOf(before) === partOf(file)) return this.ready;
    const samePartsOfFile = before && before.bytes === file.bytes && before.name === file.name && partOf(before) !== partOf(file);
    if (!samePartsOfFile) { this.held = new Map(); this.callOutKeys.clear(); this.displaced.clear(); }
    this.given = file;
    this.file = file;
    return this.load();
  }

  /** Reads the table of the file again (after anything else changed it). */
  refresh(): Promise<void> { return this.load(); }

  private load(): Promise<void> {
    const file = this.file;
    if (!file) return Promise.resolve();
    const token = ++this.token;
    this.set({ loading: true, error: null });
    this.ready = readHost(this.options.client, file).then((read) => {
      if (token === this.token) this.set({ read, loading: false });
    }, (error: unknown) => {
      if (token === this.token) this.set({ loading: false, error: error instanceof Error ? error.message : String(error) });
    });
    return this.ready;
  }

  private pending(): PendingEdits { return this.held.get(partOf(this.file ?? this.options.file)) ?? new PendingEdits(); }
  private setPending(next: PendingEdits) {
    const part = partOf(this.file ?? this.options.file);
    const held = new Map(this.held);
    if (next.size) held.set(part, next); else held.delete(part);
    this.held = held;
    this.set({});
  }

  /** The held edits of the part shown. */
  pendingEdits(): PendingEdits { return this.pending(); }

  /** SpecifyReview's props from the file's table (null until it is read, or while another file is being read). */
  reviewProps(): ReviewPropsFromHost | null {
    const { read } = this.state;
    if (!read || !this.file || read.file !== this.file) return null;
    const { answers, onAnswers, drawn } = this.options;
    const pending = this.pending();
    return {
      ...editingProps({
        read, part: partOf(this.file), pending, setPending: (next) => this.setPending(next), callOutKeys: this.callOutKeys, displaced: this.displaced,
        drawn, answerIds: Object.keys(answers),
        // A call-out is an answer in the host's store, so it outlasts this hook's state.
        answered: onAnswers ? (key, value) => {
          const next = { ...this.options.answers };
          if (value === undefined) delete next[key]; else next[key] = value;
          onAnswers(next);
        } : undefined,
      }),
      oneView: oneViewOf(read, pending),
      lengthUnit: read.units,
    };
  }

  /** The edit list Save would send now (answers, defaults accepted if asked, removals, call-outs, the BOM rule), or null before the file is read. */
  async compose(): Promise<EditList | null> {
    const { read } = this.state;
    const file = this.file;
    if (!read || !file || read.file !== file) return null;
    const { client, acceptDefaults, bom } = this.options;
    const answers = acceptDefaults ? await client.acceptDefaults(file, this.options.answers) : this.options.answers;
    return composeEditList(read, answers, this.pending(), bom ?? true);
  }

  /** Saves: composes the edit list, applies it, and reads the saved file again. null: nothing to save. */
  async save(): Promise<EditOutcome | null> {
    const file = this.file;
    if (!file || this.state.saving) return null;
    this.set({ saving: true });
    try {
      let edits = await this.compose();
      if (!edits) return null;
      if (this.options.beforeSave) edits = await this.options.beforeSave(edits);
      if (Object.keys(edits).length === 0) return null;
      const { client, project, payload, saveVia } = this.options;
      const outcome = saveVia ? await saveVia(edits, file) : await client.edit(file, { edits, ...(project !== undefined ? { project } : {}), ...(payload ? { payload } : {}) });
      if (!outcome.written) { this.set({ refused: outcome }); return outcome; }
      // The edits are in the file: the saved file is the one shown from here, read again for its table.
      const saved: EngineFile = { ...file, bytes: outcome.bytes, sourceId: `${file.sourceId.replace(/@saved\d+$/, '')}@saved${++this.saves}` };
      const part = partOf(file);
      const held = new Map(this.held); held.delete(part); this.held = held;
      for (const key of [...this.callOutKeys.keys()]) if (key.startsWith(`${part}:`)) this.callOutKeys.delete(key);
      for (const key of [...this.displaced.keys()]) if (key.startsWith(`${part}:`)) this.displaced.delete(key);
      this.file = saved;
      this.set({ lastReport: outcome.report, refused: null });
      await this.load();
      this.options.onSaved?.(outcome, saved);
      return outcome;
    } finally { this.set({ saving: false }); }
  }
}
