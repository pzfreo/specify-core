// A STEP's graphical PMI as haecceity draws it: `haecceity.presentation/1`, read and checked in
// the parts the Specify view uses, and refused with a reason otherwise. Pure, no three.js or DOM.
//
// What is drawn comes from haecceity as it is: polylines, filled triangles and text, never
// notation formatted here. It comes with Specify's pmiScene (specify-pmi 0.1.5): the presentation,
// the haecceity.pmi/1 document whose features give each item's faces, and Specify's overlay, which
// gives each item's status and question.

import type { PartOrigin } from '../types/pmi.ts';
import { CHARACTERISTIC, fmt, type LabelGroup, type LabelTolerance, type PmiLabel } from './labels.ts';

export type { PartOrigin };

export type DrawnPoint = readonly [number, number, number];

/**
 * Where a text is set, as the file or haecceity states it: its alignment point, its plane's x
 * axis and normal (the text's up is normal × x axis), and its box height in mm when stated.
 */
export interface TextPlacement {
  readonly textIndex?: number;
  readonly origin: DrawnPoint;
  readonly xAxis: DrawnPoint;
  readonly normal: DrawnPoint;
  readonly height?: number;
  /** As stated, such as "baseline left" or "centre". */
  readonly alignment?: string;
  /**
   * The cell the text is set in, as haecceity lays it out (0.4.4): its corners [min, max] in the
   * plane's x and up, in mm from ``origin``, such as a feature control frame's compartment.
   */
  readonly cellBox?: readonly [readonly [number, number], readonly [number, number]];
  /** Whether haecceity also strokes this text into the graphic's polylines (0.4.4). */
  readonly stroked?: boolean;
}

/** What a polyline draws, as haecceity says (0.4.4): "text" (its text's strokes), "frame", "leader", "extension", "dimension" or "symbol". */
export type PolylineRole = string;

/** One drawn graphic: model-space millimetres. */
export interface DrawnGraphic {
  /** One graphic listed under several items (a datum's symbol under the datum and its feature) has one id. */
  readonly id?: string;
  readonly source: 'generated' | 'file';
  readonly name: string;
  readonly polylines: readonly (readonly DrawnPoint[])[];
  /** Each polyline's role, parallel to ``polylines``, when haecceity states them (0.4.4). */
  readonly polylineRoles?: readonly PolylineRole[];
  readonly triangles: readonly (readonly [DrawnPoint, DrawnPoint, DrawnPoint])[];
  readonly text: readonly string[];
  readonly textPlacements?: readonly TextPlacement[];
  /** The annotation plane it is drawn on: an id in the presentation's ``planes``. */
  readonly planeId?: string;
  /**
   * Room the layout kept beside this callout for its inspection balloon, whatever the balloon says:
   * the balloon touches ``edge`` and extends along ``dir`` (an in-plane axis), ``height`` the text's.
   */
  readonly balloonSlot?: BalloonSlot;
}

export interface BalloonSlot {
  readonly edge: DrawnPoint;
  readonly dir: DrawnPoint;
  readonly xAxis: DrawnPoint;
  readonly normal: DrawnPoint;
  readonly height: number;
}

/** A balloon slot's length along x, and across (its height), in text heights: room for a range such as "3-5". */
export const BALLOON_SLOT_W = 3.4;
export const BALLOON_SLOT_H = 2;

/** An annotation plane as the file or haecceity states it: the in-plane y axis is normal × x axis. */
export interface DrawnPlane {
  readonly id: string;
  readonly name: string;
  readonly origin: DrawnPoint;
  readonly xAxis: DrawnPoint;
  readonly normal: DrawnPoint;
}

/** A saved view: its camera's direction (into the scene) and up when the file has a camera, and its graphics by id. */
export interface SavedView {
  readonly id: string;
  readonly name: string;
  readonly direction?: DrawnPoint;
  readonly up?: DrawnPoint;
  readonly graphics: readonly string[];
}

export interface Presentation {
  /** Graphics by the semantic item they present. */
  readonly items: Readonly<Record<string, readonly DrawnGraphic[]>>;
  /** Graphics linked to no semantic item: drawn, not selectable. */
  readonly unlinked: readonly DrawnGraphic[];
  readonly semanticIds: readonly string[];
  readonly planes: readonly DrawnPlane[];
  readonly views: readonly SavedView[];
}

/**
 * A scene item's status (ADR P11, D3): the feature table's state of the requirement it belongs
 * to. The file states it, it is planned for an open gap (a preview), or it was decided none.
 * Who wrote an item is never shown.
 */
export type DrawnStatus = 'stated' | 'gap' | 'decided_none';

/**
 * What a semantic item is on, in the session's face numbering (`snapshot().mesh.faces`), and,
 * from Specify's overlay, its status, whether the general tolerance governs it, and its question
 * if Specify asks one.
 */
export interface DrawnLink {
  readonly faces: readonly number[];
  readonly question?: string;
  readonly status?: DrawnStatus;
  readonly general?: true;
  /** Whether this item is one of the part's note statements shown by the notes panel. */
  readonly partNote?: true;
  /**
   * Its characteristic number as the engine allocates it: the file's own, or for an unsaved item
   * the number Save will write (the writer's allocation; consumers never allocate their own).
   */
  readonly number?: number;
}

/**
 * The label group an item is drawn and shown as: a general dimension's, else its status's (the
 * file's PMI when stated, Specify's own when planned for a gap, a default when decided none).
 * None for an item Specify has no status for.
 */
export function drawnGroup(link: Pick<DrawnLink, 'status' | 'general'> | undefined): LabelGroup | undefined {
  if (link?.status === undefined) return undefined;
  if (link.general) return 'general';
  return ({ stated: 'existing', gap: 'answered', decided_none: 'default' } as const)[link.status];
}

/** Structured label contents parsed directly from `haecceity.pmi/1` in a PMI scene. */
export interface SceneSemanticLabel {
  readonly kind: PmiLabel['kind'];
  readonly cells: readonly string[];
  readonly tolerance?: LabelTolerance | null;
  readonly basic?: true;
}

/** Linear/basic location dimension metadata extracted from `haecceity.pmi/1` + `specify` for 3D drafting layout. */
export interface SceneDimMeta {
  readonly item: string;
  readonly nominal: number;
  readonly text: string;
  readonly basic: boolean;
  readonly fromFaces: readonly number[];
  readonly toFaces: readonly number[];
  readonly axisHint?: 0 | 1 | 2;
}

/** Lightweight semantic identity extracted from `haecceity.pmi/1` + `specify` so `oneViewFile` can link drawn ink to review rows without requiring a separate scene reader. */
export interface SceneItemMeta {
  readonly id: string;
  readonly kind: string;
  readonly type: string;
  readonly faces: readonly number[];
  readonly number?: number;
  readonly notation?: string;
  readonly hasValue: boolean;
}

