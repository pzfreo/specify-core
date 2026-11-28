// What a host builds around SpecifyReview from the file's feature table, shared by the app and by the
// useSpecifyHost hook so there is one copy: reading the table into the review's props, the bar's edits for
// an answer, a removal or a call-out, and the one edit list a save sends. No React here.
import type { ComponentProps } from 'react';
import type { WorkerClient } from '../../engine/rpc.ts';
import type { EngineFile } from '../../engine/save.ts';
import type { IntentReview, PmiAnalysis, PmiAnswers, PmiExtra, PmiQuestion } from '../../types/pmi.ts';
import type { SpecifyReview, CalledOut } from '../SpecifyReview.tsx';
import type { DrawnPmi } from '../drawn.ts';
import { describe as describeExtra } from '../added.ts';
import { describeFaces } from '../datums.ts';
import type { OneViewPending, OneViewRow } from '../one-view.ts';
import { labelStated } from '../units.ts';
import { answerEdit, asEngineTakes, callOutEdit, isDimensionQuestion, nextExtraKey, removeEdit, statedRows, type StatedRows } from './../edits/adapter.ts';
import { bareAnalysis, fileLengthUnit, panelModel, tableOf, type FeatureTable, type PanelModel } from '../edits/table.ts';
import { PendingEdits, editList, removalTargets, type Pending } from '../edits/pending.ts';
import type { EditList } from '../edits/types.ts';

type ReviewProps = ComponentProps<typeof SpecifyReview>;

/** A file read for the review: its table, and what the review needs of it. */
export interface HostRead {
  file: EngineFile;
  analysis: PmiAnalysis;
  /** The file's feature table (the rows an edit list names). */
  table: FeatureTable;
  /** The questions asked of the bare part: every row's question, stated or not. */
  bare: PmiQuestion[];
  /** The stated rows as the review shows them. */
  stated: StatedRows;
  /** The review panel's model: the table, named. */
  model: PanelModel;
  /** The file's length unit. */
  units: 'in' | 'mm';
}

/** Reads ``file``'s table from the engine (`open`, whose analysis carries it) and the questions its rows answer. */
export async function readHost(client: Pick<WorkerClient, 'request'>, file: EngineFile): Promise<HostRead> {
  const snapshot = await client.request<{ analysis: PmiAnalysis }>('open', file);
  return readAnalysis(client, file, snapshot.analysis);
}

/** The same from an analysis the host already has (it opened the file for its mesh too). */
export async function readAnalysis(client: Pick<WorkerClient, 'request'>, file: EngineFile, analysis: PmiAnalysis): Promise<HostRead> {
  const bare = await client.request<IntentReview>('review-analysis-given', { analysis: bareAnalysis(analysis), answers: {} });
  const table = tableOf(analysis);
  return {
    file, analysis, table, bare: bare.questions, stated: statedRows(table, bare.questions, analysis),
    model: panelModel(table, bare.questions, analysis.features), units: fileLengthUnit(file.bytes),
  };
}

/** How the bar names row ``id`` ("Ø1.6 hole position", "Material"). */
export function rowName(read: Pick<HostRead, 'model' | 'bare'>, id: string): string {
  const rows = [...(read.model.part?.rows ?? []), ...read.model.features.flatMap((g) => g.rows), ...(read.model.datums?.rows ?? [])];
  const row = rows.find((r) => r.id === id);
  if (row) return row.name;
  const q = read.bare.find((x) => x.id === id);
  return q?.label ?? q?.prompt ?? id;
}

/** The pending edit answer ``value`` makes on row ``id`` (null: the row is as the file has it), named and worded as the bar lists it.
 * ``review``: the engine's latest review, for a question the bare part does not ask. */
export function buildAnswerEdit(read: HostRead, id: string, value: unknown, review?: { questions: readonly PmiQuestion[] }): Pending | null {
  const row = read.table.rows.find((r) => r.id === id);
  const q = read.bare.find((x) => x.id === id) ?? review?.questions.find((x) => x.id === id);
  // "Added by you": named and worded as the review lists it ("Flatness 0.05" on its faces).
  const added = id.startsWith('extra.') && value && typeof value === 'object' ? value as PmiExtra : null;
  const edit = answerEdit(id, value, row, q, added ? 'Added by you' : rowName(read, id));
  if (edit?.kind === 'fill' && added) edit.text = describeExtra(added);
  // Faces by what they are ("bottom face"), never by their numbers.
  const faces = (v: unknown) => Array.isArray(v) && v.length > 0 && v.every((f) => typeof f === 'number');
  if (edit && (edit.kind === 'fill' || edit.kind === 'change') && faces(value)) edit.text = describeFaces(value as number[], read.analysis);
  if (edit?.kind === 'change' && faces(edit.expect)) edit.was = describeFaces(edit.expect as number[], read.analysis);
  return edit;
}

