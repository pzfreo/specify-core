import type { IntentReview, PmiAnalysis, PmiAnswers, PmiQuestion } from '../types/pmi.ts';
import type { WorkerClient } from '../engine/rpc.ts';
import { answerText, capital, changedText, displayText, isNoneValue, kindWord, tableOf, type TableRequirement, type TableRow } from './feature-table.ts';
import { PendingEdits, removalTargets, type EditFailure, type Pending, type SaveEdits } from './edit-list.ts';

export interface ReviewFile {
  name: string;
  bytes: Uint8Array;
  sourceId: string;
  part?: number;
}

export interface ReviewPart {
  part: number;
  name: string;
  instances: number;
  same_as?: number | null;
  /** For each face on this definition, the corresponding face on the first definition. */
  faces_of_first?: readonly number[] | null;
}

export type ReviewSaveResult =
  | { bytes: Uint8Array | null; name?: string; report?: unknown }
  | { failures: readonly EditFailure[] };

/** Callbacks can run back-to-back before React renders. Hosts should read their own live state through refs. */
export interface FileReviewOptions {
  file: ReviewFile;
  worker: Pick<WorkerClient, 'request'>;
  questionLabels?: readonly PmiQuestion[];
  parts?: readonly ReviewPart[];
  /** Edits or refusals supplied by the host, by part. Local edits to the same row take precedence. */
  staged?: ReadonlyMap<number, PendingEdits>;
  stagedReplacement?: string;
  /** The review's own pending lists after a change, without host-supplied drafts. */
  onPending?: (pending: ReadonlyMap<number, PendingEdits>) => void;
  /** Optional host writer. Null keeps the edits staged. A result with failures leaves the file unchanged. */
  save?: (edits: SaveEdits) => Promise<ReviewSaveResult | null>;
  onBusy?: (busy: boolean) => void;
  onSaved?: (bytes: Uint8Array) => void;
  onDiscard?: (target?: { part: number; id: string }) => Promise<boolean>;
  bar?: HTMLElement | null;
  saveBlocked?: string;
  inspectionName?: string;
  inspectionBlocked?: string;
}

export type FileRow = TableRow & { question: PmiQuestion; decision?: boolean; bom?: boolean; other?: TableRequirement };
export const datumId = (id: string) => /^datum\.[A-H]$/.test(id);

export function otherRows(analysis: PmiAnalysis): FileRow[] {
  return tableOf(analysis).file_pmi.map((item, i) => {
    const id = item.number ?? item.id ?? `item:${i}`;
    const label = capital(item.kind.replace(/_/g, ' '));
    const question: PmiQuestion = { id, label, prompt: label, kind: 'text', faces: item.faces, default: null, attention: 'routine', why: '' };
    return { id, state: 'stated', value: item.text, kind: item.kind, feature: 'other', faces: item.faces,
      requirements: [item], open: false, question, other: item };
  });
}

export interface ReviewFocus {
  questionId?: string | null;
  number?: number;
  feature?: string;
  requirement?: string;
  faces?: readonly number[];
  scroll?: boolean;
  focusInput?: boolean;
}

/** An older diff names a question. Current reports identify the stored PMI. */
export function focusedRequirement(analysis: PmiAnalysis, request?: ReviewFocus | null): string | undefined {
  if (!request) return undefined;
  if (request.questionId) return request.questionId;
  const table = tableOf(analysis);
  const items = [...table.rows.filter(row => row.state === 'stated'), ...otherRows(analysis)];
  if (request.number !== undefined) {
    const number = `#${request.number}`;
    const match = items.find(row => row.requirements?.some(item => item.number === number));
    if (match) return match.id;
  }
  const faceIds = analysis.features.find(feature => feature.id === request.feature)?.faces ?? [];
  const candidates = items.filter(row => !request.feature || row.feature === request.feature ||
    (row.feature === 'other' && row.faces.some(face => faceIds.includes(face))));
  const text = request.requirement;
  const matches = text ? candidates.filter(row => row.requirements?.some(item =>
    item.text === text || displayText(item.text) === displayText(text))) : [];
  return matches.length === 1 ? matches[0]?.id : candidates.length === 1 ? candidates[0]?.id : undefined;
}

export function requirementName(q: PmiQuestion): string {
  if (q.id.startsWith('length:planes:')) return 'Length';
  if (q.id.startsWith('location:holes:')) return 'Location';
  const name = q.label ?? q.title ?? q.prompt;
  return q.feature && name === q.title ? `${name} ${kindWord(q.id.split(':')[0]!).toLowerCase()}` : name;
}