/** What the Specify view draws as the file's own graphics: pmiScene's presentation and links. */
export interface DrawnPmi {
  readonly presentation: Presentation;
  readonly links: Readonly<Record<string, DrawnLink>>;
  readonly reviewItems?: Readonly<Record<string, { readonly row?: string; readonly existing?: number }>>;
  readonly sceneItems?: readonly SceneItemMeta[];
  /** Structured semantic labels parsed from `haecceity.pmi/1` + `specify`, used for 3D drafting layout. */
  readonly sceneSemantic?: Readonly<Record<string, SceneSemanticLabel>>;
  /** Linear/basic location dimension metadata parsed from `haecceity.pmi/1` + `specify`, used for 3D drafting layout. */
  readonly sceneDimensions?: Readonly<Record<string, SceneDimMeta>>;
  /** Explicit coordinate system or isolated origin point stated in the STEP file, when provided on the scene. */
  readonly stepOrigin?: PartOrigin;
  /** True once Specify-created graphics have been laid out on 3D drafting annotation planes. */
  readonly draftingNormalized?: true;
}

export type Read<T> = { ok: true; value: T } | { ok: false; reason: string };

const SCHEMA = 'haecceity.presentation/1';
const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

class Refused extends Error {}
const refuse = (reason: string): never => { throw new Refused(reason); };

function point(v: unknown, at: string): DrawnPoint {
  if (!Array.isArray(v) || v.length !== 3 || !v.every((n) => typeof n === 'number' && Number.isFinite(n))) {
    return refuse(`${at} is not a point [x, y, z]`);
  }
  return [v[0], v[1], v[2]];
}

function list(v: unknown, at: string): unknown[] {
  return Array.isArray(v) ? v : refuse(`${at} is not an array`);
}

function graphic(v: unknown, at: string): DrawnGraphic {
  if (!isObject(v)) return refuse(`${at} is not a graphic`);
  const { source, name } = v;
  if (source !== 'file' && source !== 'generated') return refuse(`${at}.source is ${JSON.stringify(source)}, not "file" or "generated"`);
  if (typeof name !== 'string') return refuse(`${at}.name is not a string`);
  const polylines = list(v['polylines'], `${at}.polylines`)
    .map((line, i) => list(line, `${at}.polylines[${i}]`).map((p, j) => point(p, `${at}.polylines[${i}][${j}]`)));
  const triangles = list(v['triangles'], `${at}.triangles`).map((t, i) => {
    const corners = list(t, `${at}.triangles[${i}]`);
    if (corners.length !== 3) refuse(`${at}.triangles[${i}] does not have 3 corners`);
    return [point(corners[0], `${at}.triangles[${i}][0]`), point(corners[1], `${at}.triangles[${i}][1]`), point(corners[2], `${at}.triangles[${i}][2]`)] as const;
  });
  const text = list(v['text'], `${at}.text`).map((s, i) => (typeof s === 'string' ? s : refuse(`${at}.text[${i}] is not a string`)));
  const id = v['id'];
  if (id !== undefined && typeof id !== 'string') refuse(`${at}.id is not a string`);
  const placements = v['text_placements'] === undefined ? undefined
    : list(v['text_placements'], `${at}.text_placements`).map((t, i) => placement(t, `${at}.text_placements[${i}]`));
  const planeId = v['plane_id'];
  if (planeId !== undefined && typeof planeId !== 'string') refuse(`${at}.plane_id is not a string`);
  const rawRoles = v['polyline_roles'];
  const roles = rawRoles === undefined ? undefined
    : list(rawRoles, `${at}.polyline_roles`).map((r, i) => (typeof r === 'string' ? r : refuse(`${at}.polyline_roles[${i}] is not a string`)));
  if (roles && roles.length !== polylines.length) refuse(`${at}.polyline_roles has ${roles.length} roles for ${polylines.length} polylines`);
  return {
    ...(typeof id === 'string' ? { id } : {}), source, name, polylines, ...(roles ? { polylineRoles: roles } : {}), triangles, text,
    ...(placements ? { textPlacements: placements } : {}), ...(typeof planeId === 'string' ? { planeId } : {}),
  };
}

function plane(v: unknown, at: string): DrawnPlane {
  if (!isObject(v)) return refuse(`${at} is not a plane`);
  const { id, name } = v;
  if (typeof id !== 'string') return refuse(`${at}.id is not a string`);
  return {
    id, name: typeof name === 'string' ? name : id,
    origin: point(v['origin'], `${at}.origin`), xAxis: point(v['x_axis'], `${at}.x_axis`), normal: point(v['normal'], `${at}.normal`),
  };
}

function savedView(v: unknown, at: string): SavedView {
  if (!isObject(v)) return refuse(`${at} is not a view`);
  const { id, name } = v;
  if (typeof id !== 'string') return refuse(`${at}.id is not a string`);
  const graphics = list(v['graphics'] ?? [], `${at}.graphics`).map((g, i) => (typeof g === 'string' ? g : refuse(`${at}.graphics[${i}] is not a string`)));
  return {
    id, name: typeof name === 'string' && name ? name : id, graphics,
    ...(v['direction'] == null ? {} : { direction: point(v['direction'], `${at}.direction`) }),
    ...(v['up'] == null ? {} : { up: point(v['up'], `${at}.up`) }),
  };
}

function placement(v: unknown, at: string): TextPlacement {
  if (!isObject(v)) return refuse(`${at} is not a text placement`);
  const { text_index: textIndex, height, alignment } = v;
  if (textIndex !== undefined && !(Number.isInteger(textIndex) && (textIndex as number) >= 0)) refuse(`${at}.text_index is not an index`);
  if (height !== undefined && !(typeof height === 'number' && Number.isFinite(height) && height >= 0)) refuse(`${at}.height is not a length`);
  if (alignment !== undefined && typeof alignment !== 'string') refuse(`${at}.alignment is not a string`);
  const { cell_box: box, stroked } = v;
  const pair = (c: unknown, where: string): [number, number] => {
    const xy = list(c, where);
    if (xy.length !== 2 || !xy.every((n) => typeof n === 'number' && Number.isFinite(n))) refuse(`${where} is not a point [x, y]`);
    return [xy[0] as number, xy[1] as number];
  };
  let cellBox: TextPlacement['cellBox'];
  if (box !== undefined && box !== null) {
    const corners = list(box, `${at}.cell_box`);
    if (corners.length !== 2) refuse(`${at}.cell_box does not have 2 corners`);
    cellBox = [pair(corners[0], `${at}.cell_box[0]`), pair(corners[1], `${at}.cell_box[1]`)];
  }
  if (stroked !== undefined && typeof stroked !== 'boolean') refuse(`${at}.stroked is not a boolean`);
  return {
    ...(textIndex === undefined ? {} : { textIndex: textIndex as number }),
    origin: point(v['origin'], `${at}.origin`), xAxis: point(v['x_axis'], `${at}.x_axis`), normal: point(v['normal'], `${at}.normal`),
    ...(height === undefined ? {} : { height: height as number }),
    ...(alignment === undefined ? {} : { alignment: alignment as string }),
    ...(cellBox ? { cellBox } : {}), ...(stroked === undefined ? {} : { stroked: stroked as boolean }),
  };
}

