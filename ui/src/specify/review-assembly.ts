import type { PmiAnalysis, PmiExisting } from '../types/pmi.ts';
import { PendingEdits, removalTargets, type CopyEditOrigin, type Pending } from './edit-list.ts';
import { answerText, isNoneValue, tableOf } from './feature-table.ts';
import { decisionText, otherRows, partFile, rowText, storedChoice, type FileReviewOptions, type ReviewPart } from './file-review.ts';

export type PartQueue = ReadonlyMap<number, PendingEdits>;

export const EMPTY_PENDING = new PendingEdits();
export const EMPTY_QUEUE: PartQueue = new Map();

/** Open returns one index per definition. Multiple placements of that definition share its edits. */
export function linkedParts(parts: readonly ReviewPart[], part: number, split?: ReadonlySet<number>): ReviewPart[] {
  const selected = parts.find(one => one.part === part);
  if (!selected) return [];
  if (split?.has(part)) return [selected];
  const first = selected.same_as ?? selected.part;
  return parts.filter(one => (one.same_as ?? one.part) === first && !split?.has(one.part));
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

const stable = (value: unknown) => JSON.stringify(value, (_, item) =>
  item && typeof item === 'object' && !Array.isArray(item)
    ? Object.fromEntries(Object.entries(item).sort(([a], [b]) => a.localeCompare(b))) : item);

function storedRequirements(analysis: PmiAnalysis, anchor: PmiAnalysis, map: (face: number) => number): string | undefined {
  const resume = analysis.resume;
  if (!analysis.table?.rows.length || resume?.unverified || resume?.unresolved?.length || resume?.dropped_answers?.length) return;
  const { _specify, ...answers } = resume?.answers ?? {};
  const meta = _specify as { evidence?: unknown; no_pmi?: readonly string[]; unresolved?: Record<string, string> } | undefined;
  if (meta && (Object.keys(meta.unresolved ?? {}).length ||
    Object.keys(meta).some(key => !['evidence', 'no_pmi', 'unresolved'].includes(key)))) return;

  const idOf = (id: string) => mapId(id, analysis, anchor, map);
  const faces = (ids: readonly number[] = []) => ids.map(map).sort((a, b) => a - b);
  const table = tableOf(analysis);
  const existing = analysis.existing.map((item: PmiExisting & {
    provenance?: unknown; origin?: unknown; source?: unknown; question?: string;
  }) => {
    const { number, provenance, origin, source, question, faces: from, faces2: to, ...requirement } = item;
    return stable({ ...requirement, faces: faces(from), faces2: faces(to) });
  });
  const other = table.file_pmi.map(item => stable([item.kind, item.type, item.text, faces(item.faces)]));
  const rows = table.rows.filter(row => row.state !== 'gap')
    .map(row => stable([idOf(row.id), row.state, mappedValue(row.value, map)]));
  const saved = Object.entries(answers).map(([id, value]) => stable([idOf(id), mappedValue(value, map)]));
  const itemNumber = analysis.bom?.item_number;
  const numberedBom = typeof itemNumber === 'object' && itemNumber?.value != null;
  const bom = Object.entries(analysis.bom ?? {}).flatMap(([name, field]) => {
    if (typeof field !== 'object' || !field) return [];
    if (field.source === 'answered') return [[name, field]];
    // After a BOM is numbered, these defaults can also be saved suggestions.
    if (numberedBom && field.source === 'default' && !['item_number', 'name', 'quantity'].includes(name)) return [[name, field]];
    return [];
  });
  return stable({
    part: analysis.part, process: analysis.process,
    existing: existing.sort(), other: other.sort(), rows: rows.sort(), saved: saved.sort(), bom: Object.fromEntries(bom),
    none: table.decided_none.map(idOf).sort(),
    noPmi: meta?.no_pmi?.map(idOf).sort() ?? [],
    automatic: resume?.automatic?.map(idOf).sort() ?? [],
  });
}

/** Refine the engine's proven copy families by each definition's stated PMI and saved decisions. */
export function reviewParts<T extends ReviewPart & { written?: boolean }>(parts: readonly T[], read: (part: number) => PmiAnalysis): T[] {
  const anchors = new Map(parts.map(part => [part.part, part]));
  const related = new Set(parts.flatMap(part => part.same_as == null ? [] : [part.same_as]));
  const analyses = new Map<number, PmiAnalysis>();
  const analysisOf = (part: number) => {
    const analysis = analyses.get(part) ?? read(part);
    analyses.set(part, analysis);
    return analysis;
  };
  const groups = new Map<string, T>();
  return parts.map(part => {
    const first = part.same_as ?? part.part;
    const anchor = anchors.get(first);
    if (!related.has(first)) return part;
    const separate = { ...part, same_as: null, faces_of_first: null };
    if (!anchor) return separate;
    try {
      const analysis = analysisOf(part.part);
      if (part.written && !analysis.resume) return separate;
      const stored = storedRequirements(analysis, analysisOf(anchor.part), faceMap(part, anchor));
      if (stored === undefined) return separate;
      const key = JSON.stringify([first, stored]);
      const match = groups.get(key);
      if (!match) {
        groups.set(key, part);
        return separate;
      }
      const map = faceMap(part, match);
      return { ...part, same_as: match.part, faces_of_first: (part.faces_of_first ?? match.faces_of_first)?.map((_, face) => map(face)) };
    } catch (error) {
      // A candidate whose saved state cannot be read or mapped is its own group.
      if (error instanceof WebAssembly.RuntimeError) throw error;
      return separate;
    }
  });
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
  const choice = row?.state === 'decided_none' ? row.value : row?.state !== 'stated' ? storedChoice(target, id) : undefined;
  if (row?.state === 'decided_none' || choice !== undefined) {
    if ((row?.state === 'decided_none' && isNoneValue(value)) || JSON.stringify(value) === JSON.stringify(choice)) return { id };
    return { id, edit: { ...editBase, kind: 'replace', value, text: answerText(value), was: decisionText(row?.state, choice) } };
  }
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

interface EditLocation { part: number; id: string }

/** Independent edits can share a row ID. Only edits made by one copy action share an origin. */
export function pendingActions(queue: PartQueue) {
  const groups = new Map<CopyEditOrigin, EditLocation[]>();
  const actions: { part: number; edit: Pending; targets: EditLocation[]; propagated: boolean }[] = [];
  for (const [part, list] of queue) {
    for (const edit of list.values()) {
      const target = { part, id: edit.id };
      const origin = edit.copyOrigin;
      if (!origin) {
        actions.push({ part, edit, targets: [target], propagated: false });
        continue;
      }
      const targets = groups.get(origin);
      if (targets) targets.push(target);
      else {
        const targets = [target];
        groups.set(origin, targets);
        actions.push({ part: origin.part, edit: origin.edit, targets, propagated: true });
      }
    }
  }
  return actions;
}

/** Undo against the members still carrying this edit, even after a copy has been split off. */
export function withoutPendingAction(queue: PartQueue, { part, id }: EditLocation): PartQueue {
  const edit = queue.get(part)?.get(id);
  if (!edit) return queue;
  const origin = edit.copyOrigin;
  if (!origin) return setPartEdits(queue, part, queue.get(part)!.without(id));
  let next = queue;
  for (const [part, list] of queue) {
    const member = list.values().find(entry => entry.copyOrigin === origin);
    if (member) next = setPartEdits(next, part, list.without(member.id));
  }
  return next;
}

/** Construct matching lists without mutating any existing queue. The caller checks all affected lists before publishing. */
export async function queueCopies(options: () => FileReviewOptions, source: PmiAnalysis, group: readonly ReviewPart[],
  local: PartQueue, before: PendingEdits, after: PendingEdits): Promise<PartQueue> {
  const file = options().file;
  const index = file.part ?? 0;
  let next = local;
  for (const previous of before.values()) {
    if (!after.get(previous.id)) next = withoutPendingAction(next, { part: index, id: previous.id });
  }
  const changed = after.values().filter(edit => before.get(edit.id) !== edit);
  if (group.length < 2 || !changed.length) return setPartEdits(next, index, after);

  const origins = changed.map(edit => ({ part: index, edit }));
  const byId = new Map(origins.map(origin => [origin.edit.id, origin]));
  next = setPartEdits(next, index, new PendingEdits(after.values().map(edit => {
    const copyOrigin = byId.get(edit.id);
    return copyOrigin ? { ...edit, copyOrigin } : edit;
  })));
  const from = group.find(member => member.part === index)!;
  for (const copy of group) {
    if (copy.part === index) continue;
    try {
      const { analysis } = await options().worker.request<{ analysis: PmiAnalysis }>('part-analysis', partFile(file, copy.part));
      const map = faceMap(from, copy);
      let edits = next.get(copy.part) ?? EMPTY_PENDING;
      for (const copyOrigin of origins) {
        const translated = editForCopy(copyOrigin.edit, source, analysis, map);
        edits = translated.edit ? edits.with({ ...translated.edit, copyOrigin }) : edits.without(translated.id);
      }
      next = setPartEdits(next, copy.part, edits);
    } catch (error) {
      throw new Error(`${copy.name}: ${error instanceof Error ? error.message : String(error)}`, { cause: error });
    }
  }
  return next;
}