/** The single PMI view's rows and the part's pending edits. */
export function oneViewOf(read: HostRead, pending: PendingEdits): NonNullable<ReviewProps['oneView']> {
  const bareById = new Map(read.bare.map((q) => [q.id, q]));
  const rows: OneViewRow[] = [
    ...read.table.rows.map((row): OneViewRow => {
      const q = bareById.get(row.id);
      return { id: row.id, state: row.state, open: row.open, faces: row.faces, label: rowName(read, row.id), ...(q ? { question: q } : {}) };
    }),
    ...read.model.other.map((o): OneViewRow => ({ id: o.key, state: 'stated', open: false, faces: o.faces, label: o.label, other: true })),
  ];
  const map = new Map<string, OneViewPending>();
  for (const edit of pending.values()) {
    map.set(edit.id, {
      kind: edit.kind, id: edit.id,
      ...('value' in edit ? { value: edit.value } : {}),
      ...('text' in edit ? { text: edit.text } : {}),
      ...('was' in edit ? { was: edit.was } : {}),
      ...('targets' in edit ? { targets: edit.targets } : {}),
    });
  }
  return { rows, pending: map };
}

/** What the editing props need to read and change: the part shown's held edits, and what outlasts the review. */
export interface EditingContext {
  read: HostRead;
  /** The part shown (the call-out and bin keys are by part, so they outlast a part switch). */
  part: number;
  /** The part's held edits, and how to replace them. */
  pending: PendingEdits;
  setPending(next: PendingEdits): void;
  /** `${part}:${dimension id}` to the key its call-out is held under (every part's). Mutated. */
  callOutKeys: Map<string, string>;
  /** The edit a bin replaced on an item, by `${part}:${id}`, for its Undo. Mutated. */
  displaced: Map<string, Pending>;
  /** The scene as drawn, for the number each call-out is written under. */
  drawn?: DrawnPmi | null;
  /** The ids the host's answers already use (an "Added by you" key must not repeat one). */
  answerIds?: Iterable<string>;
  /** A call-out reaches the host's answers too: ``key`` answered ``value``, or taken back (undefined). */
  answered?: (key: string, value: unknown) => void;
}

export type EditingProps = Pick<ReviewProps, 'calledOutDimensions' | 'calledOutNumbers' | 'takenKeys' | 'otherPmi' | 'stated' | 'callOutDimension'>;

