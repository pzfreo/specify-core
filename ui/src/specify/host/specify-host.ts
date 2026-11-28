// The host's side of SpecifyReview as one piece (the useSpecifyHost hook wraps this; the local app and the Draftwright
// editor use it, so neither re-writes it): it reads the file's table and builds the review's props; it holds the
// person's edits (bins, call-outs, answers) per part, with the agent's drafts laid under them; on Save it composes
// them into one edit list, sends it, rebases what was held on what was written, and reads the saved file again.
// No React here: a host that is not React can subscribe to it directly.
import type { WorkerClient } from '../../engine/rpc.ts';
import type { Edited, EditOutcome, EditRefused, EngineFile, WriteReport } from '../../engine/save.ts';
import type { PmiAnswers, PmiQuestion } from '../../types/pmi.ts';
import type { DrawnPmi } from '../drawn.ts';
import { valueText, removeEdit } from '../edits/adapter.ts';
import { PendingEdits, withDrafts, withoutSent, editList, type Pending } from '../edits/pending.ts';
import type { AnswerDraft, EditList, PartEdits, SaveEdits } from '../edits/types.ts';
import { buildAnswerEdit, composeEditList, editingProps, oneViewOf, readHost, rowName, type EditingProps, type HostRead } from './compose.ts';

/** What a transport of the host's own (`saveVia`) reports of an edit applied elsewhere: the file as saved, when the host reopened it. */
export interface SavedElsewhere {
  written: true;
  /** The file as saved, which the host now shows (omit if it shows the same file still). */
  file?: EngineFile;
  /** Its table, if the host has read it already (else the hook reads ``file``). */
  read?: HostRead;
  /** The engine's latest review of it, for questions the bare part does not ask. */
  review?: { questions: readonly PmiQuestion[] };
}

/** The person's held edits as plain JSON (what ``SpecifyHost.heldEdits()`` returns and ``SpecifyHostOptions.held`` takes), so a host can keep
 * them across a reload. An agent's draft is never in it. ``callOuts`` and ``displaced`` are keyed as the host keeps them (``part:...``). */
export type HeldEdits = {
  version: 1;
  parts: { [part: string]: Pending[] };
  callOuts: { [at: string]: string };
  displaced: { [at: string]: Pending };
};

const SEVERAL_NEEDS_SAVE_VIA = "several parts need saveVia: the engine's edit takes one part's list";

