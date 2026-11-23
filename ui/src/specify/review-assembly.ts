import type { PmiAnalysis } from '../types/pmi.ts';
import { PendingEdits, removalTargets, type Pending } from './edit-list.ts';
import { answerText, tableOf } from './feature-table.ts';
import { otherRows, partFile, rowText, type FileReviewOptions, type ReviewPart } from './file-review.ts';

export type PartQueue = ReadonlyMap<number, PendingEdits>;

export const EMPTY_PENDING = new PendingEdits();
export const EMPTY_QUEUE: PartQueue = new Map();

/** Open returns one index per definition. Multiple placements of that definition share its edits. */
export function linkedParts(parts: readonly ReviewPart[], part: number, split: ReadonlySet<number>): ReviewPart[] {
  const selected = parts.find(one => one.part === part);
  if (!selected) return [];
  if (split.has(part)) return [selected];
  const first = selected.same_as ?? selected.part;
  return parts.filter(one => (one.same_as ?? one.part) === first && !split.has(one.part));
}

export function faceMap(from: ReviewPart, to: ReviewPart): (face: number) => number {
  const inverse = new Map(to.faces_of_first?.map((first, face) => [first, face]));
  return face => {
    const first = from.same_as == null ? face : from.faces_of_first?.[face];
    const target = first === undefined ? undefined : to.same_as == null ? first : inverse.get(first);
    if (target === undefined) throw new Error(`There is no corresponding face ${face} on ${to.name}.`);
    return target;
  };
}

const signature = (faces: readonly number[]) => [...faces].sort((a, b) => a - b).join('.');
const matchFaces = (a: readonly number[], b: readonly number[]) => signature(a) === signature(b);

function mapId(id: string, source: PmiAnalysis, target: PmiAnalysis, map: (face: number) => number): string {
  const measurement = /^((?:length:planes|location:holes):)([\d.]+)-([\d.]+)$/.exec(id);
  if (measurement) {
    const [, prefix, from, to] = measurement;
    const a = signature(from!.split('.').map(Number).map(map));
    const b = signature(to!.split('.').map(Number).map(map));
    const direct = `${prefix}${a}-${b}`;
    const reverse = `${prefix}${b}-${a}`;
    const rows = tableOf(target).rows;
    if (rows.some(row => row.id === direct)) return direct;
    if (prefix === 'length:planes:' && rows.some(row => row.id === reverse)) return reverse;
    return direct;
  }
  const colon = id.indexOf(':');
  if (colon === -1) return id;
  const feature = source.features.find(feature => feature.id === id.slice(colon + 1));
  if (!feature) throw new Error(`Cannot map ${id} to the matching part.`);
  const faces = feature.faces.map(map);
  const candidates = target.features.filter(other => other.family === feature.family && matchFaces(faces, other.faces));
  if (candidates.length !== 1) throw new Error(`Cannot identify the corresponding requirement for ${id}.`);
  return `${id.slice(0, colon + 1)}${candidates[0]!.id}`;
}

function mappedValue(value: unknown, map: (face: number) => number): unknown {
  if (Array.isArray(value) && value.every(face => typeof face === 'number')) return value.map(map);
  if (!value || typeof value !== 'object' || !('faces' in value) || !Array.isArray(value.faces)) return value;
  const reference = 'reference' in value && Array.isArray(value.reference) ? value.reference.map(map) : undefined;
  return { ...value, faces: value.faces.map(map), ...(reference ? { reference } : {}) };
}