/** `haecceity.presentation/1`, checked where the view relies on it; anything else is refused with why. */
export function readPresentation(json: unknown): Read<Presentation> {
  try {
    if (!isObject(json)) return refuse('not a JSON object');
    if (json['schema'] !== SCHEMA) return refuse(`schema is ${JSON.stringify(json['schema'] ?? null)}, not "${SCHEMA}"`);
    if (json['length_unit'] !== 'millimetre') return refuse(`length_unit is ${JSON.stringify(json['length_unit'] ?? null)}, not "millimetre"`);
    const rawItems = json['items'];
    if (!isObject(rawItems)) return refuse('items is not an object');
    const items: Record<string, readonly DrawnGraphic[]> = {};
    for (const [id, graphics] of Object.entries(rawItems)) {
      items[id] = list(graphics, `items[${JSON.stringify(id)}]`).map((g, i) => graphic(g, `items[${JSON.stringify(id)}][${i}]`));
    }
    const unlinked = list(json['unlinked'], 'unlinked').map((g, i) => graphic(g, `unlinked[${i}]`));
    const semanticIds = list(json['semantic_ids'], 'semantic_ids')
      .map((s, i) => (typeof s === 'string' ? s : refuse(`semantic_ids[${i}] is not a string`)));
    const planes = list(json['planes'] ?? [], 'planes').map((p, i) => plane(p, `planes[${i}]`));
    const views = list(json['views'] ?? [], 'views').map((p, i) => savedView(p, `views[${i}]`));
    return { ok: true, value: { items, unlinked, semanticIds, planes, views } };
  } catch (error) {
    if (error instanceof Refused) return { ok: false, reason: error.message };
    throw error;
  }
}

const PART_NOTE_ATTRIBUTES = new Set(['surface treatment', 'heat treatment', 'edge condition', 'general tolerance']);
const PART_NOTE_KINDS = new Set(['surface treatment', 'heat treatment', 'edge condition']);

function isPanelPartNoteEntry(id: string, value: unknown): boolean {
  if (!isObject(value) || value['on'] != null) return false;
  if (id.startsWith('attributes:')) {
    if (value['name'] !== 'semantic text' || !Array.isArray(value['items']) || !value['items'].length) return false;
    return value['items'].every((entry) =>
      Array.isArray(entry) && typeof entry[0] === 'string' && PART_NOTE_ATTRIBUTES.has(entry[0])
      && isObject(entry[1]) && typeof entry[1]['text'] === 'string');
  }
  if (id.startsWith('notes:')) {
    return typeof value['kind'] === 'string' && PART_NOTE_KINDS.has(value['kind']);
  }
  if (id.startsWith('surface_textures:')) {
    return Array.isArray(value['parameters']) && value['parameters'].length > 0;
  }
  return false;
}

/**
 * Each presented item's faces, from the `haecceity.pmi/1` document the presentation was made with:
 * its features' (`features[].value.items[].face`, the session's numbering). Refused unless the
 * document's ids are the presentation's `semantic_ids`.
 */
export function linksFromPmi(json: unknown, presentation: Presentation): Read<Record<string, DrawnLink>> {
  if (!isObject(json) || json['schema'] !== 'haecceity.pmi/1' || !isObject(json['pmi'])) {
    return { ok: false, reason: 'not a haecceity.pmi/1 document' };
  }
  // Every identified item: the document's collections and its units.
  const collections = [...Object.values(json['pmi']), ...(isObject(json['units']) ? Object.values(json['units']) : [])];
  const entries = collections.flatMap((v) => (Array.isArray(v) ? v : []))
    .filter((e): e is { id: string; value: unknown } => isObject(e) && typeof e['id'] === 'string');
  const faces = new Map<string, number[]>();
  for (const e of entries) {
    if (!e.id.startsWith('features:') || !isObject(e.value)) continue;
    const on = Array.isArray(e.value['items']) ? e.value['items'] : [];
    faces.set(e.id, on.flatMap((x) => (isObject(x) && typeof x['face'] === 'number' ? [x['face']] : [])));
  }
  // The presentation's links hold only for the document it was made with: the same ids.
  const ids = [...new Set(entries.map((e) => e.id))].sort();
  const expected = [...presentation.semanticIds].sort();
  if (ids.join('\n') !== expected.join('\n')) return { ok: false, reason: 'the PMI document is not the one the presentation was made with (semantic_ids differ)' };
  // An item's faces: those of every feature it names, however deep.
  const named = (v: unknown, into: Set<number>): Set<number> => {
    if (typeof v === 'string') for (const f of faces.get(v) ?? []) into.add(f);
    else if (Array.isArray(v)) for (const x of v) named(x, into);
    else if (isObject(v)) for (const x of Object.values(v)) named(x, into);
    return into;
  };
  const links: Record<string, DrawnLink> = {};
  for (const e of entries) {
    if (!(e.id in presentation.items)) continue;
    const partNote = isPanelPartNoteEntry(e.id, e.value);
    links[e.id] = {
      faces: [...named(e.id.startsWith('features:') ? e.id : e.value, new Set())].sort((a, b) => a - b),
      ...(partNote ? { partNote: true as const } : {}),
    };
  }
  return { ok: true, value: links };
}

const STATUSES: readonly DrawnStatus[] = ['stated', 'gap', 'decided_none'];

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