export interface SpecifyHostOptions {
  /** The engine. Optional for a host that supplies ``read`` and ``saveVia`` and calls neither of the engine's. */
  client?: Pick<WorkerClient, 'request' | 'edit' | 'acceptDefaults'>;
  /** The file (and, for an assembly, the part: the host draws its own picker). A change of it re-reads the table. */
  file: EngineFile;
  /** The host's own answers for the part shown (the host keeps them; call-outs are reported through ``onAnswers``). A host that
   * reports each answer as it is given (``answered``) passes none here. */
  answers: PmiAnswers;
  /** The ids the host's answers use now, when they are not ``answers``' keys (an "Added by you" key never repeats one). */
  answerIds?: () => Iterable<string>;
  /** A call-out is an answer (extra.N): the host's answers with it added, or with it taken back. */
  onAnswers?: (answers: PmiAnswers) => void;
  /** Written into the file by Save (the engine's `edit` `project`, base64) or as a `payload`. */
  project?: string;
  payload?: { name: string; value: unknown };
  /** Save writes with Specify's defaults accepted (`WorkerClient.acceptDefaults`) for the questions the answers leave open. */
  acceptDefaults?: boolean;
  /** The bom.* rows are written only when the file's table has them (default true; false: written as given). */
  bom?: boolean;
  /** The composed edit list (answers, removals, call-outs, drafts, defaults, the BOM rule) before it is sent; return the list to send. Once per part sent. */
  beforeSave?: (edits: EditList) => EditList | Promise<EditList>;
  /** Sends the edits somewhere other than the worker (the app's HTTP session), applying them to ``file`` all or nothing.
   * It throws what it likes (the host's own conflicts); ``save`` lets it through. */
  saveVia?: (edits: SaveEdits, file: EngineFile) => Promise<EditOutcome | SavedElsewhere>;
  /** The file has several parts and edits go together, a list per part, to ``saveVia``. */
  several?: boolean;
  /** The agent's drafts by part, shown under the person's own edits and sent with them. */
  drafts?: Readonly<Record<string, AnswerDraft>>;
  /** The table of ``file``, when the host has read it already (the hook then does not). */
  read?: HostRead | null;
  /** Held edits survive a new file: the host's file changes under it (a save by someone else reopens the part). */
  keepHeld?: boolean;
  /** Held edits from an earlier session (``heldEdits()``, kept as JSON): taken once, at construction, and applied to the first file the host
   * shows (it survives that file even when it is not the one they were held on). Later changes to this option are ignored; an unknown version is none. */
  held?: HeldEdits;
  /** The held edits changed, with all of them (``held``) for the host to keep: the person changed one (kind 'edit'), an answer became one
   * ('answer'), or all were discarded ('discard'). A save that wrote some of them reports kind 'edit' (for the part saved), with what is still held. */
  onHeldChange?: (kind: 'answer' | 'edit' | 'discard', part: number, held: HeldEdits) => void;
  /** The edit was applied: the outcome, and the file as saved (the host stores its bytes). The hook has already re-read it. */
  onSaved?: (outcome: Edited | SavedElsewhere, saved: EngineFile) => void;
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
const sameFile = (a: EngineFile | undefined, b: EngineFile | undefined) =>
  Boolean(a && b && a.bytes === b.bytes && a.name === b.name && a.sourceId === b.sourceId && partOf(a) === partOf(b));
const NO_EDITS = new PendingEdits();
const NO_DRAFTS: Readonly<Record<string, AnswerDraft>> = {};
function noEngine(): never { throw new Error('The host gave no engine (`client`), so it cannot read or write a file itself.'); }
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

interface Answered { part: number; id: string; value: unknown }
interface Sent { edits: SaveEdits; parts: Set<number>; sent: Map<number, PendingEdits> }

export class SpecifyHost {
  options: SpecifyHostOptions;
  private state: SpecifyHostState = { read: null, loading: false, error: null, saving: false, lastReport: null, refused: null, version: 0 };
  private listeners = new Set<() => void>();
  /** The file as the host has it: the host's, or the one a save wrote (until the host gives another). */
  private file: EngineFile | null = null;
  private given: EngineFile | null = null;
  private saves = 0;
  private token = 0;
  private reading: EngineFile | null = null;
  /** Held edits by part: they outlast a part switch, and go with a new file (unless the host keeps them). */
  private held = new Map<number, PendingEdits>();
  private readonly callOutKeys = new Map<string, string>();
  private readonly displaced = new Map<string, Pending>();
  /** Answers that land while Save runs: rebuilt against the saved file once it is read. */
  private inFlight: Map<string, Answered> | null = null;
  private memo: { held: unknown; drafts: unknown; read: unknown; part: number; value: ReadonlyMap<number, PendingEdits> } | null = null;
  /** Settles when the file last given has been read. */
  ready: Promise<void> = Promise.resolve();

  /** The ``held`` option, until the first file takes it. */
  private initialHeld: HeldEdits | null;

  constructor(options: SpecifyHostOptions) {
    this.options = options;
    this.initialHeld = options.held?.version === 1 ? options.held : null;
  }

  /** The person's held edits as plain data (never the agent's drafts), for the host to keep and give back as ``held``. */
  heldEdits(): HeldEdits {
    const parts: HeldEdits['parts'] = {};
    for (const [part, list] of this.held) if (list.size) parts[String(part)] = list.values();
    return { version: 1, parts, callOuts: Object.fromEntries(this.callOutKeys), displaced: Object.fromEntries(this.displaced) };
  }

