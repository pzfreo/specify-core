import type { PmiFeature, PmiQuestion } from '../src/types/pmi.ts';
import { answerText, capital, displayText, isNoneValue, kindWord, type FeatureTable, type RowState } from '../src/specify/feature-table.ts';
export { answerText, bareAnalysis, balloonText, changedText, displayText, isNoneValue, tableOf } from '../src/specify/feature-table.ts';
export type { FeatureTable, RowState, TableRequirement, TableRow } from '../src/specify/feature-table.ts';

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

/** The file's length unit, as its STEP declares it (an inch file names `CONVERSION_BASED_UNIT('INCH', …)`
 * for its lengths); millimetres otherwise. Read from the bytes until the analysis carries the unit the
 * engine reads (`declared_length_unit`). */
export function fileLengthUnit(bytes: Uint8Array): 'in' | 'mm' {
  const text = new TextDecoder('latin1').decode(bytes);
  return /CONVERSION_BASED_UNIT\s*\(\s*'INCH(ES)?'/i.test(text) ? 'in' : 'mm';
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