function extractSceneItems(scene: Record<string, unknown>): {
  sceneItems: SceneItemMeta[];
  resolvedFaces: Map<string, number[]>;
} {
  const sceneItems: SceneItemMeta[] = [];
  const resolvedFaces = new Map<string, number[]>();
  if (!isObject(scene['pmi']) || !isObject(scene['pmi']['pmi'])) {
    return { sceneItems, resolvedFaces };
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
    const number = numOf(d, id);
    const notation = notationOf(id);
    sceneItems.push({
      id, kind: 'datum', type: label, faces: sorted, hasValue: false,
      ...(number !== undefined ? { number } : {}), ...(notation !== undefined ? { notation } : {}),
    });
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
    const number = numOf(d, id);
    const notation = notationOf(id);
    sceneItems.push({
      id, kind: 'dimension', type, faces, hasValue: val['nominal'] != null,
      ...(number !== undefined ? { number } : {}), ...(notation !== undefined ? { notation } : {}),
    });
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
    const number = numOf(t, id);
    const notation = notationOf(id);
    sceneItems.push({
      id, kind: 'geometric_tolerance', type, faces, hasValue: val['magnitude'] != null,
      ...(number !== undefined ? { number } : {}), ...(notation !== undefined ? { notation } : {}),
    });
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
    const number = numOf(s, id);
    const notation = notationOf(id);
    sceneItems.push({
      id, kind: 'surface_texture', type: params, faces, hasValue: true,
      ...(number !== undefined ? { number } : {}), ...(notation !== undefined ? { notation } : {}),
    });
  }
  for (const t of arr('threads')) {
    const id = t['id'];
    const val = t['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const type = val['side'] === 'external' ? 'External' : 'Internal';
    const faces = facesOf(val['feature']);
    resolvedFaces.set(id, faces);
    const number = numOf(t, id);
    const notation = notationOf(id);
    sceneItems.push({
      id, kind: 'thread', type, faces, hasValue: true,
      ...(number !== undefined ? { number } : {}), ...(notation !== undefined ? { notation } : {}),
    });
  }
  for (const k of arr('knurls')) {
    const id = k['id'];
    const val = k['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const faces = facesOf(val['feature']);
    resolvedFaces.set(id, faces);
    const number = numOf(k, id);
    const notation = notationOf(id);
    sceneItems.push({
      id, kind: 'knurl', type: 'knurl', faces, hasValue: true,
      ...(number !== undefined ? { number } : {}), ...(notation !== undefined ? { notation } : {}),
    });
  }
  return { sceneItems, resolvedFaces };
}

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

function readFitClass(raw: unknown): string | null {
  if (typeof raw === 'string' && raw.trim()) return raw.trim();
  if (!isObject(raw)) return null;
  const dev = raw['deviation'];
  const grade = raw['grade'];
  if (!isObject(dev) || typeof dev['letter'] !== 'string' || !dev['letter'].trim()) return null;
  if (typeof grade !== 'string' && typeof grade !== 'number') return null;
  const g = String(grade).trim();
  if (!g) return null;
  const letter = dev['of'] === 'hole' ? dev['letter'].trim().toUpperCase() : dev['letter'].trim().toLowerCase();
  return `${letter}${g}`;
}

function decorateDimensionText(val: Record<string, unknown>, base: string): string {
  let out = base;
  const q = val['qualifier'];
  if (q === 'maximum') out += ' MAX';
  else if (q === 'minimum') out += ' MIN';
  else if (q === 'average') out += ' AVG';
  else if (isObject(q) && typeof q['other'] === 'string' && q['other'].trim()) out += ` ${q['other'].trim()}`;
  if (val['principle'] === 'envelope requirement') out += ' Ⓔ';
  const mods = Array.isArray(val['modifiers']) ? val['modifiers'] : [];
  if (mods.includes('reference') && !(out.startsWith('(') && out.endsWith(')'))) out = `(${out})`;
  return out;
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
        const text = decorateDimensionText(val, `${prefix}${fmt(lower.value)}${sfx} - ${prefix}${fmt(upper.value)}${sfx}`);
        out.set(id, { kind: 'size', cells: [text] });
        continue;
      }
    }
    if (isObject(tol) && isObject(tol['fit'])) {
      const fit = tol['fit'];
      const fitClass = readFitClass(fit['class']);
      if (isObject(fit['limits'])) {
        const upper = readQuantityNumber(fit['limits']['upper']);
        const lower = readQuantityNumber(fit['limits']['lower']);
        if (upper && lower) {
          const sfx = upper.angle ? '°' : '';
          const limitsText = `${prefix}${fmt(lower.value)}${sfx} - ${prefix}${fmt(upper.value)}${sfx}`;
          const text = decorateDimensionText(val, fitClass ? `${limitsText} (${fitClass})` : limitsText);
          out.set(id, { kind: 'size', cells: [text] });
          continue;
        }
      }
      if (fitClass) {
        out.set(id, { kind: 'size', cells: [decorateDimensionText(val, `${nomText} ${fitClass}`)] });
        continue;
      }
    }
    if (isObject(tol)) {
      const fitClass = readFitClass(tol['class']);
      if (fitClass) {
        out.set(id, { kind: 'size', cells: [decorateDimensionText(val, `${nomText} ${fitClass}`)] });
        continue;
      }
    }
    out.set(id, { kind: 'size', cells: [decorateDimensionText(val, nomText)] });
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

/** Extracts linear/basic location dimension metadata from a raw `PmiScene` for 3D drafting layout. */
export function extractSceneDimensions(rawScene: unknown): Record<string, SceneDimMeta> {
  const out: Record<string, SceneDimMeta> = {};
  if (!isObject(rawScene) || !isObject(rawScene['pmi']) || !isObject(rawScene['pmi']['pmi'])) return out;
  const pmi = rawScene['pmi']['pmi'] as Record<string, unknown>;
  const specify = isObject(rawScene['specify']) ? rawScene['specify'] as Record<string, unknown> : {};
  const arr = (k: string) => (Array.isArray(pmi[k]) ? pmi[k] as unknown[] : []).filter(isObject);

  const featureFaces = new Map<string, number[]>();
  const featureById = new Map<string, Record<string, unknown>>();
  for (const f of arr('features')) {
    if (typeof f['id'] === 'string' && isObject(f['value'])) featureById.set(f['id'], f['value']);
  }
  const facesOf = (id: unknown, seen = new Set<string>()): number[] => {
    if (typeof id !== 'string' || seen.has(id)) return [];
    const hit = featureFaces.get(id);
    if (hit) return hit;
    seen.add(id);
    const v = featureById.get(id);
    if (!v) return [];
    const s = new Set<number>();
    if (Array.isArray(v['items'])) {
      for (const x of v['items']) if (isObject(x) && typeof x['face'] === 'number') s.add(x['face']);
    }
    if (isObject(v['group']) && Array.isArray(v['group']['members'])) {
      for (const m of v['group']['members']) for (const f of facesOf(m, seen)) s.add(f);
    }
    if (isObject(v['derived']) && Array.isArray(v['derived']['from'])) {
      for (const m of v['derived']['from']) for (const f of facesOf(m, seen)) s.add(f);
    }
    const sorted = [...s].sort((a, b) => a - b);
    featureFaces.set(id, sorted);
    return sorted;
  };

  for (const d of arr('dimensions')) {
    const id = d['id'];
    const val = d['value'];
    if (typeof id !== 'string' || !isObject(val)) continue;
    const kind = isObject(val['kind']) ? val['kind'] : null;
    const loc = kind && isObject(kind['location']) ? kind['location'] : null;
    if (!loc) continue;
    const nomObj = isObject(val['nominal']) ? val['nominal'] : null;
    const qty = nomObj && isObject(nomObj['quantity']) ? nomObj['quantity'] : nomObj;
    const lenObj = qty && isObject(qty['length']) ? qty['length'] : null;
    const rawVal = lenObj?.['value'];
    const nominal = typeof rawVal === 'number' || typeof rawVal === 'string' ? Number(rawVal) : NaN;
    if (!Number.isFinite(nominal)) continue;
    const spec = isObject(specify[id]) ? specify[id] as Record<string, unknown> : undefined;
    const text = typeof spec?.['notation'] === 'string' && spec['notation'].trim()
      ? spec['notation'].trim()
      : String(Number(nominal.toFixed(4)));
    const source = isObject(spec?.['source']) ? spec['source'] as Record<string, unknown> : undefined;
    const param = typeof source?.['parameter'] === 'string' ? source['parameter'] : '';
    const axisHint: 0 | 1 | 2 | undefined = param.endsWith('.x') ? 0 : param.endsWith('.y') ? 1 : param.endsWith('.z') ? 2 : undefined;
    out[id] = {
      item: id,
      nominal,
      text,
      basic: val['tolerance'] === 'basic' || spec?.['basic'] === true,
      fromFaces: facesOf(loc['from']),
      toFaces: facesOf(loc['to']),
      ...(axisHint !== undefined ? { axisHint } : {}),
    };
  }
  return out;
}

const ORIGIN_SOURCES = new Set<PartOrigin['source']>(['step-csys', 'step-point', 'datum-frame']);

function readStepOrigin(raw: unknown): PartOrigin | undefined {
  if (!isObject(raw)) return undefined;
  const source = raw['source'];
  if (typeof source !== 'string' || !ORIGIN_SOURCES.has(source as PartOrigin['source'])) return undefined;
  const pt = (v: unknown): DrawnPoint | null =>
    (Array.isArray(v) && v.length === 3 && v.every((n) => typeof n === 'number' && Number.isFinite(n))
      ? [v[0], v[1], v[2]] : null);
  const position = pt(raw['position']);
  const xAxis = pt(raw['xAxis']);
  const yAxis = pt(raw['yAxis']);
  const zAxis = pt(raw['zAxis']);
  if (!position || !xAxis || !yAxis || !zAxis) return undefined;
  return { position, xAxis, yAxis, zAxis, source: source as PartOrigin['source'] };
}

/**
 * Specify's pmiScene (`{pmi, presentation, specify}`, specify-pmi 0.1.5) as the view draws it:
 * the presentation, and each presented item's faces with its status and question from `specify`.
 */
export function drawnFromScene(scene: unknown): Read<DrawnPmi> {
  if (!isObject(scene)) return { ok: false, reason: 'the PMI scene is not a JSON object' };
  const read = readPresentation(scene['presentation']);
  if (!read.ok) return read;
  const links = linksFromPmi(scene['pmi'], read.value);
  if (!links.ok) return links;
  const { sceneItems, resolvedFaces } = extractSceneItems(scene);
  const semantic = readSceneSemanticLabels(scene);
  const sceneDimensions = extractSceneDimensions(scene);
  const stepOrigin = readStepOrigin(scene['step_origin']);
  const overlay = isObject(scene['specify']) ? scene['specify'] : {};
  const linked: Record<string, DrawnLink> = {};
  for (const [id, link] of Object.entries(links.value)) {
    const entry = overlay[id];
    const status = isObject(entry) && STATUSES.includes(entry['status'] as DrawnStatus) ? entry['status'] as DrawnStatus : undefined;
    const question = isObject(entry) && typeof entry['question'] === 'string' ? entry['question'] : undefined;
    const general = isObject(entry) && entry['general'] === true;
    const number = isObject(entry) && typeof entry['number'] === 'number' && entry['number'] > 0 ? entry['number'] : undefined;
    const faces = link.faces.length ? link.faces : resolvedFaces.get(id) ?? [];
    linked[id] = { ...link, faces, ...(question ? { question } : {}), ...(status ? { status } : {}), ...(general ? { general } : {}), ...(number ? { number } : {}) };
  }
  const serverReview = isObject(scene['review_items'])
    ? scene['review_items'] as DrawnPmi['reviewItems']
    : undefined;
  return {
    ok: true,
    value: {
      presentation: read.value,
      links: linked,
      ...(serverReview ? { reviewItems: serverReview } : {}),
      ...(sceneItems.length ? { sceneItems } : {}),
      ...(semantic.size ? { sceneSemantic: Object.fromEntries(semantic) } : {}),
      ...(Object.keys(sceneDimensions).length ? { sceneDimensions } : {}),
      ...(stepOrigin ? { stepOrigin } : {}),
    },
  };
}

/** Whether there is anything to draw. */
export const hasGraphics = (pmi: DrawnPmi): boolean =>
  pmi.presentation.unlinked.length > 0 || Object.values(pmi.presentation.items).some((graphics) => graphics.length > 0);

/** Whether any graphic is the file's own (``source: "file"``), not one Specify generated. */
export const hasFileGraphics = (pmi: DrawnPmi): boolean =>
  [...pmi.presentation.unlinked, ...Object.values(pmi.presentation.items).flat()].some((g) => g.source === 'file');

/** Specify's general-notes block: in the 3D view a fixed panel shows the part's notes instead. */
export const NOTES_BLOCK = 'generated:general-notes';

/**
 * Whether a semantic item is one of the part's notes that the notes panel already says: a `part.*`
 * setting (by id or question), a `general:*` tolerance, or one of the recognized part-level note
 * entries (`partNote`) that the notes panel displays. Unrecognized attributes or notes stay drawn.
 */
const isPartNote = (item: string, links: Readonly<Record<string, DrawnLink>>): boolean =>
  item.startsWith('part.')
  || item.startsWith('general:')
  || Boolean(links[item]?.question?.startsWith('part.'))
  || links[item]?.partNote === true;

/**
 * The drawn PMI without Specify's general-notes block, and without the view it generates for it:
 * notes on a plane in 3D turn away as the part turns, and the panel already says them.
 */
export function withoutNotesBlock(pmi: DrawnPmi): DrawnPmi {
  const { presentation } = pmi;
  // Saved, the block is one of the file's own graphics, with an id of the file's: the one of that name that
  // draws the part's notes and nothing else. Notes the panel does not say (no item of the part's, or a
  // graphic that also draws a datum, dimension, or feature note) stay drawn.
  const draws = new Map<string, boolean>();
  for (const [item, graphics] of Object.entries(presentation.items)) {
    const note = isPartNote(item, pmi.links);
    for (const g of graphics) if (g.id !== undefined) draws.set(g.id, (draws.get(g.id) ?? true) && note);
  }
  const block = (g: DrawnGraphic) => g.id === NOTES_BLOCK
    || (g.source === 'file' && /^general notes$/i.test(g.name) && g.id !== undefined && draws.get(g.id) === true);
  const blocks = new Set([NOTES_BLOCK, ...Object.values(presentation.items).flat().filter(block).map((g) => g.id)]);
  const keep = (g: DrawnGraphic) => !blocks.has(g.id);
  return {
    ...pmi,
    presentation: {
      ...presentation,
      items: Object.fromEntries(Object.entries(presentation.items).map(([item, gs]) => [item, gs.filter(keep)])),
      unlinked: presentation.unlinked.filter(keep),
      views: presentation.views.filter((v) => !(v.graphics.length && v.graphics.every((id) => blocks.has(id)))),
    },
  };
}

/** What the 3D view's show toggles say of the drawn PMI, as they say of the labels: which kinds are shown. */
export interface DrawnShow {
  /** "Suggested by Specify": items Specify defaulted. */
  readonly default: boolean;
  /** "In the file": the file's own graphics and items Specify read from it. */
  readonly existing: boolean;
  /** "General dimensions": what the general tolerance covers. */
  readonly general: boolean;
}

/**
 * Whether a graphic is drawn under the Show menu: as a label is, by its item's group
 * ([`drawnGroup`]), and a graphic the file draws counts as the file's PMI whatever its group.
 * Items planned for a gap are always drawn, as their labels are.
 */
export function drawnShown(source: DrawnGraphic['source'], group: LabelGroup | undefined, show: DrawnShow): boolean {
  if (group === 'default' && !show.default) return false;
  if (group === 'general' && !show.general) return false;
  if ((group === 'existing' || source === 'file') && !show.existing) return false;
  return true;
}

type Vec = [number, number, number];
const sub = (a: DrawnPoint, b: DrawnPoint): Vec => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: DrawnPoint, b: DrawnPoint) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: DrawnPoint, b: DrawnPoint): Vec => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const unit = (a: DrawnPoint): Vec | null => {
  const n = Math.hypot(a[0], a[1], a[2]);
  return n > 1e-12 ? [a[0] / n, a[1] / n, a[2] / n] : null;
};