  private takeInitialHeld() {
    const initial = this.initialHeld;
    if (!initial) return;
    this.initialHeld = null;
    const held = new Map<number, PendingEdits>();
    for (const [part, list] of Object.entries(initial.parts ?? {})) {
      const local = new PendingEdits((list ?? []).filter((edit) => !edit.draft));
      if (local.size) held.set(Number(part), local);
    }
    this.held = held;
    for (const [at, key] of Object.entries(initial.callOuts ?? {})) this.callOutKeys.set(at, key);
    for (const [at, edit] of Object.entries(initial.displaced ?? {})) this.displaced.set(at, edit);
    this.memo = null;
  }

  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  getSnapshot = () => this.state;
  private set(patch: Partial<SpecifyHostState>) {
    this.state = { ...this.state, ...patch, version: this.state.version + 1 };
    for (const listener of [...this.listeners]) listener();
  }

  /** Takes the file the host now gives (its options'): a new file (other bytes or name) drops the held edits unless the host keeps them;
   * another part of the same bytes keeps each part's. Nothing is read here. */
  private sync() {
    const file = this.options.file;
    if (sameFile(this.given ?? undefined, file)) return;
    const lineage = Boolean(this.file && this.file.bytes === file.bytes && this.file.name === file.name);
    if (!lineage && !this.options.keepHeld) {
      this.held = new Map(); this.callOutKeys.clear(); this.displaced.clear(); this.memo = null;
    }
    this.takeInitialHeld();
    const read = this.state.read;
    // The same bytes again (the saved file the host now stores): the table read is still its own.
    if (lineage && read && partOf(read.file) === partOf(file)) this.state = { ...this.state, read: { ...read, file } };
    this.given = file;
    this.file = file;
  }

  private readOf(file: EngineFile | null): HostRead | null {
    const supplied = this.options.read;
    if (supplied && sameFile(supplied.file, file ?? undefined)) return supplied;
    const read = this.state.read;
    return read && sameFile(read.file, file ?? undefined) ? read : null;
  }

  /** The file the host shows now. Reads the table unless the host supplied it. */
  setFile(file: EngineFile = this.options.file): Promise<void> {
    this.options.file = file;
    this.sync();
    if (this.file && !this.readOf(this.file) && !sameFile(this.reading ?? undefined, this.file)) return this.load();
    return this.ready;
  }

  /** Reads the table of the file again (after anything else changed it). */
  refresh(): Promise<void> { this.sync(); return this.load(); }

  private load(): Promise<void> {
    const file = this.file;
    const { client } = this.options;
    if (!file || !client) return Promise.resolve();
    const token = ++this.token;
    this.reading = file;
    this.set({ loading: true, error: null });
    this.ready = readHost(client, file).then((read) => {
      if (token === this.token) { this.reading = null; this.set({ read, loading: false }); }
    }, (error: unknown) => {
      if (token === this.token) { this.reading = null; this.set({ loading: false, error: error instanceof Error ? error.message : String(error) }); }
    });
    return this.ready;
  }

  private part(): number { this.sync(); return partOf(this.file ?? this.options.file); }

  /** Every part's held edits with the agent's drafts under them (a draft is never held: the person's edit on its row replaces it). */
  allPending(): ReadonlyMap<number, PendingEdits> {
    this.sync();
    const drafts = this.options.drafts ?? NO_DRAFTS;
    const read = this.readOf(this.file);
    const part = this.part();
    const m = this.memo;
    if (m && m.held === this.held && m.drafts === drafts && m.read === read && m.part === part) return m.value;
    const rows = read ? [...(read.model.part?.rows ?? []), ...read.model.features.flatMap((g) => g.rows), ...(read.model.datums?.rows ?? [])] : [];
    const value = withDrafts(this.held, drafts, (p, id, v) => {
      const row = p === part ? rows.find((r) => r.id === id) : undefined;
      const words = id.startsWith('part.') ? id.slice(5).replace(/_/g, ' ') : id;
      return { name: row?.name ?? words.charAt(0).toUpperCase() + words.slice(1), text: valueText(v, row?.question, read?.analysis) };
    });
    this.memo = { held: this.held, drafts, read, part, value };
    return value;
  }

  /** The part shown's edits (drafts included). */
  pendingEdits(): PendingEdits { return this.allPending().get(this.part()) ?? NO_EDITS; }