/** The review's props for editing: the stated rows with their pencil and bin, Other PMI, and Call Out. */
export function editingProps(ctx: EditingContext): EditingProps {
  const { read, part, pending, setPending, callOutKeys, displaced } = ctx;
  return {
    // Called out, by characteristic: from the held edits, so it outlasts a part switch.
    calledOutDimensions: new Map([...callOutKeys].flatMap(([at, key]) => {
      const [p, ...rest] = at.split(':');
      const edit = pending.get(key);
      const tolerance = edit?.kind === 'fill' ? edit.text.replace(/ mm$/, '') : edit?.kind === 'change' ? String(edit.value) : null;
      return Number(p) === part && tolerance !== null ? [[rest.join(':'), tolerance.replace(/^±/, '')] as const] : [];
    })),
    // The number Save writes for each, called out: the engine's for its call-out's drawn callout.
    calledOutNumbers: new Map([...callOutKeys].flatMap(([at, key]) => {
      const [p, ...rest] = at.split(':');
      const number = Object.values(ctx.drawn?.links ?? {}).find((l) => l.question === key)?.number;
      return Number(p) === part && number ? [[rest.join(':'), number] as const] : [];
    })),
    // "Added by you" keys already taken (the call-outs among them): no two share one.
    takenKeys: [...pending.values().map((e) => e.id), ...callOutKeys.values()],
    otherPmi: {
      // Named as the file names it ("Flatness"), else by its kind.
      items: read.model.other.map((o) => {
        const item = read.table.file_pmi.find((r) => (o.number && r.number === o.number) || (o.id && r.id === o.id));
        const label = item?.type ? item.type.replace(/([a-z])([A-Z])/g, '$1 $2').replace(/_/g, ' ') : o.label;
        return { key: o.key, label: label.charAt(0).toUpperCase() + label.slice(1).toLowerCase(), value: labelStated(o.value, read.analysis.existing), faces: o.faces,
          // With no number and no id the engine has no way to remove it: no bin.
          removable: Boolean(o.number || o.id) };
      }),
      removed: new Set(pending.values().filter((e) => e.kind === 'remove').map((e) => e.id)),
      onRemove: (key: string) => {
        const o = read.model.other.find((x) => x.key === key);
        if (o) setPending(pending.with({ kind: 'remove', id: key, name: o.label, targets: removalTargets(o.number ? [o.number] : [], o.id), was: o.value }));
      },
      onRestore: (key: string) => setPending(pending.without(key)),
    },
    stated: {
      values: read.stated.values, texts: read.stated.texts, decided: read.stated.decided,
      removed: new Set(pending.values().filter((e) => e.kind === 'remove' || e.kind === 'clear').map((e) => e.id)),
      onRemove: (id: string) => {
        const row = read.table.rows.find((r) => r.id === id);
        if (!row) return;
        // The edit the bin replaces (a change, a value for a decision) comes back with Undo.
        const before = pending.get(id);
        if (before && before.kind !== 'remove' && before.kind !== 'clear') displaced.set(`${part}:${id}`, before);
        // A stored "none needed" taken back: the item is open again (clear); a stated one removed.
        setPending(pending.with(row.state === 'decided_none'
          ? { kind: 'clear', id, name: rowName(read, id) }
          : removeEdit(row, read.bare.find((q) => q.id === id), rowName(read, id), read.analysis)));
      },
      onRestore: (id: string) => {
        const before = displaced.get(`${part}:${id}`);
        displaced.delete(`${part}:${id}`);
        setPending(before ? pending.with(before) : pending.without(id));
      },
    },
    callOutDimension: (dimension: CalledOut, tolerance: string) => {
      // One key per dimension called out, kept while the part is shown (the same edit is replaced).
      const at = `${part}:${dimension.id}`;
      // One the file states: its feature-and-kind (the table's row stating that number) is the key.
      const row = dimension.number === undefined ? undefined
        : read.table.rows.find((r) => r.requirements?.some((q) => q.number === dimension.number));
      if (dimension.number !== undefined) {
        if (!row || typeof row.value !== 'string') return false;
        callOutKeys.set(at, row.id);
        const edit = callOutEdit(dimension, tolerance, row.id, { id: row.id, value: row.value, label: labelStated(row.value, read.analysis.existing) });
        setPending(edit ? pending.with(edit) : pending.without(row.id));
        return true;
      }
      const key = callOutKeys.get(at) ?? nextExtraKey([...read.table.rows.map((r) => r.id), ...pending.values().map((e) => e.id),
        ...(ctx.answerIds ?? []), ...callOutKeys.values()]);
      callOutKeys.set(at, key);
      const edit = callOutEdit(dimension, tolerance, key);
      setPending(edit ? pending.with(edit) : pending.without(key));
      ctx.answered?.(key, edit?.kind === 'fill' ? edit.value : undefined);
      return true;
    },
  };
}

/** The one edit list a save sends for a part: what its answers change in the file, then its held edits
 * (removals, call-outs) over them, a row's last word standing. ``bom``: the bom.* rows are written
 * only when the file's table has them (default true). */
export function composeEditList(read: Pick<HostRead, 'table' | 'bare' | 'model' | 'analysis' | 'stated'>, answers: PmiAnswers, pending: PendingEdits, bom = true): EditList {
  const hasBom = read.table.rows.some((r) => r.id.startsWith('bom.'));
  let edits = new PendingEdits();
  for (const [id, value] of Object.entries(asEngineTakes(answers))) {
    // Not an answer: absent, empty, or one of the general dimensions (Call Out answers those).
    if (id.startsWith('_') || isDimensionQuestion(id) || value === undefined || value === null || value === '' || (Array.isArray(value) && !value.length)) continue;
    const edit = buildAnswerEdit(read as HostRead, id, value);
    if (edit) edits = edits.with(edit);
  }
  for (const p of pending.values()) edits = edits.with(p);
  return editList(edits.values().filter((p) => p.kind !== 'refused' && !(bom && !hasBom && p.id.startsWith('bom.'))));
}