/** A graphic's points: its strokes, its fills and where its text is set. */
export const graphicPoints = (g: DrawnGraphic): DrawnPoint[] => [
  ...g.polylines.flat(), ...g.triangles.flat(), ...(g.textPlacements ?? []).map((t) => t.origin),
];

/**
 * The normal of the plane that best fits ``points`` (least squares), with no particular sign;
 * null when they do not make a plane (too few, on a line, or spread in depth nearly as much as
 * across).
 */
export function bestFitNormal(points: readonly DrawnPoint[]): Vec | null {
  if (points.length < 3) return null;
  const c: Vec = [0, 0, 0];
  for (const p of points) for (let k = 0; k < 3; k++) c[k] += p[k]! / points.length;
  // The covariance, and its eigenvectors by Jacobi rotations: the normal is the least spread.
  const a: Vec[] = [[0, 0, 0], [0, 0, 0], [0, 0, 0]];
  for (const p of points) {
    const d = sub(p, c);
    for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) a[i]![j] += d[i]! * d[j]!;
  }
  const v: Vec[] = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
  for (let sweep = 0; sweep < 50; sweep++) {
    let off = 0;
    for (const [p, q] of [[0, 1], [0, 2], [1, 2]] as const) {
      const apq = a[p]![q]!;
      off += Math.abs(apq);
      if (Math.abs(apq) < 1e-300) continue;
      const theta = (a[q]![q]! - a[p]![p]!) / (2 * apq);
      const t = Math.sign(theta || 1) / (Math.abs(theta) + Math.sqrt(theta * theta + 1));
      const cos = 1 / Math.sqrt(t * t + 1);
      const sin = t * cos;
      for (const m of [a, v]) {
        for (let k = 0; k < 3; k++) {
          const [kp, kq] = [m[k]![p]!, m[k]![q]!];
          m[k]![p] = cos * kp - sin * kq;
          m[k]![q] = sin * kp + cos * kq;
        }
      }
      for (let k = 0; k < 3; k++) {
        const [pk, qk] = [a[p]![k]!, a[q]![k]!];
        a[p]![k] = cos * pk - sin * qk;
        a[q]![k] = sin * pk + cos * qk;
      }
    }
    if (off < 1e-12) break;
  }
  const values = [0, 1, 2].map((i) => a[i]![i]!);
  const order = [0, 1, 2].sort((i, j) => values[i]! - values[j]!);
  const [least, middle] = [values[order[0]!]!, values[order[1]!]!];
  // On a line (two spreads nil), or not flat (the least spread over a hundredth of the next).
  if (middle <= 1e-12 || least > 1e-2 * middle) return null;
  const k = order[0]!;
  return unit([v[0]![k]!, v[1]![k]!, v[2]![k]!]);
}