/** Copy the person's intent and derive the target verb and guards from that copy's own stored PMI. */
export function editForCopy(edit: Pending, source: PmiAnalysis, target: PmiAnalysis, map: (face: number) => number): { id: string; edit?: Pending } {
  const other = otherRows(source).find(row => row.id === edit.id);
  if (other?.other) {
    const item = other.other;
    const faces = item.faces.map(map);
    const matches = otherRows(target).filter(row =>
      row.other?.kind === item.kind && row.other?.type === item.type && row.other?.text === item.text && matchFaces(faces, row.faces));
    if (matches.length !== 1 || edit.kind !== 'remove') throw new Error(`Cannot identify the corresponding item for ${edit.name}.`);
    const row = matches[0]!;
    const refs = removalTargets(row.other?.number ? [row.other.number] : [], row.other?.id);
    if (!refs.length) throw new Error(`Cannot remove the corresponding item for ${edit.name}.`);
    return { id: row.id, edit: { ...edit, id: row.id, targets: refs, was: rowText(row) } };
  }

  const id = mapId(edit.id, source, target, map);
  const row = tableOf(target).rows.find(row => row.id === id);
  const editBase = { id, name: edit.name };
  if (edit.kind === 'refused') return { id };
  if (edit.kind === 'remove') {
    if (row?.state !== 'stated') return { id };
    const targets = removalTargets(row.requirements?.flatMap(item => item.number ? [item.number] : []) ?? [], id);
    return { id, edit: { ...editBase, kind: 'remove', targets, was: answerText(row.value) } };
  }
  if (edit.kind === 'clear') return { id, edit: row?.state === 'decided_none' || id in (target.resume?.answers ?? {}) ? { ...edit, id } : undefined };
  if (edit.kind === 'none') {
    if (row?.state === 'decided_none') return { id };
    // A copy can already state this requirement. Take it back and record None in the same list.
    const targets = row?.state === 'stated'
      ? removalTargets(row.requirements?.flatMap(item => item.number ? [item.number] : []) ?? [], id) : undefined;
    return { id, edit: { ...editBase, kind: 'none', targets } };
  }
  const value = mappedValue(edit.value, map);
  if (row?.state === 'stated') {
    if (JSON.stringify(value) === JSON.stringify(row.value)) return { id };
    return { id, edit: { ...editBase, kind: 'change', value, text: answerText(value), expect: row.value, was: answerText(row.value) } };
  }
  return { id, edit: { ...editBase, kind: 'fill', value, text: answerText(value) } };
}

export function mergePending(local: PartQueue, staged?: PartQueue): PartQueue {
  if (!staged?.size) return local;
  const combined = new Map(local);
  for (const [part, list] of staged) combined.set(part, new PendingEdits([...list.values(), ...(local.get(part)?.values() ?? [])]));
  return combined;
}

export function setPartEdits(queue: PartQueue, part: number, edits: PendingEdits): PartQueue {
  const next = new Map(queue);
  if (edits.size) next.set(part, edits);
  else next.delete(part);
  return next;
}

/** Construct matching lists without mutating any existing queue. The caller checks all affected lists before publishing. */
export async function queueCopies(options: () => FileReviewOptions, source: PmiAnalysis, group: readonly ReviewPart[],
  local: PartQueue, before: PendingEdits, after: PendingEdits): Promise<PartQueue> {
  const file = options().file;
  const index = file.part ?? 0;
  let next = setPartEdits(local, index, after);
  if (group.length < 2) return next;
  const from = group.find(member => member.part === index)!;
  for (const copy of group) {
    if (copy.part === index) continue;
    const { analysis } = await options().worker.request<{ analysis: PmiAnalysis }>('part-analysis', partFile(file, copy.part));
    const map = faceMap(from, copy);
    let edits = local.get(copy.part) ?? EMPTY_PENDING;
    for (const previous of before.values()) {
      if (after.get(previous.id)) continue;
      edits = edits.without(editForCopy(previous, source, analysis, map).id);
    }
    for (const edit of after.values()) {
      if (before.get(edit.id) === edit) continue;
      const translated = editForCopy(edit, source, analysis, map);
      edits = translated.edit ? edits.with(translated.edit) : edits.without(translated.id);
    }
    next = setPartEdits(next, copy.part, edits);
  }
  return next;
}
