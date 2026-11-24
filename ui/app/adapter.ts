// The adapter between the editor's review screens (SpecifyReview) and the 0.3 engine (ADR P11).
// The screens keep working in answers; the 0.3 engine saves edit lists (specify-core docs/open.md).
// Everything that translates one into the other is here, so the screens change only at the points
// Paul agreed (draftwright-project handoffs/specify-review-ui.md, change list items 1 and 2):
// - what the file states is shown as answered rows, each with a pencil and a bin (item 2), and kept
//   out of the engine's own review, which takes no answers to stated rows;
// - general dimensions are not asked one by one (item 1): their questions are not shown, and
//   Call Out on a dimension adds it with a tolerance of its own, as the editor's Call out does.
import { answerText, isNoneValue, type FeatureTable, type TableRow } from './table.ts';
import { removalTargets as targetsOf, type Pending } from './edits.ts';
import type { IntentReview, PmiAnalysis, PmiAnswers, PmiQuestion } from '../src/types/pmi.ts';
import { describeFaces } from '../src/specify/datums.ts';
import type { CalledOut } from '../src/specify/SpecifyReview.tsx';

/** A general dimension's question (one per dimension under the general tolerance): not shown as a row. */
export const isDimensionQuestion = (id: string) => id.startsWith('length:') || id.startsWith('location:');

/** What the review needs to know of the file's stated rows. */
export interface StatedRows {
  /** Stated row ids: kept out of the engine's review, which takes no answers to them. A stored
   * "none needed" is not among them: it stays in the answers, as the file stores it. */
  ids: ReadonlySet<string>;
  /** The file's value of each, as the review's answer to its question. */
  values: Readonly<PmiAnswers>;
  /** Each stated row's question, asked of the bare part (its label, feature title and choices). */
  questions: readonly PmiQuestion[];
  /** How each reads: what the file states, once ("⌖ ⌀2Ⓜ A B C"), without its basic dimensions or counts. */
  texts: Readonly<Record<string, string>>;
  /** Which of them are a stored "none needed". */
  decided: ReadonlySet<string>;
}

/** What a stated row reads as: its requirements' own text, less basic dimensions ("60 (basic)")
 * and bare counts ("(4)"), which the Characteristics list carries. */
function statedText(row: TableRow, q: PmiQuestion | undefined, analysis?: PmiAnalysis): string {
  if (row.state === 'decided_none') return 'none needed';
  // Faces by what they are ("bottom face"), never by their numbers.
  if (analysis && Array.isArray(row.value) && row.value.length && row.value.every((f) => typeof f === 'number')) {
    return describeFaces(row.value as number[], analysis);
  }
  const own = (row.requirements ?? []).map((r) => r.text).filter((t) => t && !/\(basic\)\s*$/.test(t) && !/^\(\d+\)$/.test(t.trim()));
  return own.length ? own.join(' · ') : answerText(row.value, q);
}

/** The stated rows of ``table``, with their questions from ``bare`` (the bare part's questions). */
export function statedRows(table: FeatureTable, bare: readonly PmiQuestion[], analysis?: PmiAnalysis): StatedRows {
  // What the file states, and what it has decided none needed (a stored decision is an item too:
  // its pencil gives it a value, its bin takes the decision back).
  const rows = table.rows.filter((r) => (r.state === 'stated' || r.state === 'decided_none') && !r.id.startsWith('bom.') && !isDimensionQuestion(r.id));
  const byId = new Map(bare.map((q) => [q.id, q]));
  const questions = rows.flatMap((r) => byId.get(r.id) ?? []);
  const asked = new Set(questions.map((q) => q.id));
  return {
    ids: new Set(rows.filter((r) => r.state === 'stated').map((r) => r.id)),
    values: Object.fromEntries(rows.filter((r) => asked.has(r.id)).map((r) => [r.id, r.value])),
    questions,
    texts: Object.fromEntries(rows.filter((r) => asked.has(r.id)).map((r) => [r.id, statedText(r, byId.get(r.id), analysis)])),
    decided: new Set(rows.filter((r) => r.state === 'decided_none' && asked.has(r.id)).map((r) => r.id)),
  };
}