/**
 * The side a graphic is read from: its annotation plane's normal, else its placed text's, else
 * the plane its strokes lie in, turned away from ``centre`` (the part's), since a callout is read
 * from outside the part; null when that cannot be told.
 */
export function graphicNormal(g: DrawnGraphic, planes: readonly DrawnPlane[], centre?: DrawnPoint): Vec | null {
  const stated = planes.find((p) => p.id === g.planeId)?.normal ?? g.textPlacements?.[0]?.normal;
  if (stated) return unit(stated);
  const points = graphicPoints(g);
  const n = bestFitNormal(points);
  if (!n || !centre) return null;
  const mid = points.reduce<Vec>((m, p) => [m[0] + p[0] / points.length, m[1] + p[1] / points.length, m[2] + p[2] / points.length], [0, 0, 0]);
  const out = sub(mid, centre);
  const side = dot(out, n);
  // A plane through the centre has no outside.
  if (Math.abs(side) <= 1e-6 * Math.max(1, Math.hypot(...out))) return null;
  return side > 0 ? n : [-n[0], -n[1], -n[2]];
}

/** The middle of the box around a mesh's flat ``positions`` (x, y, z, x, …): the part's centre. */
export function partCentre(positions: readonly number[]): DrawnPoint | undefined {
  if (positions.length < 3) return undefined;
  const min: Vec = [Infinity, Infinity, Infinity];
  const max: Vec = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i + 2 < positions.length; i += 3) {
    for (let k = 0; k < 3; k++) { min[k] = Math.min(min[k]!, positions[i + k]!); max[k] = Math.max(max[k]!, positions[i + k]!); }
  }
  return [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2];
}

/** Whether a plane with ``normal`` is seen from behind by a camera looking along ``direction``. */
export const facesAway = (normal: DrawnPoint, direction: DrawnPoint): boolean => dot(normal, direction) > 1e-9;

/**
 * A way to look at the drawn PMI square on: a saved view's camera, or an annotation plane's
 * front. The camera stands at ``eye`` (a unit vector) from what it looks at, ``up`` its way up
 * when known, framing the box of the graphics it shows.
 */
export interface DrawnView {
  readonly key: string;
  readonly name: string;
  /** The graphics it shows, by name, one a line. */
  readonly title: string;
  readonly eye: DrawnPoint;
  readonly up?: DrawnPoint;
  readonly min: DrawnPoint;
  readonly max: DrawnPoint;
  /** Number of callouts on this view, when known. */
  readonly count?: number;
}

