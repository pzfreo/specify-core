// The review panel's reading of the engine's feature table (specify-pmi docs/answers.md, "Reopening
// a file"; ADR P11): `analysis.table`, `{rows, file_pmi, decided_none}`. Everything the panel knows of
// the payload's shape is here, so a change to the payload is a change to this file only.
//
// Each row is a feature-and-kind: `stated` in the file (its value and requirements, with their
// characteristic numbers), a `gap` (Specify's suggestion, and whether it is open), or `decided_none`.
// `file_pmi` is the file's PMI on no recognised feature-and-kind ("Other PMI in the file").
import { codeOf } from '../src/specify/choose.ts';
import { CHARACTERISTIC } from '../src/specify/labels.ts';
import type { PmiAnalysis, PmiFeature, PmiQuestion } from '../src/types/pmi.ts';

export type RowState = 'stated' | 'gap' | 'decided_none';

/** One requirement in the file: a PMI item or a part setting. */
export interface TableRequirement { number?: string; id?: string; kind: string; type: string; faces: number[]; text: string }

/** One feature-and-kind, as the engine gives it. */
export interface TableRow {
  id: string; kind: string; feature: string; faces: number[]; state: RowState;
  value?: unknown; requirements?: TableRequirement[]; suggestion?: unknown; open: boolean;
}

export interface FeatureTable { rows: TableRow[]; file_pmi: TableRequirement[]; decided_none: string[] }

/** The table in an analysis (an empty one when the engine gave none). */
export function tableOf(analysis: PmiAnalysis): FeatureTable {
  const table = (analysis as { table?: Partial<FeatureTable> }).table;
  return { rows: table?.rows ?? [], file_pmi: table?.file_pmi ?? [], decided_none: table?.decided_none ?? [] };
}

/** The analysis with the file's PMI set aside: what Specify asks of the bare part, so every row has
 * its question (its label, its feature's title and its choices), stated or not. The table's rows are
 * those questions (feature_table.rs). */
export function bareAnalysis(analysis: PmiAnalysis): PmiAnalysis {
  const bare: Record<string, unknown> = { ...analysis, existing: [], part: {}, characteristics: [] };
  delete bare.table;
  delete bare.resume;
  return bare as unknown as PmiAnalysis;
}

/** A row as the panel shows it. */
export interface PanelRow {
  notes?: readonly string[];
  id: string;
  /** Its short name under its feature ("Position", "Material"). */
  label: string;
  /** Its full name, for the bar ("Ø1.6 hole position"). */
  name: string;
  state: RowState;
  /** Stated: the file's value as written; decided none: the decision stored. */
  value: string;
  /** Stated: the value to compare a change with (`expect`): the file's value. */
  expect: unknown;
  /** Gap: Specify's suggestion as an answer (`undefined`: none), and as it reads. */
  suggestion?: unknown;
  suggestionText?: string;
  open: boolean;
  /** The characteristic numbers of the requirements that state it ("#4"). */
  numbers: string[];
  faces: number[];
  /** Its question on the bare part, when Specify asks one: the kind of answer and the choices. */
  question?: PmiQuestion;
}

export interface PanelGroup { key: string; title: string; rows: PanelRow[] }

/** A requirement in "Other PMI in the file": remove only. */
/** ``id``: the item's id in the engine's table (`file_pmi:#<STEP entity>`), for a removal without a
 * number; absent for an item the engine cannot name, which has no removal without a number. */
export interface OtherRow { key: string; id?: string; number?: string; label: string; value: string; faces: number[] }

export interface PanelModel { part: PanelGroup | null; features: PanelGroup[]; datums: PanelGroup | null; other: OtherRow[] }

/** Whether ``value`` states nothing: absent, no faces, no notes, or a "None …" choice. */
export const isNoneValue = (value: unknown): boolean => value === null || value === undefined || value === ''
  || (Array.isArray(value) ? value.length === 0 : typeof value === 'string' && /^none\b/i.test(value.trim()));

