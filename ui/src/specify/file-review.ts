import type { IntentReview, PmiAnalysis, PmiAnswers, PmiQuestion } from '../types/pmi.ts';
import type { WorkerClient } from '../engine/rpc.ts';
import { answerText, changedText, displayText, isNoneValue, kindWord, tableOf, type TableRow } from './feature-table.ts';
import { PendingEdits, type EditFailure, type EditList } from './edit-list.ts';

export interface ReviewFile {
  name: string;
  bytes: Uint8Array;
  sourceId: string;
  part?: number;
}

export interface FileReviewOptions {
  file: ReviewFile;
  worker: Pick<WorkerClient, 'request'>;
  /** Labels for stated requirements whose question the engine no longer asks. */
  questionLabels?: readonly PmiQuestion[];
  /** Optional host-owned pending list. Refusals can be supplied as entries in this list. */
  pending?: PendingEdits;
  onPending?: (pending: PendingEdits) => void;
  /** Optional host writer. A null result leaves the edits staged. */
  save?: (edits: EditList) => Promise<Uint8Array | null>;
  onBusy?: (busy: boolean) => void;
  onSaved?: (bytes: Uint8Array) => void;
  /** Used by a host which puts Save and Discard in its own workspace. */
  renderBar?: (save: () => void) => React.ReactNode;
  onUndo?: (id: string) => void;
  /** Rows still rendered by a host during migration. */
  hostRows?: readonly string[];
  /** The name on the stored file used for the inspection downloads. */
  inspectionName?: string;
  inspectionBlocked?: string;
  failures?: ReadonlyMap<string, string>;
}

export type FileRow = TableRow & { question: PmiQuestion; decision?: boolean; bom?: boolean };
export const datumId = (id: string) => /^datum\.[A-H]$/.test(id);

export const rowText = (row: FileRow): string => displayText(
  answerText(row.value) || row.requirements?.map(r => r.text).join('; ') || '',
);

const storedAnswers = (analysis: PmiAnalysis): PmiAnswers => analysis.resume?.answers ?? {};

/** Stored decisions and edits become given answers. Existing PMI is read by the engine from the file. */
export function impliedAnswers(analysis: PmiAnalysis, pending: PendingEdits): PmiAnswers {
  const answers = { ...storedAnswers(analysis) };
  const rows = new Map(tableOf(analysis).rows.map(row => [row.id, row]));
  for (const edit of pending.values()) {
    switch (edit.kind) {
      case 'accept':
      case 'fill':
      case 'change': answers[edit.id] = edit.value; break;
      case 'remove':
      case 'clear': delete answers[edit.id]; break;
      case 'none': {
        const row = rows.get(edit.id);
        const value = row?.state === 'decided_none' ? row.value : row?.suggestion;
        answers[edit.id] = value != null && isNoneValue(value) ? value : datumId(edit.id) ? [] : 'none';
        break;
      }
      case 'refused': break;
    }
  }
  return answers;
}

export function askFile(options: FileReviewOptions, answers: PmiAnswers, pending: PendingEdits): Promise<IntentReview> {
  const without = pending.values().filter(edit => edit.kind === 'remove' || (edit.kind === 'change' && datumId(edit.id)))
    .map(edit => edit.id);
  return options.worker.request('review-edits', { ...options.file, answers, without });
}

/** Review determines which gaps apply. The table remains authoritative for saved PMI and decisions. */
export function fileRows(
  analysis: PmiAnalysis, questions: readonly PmiQuestion[], labels: readonly PmiQuestion[], hostRows: readonly string[],
): FileRow[] {
  const stored = storedAnswers(analysis);
  const table = new Map(tableOf(analysis).rows.map(row => [row.id, row]));
  const asked = new Map(questions.map(q => [q.id, q]));
  const named = new Map(labels.map(q => [q.id, q]));
  const rows: FileRow[] = [];
  for (const id of new Set([...asked.keys(), ...table.keys()])) {
    if (hostRows.includes(id)) continue;
    const q = asked.get(id);
    const row = table.get(id);
    if (!q && (!row || row.state === 'gap')) continue;
    const [kind, ...kindParts] = id.split(':');
    const feature = row?.feature ?? q?.feature ?? (kindParts.join(':') || id);
    const fallback = kindWord(kind);
    const described: PmiQuestion = q ?? named.get(id) ?? {
      id, kind: datumId(id) ? 'faces' : 'text', feature: feature || id, label: fallback, prompt: fallback,
      faces: row?.faces ?? [], attention: 'routine', default: null, why: '',
      title: analysis.features.find(f => f.id === feature)?.family.replace(/_/g, ' '),
    };
    const question = row?.state === 'stated' ? {
      ...described, faces: row.faces, ...(datumId(id) ? { default: row.faces, kind: 'faces' as const } : {}),
    } : described;
    const decision = id === 'part.locating';
    const bom = id.startsWith('bom.');
    const storedDecision = decision ? stored[id] : undefined;
    rows.push({
      id, kind, feature, faces: question.faces.slice(), open: false, state: 'gap',
      ...(storedDecision === undefined ? {} : { state: isNoneValue(storedDecision) ? 'decided_none' : 'stated', value: storedDecision }),
      ...row, question, decision, bom,
    });
  }
  return rows;
}

export function queueValue(row: FileRow | undefined, question: PmiQuestion, pending: PendingEdits, value: unknown, suggested: boolean): PendingEdits {
  const { id } = question;
  const name = question.label ?? question.title ?? question.prompt;
  const text = answerText(value, question);
  if (row?.state === 'stated' && !row.decision && !row.bom) {
    const was = rowText(row);
    return pending.with({ kind: 'change', id, name, value, text: changedText(was, value, question), expect: row.value, was });
  }
  if (isNoneValue(value) && !row?.bom) return pending.with({ kind: 'none', id, name });
  // The writer resolves accept_suggestions on the stored file. A new suggestion due to
  // earlier pending answers must instead be pinned to exactly what the person saw.
  const accept = suggested && row && JSON.stringify(row.suggestion) === JSON.stringify(value);
  return pending.with({ kind: accept ? 'accept' : 'fill', id, name, value, text });
}

export async function checkPending(options: FileReviewOptions, pending: PendingEdits): Promise<void> {
  const result = await options.worker.request<{ applicable: boolean; message?: string; failures?: EditFailure[] }>(
    'check-edits', { ...options.file, edits: pending.list() },
  );
  if (!result.applicable) throw new Error(result.failures?.map(f => f.reason).join('\n') || result.message || 'The edits cannot be applied.');
}

export async function savePending(options: FileReviewOptions, pending: PendingEdits): Promise<Uint8Array | null> {
  const edits = pending.list();
  if (options.save) return options.save(edits);
  if (!Object.keys(edits).length) return null;
  const result = await options.worker.request<
    { written: true; bytes: Uint8Array } | { written: false; message: string }
  >('edit', { ...options.file, edits });
  if (!result.written) throw new Error(result.message);
  return result.bytes;
}