const rounded = (v: DrawnPoint) => v.map((x) => (Math.abs(x) < 5e-4 ? 0 : x).toFixed(3)).join(',');

/**
 * A graphic's in-plane basis when known: ``normal`` (facing the reader), ``xAxis`` (along the
 * text's reading direction) and ``up`` (``normal × xAxis``).
 */
export function graphicAxes(
  g: DrawnGraphic, planes: readonly DrawnPlane[], centre?: DrawnPoint,
): { normal: Vec; xAxis: Vec | null; up: Vec | null } | null {
  const normal = graphicNormal(g, planes, centre);
  if (!normal) return null;
  const rawX = planes.find((p) => p.id === g.planeId)?.xAxis ?? g.textPlacements?.[0]?.xAxis;
  const xAxis = rawX ? unit(rawX) : null;
  const up = xAxis ? unit(cross(normal, xAxis)) : null;
  return { normal, xAxis, up };
}

/**
 * The camera view at which the most shown callouts face the viewer and read upright, framing
 * all shown callouts. When ``preferred`` is provided (e.g. from the file's saved view or
 * drafting reference view), uses that exact camera framing all shown callouts.
 */
export function bestDrawnView(
  pmi: DrawnPmi,
  shown: (g: DrawnGraphic, item: string | null) => boolean,
  centre?: DrawnPoint,
  preferred?: { readonly eye: DrawnPoint; readonly up: DrawnPoint } | null,
): DrawnView | null {
  const { presentation } = pmi;
  const all = [
    ...Object.entries(presentation.items).flatMap(([item, gs]) => gs.map((g) => ({ g, item: item as string | null }))),
    ...presentation.unlinked.map((g) => ({ g, item: null })),
  ];
  const byId = new Map<string, DrawnGraphic>();
  const graphics: DrawnGraphic[] = [];
  for (const { g, item } of all) {
    if (!shown(g, item)) continue;
    if (g.id === undefined) graphics.push(g);
    else if (!byId.has(g.id)) { byId.set(g.id, g); graphics.push(g); }
  }
  const points = graphics.flatMap(graphicPoints);
  if (!points.length) return null;
  const min: Vec = [Infinity, Infinity, Infinity];
  const max: Vec = [-Infinity, -Infinity, -Infinity];
  for (const p of points) {
    for (let k = 0; k < 3; k++) { min[k] = Math.min(min[k]!, p[k]!); max[k] = Math.max(max[k]!, p[k]!); }
  }
  if (preferred) {
    const eye = unit(preferred.eye);
    const up = unit(preferred.up);
    if (eye && up) {
      return {
        key: 'best', name: 'Best view', title: 'Saved / best reference view',
        eye, up, min, max,
      };
    }
  }
  const oriented = graphics.flatMap((g) => {
    const axes = graphicAxes(g, presentation.planes, centre);
    return axes ? [axes] : [];
  });
  if (!oriented.length) return null;

  // Candidate eye directions: 26 cube directions, 24 3/4 isometric directions, each plane normal,
  // and the average plane normal.
  const rawEyes: DrawnPoint[] = [];
  for (const x of [-1, 0, 1]) for (const y of [-1, 0, 1]) for (const z of [-1, 0, 1]) {
    if (x || y || z) rawEyes.push([x, y, z]);
  }
  for (const [a, b, c] of [[2, 1, 1], [1, 2, 1], [1, 1, 2]] as const) {
    for (const sx of [-1, 1]) for (const sy of [-1, 1]) for (const sz of [-1, 1]) {
      rawEyes.push([sx * a, sy * b, sz * c]);
    }
  }
  const avg: Vec = [0, 0, 0];
  for (const o of oriented) {
    rawEyes.push(o.normal);
    avg[0] += o.normal[0]; avg[1] += o.normal[1]; avg[2] += o.normal[2];
  }
  const avgU = unit(avg);
  if (avgU) rawEyes.push(avgU);

  const eyeKeys = new Set<string>();
  const eyes: Vec[] = [];
  for (const e of rawEyes) {
    const u = unit(e);
    if (!u) continue;
    const key = rounded(u);
    if (eyeKeys.has(key)) continue;
    eyeKeys.add(key);
    eyes.push(u);
  }
  const ups: readonly Vec[] = [[0, 0, 1], [0, 1, 0], [1, 0, 0], [-1, 0, 0], [0, -1, 0], [0, 0, -1]];
  let best: { score: number; eye: Vec; up: Vec } | null = null;
  for (const eye of eyes) {
    const look: Vec = [-eye[0], -eye[1], -eye[2]];
    const isoBonus = 0.08 * (1 - Math.max(Math.abs(eye[0]), Math.abs(eye[1]), Math.abs(eye[2])));
    for (const rawUp of ups) {
      if (Math.abs(dot(eye, rawUp)) > 0.98) continue;
      const right = unit(cross(look, rawUp));
      if (!right) continue;
      const camUp = unit(cross(right, look));
      if (!camUp) continue;
      let facingCount = 0;
      let facingSum = 0;
      let uprightScore = 0;
      for (const o of oriented) {
        const f = dot(o.normal, eye);
        if (f <= 0.15) continue;
        facingCount += 1;
        facingSum += Math.min(f, 0.75);
        if (o.xAxis && o.up) {
          const r = dot(o.xAxis, right);
          const u = dot(o.up, camUp);
          if (r > 0.25 && u > 0.25) uprightScore += 1 + 0.3 * (r + u);
          else if (r < -0.15 || u < -0.15) uprightScore -= 0.6;
          else uprightScore += 0.2;
        }
      }
      const zUpBonus = rawUp[2] === 1 ? 0.05 : rawUp[1] === 1 ? 0.02 : 0;
      const score = facingCount * 100 + uprightScore * 4 + facingSum * 2 + isoBonus + zUpBonus;
      if (!best || score > best.score) best = { score, eye, up: rawUp };
    }
  }
  if (!best) return null;
  return {
    key: 'best', name: 'Best view', title: 'Most callouts facing the viewer',
    eye: best.eye, up: best.up, min, max,
  };
}

// sin(22.5°): partitions the unit sphere into the orientation cube's 6 faces, 12 edges and 8 corners.
const EDGE_THRESHOLD = Math.sin(Math.PI / 8);
const DIRECTION_ORDER: readonly string[] = ['Front', 'Top', 'Right', 'Left', 'Back', 'Bottom'];

/** View-cube direction name for an outward plane normal (Z up, -Y front, +X right, matching orientation-cube.ts). */
export function viewDirectionName(normal: DrawnPoint): string {
  const u = unit(normal);
  if (!u) return 'View';
  const [x, y, z] = u;
  const parts: string[] = [];
  if (z >= EDGE_THRESHOLD) parts.push('Top');
  else if (z <= -EDGE_THRESHOLD) parts.push('Bottom');
  if (y <= -EDGE_THRESHOLD) parts.push('Front');
  else if (y >= EDGE_THRESHOLD) parts.push('Back');
  if (x >= EDGE_THRESHOLD) parts.push('Right');
  else if (x <= -EDGE_THRESHOLD) parts.push('Left');
  return parts.join('-') || 'View';
}