/** An answer as the panel writes it in a line: a choice by its code, faces by number, a list joined. */
export function answerText(value: unknown, q?: PmiQuestion): string {
  if (value === null || value === undefined) return '';
  if (Array.isArray(value)) {
    if (!value.length) return 'none';
    if (value.every((v) => typeof v === 'number')) return `${value.length === 1 ? 'face' : 'faces'} ${value.join(', ')}`;
    return value.map((v) => answerText(v, q)).join('; ');
  }
  if (typeof value === 'string') {
    if (/^none\b/i.test(value.trim())) return value.trim() === 'none' ? 'none' : value;
    return q ? codeOf(q, value) || value : value;
  }
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

/** The file's length unit, as its STEP declares it (an inch file names `CONVERSION_BASED_UNIT('INCH', …)`
 * for its lengths); millimetres otherwise. Read from the bytes until the analysis carries the unit the
 * engine reads (`declared_length_unit`). */
export function fileLengthUnit(bytes: Uint8Array): 'in' | 'mm' {
  const text = new TextDecoder('latin1').decode(bytes);
  return /CONVERSION_BASED_UNIT\s*\(\s*'INCH(ES)?'/i.test(text) ? 'in' : 'mm';
}

/** A requirement's text as a drawing reads it: "Position 0.2 | A" → "⌖ 0.2 A", "Size_Diameter 3.3" →
 * "Ø3.3", "Location_LinearDistance 37.6" → "37.6"; several joined by "; ". Anything else as it is. */
export function displayText(text: string): string {
  return text.split('; ').map((piece) => {
    const [head = '', ...rest] = piece.trim().split(' ');
    const tail = rest.join(' ');
    const symbol = CHARACTERISTIC[head];
    if (symbol) return [symbol, ...tail.split(' | ').map((s) => s.trim()).filter(Boolean)].join(' ');
    if (/^Size_Diameter$/.test(head)) return `Ø${tail}`;
    if (/^Size_Radius$/.test(head)) return `R${tail}`;
    if (/^[A-Z][a-z]+_[A-Za-z]+$/.test(head)) return tail || readable(head);
    return piece;
  }).join('; ');
}

/** An engine type name as words: ``Location_LinearDistance`` → "Linear distance". */
const readable = (name: string) => capital((name.split('_').pop() ?? name).replace(/([a-z])([A-Z])/g, '$1 $2').toLowerCase());

/** A changed value as the row will read: a tolerance's new magnitude in its frame, its zone and
 * modifiers kept as the engine keeps them ("⌖ ⌀0.3Ⓜ A" with "0.2" → "⌖ ⌀0.2Ⓜ A"); otherwise the answer
 * as it reads. */
export function changedText(was: string, value: unknown, q?: PmiQuestion): string {
  const m = /^(\S+) (⌀?)[\d.]+(.*)$/.exec(was);
  if (m && typeof value === 'string' && /^[\d.]+$/.test(value) && Object.values(CHARACTERISTIC).includes(m[1]!)) return `${m[1]} ${m[2]}${value}${m[3]}`;
  return answerText(value, q);
}

const capital = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);
/** The kind's own word: ``hole.position`` → "Position", ``part.general_tolerance`` → "General tolerance". */
const kindWord = (kind: string) => capital((kind.split('.').pop() ?? kind).replace(/_/g, ' '));

/** Characteristic numbers as a balloon reads: ``[1, 2, 4]`` → "1–2, 4". */
export function balloonText(numbers: readonly string[]): string {
  const ns = [...new Set(numbers.map((n) => Number(n.replace(/^#/, ''))).filter(Number.isFinite))].sort((a, b) => a - b);
  const runs: string[] = [];
  for (let i = 0; i < ns.length;) {
    let j = i;
    while (j + 1 < ns.length && ns[j + 1] === ns[j]! + 1) j++;
    runs.push(j > i ? `${ns[i]}–${ns[j]}` : String(ns[i]));
    i = j + 1;
  }
  return runs.join(', ');
}

/** The panel's model of ``table``, named with ``questions`` (the bare part's: see bareAnalysis). */
export function panelModel(table: FeatureTable, questions: readonly PmiQuestion[], features: readonly PmiFeature[] = []): PanelModel {
  const byId = new Map(questions.map((q) => [q.id, q]));
  const titles = new Map<string, string>();
  for (const row of table.rows) {
    const title = byId.get(row.id)?.title;
    if (title) titles.set(row.feature, title);
  }
  const holes = features.filter(f => f.family === 'holes' || f.family.startsWith('hole_patterns'))
    .sort((a, b) => a.faces.length - b.faces.length);
  const groups = new Map<string, PanelGroup>();
  const group = (key: string, title: string) => {
    let g = groups.get(key);
    if (!g) { g = { key, title, rows: [] }; groups.set(key, g); }
    return g;
  };
  for (const row of table.rows) {
    if (row.id.startsWith('bom.')) continue;
    const q = byId.get(row.id);
    const title = q?.title ?? '';
    // A row's full name names its feature and its kind ("Ø1.6 hole function"), never the feature alone.
    const label = q?.label ?? kindWord(row.kind);
    const length = row.id.startsWith('length:planes:');
    const holeLocation = row.id.startsWith('location:holes:');
    const owner = holeLocation ? holes.find(f => row.faces.length && row.faces.every(face => f.faces.includes(face))) : undefined;
    const locationTitle = owner
      ? titles.get(owner.id) ?? owner.id
      : `Hole location on ${row.faces.length === 1 ? 'face' : 'faces'} ${row.faces.join(', ')}`;
    const name = length ? 'Length' : holeLocation ? (owner ? `${locationTitle} location` : locationTitle)
      : title && label === title ? `${title} ${kindWord(row.kind).toLowerCase()}` : label;
    const short = title && name.startsWith(title) ? name.slice(title.length).trim() : name;
    const numbers = (row.requirements ?? []).flatMap((r) => (r.number ? [r.number] : []));
    const value = row.state === 'stated' ? displayText(answerText(row.value) || (row.requirements ?? []).map((r) => r.text).join('; '))
      : row.state === 'decided_none' ? 'none needed' : 'not stated';
    // A decided none taken back (Undo) is a gap again: its question's default is what it would suggest.
    const offered = row.state === 'gap' ? row.suggestion : row.state === 'decided_none' ? q?.default : undefined;
    const suggestion = offered !== null && offered !== undefined ? offered : undefined;
    const panelRow: PanelRow = {
      id: row.id, label: length ? 'Length' : holeLocation ? 'Location' : short ? capital(short) : kindWord(row.kind), name, state: row.state, value, expect: row.value,
      open: row.open, numbers, faces: row.faces, ...(q ? { question: q } : {}),
      ...(row.id === 'part.notes' && Array.isArray(row.value) && row.value.every((note) => typeof note === 'string')
        ? { notes: row.value } : {}),
      ...(suggestion !== undefined ? { suggestion, suggestionText: isNoneValue(suggestion) ? 'none' : answerText(suggestion, q) } : {}),
    };
    if (length || row.feature === 'part') group('part', 'The part').rows.push(panelRow);
    else if (holeLocation) group(`feature:${owner?.id ?? row.feature}`, locationTitle).rows.push(panelRow);
    else if (row.id.startsWith('datum.')) group('datums', 'Datums').rows.push(panelRow);
    else group(`feature:${row.feature}`, title || row.feature).rows.push(panelRow);
  }
  const other = table.file_pmi.map((r, i) => ({
    key: r.number ?? r.id ?? `item:${i}`, ...(r.id ? { id: r.id } : {}), ...(r.number ? { number: r.number } : {}), label: capital(r.kind.replace(/_/g, ' ')), value: displayText(r.text), faces: r.faces,
  }));
  return {
    part: groups.get('part') ?? null,
    features: [...groups.values()].filter((g) => g.key.startsWith('feature:')),
    datums: groups.get('datums') ?? null,
    other,
  };
}
