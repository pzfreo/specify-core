// Single PMI view model: binds the active PMI scene (file graphics and generated 3D graphics) to
// review rows, tracks pending unsaved edits, identifies unpresented requirements and real open
// gaps, and extracts structured semantic labels from `haecceity.pmi/1`. Pure, no three.js or DOM.
import type { PmiAnalysis, PmiAnswers, PmiIntent, PmiQuestion } from '../types/pmi.ts';
import { describeFaces } from './datums.ts';
import { drawnFromScene, withoutNotesBlock, type DrawnGraphic, type DrawnLink, type DrawnPmi, type Read } from './drawn.ts';
import { CHARACTERISTIC, fmt, toLabels, type LabelTolerance, type PmiLabel } from './labels.ts';

/** One review row (feature table row or "Other PMI in the file" item) known to the PMI view. */
export interface OneViewRow {
  readonly id: string;
  readonly state: 'stated' | 'gap' | 'decided_none';
  readonly open: boolean;
  readonly faces: readonly number[];
  readonly label: string;
  readonly question?: PmiQuestion;
  readonly other?: boolean;
}

/** One pending edit held in the page or session until Save. */
export interface OneViewPending {
  readonly kind: 'fill' | 'change' | 'remove' | 'none' | 'clear' | 'refused';
  readonly id: string;
  readonly value?: unknown;
  readonly text?: string;
  readonly was?: string;
  readonly targets?: readonly unknown[];
}

/** Structured label contents parsed directly from `haecceity.pmi/1` in a PMI scene. */
export interface SceneSemanticLabel {
  readonly kind: PmiLabel['kind'];
  readonly cells: readonly string[];
  readonly tolerance?: LabelTolerance | null;
  readonly basic?: true;
}

const SIZE_KIND: Readonly<Record<string, string>> = {
  'curve length': 'Size_CurveLength',
  diameter: 'Size_Diameter',
  'spherical diameter': 'Size_SphericalDiameter',
  radius: 'Size_Radius',
  'spherical radius': 'Size_SphericalRadius',
  'toroidal minor diameter': 'Size_ToroidalMinorDiameter',
  'toroidal major diameter': 'Size_ToroidalMajorDiameter',
  'toroidal minor radius': 'Size_ToroidalMinorRadius',
  'toroidal major radius': 'Size_ToroidalMajorRadius',
  'toroidal high major diameter': 'Size_ToroidalHighMajorDiameter',
  'toroidal low major diameter': 'Size_ToroidalLowMajorDiameter',
  'toroidal high major radius': 'Size_ToroidalHighMajorRadius',
  'toroidal low major radius': 'Size_ToroidalLowMajorRadius',
  thickness: 'Size_Thickness',
};

const LOCATION_KIND: Readonly<Record<string, string>> = {
  'curved distance': 'Location_CurvedDistance',
  'linear distance': 'Location_LinearDistance',
  'linear distance centre outer': 'Location_LinearDistance_FromCenterToOuter',
  'linear distance centre inner': 'Location_LinearDistance_FromCenterToInner',
  'linear distance outer centre': 'Location_LinearDistance_FromOuterToCenter',
  'linear distance outer outer': 'Location_LinearDistance_FromOuterToOuter',
  'linear distance outer inner': 'Location_LinearDistance_FromOuterToInner',
  'linear distance inner centre': 'Location_LinearDistance_FromInnerToCenter',
  'linear distance inner outer': 'Location_LinearDistance_FromInnerToOuter',
  'linear distance inner inner': 'Location_LinearDistance_FromInnerToInner',
};

const TOLERANCE_KIND: Readonly<Record<string, string>> = {
  angularity_tolerance: 'Angularity',
  circular_runout_tolerance: 'CircularRunout',
  coaxiality_tolerance: 'Coaxiality',
  concentricity_tolerance: 'Concentricity',
  cylindricity_tolerance: 'Cylindricity',
  flatness_tolerance: 'Flatness',
  line_profile_tolerance: 'ProfileOfLine',
  parallelism_tolerance: 'Parallelism',
  perpendicularity_tolerance: 'Perpendicularity',
  position_tolerance: 'Position',
  roundness_tolerance: 'CircularityOrRoundness',
  straightness_tolerance: 'Straightness',
  surface_profile_tolerance: 'ProfileOfSurface',
  symmetry_tolerance: 'Symmetry',
  total_runout_tolerance: 'TotalRunout',
};

const MODIFIER_SYMBOL: Readonly<Record<string, string>> = {
  'maximum material requirement': 'Ⓜ',
  'maximum_material_requirement': 'Ⓜ',
  'least material requirement': 'Ⓛ',
  'least_material_requirement': 'Ⓛ',
  'regardless of feature size': 'Ⓢ',
  'projected tolerance zone': 'Ⓟ',
  'free state': 'Ⓕ',
  'tangent plane': 'Ⓣ',
};

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);

const isNoneValue = (value: unknown): boolean => value === null || value === undefined || value === ''
  || (Array.isArray(value) ? value.length === 0 : typeof value === 'string' && /^none\b/i.test(value.trim()));