/**
 * The views of the drawn PMI: the file's saved views that have a camera (not the views Specify
 * generates for its own graphics), then one for each way
 * the shown graphics' annotation planes face (planes facing alike, with the same up, are one
 * view), named by view-cube direction and callout count.
 */
export function drawnViews(
  pmi: DrawnPmi, shown: (g: DrawnGraphic, item: string | null) => boolean, centre?: DrawnPoint,
): DrawnView[] {
  const { presentation } = pmi;
  const all = [
    ...Object.entries(presentation.items).flatMap(([item, gs]) => gs.map((g) => ({ g, item: item as string | null }))),
    ...presentation.unlinked.map((g) => ({ g, item: null })),
  ];
  // One graphic under several items is one graphic, shown if shown under any of them.
  const byId = new Map<string, DrawnGraphic>();
  const graphics: DrawnGraphic[] = [];
  for (const { g, item } of all) {
    if (!shown(g, item)) continue;
    if (g.id === undefined) graphics.push(g);
    else if (!byId.has(g.id)) { byId.set(g.id, g); graphics.push(g); }
  }
  const box = (gs: readonly DrawnGraphic[]) => {
    const points = gs.flatMap(graphicPoints);
    if (!points.length) return null;
    const min: Vec = [Infinity, Infinity, Infinity];
    const max: Vec = [-Infinity, -Infinity, -Infinity];
    for (const p of points) {
      for (let k = 0; k < 3; k++) { min[k] = Math.min(min[k]!, p[k]!); max[k] = Math.max(max[k]!, p[k]!); }
    }
    return { min, max };
  };
  const out: DrawnView[] = [];
  for (const view of presentation.views) {
    const pos = view.direction ? unit(view.direction) : null;
    if (!pos) continue;
    const members = view.graphics.flatMap((id) => { const g = byId.get(id); return g ? [g] : []; });
    // The file's saved views only: Specify generates a view for each graphic it draws (0.1.6),
    // which the planes below already offer, grouped. A view whose graphics are all hidden (by
    // the show toggles, or facing away) has nothing to look square at, so it is not offered either.
    if (view.graphics.length && !members.length) continue;
    if (members.length && members.every((g) => g.source === 'generated')) continue;
    const b = box(members.length ? members : graphics);
    if (!b) continue;
    // STEP CAMERA_MODEL_D3 stores view direction pointing toward the camera, whereas Specify's
    // generated views point into the scene; pick whichever sign faces more of the view's graphics.
    let posFacing = 0;
    let negFacing = 0;
    for (const g of members) {
      const n = graphicNormal(g, presentation.planes, centre);
      if (!n) continue;
      const d = dot(n, pos);
      if (d > 0.05) posFacing += 1;
      else if (d < -0.05) negFacing += 1;
    }
    const eye: Vec = posFacing > negFacing ? pos : [-pos[0], -pos[1], -pos[2]];
    const up = view.up ? unit(view.up) : null;
    out.push({ key: `view:${view.id}`, name: view.name, title: members.map((g) => g.name).join('\n'), eye, ...(up ? { up } : {}), ...b });
  }
  const groups = new Map<string, { eye: Vec; up: Vec | null; graphics: DrawnGraphic[] }>();
  for (const g of graphics) {
    const eye = graphicNormal(g, presentation.planes, centre);
    if (!eye) continue;
    const x = presentation.planes.find((p) => p.id === g.planeId)?.xAxis ?? g.textPlacements?.[0]?.xAxis;
    const up = x ? unit(cross(eye, x)) : null;
    const key = `${rounded(eye)}|${up ? rounded(up) : ''}`;
    const group = groups.get(key) ?? { eye, up, graphics: [] };
    group.graphics.push(g);
    groups.set(key, group);
  }
  const planeEntries: { key: string; direction: string; eye: Vec; up: Vec | null; names: string[]; count: number; min: Vec; max: Vec }[] = [];
  const dirCounts = new Map<string, number>();
  for (const [key, { eye, up, graphics: gs }] of groups) {
    const b = box(gs);
    if (!b) continue;
    const direction = viewDirectionName(eye);
    const names = [...new Set(gs.map((g) => g.name).filter(Boolean))];
    const count = gs.length;
    dirCounts.set(direction, (dirCounts.get(direction) ?? 0) + 1);
    planeEntries.push({ key, direction, eye, up, names, count, ...b });
  }
  const dirRank = (dir: string) => {
    const idx = DIRECTION_ORDER.indexOf(dir);
    return idx >= 0 ? idx : DIRECTION_ORDER.length;
  };
  planeEntries.sort((a, b) => dirRank(a.direction) - dirRank(b.direction) || a.direction.localeCompare(b.direction) || b.count - a.count);
  const dirIndex = new Map<string, number>();
  for (const entry of planeEntries) {
    const total = dirCounts.get(entry.direction) ?? 1;
    const index = (dirIndex.get(entry.direction) ?? 0) + 1;
    dirIndex.set(entry.direction, index);
    const label = total > 1 ? `${entry.direction} ${index}` : entry.direction;
    out.push({
      key: `plane:${entry.key}`,
      name: `${label} · ${entry.count}`,
      title: entry.names.join('\n'),
      eye: entry.eye,
      ...(entry.up ? { up: entry.up } : {}),
      min: entry.min,
      max: entry.max,
      count: entry.count,
    });
  }
  return out;
}

export const MAX_VIEW_BUTTONS = 5;

/**
 * Splits views into primary bar buttons and overflow items for a "More views" dropdown.
 * Priority: Best view first, then the file's saved views in order, then the most-populated
 * directions (ties broken by direction order). Keeps at most ``maxButtons`` controls in the bar
 * (all views when ``total <= maxButtons``, otherwise ``maxButtons - 1`` views plus the dropdown).
 */
export function splitDrawnViews(
  bestView: DrawnView | null,
  views: readonly DrawnView[],
  maxButtons = MAX_VIEW_BUTTONS,
): { primary: DrawnView[]; overflow: DrawnView[] } {
  const all = bestView ? [bestView, ...views] : [...views];
  if (all.length <= maxButtons) return { primary: all, overflow: [] };
  const saved = views.filter((v) => v.key.startsWith('view:'));
  const planes = views.filter((v) => !v.key.startsWith('view:')).sort((a, b) => (b.count ?? 0) - (a.count ?? 0));
  const ranked = [...(bestView ? [bestView] : []), ...saved, ...planes];
  const keep = Math.max(1, maxButtons - 1);
  return { primary: ranked.slice(0, keep), overflow: ranked.slice(keep) };
}