export const rowText = (row: TableRow): string => displayText(
  answerText(row.value) || row.requirements?.map(r => r.text).join('; ') || '',
);

/** Holes located by is a stored choice outside the feature table. */
export function storedChoiceRow(analysis: PmiAnalysis, id: string): TableRow | undefined {
  if (id !== 'part.locating') return;
  const value = analysis.resume?.answers[id];
  if (value === undefined) return;
  return { id, kind: id, feature: 'part', faces: [], open: false, state: isNoneValue(value) ? 'decided_none' : 'stated', value };
}

/** A host can omit the file's decisions from its own answer map. */
export function withStoredAnswers(analysis: PmiAnalysis, given: PmiAnswers): PmiAnswers {
  return { ...analysis.resume?.answers, ...given };
}

/** Stored decisions and edits become given answers. Existing PMI is read by the engine from the file. */
export function impliedAnswers(analysis: PmiAnalysis, pending: PendingEdits): PmiAnswers {
  const answers = { ...analysis.resume?.answers };
  const rows = new Map(tableOf(analysis).rows.map(row => [row.id, row]));
  for (const edit of pending.values()) {
    switch (edit.kind) {
      case 'accept':
      case 'fill':
      case 'replace':
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

export function replacedRequirements(pending: PendingEdits): string[] {
  return pending.values().filter(edit => edit.kind === 'remove' || edit.kind === 'change' || (edit.kind === 'none' && edit.targets?.length)).map(edit => edit.id);
}

export function askFile(options: FileReviewOptions, answers: PmiAnswers, pending: PendingEdits, signal?: AbortSignal): Promise<IntentReview> {
  return options.worker.request('review-edits', { ...options.file, answers, without: replacedRequirements(pending) }, signal);
}

/** Review determines which gaps apply. The table remains authoritative for saved PMI and decisions. */
export function fileRows(
  analysis: PmiAnalysis, questions: readonly PmiQuestion[], labels: readonly PmiQuestion[],
): FileRow[] {
  const table = new Map(tableOf(analysis).rows.map(row => [row.id, row]));
  const locating = storedChoiceRow(analysis, 'part.locating');
  if (locating && !table.has(locating.id)) table.set(locating.id, locating);
  const asked = new Map(questions.map(q => [q.id, q]));
  const named = new Map(labels.map(q => [q.id, q]));
  const rows: FileRow[] = [];
  for (const id of new Set([...asked.keys(), ...table.keys()])) {
    const q = asked.get(id);
    const row = table.get(id);
    if (!q && (!row || (row.state === 'gap' && !id.startsWith('bom.')))) continue;
    const faces = row?.faces ?? q?.faces ?? [];
    const [kind, ...kindParts] = id.split(':');
    const owner = id.startsWith('location:holes:') && faces.length
      ? analysis.features.filter(f => (f.family === 'holes' || f.family.startsWith('hole_patterns')) && faces.every(face => f.faces.includes(face)))
        .sort((a, b) => a.faces.length - b.faces.length)[0] : undefined;
    const feature = owner?.id ?? row?.feature ?? q?.feature ?? (kindParts.join(':') || id);
    const fallback = kindWord(kind);
    const described: PmiQuestion = q ?? named.get(id) ?? {
      id, kind: datumId(id) ? 'faces' : 'text', feature: feature || id, label: fallback, prompt: fallback,
      faces: row?.faces ?? [], attention: 'routine', default: null, why: '',
      title: analysis.features.find(f => f.id === feature)?.family.replace(/_/g, ' '),
    };
    const decision = id === 'part.locating';
    const bom = id.startsWith('bom.');
    // BOM questions carry proposal strings in suggestions instead of ordinary defaults.
    const question = row?.state === 'stated' ? {
      ...described, faces: row.faces, ...(datumId(id) ? { default: row.faces, kind: 'faces' as const } : {}),
    } : bom ? { ...described, default: described.default ?? described.suggestions?.find(value => typeof value === 'string') } : described;
    rows.push({
      id, kind, faces: question.faces.slice(), open: false, state: 'gap',
      ...row, feature, question: owner ? { ...question, feature, title: labels.find(q => q.feature === feature)?.title ?? question.title } : question, decision, bom,
    });
  }
  return rows;
}

export function requirementLabel(q: PmiQuestion): string {
  const name = requirementName(q);
  if (!q.title || !name.startsWith(q.title)) return name;
  const short = name.slice(q.title.length).trim();
  return short ? capital(short) : kindWord(q.id.split(':')[0]!);
}

export function rowSuggestion(row: FileRow): unknown {
  return row.question.default ?? row.suggestion ?? undefined;
}

export function rowSuggestionText(row: FileRow): string | undefined {
  const suggestion = rowSuggestion(row);
  return suggestion === undefined ? undefined : isNoneValue(suggestion) ? 'none' : answerText(suggestion, row.question);
}

/** Build from the file's row, even when a pending clear makes it appear to be a gap. */
export function valueEdit(row: TableRow | undefined, id: string, name: string, value: unknown, question?: PmiQuestion): Pending | undefined {
  const text = answerText(value, question);
  if (row && row.state !== 'gap') {
    if (JSON.stringify(value) === JSON.stringify(row.value) ||
      (row.state === 'decided_none' && isNoneValue(row.value) && isNoneValue(value))) return;
    const was = row.state === 'decided_none' && isNoneValue(row.value) ? 'none needed' : rowText(row);
    if (row.state === 'decided_none' || id === 'part.locating') return { kind: 'replace', id, name, value, text, was };
    return { kind: 'change', id, name, value, text: changedText(was, value, question), expect: row.value, was };
  }
  if (value !== undefined && isNoneValue(value) && !id.startsWith('bom.')) return { kind: 'none', id, name };
  return { kind: 'fill', id, name, value, text };
}

/** Decide None from this part's saved row, removing a requirement only when it states PMI. */
export function noneEdit(row: TableRow | undefined, id: string, name: string): Pending | undefined {
  if (row?.state === 'decided_none' || id === 'part.locating') return valueEdit(row, id, name, 'none');
  const targets = row?.state === 'stated'
    ? removalTargets(row.requirements?.flatMap(item => item.number ? [item.number] : []) ?? [], id) : undefined;
  return { kind: 'none', id, name, targets };
}

export function queueValue(row: FileRow | undefined, question: PmiQuestion, pending: PendingEdits, value: unknown): PendingEdits {
  const edit = valueEdit(row, question.id, requirementName(question), value, question);
  if (edit) return pending.with(edit);
  return pending.get(question.id) ? pending.without(question.id) : pending;
}

export class CheckRefused extends Error {
  readonly failures: readonly EditFailure[];
  constructor(failures: readonly EditFailure[]) {
    super(failures.map(f => f.reason).join('\n'));
    this.failures = failures;
  }
}

export async function checkPending(options: FileReviewOptions, pending: PendingEdits): Promise<void> {
  const result = await options.worker.request<{ applicable: boolean; message?: string; failures?: EditFailure[] }>(
    'check-edits', { ...options.file, edits: pending.list() },
  );
  if (!result.applicable) throw new CheckRefused(result.failures?.length ? result.failures :
    [{ entry: '', reason: result.message || 'The edits cannot be applied.' }]);
}

export function partFile(file: ReviewFile, part: number): ReviewFile {
  return file.part === part ? file : { ...file, part, sourceId: `${file.sourceId}:part:${part}` };
}

export async function savePending(options: () => FileReviewOptions, edits: SaveEdits): Promise<ReviewSaveResult | null> {
  const host = options();
  if (host.save) return host.save(edits);
  const lists = Array.isArray(edits) ? edits : [{ part: host.file.part, edits }];
  let bytes = host.file.bytes;
  const reports: unknown[] = [];
  for (const [i, { part, edits }] of lists.entries()) {
    const file = { ...host.file, part, bytes, sourceId: `${host.file.sourceId}:write:${i}` };
    const result = await options().worker.request<
      { written: true; bytes: Uint8Array; report: unknown } | { written: false; message: string; failures: EditFailure[] }
    >('edit', { ...file, edits });
    if (!result.written) return { failures: result.failures.map(failure => ({ ...failure, part })) };
    bytes = result.bytes;
    reports.push(part === undefined ? result.report : { part, report: result.report });
  }
  return { bytes, report: reports };
}

export function retiredNumbers(report: unknown): string[] {
  if (Array.isArray(report)) return report.flatMap(retiredNumbers);
  if (!report || typeof report !== 'object') return [];
  const fields = report as { retired?: unknown; parts?: unknown; report?: unknown; part?: unknown; name?: unknown };
  const name = typeof fields.name === 'string' ? fields.name : typeof fields.part === 'number' ? `Part ${fields.part}` : '';
  const numbers = Array.isArray(fields.retired) ? fields.retired.map(value => `#${value}`) : retiredNumbers(fields.report);
  return [...numbers.map(number => name ? `${name} ${number}` : number), ...retiredNumbers(fields.parts)];
}
