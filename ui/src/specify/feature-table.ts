// The review panel's reading of the engine's feature table (specify-core docs/answers.md, "Reopening
// a file"; ADR P11): `analysis.table`, `{rows, file_pmi, decided_none}`. Everything the panel knows of
// the payload's shape is here, so a change to the payload is a change to this file only.
//
// Each row is a feature-and-kind: `stated` in the file (its value and requirements, with their
// characteristic numbers), a `gap` (Specify's suggestion, and whether it is open), or `decided_none`.
// `file_pmi` is the file's PMI on no recognised feature-and-kind ("Other PMI in the file").
import { codeOf } from './choose.ts';
import { CHARACTERISTIC } from './labels.ts';
import type { PmiAnalysis, PmiQuestion } from '../types/pmi.ts';

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

export const capital = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);
/** The kind's own word: ``hole.position`` → "Position", ``part.general_tolerance`` → "General tolerance". */
export const kindWord = (kind: string) => capital((kind.split('.').pop() ?? kind).replace(/_/g, ' '));

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


/** A review with selected stated PMI taken out of the analysis so the pending edit can replace it. */
export function withoutRequirements(analysis: PmiAnalysis, ids: readonly string[]): PmiAnalysis {
  const table = tableOf(analysis);
  const part = { ...analysis.part };
  const hidden = new Set<number>();
  const targets = new Set(ids);
  const hide = (requirement: TableRequirement) => {
    const number = requirement.number;
    const index = analysis.existing.findIndex((item, i) => !hidden.has(i) && (
      number ? `#${item.number}` === number
        : item.type === requirement.type && item.kind === requirement.kind &&
          JSON.stringify(item.faces) === JSON.stringify(requirement.faces) &&
          (!item.text || item.text === requirement.text)
    ));
    if (index >= 0) hidden.add(index);
  };
  const rows = table.rows.map(row => {
    if (!targets.has(row.id) || row.state !== 'stated') return row;
    if (row.id.startsWith('part.')) delete part[row.id.slice(5)];
    for (const requirement of row.requirements ?? []) hide(requirement);
    return { ...row, state: 'gap' as const, value: undefined, requirements: [] };
  });
  const file_pmi = table.file_pmi.filter(item => {
    const id = item.number ?? item.id;
    if (!id || !targets.has(id)) return true;
    hide(item);
    return false;
  });
  const given = { ...analysis, part, table: { ...table, rows, file_pmi }, existing: analysis.existing.filter((_, i) => !hidden.has(i)) };
  if (targets.has('part.process')) Reflect.deleteProperty(given, 'process');
  return given;
}