  /** Replaces the part shown's held edits with ``next`` (a draft in it is not the person's: dropped). */
  setPending(next: PendingEdits) {
    const part = this.part();
    const local = new PendingEdits(next.values().filter((edit) => !edit.draft));
    const held = new Map(this.held);
    if (local.size) held.set(part, local); else held.delete(part);
    this.held = held;
    this.set({});
    this.options.onHeldChange?.('edit', part, this.heldEdits());
  }

  /** An answer of the review changed: part ``part``'s row ``id`` is ``value`` (undefined: taken back). With ``at`` (the part shown's table,
   * and the engine's review) it becomes that row's held edit, replacing any it had; without, it is only kept for a Save in flight. */
  answered(part: number, id: string, value: unknown, at?: { read: HostRead; review?: { questions: readonly PmiQuestion[] } }) {
    this.inFlight?.set(`${part}:${id}`, { part, id, value });
    if (!at) return;
    const edit = buildAnswerEdit(at.read, id, value, at.review);
    const held = new Map(this.held);
    const list = held.get(part) ?? new PendingEdits();
    const next = edit ? list.with(edit) : list.without(id);
    if (next.size) held.set(part, next); else held.delete(part);
    this.held = held;
    this.set({});
    this.options.onHeldChange?.('answer', part, this.heldEdits());
  }

  /** Drops every held edit (Discard). */
  discard() {
    this.held = new Map(); this.callOutKeys.clear(); this.displaced.clear(); this.memo = null;
    this.set({});
    this.options.onHeldChange?.('discard', this.part(), this.heldEdits());
  }