/** ``answers`` in the shapes the 0.3 engine takes: the part's notes as a list of lines (the
 * editor's field gives one text). */
export function asEngineTakes(answers: PmiAnswers): PmiAnswers {
  const notes = answers['part.notes'];
  if (typeof notes !== 'string') return answers;
  return { ...answers, 'part.notes': notes.split(/\r?\n/).map((line) => line.trim()).filter(Boolean) };
}

/** ``answers`` as the engine's review takes them: without the stated rows' values. */
export function forEngine(answers: PmiAnswers, stated: StatedRows): PmiAnswers {
  return Object.fromEntries(Object.entries(answers).filter(([id]) => !stated.ids.has(id)));
}

/** The engine's ``review`` as the screens show it: no general-dimension questions; the stated rows'
 * questions in their place among the rest. */
export function forScreens(review: IntentReview, stated: StatedRows): IntentReview {
  const own = new Set(stated.questions.map((q) => q.id));
  const shown = review.questions.filter((q) => !isDimensionQuestion(q.id) && !own.has(q.id));
  return {
    ...review,
    questions: [...shown, ...stated.questions],
    open: review.open.filter((id) => !isDimensionQuestion(id) && !own.has(id)),
  };
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/**
 * The pending edit an answer makes (null: none, the row is as the file has it). ``row`` is the
 * table's row for the question, ``q`` its question; ``value`` undefined is an answer taken back.
 * - a gap answered: Add (a "none" answer: None needed);
 * - a decided none answered: Add, after taking the decision back (clear + fill, one bar entry);
 * - a stated row answered with another value: Change, against the file's value.
 */
export function answerEdit(id: string, value: unknown, row: TableRow | undefined, q: PmiQuestion | undefined, name: string): Pending | null {
  if (value === undefined) return null;
  const text = answerText(value, q);
  if (row?.state === 'stated') {
    if (same(value, row.value)) return null;
    return { kind: 'change', id, name, value, text, expect: row.value, was: answerText(row.value, q) };
  }
  if (isNoneValue(value) || value === 'none') return row?.state === 'decided_none' ? null : { kind: 'none', id, name };
  return { kind: 'fill', id, name, value, text, ...(row?.state === 'decided_none' ? { clear: true as const } : {}) };
}

/** Remove a stated row: its requirements by characteristic number, else by its id. */
export function removeEdit(row: TableRow, q: PmiQuestion | undefined, name: string, analysis?: PmiAnalysis): Pending {
  const numbers = (row.requirements ?? []).flatMap((r) => (r.number ? [r.number] : []));
  return { kind: 'remove', id: row.id, name, targets: targetsOf(numbers, row.id), was: statedText(row, q, analysis) };
}

/** A value as the bar says it: faces by what they are ("bottom face"), never by their numbers. */
export function valueText(value: unknown, q: PmiQuestion | undefined, analysis?: PmiAnalysis): string {
  if (analysis && Array.isArray(value) && value.length && value.every((f) => typeof f === 'number')) return describeFaces(value as number[], analysis);
  return answerText(value, q);
}

/**
 * Call Out on a general dimension (change list item 1): the dimension added with a tolerance of
 * its own, as the editor's Call out has always written it: an "Added by you" size or distance on
 * the dimension's faces (and, for a distance, its reference), under ``key`` (extra.N). Null when
 * the tolerance is empty: the call-out taken back.
 */
export function callOutEdit(d: CalledOut, tolerance: string, key: string): Pending | null {
  const t = tolerance.trim().replace(/^±\s*/, '');
  if (!t) return null;
  const value = d.kind === 'size'
    ? { faces: [...d.faces], kind: 'size', value: `±${t}` }
    : { faces: [...d.faces], kind: 'distance', value: `±${t}`, reference: [...d.reference] };
  return { kind: 'fill', id: key, name: `Call out ${d.what}`, value, text: `±${t}` };
}

/** The next free "Added by you" key (extra.N) among ``ids``. */
export function nextExtraKey(ids: Iterable<string>): string {
  let n = 0;
  for (const id of ids) { const m = /^extra\.(\d+)$/.exec(id); if (m) n = Math.max(n, Number(m[1])); }
  return `extra.${n + 1}`;
}