/**
 * Whether a non-empty answer or default explicitly decides that no feature-specific callout is
 * needed (`general` for general tolerance, or `none` / `None (...)`). Unlike `isNoneValue`,
 * `null` and `undefined` return false because an unanswered required question is still a real gap.
 */
export const isDecidedNoCallout = (value: unknown): boolean => {
  if (typeof value !== 'string') return false;
  const trimmed = value.trim();
  return /^general$/i.test(trimmed) || /^none\b/i.test(trimmed);
};

const isDimensionId = (id: string) => id.startsWith('length:') || id.startsWith('location:');

const readable = (name: string) => {
  const text = (name.split('_').pop() ?? name).replace(/([a-z])([A-Z])/g, '$1 $2').toLowerCase();
  return text.charAt(0).toUpperCase() + text.slice(1);
};

function displayText(text: string): string {
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

interface LocatedItem {
  id: string;
  kind: string;
  type: string;
  faces: number[];
  number?: number;
  notation?: string;
  hasValue: boolean;
}

interface TableReq {
  number?: string;
  id?: string;
  kind: string;
  type: string;
  faces?: readonly number[];
  text?: string;
}

interface TableRowLike {
  id: string;
  state: 'stated' | 'gap' | 'decided_none';
  requirements?: readonly TableReq[];
}

interface TableLike {
  rows?: readonly TableRowLike[];
  file_pmi?: readonly TableReq[];
}

function locateSceneItems(scene: unknown, analysis?: PmiAnalysis): {
  reviewItems: Record<string, { row?: string; existing?: number }>;
  resolvedFaces: Map<string, number[]>;
} {
  const reviewItems: Record<string, { row?: string; existing?: number; partial?: boolean }> = {};
  const resolvedFaces = new Map<string, number[]>();
  if (!isObject(scene) || !isObject(scene['pmi']) || !isObject(scene['pmi']['pmi'])) {
    return { reviewItems, resolvedFaces };
  }
  const pmi = scene['pmi']['pmi'] as Record<string, unknown>;
  const specify = isObject(scene['specify']) ? scene['specify'] : {};
  const arr = (key: string): Record<string, unknown>[] =>
    (Array.isArray(pmi[key]) ? pmi[key] as unknown[] : []).filter(isObject);

  const featureById = new Map<string, Record<string, unknown>>();
  for (const f of arr('features')) {
    if (typeof f['id'] === 'string' && isObject(f['value'])) featureById.set(f['id'], f['value']);
  }
  const targetById = new Map<string, Record<string, unknown>>();
  for (const t of arr('datum_targets')) {
    if (typeof t['id'] === 'string' && isObject(t['value'])) targetById.set(t['id'], t['value']);
  }
  const cache = new Map<string, number[]>();
  const facesOf = (id: unknown, seen = new Set<string>()): number[] => {
    if (typeof id !== 'string') return [];
    const hit = cache.get(id);
    if (hit) return hit;
    if (seen.has(id)) return [];
    seen.add(id);
    const val = featureById.get(id);
    if (!val) return [];
    const out = new Set<number>();
    if (Array.isArray(val['items'])) {
      for (const x of val['items']) if (isObject(x) && typeof x['face'] === 'number') out.add(x['face']);
    }
    if (isObject(val['group']) && Array.isArray(val['group']['members'])) {
      for (const m of val['group']['members']) for (const f of facesOf(m, seen)) out.add(f);
    }
    if (isObject(val['derived']) && Array.isArray(val['derived']['from'])) {
      for (const m of val['derived']['from']) for (const f of facesOf(m, seen)) out.add(f);
    }
    const sorted = [...out].sort((a, b) => a - b);
    cache.set(id, sorted);
    return sorted;
  };
  for (const id of featureById.keys()) resolvedFaces.set(id, facesOf(id));

  const numOf = (entry: Record<string, unknown>, id: string): number | undefined => {
    const spec = isObject(specify[id]) ? specify[id] as Record<string, unknown> : undefined;
    const raw = entry['identifier'] ?? spec?.['number'];
    if (typeof raw === 'number' && Number.isFinite(raw)) return raw;
    if (typeof raw === 'string' && /^\d+$/.test(raw)) return Number(raw);
    return undefined;
  };
  const notationOf = (id: string): string | undefined => {
    const spec = isObject(specify[id]) ? specify[id] as Record<string, unknown> : undefined;
    return typeof spec?.['notation'] === 'string' ? spec['notation'] : undefined;
  };

  const dimFaces = new Map<string, number[]>();
  const located: LocatedItem[] = [];
  for (const d of arr('datums')) {
    const id = d['id'];
    const val = d['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const label = typeof val['label'] === 'string' ? val['label'] : '';
    const faces = new Set<number>(facesOf(val['feature']));
    if (!faces.size && Array.isArray(val['targets'])) {
      for (const tid of val['targets']) {
        const target = typeof tid === 'string' ? targetById.get(tid) : undefined;
        for (const f of facesOf(target?.['on'])) faces.add(f);
      }
    }
    const sorted = [...faces].sort((a, b) => a - b);
    resolvedFaces.set(id, sorted);
    located.push({ id, kind: 'datum', type: label, faces: sorted, number: numOf(d, id), notation: notationOf(id), hasValue: false });
  }
  for (const d of arr('dimensions')) {
    const id = d['id'];
    const val = d['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const k = isObject(val['kind']) ? val['kind'] : {};
    let type = 'Location_None';
    let faces: number[] = [];
    if (isObject(k['size'])) {
      const s = k['size'];
      const sk = typeof s['kind'] === 'string' ? s['kind'] : '';
      type = s['angle'] != null ? 'Size_Angular' : SIZE_KIND[sk] ?? (s['path'] != null ? 'Size_WithPath' : 'Location_None');
      faces = facesOf(s['feature']);
    } else if (isObject(k['location'])) {
      const l = k['location'];
      const lk = typeof l['kind'] === 'string' ? l['kind'] : '';
      type = l['angle'] != null ? 'Location_Angular' : LOCATION_KIND[lk] ?? (l['path'] != null ? 'Location_WithPath' : 'Location_None');
      faces = [...new Set([...facesOf(l['from']), ...facesOf(l['to'])])].sort((a, b) => a - b);
    }
    dimFaces.set(id, faces);
    resolvedFaces.set(id, faces);
    located.push({ id, kind: 'dimension', type, faces, number: numOf(d, id), notation: notationOf(id), hasValue: val['nominal'] != null });
  }
  for (const t of arr('tolerances')) {
    const id = t['id'];
    const val = t['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const rawKind = typeof val['kind'] === 'string' ? val['kind'] : '';
    const type = TOLERANCE_KIND[rawKind] ?? rawKind;
    const tgt = isObject(val['target']) ? val['target'] : {};
    let faces: number[] = [];
    if (typeof tgt['feature'] === 'string') faces = facesOf(tgt['feature']);
    else if (typeof tgt['dimension'] === 'string') faces = dimFaces.get(tgt['dimension']) ?? [];
    else if (isObject(tgt['relation'])) {
      faces = [...new Set([...facesOf(tgt['relation']['relating']), ...facesOf(tgt['relation']['related'])])].sort((a, b) => a - b);
    }
    resolvedFaces.set(id, faces);
    located.push({ id, kind: 'geometric_tolerance', type, faces, number: numOf(t, id), notation: notationOf(id), hasValue: val['magnitude'] != null });
  }
  for (const s of arr('surface_textures')) {
    const id = s['id'];
    const val = s['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const params = (Array.isArray(val['parameters']) ? val['parameters'] : [])
      .filter(isObject)
      .map((p) => (typeof p['characteristic'] === 'string' ? p['characteristic'] : ''))
      .sort().join('|');
    const faces = facesOf(val['on']);
    resolvedFaces.set(id, faces);
    located.push({ id, kind: 'surface_texture', type: params, faces, number: numOf(s, id), notation: notationOf(id), hasValue: true });
  }
  for (const t of arr('threads')) {
    const id = t['id'];
    const val = t['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const type = val['side'] === 'external' ? 'External' : 'Internal';
    const faces = facesOf(val['feature']);
    resolvedFaces.set(id, faces);
    located.push({ id, kind: 'thread', type, faces, number: numOf(t, id), notation: notationOf(id), hasValue: true });
  }
  for (const k of arr('knurls')) {
    const id = k['id'];
    const val = k['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const faces = facesOf(val['feature']);
    resolvedFaces.set(id, faces);
    located.push({ id, kind: 'knurl', type: 'knurl', faces, number: numOf(k, id), notation: notationOf(id), hasValue: true });
  }

  if (!analysis) return { reviewItems, resolvedFaces };

  const faceKey = (fs: readonly number[]) => [...new Set(fs)].sort((a, b) => a - b).join(',');
  const norm = (kind: string, type: string): [string, string] => {
    if (kind === 'note' && type === 'internal thread') return ['thread', 'Internal'];
    if (kind === 'note' && type === 'external thread') return ['thread', 'External'];
    if (kind === 'note' && type === 'knurl') return ['knurl', 'knurl'];
    return [kind, type];
  };
  const locate = (
    req: { number?: number | string; kind: string; type: string; faces: readonly number[]; text?: string; hasValue?: boolean },
    pool: readonly LocatedItem[],
    taken: ReadonlySet<string>,
  ): string | undefined => {
    if (req.number !== undefined) {
      const n = typeof req.number === 'number' ? req.number : Number(String(req.number).replace(/^#/, ''));
      if (Number.isFinite(n)) {
        const hit = pool.find((l) => l.number === n && !taken.has(l.id)) ?? pool.find((l) => l.number === n);
        if (hit) return hit.id;
      }
    }
    const [kind, type] = norm(req.kind, req.type);
    const fk = faceKey(req.faces);
    const candidates = pool.filter((l) => {
      if (taken.has(l.id) || faceKey(l.faces) !== fk) return false;
      if (l.kind === kind && l.type === type) return true;
      if (kind === 'note' && type.startsWith('surface') && l.kind === 'surface_texture') return true;
      return false;
    });
    if (candidates.length <= 1) return candidates[0]?.id;
    if (req.text) {
      const byText = candidates.find((l) => l.notation && req.text!.includes(l.notation));
      if (byText) return byText.id;
    }
    if (req.hasValue !== undefined) {
      const byVal = candidates.find((l) => l.hasValue === req.hasValue);
      if (byVal) return byVal.id;
    }
    return candidates[0]?.id;
  };

  const ensure = (id: string) => (reviewItems[id] ??= {});
  const takenExisting = new Set<string>();
  for (const [i, e] of (analysis.existing ?? []).entries()) {
    const faces = [...(e.faces ?? []), ...(e.faces2 ?? [])];
    const id = locate({ number: e.number, kind: e.kind, type: e.type, faces, text: e.text, hasValue: e.value != null }, located, takenExisting);
    if (id) {
      ensure(id).existing = i;
      takenExisting.add(id);
    }
  }

  const available = [...located];
  const takeOut = (id: string) => {
    const idx = available.findIndex((x) => x.id === id);
    if (idx >= 0) available.splice(idx, 1);
  };
  const table = (analysis as { table?: TableLike }).table;
  for (const [i, req] of (table?.file_pmi ?? []).entries()) {
    const rowId = req.number ?? req.id ?? `item:${i}`;
    const id = locate({
      number: req.number, kind: req.kind, type: req.type, faces: req.faces ?? [],
      text: req.text, hasValue: !/no value stated/i.test(req.text ?? ''),
    }, available, new Set());
    if (id) {
      ensure(id).row = rowId;
      takeOut(id);
    }
  }
  for (const row of table?.rows ?? []) {
    const taken = new Set<string>();
    for (const req of row.requirements ?? []) {
      const id = locate({
        number: req.number, kind: req.kind, type: req.type, faces: req.faces ?? [],
        text: req.text, hasValue: !/no value stated/i.test(req.text ?? ''),
      }, available, taken);
      if (id) {
        const item = ensure(id);
        if (item.row === undefined || (item.partial && row.state === 'stated')) {
          item.row = row.id;
          item.partial = row.state === 'gap';
        }
        taken.add(id);
      }
    }
  }
  const clean: Record<string, { row?: string; existing?: number }> = {};
  for (const [id, entry] of Object.entries(reviewItems)) {
    clean[id] = {
      ...(entry.row !== undefined ? { row: entry.row } : {}),
      ...(entry.existing !== undefined ? { existing: entry.existing } : {}),
    };
  }
  return { reviewItems: clean, resolvedFaces };
}

/**
 * Reads a PMI scene for the review view: resolves each drawn item's review identity (`existing`
 * index and feature-table row id) from the scene's `haecceity.pmi/1` document and `analysis`,
 * resolves composite/derived feature faces, and strips general-notes graphics that the notes
 * panel already displays.
 */
export function reviewDrawnFromScene(scene: unknown, analysis?: PmiAnalysis): Read<DrawnPmi> {
  const drawn = drawnFromScene(scene);
  if (!drawn.ok) return drawn;
  const serverReview = isObject(scene) && isObject(scene['review_items'])
    ? scene['review_items'] as DrawnPmi['reviewItems']
    : undefined;
  const { reviewItems, resolvedFaces } = locateSceneItems(scene, analysis);
  const links: Record<string, DrawnLink> = {};
  for (const [id, link] of Object.entries(drawn.value.links)) {
    const faces = link.faces.length ? link.faces : resolvedFaces.get(id) ?? [];
    links[id] = { ...link, faces };
  }
  const merged = serverReview ?? reviewItems;
  return {
    ok: true,
    value: withoutNotesBlock({
      ...drawn.value,
      links,
      ...(Object.keys(merged).length ? { reviewItems: merged } : {}),
    }),
  };
}

const hasInk = (graphic: DrawnGraphic) => graphic.polylines.some((line) => line.length > 1)
  || graphic.triangles.length > 0
  || Boolean(graphic.textPlacements?.some((at) => graphic.text[at.textIndex ?? 0]));

/**
 * Binds `drawn` graphics to review rows, hides graphics whose stated row has a pending change or
 * removal, tracks which `analysis.existing` indices are already presented by drawn ink, and
 * collects unlinked drawing annotations for the panel.
 */
export function oneViewFile(
  drawn: DrawnPmi | null,
  analysis: PmiAnalysis,
  rows: readonly OneViewRow[],
  pending: ReadonlyMap<string, OneViewPending>,
) {
  const labels = new Map<string, PmiLabel>();
  const extras = new Map<string, PmiLabel>();
  const covered = new Set<number>();
  const replaced = new Set<number>();
  const existingRows = new Map<number, string>();
  const existingItems = new Map<number, string>();
  const rowsById = new Map(rows.map((row) => [row.id, row]));
  const review = drawn?.reviewItems ?? {};
  const hidden = (id: string | undefined) => {
    const edit = id ? pending.get(id) : undefined;
    return edit?.kind === 'change' || edit?.kind === 'remove'
      || (edit?.kind === 'none' && Boolean(edit.targets?.length));
  };
  for (const [item, match] of Object.entries(review)) {
    if (match.existing === undefined) continue;
    existingItems.set(match.existing, item);
    const row = match.row ?? drawn?.links[item]?.question;
    if (row) existingRows.set(match.existing, row);
    if (hidden(row)) replaced.add(match.existing);
  }
  if (!drawn) return { drawn, labels, extras, covered, replaced, existingRows, existingItems };

  const cleaned = withoutNotesBlock(drawn);
  const { presentation } = cleaned;
  const entries = Object.entries(presentation.items);
  const rowOf = (id: string) => {
    const q = cleaned.links[id]?.question;
    if (q && rowsById.has(q)) return q;
    const rev = review[id];
    if (rev?.row && rowsById.has(rev.row)) return rev.row;
    if (rev?.existing !== undefined && rowsById.has(`existing:${rev.existing}`)) return `existing:${rev.existing}`;
    return rev?.row ?? q ?? (rowsById.has(id) ? id : undefined);
  };
  // A shared graphic (such as a datum symbol on a datum and its support) belongs to the item
  // with a review row (or with an existing link). Keep every independently selectable callout.
  const rank = (id: string) => (rowsById.has(rowOf(id) ?? '') ? 2 : review[id]?.existing !== undefined ? 1 : 0);
  entries.sort(([a], [b]) => rank(b) - rank(a));
  const removedInk = new Set(entries.flatMap(([id, graphics]) => (hidden(rowOf(id))
    ? graphics.flatMap((g) => (g.id ? [g.id] : [])) : [])));
  const items: Record<string, readonly DrawnGraphic[]> = {};
  const links: Record<string, DrawnLink> = {};
  const seen = new Map<string, string>();
  const add = (id: string, graphics: readonly DrawnGraphic[]) => {
    const link = cleaned.links[id];
    const rowId = rowOf(id);
    if (hidden(rowId)) return;
    const row = rowId ? rowsById.get(rowId) : undefined;
    const question = row?.id ?? (rowId && isDimensionId(rowId) ? rowId : `drawing:${id}`);
    const shown = graphics.filter((g) => {
      if (!hasInk(g) || (g.id && (seen.has(g.id) || (!row && removedInk.has(g.id))))) return false;
      if (g.id) seen.set(g.id, question);
      return true;
    });
    const i = review[id]?.existing;
    if (i !== undefined && (shown.length > 0 || graphics.some((g) => hasInk(g) && g.id !== undefined && seen.get(g.id) === question))) {
      covered.add(i);
    }
    if (!shown.length) return;

    const existing = i === undefined ? undefined : analysis.existing[i];
    const faces = [...new Set([...(link?.faces ?? []), ...(existing?.faces ?? []), ...(existing?.faces2 ?? [])])];
    const name = shown[0]?.name || 'Drawing annotation';
    const support = faces.length ? describeFaces(faces, analysis) : '';
    const number = existing?.number ? `Characteristic #${existing.number}` : '';
    const label: PmiLabel = {
      id: i === undefined ? `drawn:${id}` : `existing:${i}`,
      kind: 'note', group: link?.general ? 'general' : 'existing', question, sceneItem: id,
      cells: [name], title: [name, support, number].filter(Boolean).join(' · '),
      faces: faces.length ? faces : row?.faces ?? [],
    };
    if (!row && !(rowId && isDimensionId(rowId))) extras.set(question, label);
    labels.set(id, label);
    items[id] = shown;
    links[id] = { ...link, faces: label.faces, status: 'stated', question };
  };
  for (const [id, graphics] of entries) add(id, graphics);
  presentation.unlinked.forEach((graphic, i) => add(`unlinked:${i}`, [graphic]));
  return {
    drawn: Object.keys(items).length ? { presentation: { ...presentation, items, unlinked: [] }, links } : null,
    labels, extras, covered, replaced, existingRows, existingItems,
  };
}

/**
 * Computes which scene items in `file.drawn` represent pending (unsaved) edits, and which
 * questions are already covered by a 3D scene graphic.
 */
export function scenePendingState(
  file: ReturnType<typeof oneViewFile>,
  pending: ReadonlyMap<string, OneViewPending>,
): { unsavedItems: ReadonlySet<string>; coveredQuestions: ReadonlySet<string> } {
  const unsavedItems = new Set<string>();
  const coveredQuestions = new Set<string>();
  const drawn = file.drawn;
  if (!drawn) return { unsavedItems, coveredQuestions };

  for (const [sceneItem, graphics] of Object.entries(drawn.presentation.items)) {
    if (!graphics.length) continue;
    const baseLabel = file.labels.get(sceneItem);
    const link = drawn.links[sceneItem];
    const question = baseLabel?.question ?? link?.question;
    if (question) coveredQuestions.add(question);
    const edit = question ? pending.get(question) : undefined;
    if (edit && (edit.kind === 'fill' || edit.kind === 'change' || edit.kind === 'clear')) {
      unsavedItems.add(sceneItem);
    }
  }
  return { unsavedItems, coveredQuestions };
}

/**
 * Computes fallback camera-facing labels (for unpresented requirements, pending edits not covered
 * by 3D ink, and real open feature gaps), unlinked drawing annotations, and pending part-wide
 * notes for the Notes panel.
 */
export function oneViewLabels(
  intent: PmiIntent | null,
  analysis: PmiAnalysis,
  rows: readonly OneViewRow[],
  file: ReturnType<typeof oneViewFile>,
  pending: ReadonlyMap<string, OneViewPending>,
  answers: PmiAnswers,
  open: ReadonlySet<string>,
  readOnly: boolean,
) {
  const rowsById = new Map(rows.map((row) => [row.id, row]));
  const proposals = new Set<string>();
  const shownEdits = new Set<string>();
  const extras = new Map(file.extras);
  const notes: PmiLabel[] = [];
  const generated = toLabels(intent, analysis, open);
  const generatedIds = new Set(generated.map((label) => label.id));
  analysis.existing.forEach((item, i) => {
    const id = `existing:${i}`;
    if (generatedIds.has(id) || item.kind === 'datum' || !item.faces?.length) return;
    generated.push({ id, kind: 'note', group: 'existing', faces: item.faces, cells: [displayText(item.text ?? item.type)] });
  });
  const labels = generated.flatMap((label) => {
    if (label.group === 'existing') {
      const i = Number(label.id.split(':')[1]);
      if (file.covered.has(i) || file.replaced.has(i)) return [];
      const existing = analysis.existing[i];
      const faces = existing?.faces2?.length ? [...label.faces, ...existing.faces2] : label.faces;
      const row = file.existingRows.get(i) ?? (label.kind === 'datum' ? `datum.${label.cells[0]}` : undefined);
      const question = row && (rowsById.has(row) || isDimensionId(row)) ? row : `drawing:${label.id}`;
      const linked: PmiLabel = { ...label, faces, question, sceneItem: file.existingItems.get(i) };
      if (question !== row) extras.set(question, linked);
      return [linked];
    }
    if (!label.question) return [label];
    const id = label.question;
    const edit = pending.get(id);
    if (edit?.kind === 'none' || edit?.kind === 'remove') return [];
    if (edit && 'value' in edit && edit.value !== undefined) {
      if (isNoneValue(edit.value) || isDecidedNoCallout(edit.value)) return [];
      // An agent draft can arrive before its engine preview. Show its current queued
      // value via the fallback below until the reviewed answers catch up.
      if (JSON.stringify(answers[id]) !== JSON.stringify(edit.value)) return [];
      shownEdits.add(id);
      return [{ ...label, unsaved: true as const, group: 'answered' as const }];
    }
    const row = rowsById.get(id);
    if (!readOnly && row && (edit?.kind === 'clear' || (row.state === 'gap' && (!edit || edit.kind === 'refused')))) {
      const first = !proposals.has(id);
      proposals.add(id);
      if (edit?.kind === 'clear') {
        shownEdits.add(id);
        return [{
          ...label, unsaved: true as const, group: 'answered' as const,
          ...(first ? { gap: row.label } : {}),
        }];
      }
      return [{ ...label, group: 'default' as const, ...(first ? { gap: row.label } : {}) }];
    }
    return [label];
  });
  if (readOnly) return { labels, extras, notes };

  // Emit a light ⊕ gap marker only for real open feature gaps that have no previewed callout and
  // are not decided or routinely defaulted to `general` / `none`. Rows whose default or answer is
  // `general` (e.g. a general-tolerance hole) or `none` are not real gaps in the review panel and
  // remain identical before and after Save.
  for (const row of rows) {
    if (row.other || isDimensionId(row.id) || row.id.startsWith('bom.')) continue;
    const edit = pending.get(row.id);
    if (!row.faces.length || proposals.has(row.id)) continue;
    if (row.state !== 'gap' || !row.open || (edit && edit.kind !== 'refused')) continue;
    const effective = row.id in answers ? answers[row.id] : row.question?.default;
    if (isDecidedNoCallout(effective)) continue;
    const q = row.question;
    const feature = q?.feature ? analysis.features.find((f) => f.id === q.feature) : undefined;
    const diameter = q?.diameter ?? feature?.record['diameter'];
    const what = typeof diameter === 'number' && (feature?.family === 'holes' || row.id.startsWith('hole.'))
      ? `hole Ø${fmt(diameter)}` : q?.title ?? describeFaces(row.faces, analysis);
    labels.push({
      id: `gap:${row.id}`, question: row.id, kind: 'note', group: 'default', faces: row.faces,
      cells: [what], gap: row.label,
    });
  }
  for (const edit of pending.values()) {
    if (edit.kind === 'refused' || shownEdits.has(edit.id)) continue;
    // A `general` answer on a feature writes only general-tolerance coverage (or no PMI on a datum
    // face), so omit floating fallback pills to match the saved view.
    if (edit.kind === 'fill' && isDecidedNoCallout(edit.value)) continue;
    const row = rowsById.get(edit.id);
    const val = edit.value && typeof edit.value === 'object' && !Array.isArray(edit.value)
      ? edit.value as { faces?: readonly number[]; value?: string } : null;
    const faces = val?.faces?.length ? val.faces
      : Array.isArray(edit.value) && edit.value.every((x) => typeof x === 'number') ? edit.value as number[]
        : row?.faces ?? [];
    // Decided none on a feature has no callout after Save; do not show a floating badge before Save.
    if (edit.kind === 'none' && faces.length > 0) continue;
    const rawText = val?.value ?? edit.text ?? String(edit.value ?? '');
    const changed = edit.kind === 'change' && edit.was
      ? /^(\S+) (⌀?)[\d.]+(.*)$/.exec(displayText(edit.was)) : null;
    const text = edit.kind === 'remove' ? `Remove ${displayText(edit.was ?? '')}`
      : edit.kind === 'clear' ? 'Undo none needed'
        : edit.kind === 'none' || isNoneValue(edit.value) ? 'None needed'
          : changed && /^[\d.]+$/.test(rawText) && Object.values(CHARACTERISTIC).includes(changed[1]!)
            ? `${changed[1]} ${changed[2]}${rawText}${changed[3]}`
            : displayText(rawText);
    const label: PmiLabel = {
      id: `pending:${edit.id}`, question: edit.id, kind: 'note', group: 'answered', unsaved: true,
      cells: [text], faces,
    };
    (faces.length ? labels : notes).push(label);
  }
  return { labels, extras, notes };
}

function readQuantityNumber(q: unknown): { value: number; angle: boolean } | null {
  if (!isObject(q)) return null;
  const inner = isObject(q['quantity']) ? q['quantity'] : q;
  for (const [key, angle] of [['length', false], ['angle', true]] as const) {
    const entry = inner[key];
    if (isObject(entry) && (typeof entry['value'] === 'string' || typeof entry['value'] === 'number')) {
      const n = Number(entry['value']);
      if (Number.isFinite(n)) return { value: n, angle };
    }
  }
  return null;
}

function fmtDeviation(value: number, suffix: string): string {
  return value > 0 ? `+${fmt(value)}${suffix}` : value < 0 ? `-${fmt(-value)}${suffix}` : '0';
}

function readModifiers(raw: unknown): string {
  if (!Array.isArray(raw)) return '';
  return raw.map((m) => {
    const s = typeof m === 'string' ? m.toLowerCase() : isObject(m) && typeof m['kind'] === 'string' ? m['kind'].toLowerCase() : '';
    return MODIFIER_SYMBOL[s] ?? '';
  }).join('');
}

/**
 * Extracts structured label contents for each semantic item in a `PmiScene` (`scene.pmi.pmi` +
 * `scene.specify`), independent of whether graphics came from `engine.pmiScene` (before Save)
 * or `wasm.pmiScene` (after Save). Pure, no three.js or DOM.
 */
export function readSceneSemanticLabels(scene: unknown): ReadonlyMap<string, SceneSemanticLabel> {
  const out = new Map<string, SceneSemanticLabel>();
  if (!isObject(scene) || !isObject(scene['pmi']) || !isObject(scene['pmi']['pmi'])) return out;
  const pmi = scene['pmi']['pmi'] as Record<string, unknown>;
  const specify = isObject(scene['specify']) ? scene['specify'] as Record<string, unknown> : {};
  const arr = (key: string): Record<string, unknown>[] =>
    (Array.isArray(pmi[key]) ? pmi[key] as unknown[] : []).filter(isObject);
  const notationOf = (id: string): string | undefined => {
    const spec = isObject(specify[id]) ? specify[id] as Record<string, unknown> : undefined;
    return typeof spec?.['notation'] === 'string' && spec['notation'].trim() ? spec['notation'].trim() : undefined;
  };

  const datumLabels = new Map<string, string>();
  for (const d of arr('datums')) {
    const id = d['id'];
    const val = d['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const label = typeof val['label'] === 'string' ? val['label'].trim() : '';
    if (!label) continue;
    datumLabels.set(id, label);
    out.set(id, { kind: 'datum', cells: [label] });
  }

  for (const t of arr('tolerances')) {
    const id = t['id'];
    const val = t['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const rawKind = typeof val['kind'] === 'string' ? val['kind'] : '';
    const type = TOLERANCE_KIND[rawKind] ?? rawKind;
    const sym = CHARACTERISTIC[type] ?? CHARACTERISTIC[rawKind] ?? type;
    const mag = readQuantityNumber(val['magnitude']);
    const zone = isObject(val['zone']) ? val['zone'] : null;
    const diametral = typeof zone?.['form'] === 'string' && /cylindrical|circular/i.test(zone['form']);
    const mods = readModifiers(val['modifiers']);
    const magText = mag ? `${diametral ? '⌀' : ''}${fmt(mag.value)}${mods}` : '';
    const datumCells: string[] = [];
    if (Array.isArray(val['datums'])) {
      for (const group of val['datums']) {
        if (!isObject(group)) continue;
        const refs = Array.isArray(group['references']) ? group['references'] : [];
        const text = refs.map((r) => {
          if (!isObject(r) || typeof r['datum'] !== 'string') return '';
          const letter = datumLabels.get(r['datum']) ?? r['datum'].replace(/^datum:/, '');
          return `${letter}${readModifiers(r['modifiers'])}`;
        }).filter(Boolean).join('-');
        if (text) datumCells.push(`${text}${readModifiers(group['modifiers'])}`);
      }
    }
    out.set(id, {
      kind: 'frame',
      cells: [sym, ...(magText ? [magText] : []), ...datumCells],
    });
  }

  for (const d of arr('dimensions')) {
    const id = d['id'];
    const val = d['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const nominal = readQuantityNumber(val['nominal']);
    const tol = val['tolerance'];
    // Auxiliary witness-only dimensions (no nominal and no tolerance) carry no label of their own.
    if (!nominal && (tol === 'none' || tol === null || tol === undefined)) continue;
    const k = isObject(val['kind']) ? val['kind'] : {};
    const sizeKind = isObject(k['size']) && typeof k['size']['kind'] === 'string' ? k['size']['kind'] : '';
    const prefix = sizeKind.includes('spherical diameter') ? 'S⌀'
      : sizeKind.includes('diameter') ? '⌀'
        : sizeKind.includes('spherical radius') ? 'SR'
          : sizeKind.includes('radius') ? 'R' : '';
    const suffix = nominal?.angle ? '°' : '';
    const nomText = nominal ? `${prefix}${fmt(nominal.value)}${suffix}` : notationOf(id) ?? '';
    if (!nomText) continue;
    if (tol === 'basic') {
      out.set(id, { kind: 'datum', cells: [nomText], basic: true });
      continue;
    }
    if (isObject(tol) && isObject(tol['deviations'])) {
      const upper = readQuantityNumber(tol['deviations']['upper']);
      const lower = readQuantityNumber(tol['deviations']['lower']);
      if (upper && lower) {
        const sfx = upper.angle ? '°' : '';
        const tolerance: LabelTolerance = Math.abs(upper.value + lower.value) < 1e-12
          ? { style: 'symmetric', upper: `±${fmt(Math.abs(upper.value))}${sfx}` }
          : { style: 'pair', upper: fmtDeviation(upper.value, sfx), lower: fmtDeviation(lower.value, sfx) };
        out.set(id, { kind: 'size', cells: [nomText], tolerance });
        continue;
      }
    }
    if (isObject(tol) && isObject(tol['limits'])) {
      const upper = readQuantityNumber(tol['limits']['upper']);
      const lower = readQuantityNumber(tol['limits']['lower']);
      if (upper && lower) {
        const sfx = upper.angle ? '°' : '';
        out.set(id, { kind: 'size', cells: [`${prefix}${fmt(lower.value)}${sfx} - ${fmt(upper.value)}${sfx}`] });
        continue;
      }
    }
    if (isObject(tol) && typeof tol['class'] === 'string' && tol['class'].trim()) {
      out.set(id, { kind: 'size', cells: [`${nomText} ${tol['class'].trim()}`] });
      continue;
    }
    out.set(id, { kind: 'size', cells: [nomText] });
  }

  for (const t of arr('threads')) {
    const id = t['id'];
    const val = t['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const note = notationOf(id)
      ?? [typeof val['qualifier'] === 'string' ? val['qualifier'].replace('x', '×') : '', typeof val['fit_class'] === 'string' ? val['fit_class'] : '']
        .filter(Boolean).join('-');
    if (note) out.set(id, { kind: 'note', cells: [note] });
  }
  for (const k of arr('knurls')) {
    const id = k['id'];
    if (typeof id !== 'string') continue;
    const note = notationOf(id) ?? 'Knurl';
    out.set(id, { kind: 'note', cells: [note] });
  }
  for (const s of arr('surface_textures')) {
    const id = s['id'];
    if (typeof id !== 'string') continue;
    const note = notationOf(id);
    if (note) out.set(id, { kind: 'note', cells: [note] });
  }
  for (const a of arr('attributes')) {
    const id = a['id'];
    const val = a['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const texts = (Array.isArray(val['items']) ? val['items'] : [])
      .map((entry) => (Array.isArray(entry) && isObject(entry[1]) && typeof entry[1]['text'] === 'string' ? entry[1]['text'].trim() : ''))
      .filter(Boolean);
    const note = notationOf(id) ?? texts.join(' ');
    if (note) out.set(id, { kind: 'note', cells: [note] });
  }
  for (const n of arr('notes')) {
    const id = n['id'];
    const val = n['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const note = notationOf(id) ?? (typeof val['text'] === 'string' ? val['text'].trim() : '');
    if (note) out.set(id, { kind: 'note', cells: [note] });
  }
  return out;
}