  /** SpecifyReview's props from the file's table (null until it is read, or while another file is being read). */
  reviewProps(): ReviewPropsFromHost | null {
    this.sync();
    const read = this.readOf(this.file);
    if (!read || !this.file) return null;
    const { onAnswers, drawn } = this.options;
    const pending = this.pendingEdits();
    return {
      ...editingProps({
        read, part: partOf(this.file), pending, setPending: (next) => this.setPending(next), callOutKeys: this.callOutKeys, displaced: this.displaced,
        drawn, answerIds: this.options.answerIds ?? (() => Object.keys(this.options.answers)),
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

  /** What Save sends: a list for the part shown, or (several parts) a list per part with edits; null when there is nothing. */
  private async plan(): Promise<Sent | null> {
    this.sync();
    const file = this.file!;
    const read = this.readOf(file);
    if (!read) return null;
    const { client, acceptDefaults, bom, beforeSave, several, saveVia } = this.options;
    if (several && !saveVia) throw new Error(SEVERAL_NEEDS_SAVE_VIA);
    const engine = () => client ?? noEngine();
    const part = partOf(file);
    const all = this.allPending();
    const answers = acceptDefaults ? await engine().acceptDefaults(file, this.options.answers) : this.options.answers;
    const listFor = async (p: number): Promise<EditList> => {
      const edits = p === part ? composeEditList(read, answers, all.get(p) ?? new PendingEdits(), bom ?? true) : editList(all.get(p)?.values() ?? []);
      return beforeSave ? beforeSave(edits) : edits;
    };
    const parts = new Set<number>(several ? [...all.keys(), part] : [part]);
    const lists: PartEdits[] = [];
    for (const p of [...parts].sort((a, b) => a - b)) {
      const edits = await listFor(p);
      if (Object.keys(edits).length > 0) lists.push({ part: p, edits });
    }
    if (!lists.length) return null;
    const sentParts = new Set(several ? lists.map((l) => l.part) : [part]);
    return {
      edits: several ? lists : lists[0]!.edits, parts: sentParts,
      sent: new Map([...all].filter(([p]) => sentParts.has(p))),
    };
  }

  /** The edit list Save would send now for the part shown, or null before the file is read. */
  async compose(): Promise<EditList | null> {
    this.sync();
    const read = this.readOf(this.file);
    if (!read || !this.file) return null;
    const { client, acceptDefaults, bom } = this.options;
    const answers = acceptDefaults ? await (client ?? noEngine()).acceptDefaults(this.file, this.options.answers) : this.options.answers;
    return composeEditList(read, answers, this.pendingEdits(), bom ?? true);
  }

  /** Saves: composes the edit list, sends it, rebases what was held on what was written, and reads the saved file again. null: nothing to save. */
  async save(): Promise<EditOutcome | SavedElsewhere | null> {
    this.sync();
    const file = this.file;
    if (!file || this.state.saving) return null;
    this.set({ saving: true });
    const during = new Map<string, Answered>();
    try {
      const plan = await this.plan();
      if (!plan) return null;
      const sentCallOuts = new Map(this.callOutKeys);
      const sentDisplaced = new Map(this.displaced);
      this.inFlight = during;
      const { client, project, payload, saveVia } = this.options;
      const one = Array.isArray(plan.edits) ? null : plan.edits;
      if (!saveVia && !one) throw new Error(SEVERAL_NEEDS_SAVE_VIA);
      const outcome = saveVia ? await saveVia(plan.edits, file)
        : await (client ?? noEngine()).edit(file, { edits: one!, ...(project !== undefined ? { project } : {}), ...(payload ? { payload } : {}) });
      if (!outcome.written) { this.set({ refused: outcome }); return outcome; }
      // The edits are in the file: the saved file is the one shown from here, read again for its table.
      let saved = file;
      let read = this.readOf(file);
      let review: SavedElsewhere['review'];
      if ('bytes' in outcome) {
        saved = { ...file, bytes: outcome.bytes, sourceId: `${file.sourceId.replace(/@saved\d+$/, '')}@saved${++this.saves}` };
        read = await readHost(client ?? noEngine(), saved);
      } else {
        review = outcome.review;
        if (outcome.file) { saved = outcome.file; read = outcome.read ?? await readHost(client ?? noEngine(), saved); }
      }
      this.rebase(plan.sent, sentCallOuts, sentDisplaced, during, outcome.written && saved !== file ? read : null, review);
      this.file = saved;
      this.memo = null;
      this.set({ read, lastReport: 'report' in outcome ? outcome.report : this.state.lastReport, refused: null });
      this.options.onHeldChange?.('edit', partOf(file), this.heldEdits());
      this.options.onSaved?.(outcome, saved);
      return outcome;
    } finally { if (this.inFlight === during) this.inFlight = null; this.set({ saving: false }); }
  }

  /** What was sent is in the file: drop it from the held edits, and rebuild any made while Save ran against the saved file. */
  private rebase(sent: ReadonlyMap<number, PendingEdits>, sentCallOuts: ReadonlyMap<string, string>, sentDisplaced: ReadonlyMap<string, Pending>,
    during: ReadonlyMap<string, Answered>, reopened: HostRead | null, review: SavedElsewhere['review']) {
    const remaining = withoutSent(this.held, sent);
    for (const { part, id, value } of during.values()) {
      const savedEdit = sent.get(part)?.get(id);
      if (!savedEdit || savedEdit.kind === 'refused' || !reopened || partOf(reopened.file) !== part) continue;
      const matchesSaved = (savedEdit.kind === 'fill' || savedEdit.kind === 'change') && same(value, savedEdit.value);
      let rebuilt: Pending | null = null;
      if (!matchesSaved) {
        if (value === undefined) {
          const row = reopened.table.rows.find((r) => r.id === id);
          const q = reopened.bare.find((x) => x.id === id) ?? review?.questions.find((x) => x.id === id);
          const name = rowName(reopened, id);
          if (row?.state === 'stated') rebuilt = removeEdit(row, q, name, reopened.analysis);
          else if (row?.state === 'decided_none') rebuilt = { kind: 'clear', id, name };
        } else rebuilt = buildAnswerEdit(reopened, id, value, review);
      }
      const list = remaining.get(part) ?? new PendingEdits();
      const next = rebuilt ? list.with(rebuilt) : list.without(id);
      if (next.size) remaining.set(part, next); else remaining.delete(part);
    }
    for (const [at, key] of sentCallOuts) {
      const part = Number(at.split(':')[0]);
      if (this.callOutKeys.get(at) === key && !remaining.get(part)?.get(key)) this.callOutKeys.delete(at);
    }
    for (const [key, edit] of sentDisplaced) if (this.displaced.get(key) === edit) this.displaced.delete(key);
    this.held = remaining;
  }
}
