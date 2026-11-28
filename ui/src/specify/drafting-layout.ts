// Standard-drafting 3D annotation-plane layout for Specify-created PMI.
//
// Leaves file-native CAD graphics untouched on their own planes. For Specify-created callouts —
// both pending edits (`source: 'generated'`) and those already written into the STEP — lays them
// out deterministically on part-axis-aligned 3D annotation planes following ASME Y14.5 / ISO 1101:
//   - Exactly one short, straight leader per feature callout stack (thread/size callouts stack
//     directly above their FCF with one shared leader).
//   - Filled arrowheads (holes/edges), filled datum triangles flush on datum faces (datums), or
//     filled dots (planar faces).
//   - Linear/basic dimensions with feature-touching extension lines, overshoots, filled arrowheads,
//     and upright text or basic frame.
//   - Cardinal part-axis annotation planes oriented facing, left-to-right, and right-side-up from
//     the reference view, producing identical 3D geometry before and after Save.
//
// Pure, no three.js or DOM.

import type { PmiAnalysis, PmiMesh } from '../types/pmi.ts';
import {
  extractSceneDimensions, graphicNormal, graphicPoints, readSceneSemanticLabels,
  BALLOON_SLOT_H, BALLOON_SLOT_W, type BalloonSlot, type DrawnGraphic, type DrawnPlane, type DrawnPmi, type DrawnPoint,
  type PartOrigin, type SceneDimMeta, type SceneSemanticLabel, type TextPlacement,
} from './drawn.ts';

type Vec3 = [number, number, number];
type Axis = 0 | 1 | 2;
type Triangle3 = [Vec3, Vec3, Vec3];

const add = (a: DrawnPoint, b: DrawnPoint): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a: DrawnPoint, b: DrawnPoint): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const scale = (a: DrawnPoint, s: number): Vec3 => [a[0] * s, a[1] * s, a[2] * s];
const dot = (a: DrawnPoint, b: DrawnPoint): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: DrawnPoint, b: DrawnPoint): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const len = (a: DrawnPoint): number => Math.hypot(a[0], a[1], a[2]);
const unit = (a: DrawnPoint): Vec3 | null => {
  const n = len(a);
  return n > 1e-12 ? [a[0] / n, a[1] / n, a[2] / n] : null;
};

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);

const CARDINAL_AXES: readonly Vec3[] = [
  [1, 0, 0], [-1, 0, 0],
  [0, 1, 0], [0, -1, 0],
  [0, 0, 1], [0, 0, -1],
];

const SPECIFY_ITEM_PREFIX = /^(datum:|planner:dim:|position:|thread:|flatness:|perpendicularity:|parallelism:|angularity:|cylindricity:|circularity:|runout:|total_runout:|profile:|concentricity:|symmetry:|straightness:|knurl:|surface:|extra\.)/;

/** Closest point on 3D triangle (a, b, c) to query point p (Ericson Voronoi regions). */
function closestPointOnTriangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3): Vec3 {
  const ab = sub(b, a);
  const ac = sub(c, a);
  const ap = sub(p, a);
  const d1 = dot(ab, ap);
  const d2 = dot(ac, ap);
  if (d1 <= 0 && d2 <= 0) return a;
  const bp = sub(p, b);
  const d3 = dot(ab, bp);
  const d4 = dot(ac, bp);
  if (d3 >= 0 && d4 <= d3) return b;
  const vc = d1 * d4 - d3 * d2;
  if (vc <= 0 && d1 >= 0 && d3 <= 0) {
    const v = d1 / (d1 - d3);
    return add(a, scale(ab, v));
  }
  const cp = sub(p, c);
  const d5 = dot(ab, cp);
  const d6 = dot(ac, cp);
  if (d6 >= 0 && d5 <= d6) return c;
  const vb = d5 * d2 - d1 * d6;
  if (vb <= 0 && d2 >= 0 && d6 <= 0) {
    const w = d2 / (d2 - d6);
    return add(a, scale(ac, w));
  }
  const va = d3 * d6 - d5 * d4;
  if (va <= 0 && (d4 - d3) >= 0 && (d5 - d6) >= 0) {
    const w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
    return add(b, scale(sub(c, b), w));
  }
  const denom = 1 / (va + vb + vc);
  const v = vb * denom;
  const w = vc * denom;
  return add(a, add(scale(ab, v), scale(ac, w)));
}

function faceTriangles(faces: readonly number[], mesh: PmiMesh): Triangle3[] {
  const tris: Triangle3[] = [];
  for (const fid of faces) {
    const [start, count] = mesh.faces[fid] ?? [0, 0];
    for (let t = start; t < start + count; t++) {
      const i0 = mesh.indices[t * 3] ?? 0;
      const i1 = mesh.indices[t * 3 + 1] ?? 0;
      const i2 = mesh.indices[t * 3 + 2] ?? 0;
      tris.push([
        [mesh.positions[i0 * 3] ?? 0, mesh.positions[i0 * 3 + 1] ?? 0, mesh.positions[i0 * 3 + 2] ?? 0],
        [mesh.positions[i1 * 3] ?? 0, mesh.positions[i1 * 3 + 1] ?? 0, mesh.positions[i1 * 3 + 2] ?? 0],
        [mesh.positions[i2 * 3] ?? 0, mesh.positions[i2 * 3 + 1] ?? 0, mesh.positions[i2 * 3 + 2] ?? 0],
      ]);
    }
  }
  return tris;
}

function nearestOnTriangles(p: Vec3, tris: readonly Triangle3[]): { point: Vec3; dist: number } | null {
  if (!tris.length) return null;
  let bestPt = tris[0]![0];
  let bestD2 = Infinity;
  for (const [a, b, c] of tris) {
    const q = closestPointOnTriangle(p, a, b, c);
    const d = sub(q, p);
    const d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if (d2 < bestD2) {
      bestD2 = d2;
      bestPt = q;
    }
  }
  return { point: bestPt, dist: Math.sqrt(bestD2) };
}

interface FaceExtent {
  min: Vec3;
  max: Vec3;
  verts: Vec3[];
}

function faceMeshExtent(faces: readonly number[], mesh: PmiMesh): FaceExtent | null {
  const min: Vec3 = [Infinity, Infinity, Infinity];
  const max: Vec3 = [-Infinity, -Infinity, -Infinity];
  const verts: Vec3[] = [];
  const seen = new Set<number>();
  for (const fid of faces) {
    const [start, count] = mesh.faces[fid] ?? [0, 0];
    for (let k = start * 3; k < (start + count) * 3; k++) {
      const v = mesh.indices[k] ?? 0;
      if (seen.has(v)) continue;
      seen.add(v);
      const pt: Vec3 = [mesh.positions[v * 3] ?? 0, mesh.positions[v * 3 + 1] ?? 0, mesh.positions[v * 3 + 2] ?? 0];
      verts.push(pt);
      for (let ax = 0; ax < 3; ax++) {
        if (pt[ax]! < min[ax]!) min[ax] = pt[ax]!;
        if (pt[ax]! > max[ax]!) max[ax] = pt[ax]!;
      }
    }
  }
  return verts.length ? { min, max, verts } : null;
}

function orthonormalBasis(rawZ: Vec3, rawX?: Vec3 | null): { xAxis: Vec3; yAxis: Vec3; zAxis: Vec3 } {
  const zAxis = unit(rawZ) ?? [0, 0, 1];
  let xCand = rawX ? sub(rawX, scale(zAxis, dot(rawX, zAxis))) : null;
  let xAxis = xCand ? unit(xCand) : null;
  if (!xAxis) {
    const fallback: Vec3 = Math.abs(zAxis[0]) < 0.9 ? [1, 0, 0] : [0, 1, 0];
    xCand = sub(fallback, scale(zAxis, dot(fallback, zAxis)));
    xAxis = unit(xCand) ?? [1, 0, 0];
  }
  const yAxis = unit(cross(zAxis, xAxis)) ?? [0, 1, 0];
  return { xAxis, yAxis, zAxis };
}

function intersectLinePlane(lineO: Vec3, lineD: Vec3, plP: Vec3, plN: Vec3): Vec3 | null {
  const denom = dot(lineD, plN);
  if (Math.abs(denom) < 0.1) return null;
  const t = dot(sub(plP, lineO), plN) / denom;
  return add(lineO, scale(lineD, t));
}

function resolveFileDatumFrame(
  analysis: PmiAnalysis,
  pmi: DrawnPmi | null,
): { position: Vec3; xAxis: Vec3; yAxis: Vec3; zAxis: Vec3 } | null {
  const faceById = new Map(analysis.faces.map((f) => [f.id, f]));
  const fileDatums = new Map<string, readonly number[]>();
  for (const e of analysis.existing ?? []) {
    if (e.kind !== 'datum' || !e.type || !e.faces?.length) continue;
    const letter = e.type.trim().toUpperCase();
    // Skip Specify-written datums so files without a native datum origin stay consistent across Save.
    if (analysis.resume) {
      const gs = pmi?.presentation.items[`datum:${letter}`] ?? [];
      if (gs.length > 0 && gs.every((g) => isSpecifyCreated(`datum:${letter}`, g))) continue;
      if (gs.length === 0) continue;
    }
    if (!fileDatums.has(letter)) fileDatums.set(letter, e.faces);
  }
  const facesA = fileDatums.get('A');
  const facesB = fileDatums.get('B');
  const facesC = fileDatums.get('C');
  if (!facesA || !facesB || !facesC) return null;

  const planeOf = (fids: readonly number[]) => {
    const f = fids.map((id) => faceById.get(id)).find((x) => x?.kind === 'plane' && x.direction);
    if (!f?.direction) return null;
    const n = unit(f.direction);
    return n ? { point: [f.centroid[0], f.centroid[1], f.centroid[2]] as Vec3, normal: n } : null;
  };
  const cylOf = (fids: readonly number[]) => {
    const f = fids.map((id) => faceById.get(id)).find((x) => x?.kind === 'cylinder' && x.direction);
    if (!f?.direction) return null;
    const d = unit(f.direction);
    const o = f.origin ?? f.centroid;
    return d ? { origin: [o[0], o[1], o[2]] as Vec3, dir: d } : null;
  };

  const pA = planeOf(facesA);
  if (!pA) return null;
  const cB = cylOf(facesB);
  const cC = cylOf(facesC);
  const pB = planeOf(facesB);
  const pC = planeOf(facesC);

  if (cB && cC) {
    // Primary plane A + secondary cylinder B + tertiary cylinder C.
    const orig = intersectLinePlane(cB.origin, cB.dir, pA.point, pA.normal);
    const origC = intersectLinePlane(cC.origin, cC.dir, pA.point, pA.normal);
    if (!orig || !origC) return null;
    return { position: orig, ...orthonormalBasis(pA.normal, sub(origC, orig)) };
  }
  if (cB && pC) {
    const orig = intersectLinePlane(cB.origin, cB.dir, pA.point, pA.normal);
    if (!orig) return null;
    const yCand = sub(pC.normal, scale(pA.normal, dot(pC.normal, pA.normal)));
    const yAxis = unit(yCand);
    if (!yAxis) return null;
    const xAxis = unit(cross(yAxis, pA.normal)) ?? [1, 0, 0];
    return { position: orig, xAxis, yAxis, zAxis: pA.normal };
  }
  if (pB && pC) {
    // Three mutually non-coplanar datum planes A, B, C: Cramer's rule intersection.
    const [n1, n2, n3] = [pA.normal, pB.normal, pC.normal];
    const n2xn3 = cross(n2, n3);
    const det = dot(n1, n2xn3);
    if (Math.abs(det) < 0.1) return null;
    const num = add(
      scale(n2xn3, dot(n1, pA.point)),
      add(scale(cross(n3, n1), dot(n2, pB.point)), scale(cross(n1, n2), dot(n3, pC.point))),
    );
    const orig: Vec3 = [num[0] / det, num[1] / det, num[2] / det];
    return { position: orig, ...orthonormalBasis(n1, scale(n2, -1)) };
  }
  return null;
}

/**
 * Parses an explicit STEP coordinate system or isolated origin point from raw STEP bytes
 * (never a default B-rep placement). Pure, no WASM or DOM.
 */
export function findStepOrigin(bytes: Uint8Array | null): PartOrigin | null {
  if (!bytes || bytes.length === 0) return null;
  const text = new TextDecoder('latin1').decode(bytes);
  const dataIdx = text.indexOf('DATA;');
  const body = dataIdx >= 0 ? text.slice(dataIdx) : text;

  const points = new Map<string, { name: string; pt: Vec3 }>();
  const dirs = new Map<string, Vec3>();
  const axes3d = new Map<string, { name: string; ptRef: string; zRef?: string; xRef?: string }>();
  const curveSets = new Map<string, string[]>();
  const constructiveRefs: string[] = [];
  const wireframeRefs: string[] = [];

  const entRe = /(#\d+)\s*=\s*(CARTESIAN_POINT|DIRECTION|AXIS2_PLACEMENT_3D|GEOMETRIC_CURVE_SET|GEOMETRIC_SET|CONSTRUCTIVE_GEOMETRY_REPRESENTATION|GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION)\s*\(([^;]*)\);/g;
  let m: RegExpExecArray | null;
  while ((m = entRe.exec(body)) !== null) {
    const [, id, ty, args] = m as unknown as [string, string, string, string];
    if (ty === 'CARTESIAN_POINT') {
      const pm = /^'([^']*)'\s*,\s*\(\s*([^,]+),\s*([^,]+),\s*([^)]+)\)/.exec(args.trim());
      if (pm) {
        const x = Number(pm[2]), y = Number(pm[3]), z = Number(pm[4]);
        if (Number.isFinite(x) && Number.isFinite(y) && Number.isFinite(z)) {
          points.set(id, { name: pm[1]!, pt: [x, y, z] });
        }
      }
    } else if (ty === 'DIRECTION') {
      const dm = /\(\s*([^,]+),\s*([^,]+),\s*([^)]+)\)/.exec(args);
      if (dm) {
        const u = unit([Number(dm[1]), Number(dm[2]), Number(dm[3])]);
        if (u) dirs.set(id, u);
      }
    } else if (ty === 'AXIS2_PLACEMENT_3D') {
      const nm = /^'([^']*)'/.exec(args.trim());
      const refs = args.match(/#\d+/g) ?? [];
      if (refs[0]) {
        axes3d.set(id, {
          name: nm?.[1] ?? '',
          ptRef: refs[0],
          ...(refs[1] ? { zRef: refs[1] } : {}),
          ...(refs[2] ? { xRef: refs[2] } : {}),
        });
      }
    } else if (ty === 'GEOMETRIC_CURVE_SET' || ty === 'GEOMETRIC_SET') {
      curveSets.set(id, args.match(/#\d+/g) ?? []);
    } else if (ty === 'CONSTRUCTIVE_GEOMETRY_REPRESENTATION') {
      constructiveRefs.push(...(args.match(/#\d+/g) ?? []));
    } else if (ty === 'GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION') {
      wireframeRefs.push(...(args.match(/#\d+/g) ?? []));
    }
  }

  const resolveAxis = (ref: string): PartOrigin | null => {
    const ax = axes3d.get(ref);
    if (!ax) return null;
    const p = points.get(ax.ptRef);
    if (!p) return null;
    const rawZ = (ax.zRef ? dirs.get(ax.zRef) : null) ?? [0, 0, 1];
    const rawX = ax.xRef ? dirs.get(ax.xRef) : null;
    return { position: p.pt, ...orthonormalBasis(rawZ, rawX), source: 'step-csys' };
  };

  const expandRefs = (refs: string[]) => refs.flatMap((r) => [r, ...(curveSets.get(r) ?? [])]);
  for (const ref of expandRefs([...constructiveRefs, ...wireframeRefs])) {
    const resolved = resolveAxis(ref);
    if (resolved) return resolved;
  }
  for (const [id, ax] of axes3d) {
    if (/^(origin|csys|coord|datum\s*csys|wcs|mcs)/i.test(ax.name.trim())) {
      const resolved = resolveAxis(id);
      if (resolved) return resolved;
    }
  }
  for (const ref of expandRefs([...wireframeRefs, ...constructiveRefs])) {
    const p = points.get(ref);
    if (!p) continue;
    return {
      position: p.pt,
      xAxis: [1, 0, 0],
      yAxis: [0, 1, 0],
      zAxis: [0, 0, 1],
      source: 'step-point',
    };
  }
  return null;
}

/**
 * Resolves the effective stated origin by combining any explicit STEP origin (from the worker or
 * scene) with the file-native 3-datum reference frame (`A | B | C` in `analysis.existing`,
 * excluding Specify-created datums).
 */
export function resolveStatedOrigin(
  stepOrigin: PartOrigin | null,
  analysis: PmiAnalysis,
  pmi: DrawnPmi | null,
): PartOrigin | null {
  if (stepOrigin?.source === 'step-csys') return stepOrigin;
  const datumFrame = resolveFileDatumFrame(analysis, pmi);
  if (stepOrigin?.source === 'step-point') {
    return {
      position: stepOrigin.position,
      xAxis: datumFrame?.xAxis ?? stepOrigin.xAxis,
      yAxis: datumFrame?.yAxis ?? stepOrigin.yAxis,
      zAxis: datumFrame?.zAxis ?? stepOrigin.zAxis,
      source: 'step-point',
    };
  }
  if (stepOrigin) return stepOrigin;
  return datumFrame ? { ...datumFrame, source: 'datum-frame' } : null;
}

/**
 * Finds an explicit coordinate-system or datum origin stated by a STEP file (never a default
 * B-rep placement):
 * 1. Explicit STEP coordinate system (`CONSTRUCTIVE_GEOMETRY_REPRESENTATION` or wireframe
 *    `AXIS2_PLACEMENT_3D`, or a named origin/CSYS `AXIS2_PLACEMENT_3D`).
 * 2. Explicit STEP isolated origin point (`GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION`
 *    -> `GEOMETRIC_CURVE_SET` / `GEOMETRIC_SET` -> `CARTESIAN_POINT`).
 * 3. File-native 3-datum reference frame (`A | B | C` in `analysis.existing`, excluding
 *    Specify-created datums).
 */
export function findStatedOrigin(
  bytes: Uint8Array | null,
  analysis: PmiAnalysis,
  pmi: DrawnPmi | null,
): PartOrigin | null {
  return resolveStatedOrigin(findStepOrigin(bytes), analysis, pmi);
}

/**
 * Whether a graphic was created by Specify (live `source: 'generated'` before Save, or written
 * into the STEP by Specify after Save), rather than originating from an external CAD system's
 * graphical PMI.
 */
export function isSpecifyCreated(item: string, graphic: DrawnGraphic): boolean {
  if (graphic.source === 'generated') return true;
  if (graphic.triangles.length === 0 && SPECIFY_ITEM_PREFIX.test(item)) return true;
  return false;
}

export interface CameraBasis {
  readonly eye: Vec3;
  readonly up: Vec3;
  readonly right: Vec3;
  readonly camUp: Vec3;
}

function cameraBasis(eye: DrawnPoint, up: DrawnPoint): CameraBasis {
  const e = unit(eye) ?? [0.577, -0.577, 0.577];
  const look: Vec3 = [-e[0], -e[1], -e[2]];
  let u = unit(up) ?? [0, 0, 1];
  if (Math.abs(dot(e, u)) > 0.99) u = Math.abs(e[2]) < 0.9 ? [0, 0, 1] : [0, 1, 0];
  const right = unit(cross(look, u)) ?? [1, 0, 0];
  const camUp = unit(cross(right, look)) ?? [0, 0, 1];
  return { eye: e, up: camUp, right, camUp };
}

/**
 * Resolves a file saved view's camera eye direction and up vector. STEP `CAMERA_MODEL_D3` stores
 * `direction` pointing from target toward the camera, whereas Specify's generated views point into
 * the scene; testing both signs against the view's annotation plane normals selects the
 * front-facing side deterministically.
 */
export function resolveSavedViewCamera(
  view: DrawnPmi['presentation']['views'][number],
  pmi: DrawnPmi,
  centre?: DrawnPoint,
): { eye: Vec3; up?: Vec3 } | null {
  if (!view.direction) return null;
  const pos = unit(view.direction);
  if (!pos) return null;
  const neg: Vec3 = [-pos[0], -pos[1], -pos[2]];

  const all = new Map<string, DrawnGraphic>();
  for (const gs of Object.values(pmi.presentation.items)) {
    for (const g of gs) if (g.id) all.set(g.id, g);
  }
  for (const g of pmi.presentation.unlinked) if (g.id) all.set(g.id, g);

  const members = view.graphics.flatMap((id) => {
    const g = all.get(id);
    return g ? [g] : [];
  });
  let posFacing = 0;
  let negFacing = 0;
  for (const g of members) {
    const n = graphicNormal(g, pmi.presentation.planes, centre);
    if (!n) continue;
    const d = dot(n, pos);
    if (d > 0.05) posFacing += 1;
    else if (d < -0.05) negFacing += 1;
  }
  const eye = posFacing > negFacing ? pos : neg;
  const up = view.up ? unit(view.up) ?? undefined : undefined;
  return up ? { eye, up } : { eye };
}

interface HoleGeom {
  id: string;
  faces: readonly number[];
  axis: Vec3;
  location: Vec3;
  diameter: number;
  depth: number;
  through: boolean;
  openings: { center: Vec3; normal: Vec3; radius: number }[];
  count: number;
}

function readHoleGeoms(analysis: PmiAnalysis): HoleGeom[] {
  const out: HoleGeom[] = [];
  const parseRec = (rec: Record<string, unknown>, id: string, faces: readonly number[], count = 1): HoleGeom | null => {
    const rawAxis = rec['axis'];
    const rawLoc = rec['location'];
    const diam = typeof rec['diameter'] === 'number' ? rec['diameter'] : 0;
    const depth = typeof rec['depth'] === 'number' ? rec['depth'] : 0;
    if (!Array.isArray(rawAxis) || !Array.isArray(rawLoc) || diam <= 0) return null;
    const axis = unit([Number(rawAxis[0]) || 0, Number(rawAxis[1]) || 0, Number(rawAxis[2]) || 0]);
    if (!axis) return null;
    const loc: Vec3 = [Number(rawLoc[0]) || 0, Number(rawLoc[1]) || 0, Number(rawLoc[2]) || 0];
    const cbore = isObject(rec['cbore']) && typeof rec['cbore']['diameter'] === 'number' ? rec['cbore']['diameter'] : null;
    const through = rec['bottom'] === 'through';
    const openings: { center: Vec3; normal: Vec3; radius: number }[] = [
      { center: loc, normal: [-axis[0], -axis[1], -axis[2]], radius: (cbore ?? diam) / 2 },
    ];
    if (through && depth > 0) {
      openings.push({ center: add(loc, scale(axis, depth)), normal: axis, radius: diam / 2 });
    }
    return { id, faces, axis, location: loc, diameter: diam, depth, through, openings, count };
  };
  for (const f of analysis.features) {
    if (f.family === 'holes') {
      const g = parseRec(f.record, f.id, f.faces, 1);
      if (g) out.push(g);
    } else if (f.family === 'hole_patterns' && Array.isArray(f.record['holes'])) {
      const holes = f.record['holes'].filter(isObject);
      const parsed = holes.map((h) => parseRec(h, f.id, f.faces, holes.length)).filter((x): x is HoleGeom => x !== null);
      if (parsed.length) {
        out.push({
          ...parsed[0]!,
          id: f.id,
          faces: f.faces,
          openings: parsed.flatMap((h) => h.openings),
          count: parsed.length,
        });
      }
    }
  }
  return out;
}

/**
 * Best reference isometric camera `(eye, up)` for a part:
 * 1. If the file has a saved view with file-native graphics, uses that saved view.
 * 2. Otherwise votes on the isometric octant facing the most datum planes and hole openings,
 *    keeping elongated Z-axis parts horizontal in the viewport.
 */
export function bestReferenceView(
  pmi: DrawnPmi,
  analysis: PmiAnalysis,
  mesh: PmiMesh,
): CameraBasis {
  const centre: Vec3 = [
    (mesh.bbox[0][0] + mesh.bbox[1][0]) / 2,
    (mesh.bbox[0][1] + mesh.bbox[1][1]) / 2,
    (mesh.bbox[0][2] + mesh.bbox[1][2]) / 2,
  ];
  const all = new Map<string, DrawnGraphic>();
  for (const [item, gs] of Object.entries(pmi.presentation.items)) {
    for (const g of gs) if (g.id && !isSpecifyCreated(item, g)) all.set(g.id, g);
  }
  for (const g of pmi.presentation.unlinked) {
    if (g.id && g.source === 'file') all.set(g.id, g);
  }
  for (const view of pmi.presentation.views) {
    if (!view.direction) continue;
    if (!view.graphics.some((id) => all.has(id))) continue;
    const resolved = resolveSavedViewCamera(view, pmi, centre);
    if (resolved) return cameraBasis(resolved.eye, resolved.up ?? [0, 0, 1]);
  }

  const votes: Vec3 = [0, 0, 0];
  const faceById = new Map(analysis.faces.map((f) => [f.id, f]));
  for (const [item, link] of Object.entries(pmi.links)) {
    if (!item.startsWith('datum:') && !item.startsWith('datums:')) continue;
    for (const fid of link.faces) {
      const f = faceById.get(fid);
      if (f?.kind === 'plane' && f.direction) {
        votes[0] += f.direction[0] * 3;
        votes[1] += f.direction[1] * 3;
        votes[2] += f.direction[2] * 3;
      }
    }
  }
  for (const h of readHoleGeoms(analysis)) {
    if (!h.through && h.openings[0]) {
      const n = h.openings[0].normal;
      votes[0] += n[0] * 1.5;
      votes[1] += n[1] * 1.5;
      votes[2] += n[2] * 1.5;
    } else if (h.through && h.openings.length >= 2 && h.openings[0]!.radius > h.openings[1]!.radius + 0.05) {
      // Prefer the counterbore opening side.
      const n = h.openings[0]!.normal;
      votes[0] += n[0] * 2;
      votes[1] += n[1] * 2;
      votes[2] += n[2] * 2;
    }
  }
  const sx = votes[0] < -0.1 ? -1 : 1;
  const sy = votes[1] > 0.1 ? 1 : -1;
  const sz = votes[2] < -0.1 ? -1 : 1;

  const span: Vec3 = [
    mesh.bbox[1][0] - mesh.bbox[0][0],
    mesh.bbox[1][1] - mesh.bbox[0][1],
    mesh.bbox[1][2] - mesh.bbox[0][2],
  ];
  const order = [0, 1, 2].sort((a, b) => span[b]! - span[a]!);
  const longest = order[0]!;
  const second = order[1]!;

  // Elongated Z-axis parts orient Z left-to-right so the part fills a landscape viewport cleanly.
  if (longest === 2 && span[2] > 2.2 * span[second]!) {
    const up: Vec3 = second === 0 ? [sx, 0, 0] : [0, sy, 0];
    const rawEye: Vec3 = [sx * 0.50, sy * 0.72, sz * 0.48];
    const cam = cameraBasis(rawEye, up);
    if (dot([0, 0, 1], cam.right) < 0) {
      return cameraBasis(rawEye, [-up[0], -up[1], -up[2]]);
    }
    return cam;
  }

  return cameraBasis([sx * 0.58, sy * 0.57, sz * 0.58], [0, 0, 1]);
}

interface PlaneBasis {
  normal: Vec3;
  xAxis: Vec3;
  yAxis: Vec3;
}

/** The in-plane axis (``dir``) nearest ``toward``: the side a balloon is kept on. */
function slotDir(basis: PlaneBasis, toward: Vec3): Vec3 {
  const ax = dot(toward, basis.xAxis);
  const ay = dot(toward, basis.yAxis);
  return Math.abs(ax) >= Math.abs(ay) ? scale(basis.xAxis, ax >= 0 ? 1 : -1) : scale(basis.yAxis, ay >= 0 ? 1 : -1);
}

/**
 * The room kept for a callout's inspection balloon, a fixed size whatever the balloon says (so the
 * layout never depends on the numbers, and is the same before and after Save): the slot to draw it
 * in, and the points its footprint adds.
 */
function balloonSlot(basis: PlaneBasis, edge: Vec3, dir: Vec3, H: number): { slot: BalloonSlot; pts: Vec3[] } {
  const alongX = Math.abs(dot(dir, basis.xAxis)) > 0.5;
  const perp = alongX ? basis.yAxis : basis.xAxis;
  const length = (alongX ? BALLOON_SLOT_W : BALLOON_SLOT_H) * H;
  const half = (alongX ? BALLOON_SLOT_H : BALLOON_SLOT_W) * H / 2;
  const far = add(edge, scale(dir, length));
  return {
    slot: { edge, dir, xAxis: basis.xAxis, normal: basis.normal, height: H },
    pts: [add(edge, scale(perp, half)), add(edge, scale(perp, -half)), add(far, scale(perp, half)), add(far, scale(perp, -half))],
  };
}

/** Given a cardinal normal facing the camera, returns the cardinal `(xAxis, yAxis)` reading left-to-right and upright. */
function readablePlaneBasis(normal: Vec3, cam: CameraBasis, preferX?: Vec3): PlaneBasis {
  const candidates: PlaneBasis[] = [];
  for (const ax of CARDINAL_AXES) {
    if (Math.abs(dot(ax, normal)) > 0.1) continue;
    const ay = unit(cross(normal, ax));
    if (ay) candidates.push({ normal, xAxis: ax, yAxis: ay });
  }
  const score = (p: PlaneBasis) => {
    const r = dot(p.xAxis, cam.right);
    const u = dot(p.yAxis, cam.camUp);
    const upright = (r > 0.05 && u > 0.05 ? 10 : 0) + r * 2 + u * 2;
    const pref = preferX && dot(p.xAxis, preferX) > 0.9 ? 1.5 : 0;
    return upright + pref;
  };
  candidates.sort((a, b) => score(b) - score(a));
  return candidates[0]!;
}

/** Projects a 3D point into camera screen-aligned 2D coordinates `(right, up)` in mm. */
function project2(p: DrawnPoint, cam: CameraBasis): [number, number] {
  return [dot(p, cam.right), dot(p, cam.camUp)];
}

interface Rect2 {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

function rectOfPoints(pts: readonly DrawnPoint[], cam: CameraBasis, pad = 0): Rect2 {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const p of pts) {
    const [x, y] = project2(p, cam);
    if (x < x0) x0 = x;
    if (x > x1) x1 = x;
    if (y < y0) y0 = y;
    if (y > y1) y1 = y;
  }
  return { x0: x0 - pad, y0: y0 - pad, x1: x1 + pad, y1: y1 + pad };
}

function rectsOverlap(a: Rect2, b: Rect2): boolean {
  return a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;
}

function overlapArea(a: Rect2, b: Rect2): number {
  const ox = Math.min(a.x1, b.x1) - Math.max(a.x0, b.x0);
  const oy = Math.min(a.y1, b.y1) - Math.max(a.y0, b.y0);
  return ox > 0 && oy > 0 ? ox * oy : 0;
}

function segsIntersect2(
  ax: number, ay: number, bx: number, by: number,
  cx: number, cy: number, dx: number, dy: number,
): boolean {
  const ccw = (p1x: number, p1y: number, p2x: number, p2y: number, p3x: number, p3y: number) =>
    (p3y - p1y) * (p2x - p1x) - (p2y - p1y) * (p3x - p1x);
  const d1 = ccw(cx, cy, dx, dy, ax, ay);
  const d2 = ccw(cx, cy, dx, dy, bx, by);
  const d3 = ccw(ax, ay, bx, by, cx, cy);
  const d4 = ccw(ax, ay, bx, by, dx, dy);
  return ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0));
}

function segIntersectsRect2(ax: number, ay: number, bx: number, by: number, r: Rect2): boolean {
  if (Math.max(ax, bx) < r.x0 || Math.min(ax, bx) > r.x1 || Math.max(ay, by) < r.y0 || Math.min(ay, by) > r.y1) {
    return false;
  }
  const inside = (x: number, y: number) => x >= r.x0 && x <= r.x1 && y >= r.y0 && y <= r.y1;
  if (inside(ax, ay) || inside(bx, by)) return true;
  return (
    segsIntersect2(ax, ay, bx, by, r.x0, r.y0, r.x1, r.y0)
    || segsIntersect2(ax, ay, bx, by, r.x1, r.y0, r.x1, r.y1)
    || segsIntersect2(ax, ay, bx, by, r.x1, r.y1, r.x0, r.y1)
    || segsIntersect2(ax, ay, bx, by, r.x0, r.y1, r.x0, r.y0)
  );
}

/** 2D convex hull (Monotone Chain, CCW) of projected points. */
function convexHull2(pts: readonly [number, number][]): [number, number][] {
  if (pts.length <= 2) return [...pts];
  const sorted = [...pts].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cross2 = (o: [number, number], a: [number, number], b: [number, number]) =>
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const lower: [number, number][] = [];
  for (const p of sorted) {
    while (lower.length >= 2 && cross2(lower[lower.length - 2]!, lower[lower.length - 1]!, p) <= 0) lower.pop();
    lower.push(p);
  }
  const upper: [number, number][] = [];
  for (let i = sorted.length - 1; i >= 0; i--) {
    const p = sorted[i]!;
    while (upper.length >= 2 && cross2(upper[upper.length - 2]!, upper[upper.length - 1]!, p) <= 0) upper.pop();
    upper.push(p);
  }
  lower.pop();
  upper.pop();
  return [...lower, ...upper];
}

function pointInHull2(x: number, y: number, hull: readonly [number, number][], pad = 0): boolean {
  if (hull.length < 3) return false;
  for (let i = 0; i < hull.length; i++) {
    const a = hull[i]!;
    const b = hull[(i + 1) % hull.length]!;
    const ex = b[0] - a[0];
    const ey = b[1] - a[1];
    const elen = Math.hypot(ex, ey) || 1;
    // Signed distance inside CCW edge (positive = inside).
    const dist = (ex * (y - a[1]) - ey * (x - a[0])) / elen;
    if (dist < -pad) return false;
  }
  return true;
}

/** Fraction of a 4×4 sample grid of `r` that lies inside the 2D convex hull of the part (0..1). */
function rectHullOverlap(r: Rect2, hull: readonly [number, number][], pad = 0): number {
  let hits = 0;
  const N = 3;
  for (let ix = 0; ix <= N; ix++) {
    const x = r.x0 + ((r.x1 - r.x0) * ix) / N;
    for (let iy = 0; iy <= N; iy++) {
      const y = r.y0 + ((r.y1 - r.y0) * iy) / N;
      if (pointInHull2(x, y, hull, pad)) hits += 1;
    }
  }
  return hits / ((N + 1) * (N + 1));
}

/** Filled arrowhead triangle in the given annotation plane, tip at `tip`, pointing along `dir`. */
function arrowTriangle(
  tip: Vec3,
  dir: Vec3,
  normal: Vec3,
  length: number,
  halfWidth: number,
): readonly [DrawnPoint, DrawnPoint, DrawnPoint] {
  const d = unit(dir) ?? [1, 0, 0];
  const perp = unit(cross(normal, d)) ?? [0, 1, 0];
  const base = add(tip, scale(d, -length));
  return [tip, add(base, scale(perp, halfWidth)), add(base, scale(perp, -halfWidth))];
}

/** Filled octagonal surface dot for leaders terminating on planar faces. */
function surfaceDot(
  center: Vec3,
  xAxis: Vec3,
  yAxis: Vec3,
  radius: number,
): (readonly [DrawnPoint, DrawnPoint, DrawnPoint])[] {
  const tris: (readonly [DrawnPoint, DrawnPoint, DrawnPoint])[] = [];
  const N = 8;
  for (let i = 0; i < N; i++) {
    const a0 = (i / N) * Math.PI * 2;
    const a1 = ((i + 1) / N) * Math.PI * 2;
    const p0 = add(center, add(scale(xAxis, Math.cos(a0) * radius), scale(yAxis, Math.sin(a0) * radius)));
    const p1 = add(center, add(scale(xAxis, Math.cos(a1) * radius), scale(yAxis, Math.sin(a1) * radius)));
    tris.push([center, p0, p1]);
  }
  return tris;
}

/** Closed rectangle (4 polyline segments) in a plane from `(u0,v0)` to `(u1,v1)`. */
function rectPolylines(
  origin: Vec3,
  xAxis: Vec3,
  yAxis: Vec3,
  u0: number,
  v0: number,
  u1: number,
  v1: number,
): DrawnPoint[][] {
  const pt = (u: number, v: number): Vec3 => add(origin, add(scale(xAxis, u), scale(yAxis, v)));
  const p00 = pt(u0, v0);
  const p10 = pt(u1, v0);
  const p11 = pt(u1, v1);
  const p01 = pt(u0, v1);
  return [[p00, p10], [p10, p11], [p11, p01], [p01, p00]];
}

/** Approximate width of a monospace string at cap height `H`. */
function textWidthMm(text: string, H: number): number {
  let chars = 0;
  for (const ch of text) {
    if (ch === ' ') chars += 0.45;
    else if (ch === '.' || ch === '-' || ch === '1') chars += 0.72;
    else if ('⌖⌀⏥⟂∥∠⌭○↗⌰⌓⌒◎⌯⏤ⓂⓁⓈ'.includes(ch)) chars += 1.05;
    else chars += 0.84;
  }
  return Math.max(H * 0.9, chars * H * 0.88);
}

type DimMeta = SceneDimMeta;

function parseDimMeta(rawScene: unknown): Map<string, DimMeta> {
  return new Map(Object.entries(extractSceneDimensions(rawScene)));
}

interface FeatureAnchor {
  point: Vec3;
  hole?: HoleGeom;
  planeNormal?: Vec3;
}

function featurePoint(
  faces: readonly number[],
  analysis: PmiAnalysis,
  mesh: PmiMesh,
  holes: readonly HoleGeom[],
): FeatureAnchor | null {
  const fset = new Set(faces);
  const matchingHole = holes.find((h) => h.count === 1 && h.faces.some((f) => fset.has(f)));
  if (matchingHole) return { point: matchingHole.location, hole: matchingHole };

  const faceObjs = faces.map((id) => analysis.faces.find((f) => f.id === id)).filter((x): x is NonNullable<typeof x> => Boolean(x));
  const plane = faceObjs.find((f) => f.kind === 'plane');
  if (plane) {
    const n = plane.direction ? unit(plane.direction) ?? undefined : undefined;
    return {
      point: [plane.centroid[0], plane.centroid[1], plane.centroid[2]],
      ...(n ? { planeNormal: n } : {}),
    };
  }
  if (faceObjs.length) {
    const c: Vec3 = [0, 0, 0];
    for (const f of faceObjs) {
      c[0] += f.centroid[0] / faceObjs.length;
      c[1] += f.centroid[1] / faceObjs.length;
      c[2] += f.centroid[2] / faceObjs.length;
    }
    return { point: c };
  }
  let sx = 0, sy = 0, sz = 0, cnt = 0;
  for (const fid of faces) {
    const [start, count] = mesh.faces[fid] ?? [0, 0];
    for (let k = start * 3; k < (start + count) * 3; k++) {
      const v = mesh.indices[k] ?? 0;
      sx += mesh.positions[3 * v] ?? 0;
      sy += mesh.positions[3 * v + 1] ?? 0;
      sz += mesh.positions[3 * v + 2] ?? 0;
      cnt += 1;
    }
  }
  return cnt ? { point: [sx / cnt, sy / cnt, sz / cnt] } : null;
}

interface LayoutContext {
  readonly pmi: DrawnPmi;
  readonly analysis: PmiAnalysis;
  readonly mesh: PmiMesh;
  readonly semantic: ReadonlyMap<string, SceneSemanticLabel>;
  readonly cam: CameraBasis;
  readonly bmin: Vec3;
  readonly bmax: Vec3;
  readonly centre: Vec3;
  readonly diag: number;
  readonly H: number;
  readonly frameH: number;
  readonly arrLen: number;
  readonly arrHalfW: number;
  readonly visibleSigns: readonly [number, number, number];
  readonly frontNormals: readonly Vec3[];
  readonly partHull: readonly [number, number][];
  readonly hasNative: boolean;
  readonly holes: readonly HoleGeom[];
  readonly faceById: ReadonlyMap<number, PmiAnalysis['faces'][number]>;
  readonly occupied: Rect2[];
  readonly existingLeaders: [number, number, number, number][];
  readonly notesKeepOut: Rect2 | null;
  readonly nextItems: Record<string, readonly DrawnGraphic[]>;
  readonly nextPlanes: DrawnPlane[];
}

function registerPlane(ctx: LayoutContext, id: string, name: string, origin: Vec3, basis: PlaneBasis): void {
  ctx.nextPlanes.push({ id, name, origin, xAxis: basis.xAxis, normal: basis.normal });
}

function scorePlacement(
  ctx: LayoutContext,
  boxPts: readonly DrawnPoint[],
  leader?: [Vec3, Vec3],
  preferOutsidePart = true,
): number {
  const rect = rectOfPoints(boxPts, ctx.cam, ctx.H * 0.3);
  let penalty = 0;
  for (const occ of ctx.occupied) {
    if (rectsOverlap(rect, occ)) penalty += 6000 + overlapArea(rect, occ) * 25;
  }
  if (ctx.notesKeepOut && rectsOverlap(rect, ctx.notesKeepOut)) {
    penalty += 2200 + overlapArea(rect, ctx.notesKeepOut) * 15;
  }
  if (preferOutsidePart) {
    const hullFrac = rectHullOverlap(rect, ctx.partHull, ctx.H * 0.25);
    if (hullFrac > 0) penalty += 2400 + hullFrac * 3600;
  }
  if (leader) {
    const [ax, ay] = project2(leader[0], ctx.cam);
    const [bx, by] = project2(leader[1], ctx.cam);
    for (const [cx, cy, dx, dy] of ctx.existingLeaders) {
      if (segsIntersect2(ax, ay, bx, by, cx, cy, dx, dy)) penalty += 2200;
    }
    for (const occ of ctx.occupied) {
      if (segIntersectsRect2(ax, ay, bx, by, occ)) penalty += 3200;
    }
    penalty += Math.hypot(bx - ax, by - ay) * 0.45;
  }
  return penalty;
}

/**
 * Separates file-native CAD graphics from Specify-created graphics and drops post-Save
 * `features:*` aliases that share a Specify-created STEP graphic id.
 */
function partitionGraphics(pmi: DrawnPmi): {
  specifyItems: [string, DrawnGraphic][];
  nativeItems: Record<string, readonly DrawnGraphic[]>;
} {
  const itemsEntries = Object.entries(pmi.presentation.items);
  const specifyGraphicIds = new Set<string>();
  for (const [item, gs] of itemsEntries) {
    for (const g of gs) {
      if (g.id && isSpecifyCreated(item, g)) specifyGraphicIds.add(g.id);
    }
  }
  const specifyItems: [string, DrawnGraphic][] = [];
  const nativeItems: Record<string, readonly DrawnGraphic[]> = {};
  for (const [item, gs] of itemsEntries) {
    const native: DrawnGraphic[] = [];
    let specG: DrawnGraphic | null = null;
    for (const g of gs) {
      if (isSpecifyCreated(item, g)) {
        if (!specG) specG = g;
      } else if (g.id && specifyGraphicIds.has(g.id)) {
        continue;
      } else {
        native.push(g);
      }
    }
    if (native.length) nativeItems[item] = native;
    if (specG) specifyItems.push([item, specG]);
  }
  return { specifyItems, nativeItems };
}

/** Exact start point of a linear dimension's extension line directly on the referenced feature. */
function witnessStartPoint(
  ctx: LayoutContext,
  src: FeatureAnchor,
  srcExt: FaceExtent | null,
  axis: Axis,
  coord: number,
  pAxis: Axis,
  planeCoord: number,
  eAxis: Axis,
  eSign: 1 | -1,
): Vec3 {
  const pt: Vec3 = [0, 0, 0];
  pt[axis] = coord;
  pt[pAxis] = planeCoord;
  if (src.hole) {
    if (Math.abs(src.hole.axis[eAxis]!) > 0.9 && src.hole.openings.length) {
      // Hole axis runs along the extension direction: start directly on the hole opening on that side.
      const opCoords = src.hole.openings.map((o) => o.center[eAxis]!);
      pt[eAxis] = eSign > 0 ? Math.max(...opCoords) : Math.min(...opCoords);
      return pt;
    }
    // Hole axis perpendicular to extension direction: start on the circular rim of the nearest opening.
    let rad = src.hole.diameter / 2;
    if (src.hole.openings.length) {
      const nearestOp = src.hole.openings.slice().sort(
        (a, b) => Math.abs(a.center[pAxis]! - planeCoord) - Math.abs(b.center[pAxis]! - planeCoord),
      )[0]!;
      rad = nearestOp.radius;
    }
    pt[eAxis] = src.point[eAxis]! + eSign * rad;
    return pt;
  }
  if (srcExt && srcExt.verts.length) {
    // Planar or general face: use the actual mesh vertex extent of this face near `planeCoord`.
    const band = Math.max(1.5, ctx.H * 0.85);
    const near = srcExt.verts.filter((v) => Math.abs(v[pAxis]! - planeCoord) <= band);
    const pool = near.length ? near : srcExt.verts;
    let lim = eSign > 0 ? -Infinity : Infinity;
    for (const v of pool) {
      const c = v[eAxis]!;
      if (eSign > 0 ? c > lim : c < lim) lim = c;
    }
    if (Number.isFinite(lim)) {
      pt[eAxis] = lim;
      return pt;
    }
  }
  pt[eAxis] = eSign > 0 ? ctx.bmax[eAxis]! : ctx.bmin[eAxis]!;
  return pt;
}

/** Places one linear or basic dimension on the best cardinal plane containing its measurement axis. */
function placeLinearDimension(
  ctx: LayoutContext,
  item: string,
  g: DrawnGraphic,
  meta: DimMeta,
  isGeneral: boolean,
): void {
  const f1 = featurePoint(meta.fromFaces, ctx.analysis, ctx.mesh, ctx.holes);
  const f2 = featurePoint(meta.toFaces, ctx.analysis, ctx.mesh, ctx.holes);
  if (!f1 || !f2) {
    ctx.nextItems[item] = [g];
    return;
  }

  let axis: Axis;
  if (meta.axisHint !== undefined) {
    axis = meta.axisHint;
  } else if (f1.planeNormal) {
    axis = [0, 1, 2].reduce((best, k) => (Math.abs(f1.planeNormal![k]!) > Math.abs(f1.planeNormal![best]!) ? k : best), 0) as Axis;
  } else {
    const d = sub(f2.point, f1.point);
    axis = [0, 1, 2].reduce((best, k) => (Math.abs(d[k]!) > Math.abs(d[best]!) ? k : best), 0) as Axis;
  }

  const c1 = f1.point[axis]!;
  const c2 = f2.point[axis]!;
  const lo = Math.min(c1, c2);
  const hi = Math.max(c1, c2);
  const dimSpan = Math.abs(hi - lo);
  const pLoSrc = c1 <= c2 ? f1 : f2;
  const pHiSrc = c1 <= c2 ? f2 : f1;
  const pLoFaces = c1 <= c2 ? meta.fromFaces : meta.toFaces;
  const pHiFaces = c1 <= c2 ? meta.toFaces : meta.fromFaces;
  const loExt = faceMeshExtent(pLoFaces, ctx.mesh);
  const hiExt = faceMeshExtent(pHiFaces, ctx.mesh);
  const uUnit: Vec3 = [0, 0, 0]; uUnit[axis] = 1;
  const dirLoToHi = uUnit;
  const dirHiToLo: Vec3 = [-uUnit[0], -uUnit[1], -uUnit[2]];

  const otherAxes = ([0, 1, 2] as const).filter((k) => k !== axis);
  const textW = textWidthMm(meta.text, ctx.H) + (meta.basic ? ctx.H * 0.9 : ctx.H * 0.4);
  const boxH = meta.basic ? ctx.frameH : ctx.H * 1.4;

  interface DimCandidate {
    planeNormal: Vec3;
    planeAxis: Axis;
    extAxis: Axis;
    extSign: 1 | -1;
    planeCoord: number;
    extCoord: number;
    sideSign: 1 | -1;
    score: number;
  }
  const dimCands: DimCandidate[] = [];
  const visiblePlaneAxes = otherAxes.filter((pAxis) => {
    const pNormal: Vec3 = [0, 0, 0];
    pNormal[pAxis] = ctx.visibleSigns[pAxis]!;
    return dot(pNormal, ctx.cam.eye) >= 0.2;
  });
  const candidatePlaneAxes = visiblePlaneAxes.length ? visiblePlaneAxes : otherAxes;

  for (const pAxis of candidatePlaneAxes) {
    const pSign = ctx.visibleSigns[pAxis]!;
    const pNormal: Vec3 = [0, 0, 0];
    pNormal[pAxis] = pSign;
    const eAxis = otherAxes.find((k) => k !== pAxis)!;
    const measRight: Vec3 = dot(uUnit, ctx.cam.right) >= 0 ? uUnit : dirHiToLo;
    const candBasis = readablePlaneBasis(pNormal, ctx.cam, measRight);
    const textAlongDim = Math.abs(dot(candBasis.xAxis, uUnit)) > 0.5 ? textW : boxH;
    const fitsInside = dimSpan >= textAlongDim + ctx.arrLen * 2.6;

    // Choose an annotation plane where both referenced features genuinely overlap.
    const holeOpensOnPlane = Boolean(
      pLoSrc.hole?.openings.some((o) => dot(o.normal, pNormal) > 0.9)
      && pHiSrc.hole?.openings.some((o) => dot(o.normal, pNormal) > 0.9),
    );
    let planeCoord = pSign > 0 ? ctx.bmax[pAxis]! : ctx.bmin[pAxis]!;
    let overlapPen = 0;
    if (holeOpensOnPlane && pLoSrc.hole) {
      const holeVis = pLoSrc.hole.openings.find((o) => dot(o.normal, pNormal) > 0.9);
      if (holeVis) planeCoord = holeVis.center[pAxis]!;
    } else if (loExt && hiExt) {
      const ovMin = Math.max(loExt.min[pAxis]!, hiExt.min[pAxis]!);
      const ovMax = Math.min(loExt.max[pAxis]!, hiExt.max[pAxis]!);
      const perpHole = [pLoSrc.hole, pHiSrc.hole].find((h) => h && Math.abs(h.axis[pAxis]!) < 0.1);
      if (perpHole && perpHole.location[pAxis]! >= ovMin - 0.15 && perpHole.location[pAxis]! <= ovMax + 0.15) {
        planeCoord = perpHole.location[pAxis]!;
      } else if (ovMin <= ovMax + 0.05) {
        planeCoord = pSign > 0 ? ovMax : ovMin;
      } else {
        planeCoord = loExt.max[pAxis]! < hiExt.min[pAxis]!
          ? (loExt.max[pAxis]! + hiExt.min[pAxis]!) / 2
          : (hiExt.max[pAxis]! + loExt.min[pAxis]!) / 2;
        overlapPen = 1800;
      }
    }

    for (const eSign of [-1, 1] as const) {
      // If a hole's axis runs along eAxis, strongly prefer extending from its visible opening side.
      let wrongHoleEndPen = 0;
      for (const src of [pLoSrc, pHiSrc]) {
        if (src.hole && Math.abs(src.hole.axis[eAxis]!) > 0.9 && src.hole.openings.length) {
          const hasVisOnSide = src.hole.openings.some((o) => dot(o.normal, ctx.cam.eye) > 0.15 && o.normal[eAxis]! * eSign > 0.5);
          const hasVisOtherSide = src.hole.openings.some((o) => dot(o.normal, ctx.cam.eye) > 0.15 && o.normal[eAxis]! * eSign < -0.5);
          if (!hasVisOnSide && hasVisOtherSide) wrongHoleEndPen += 4500;
        }
      }
      const edgeLimit = eSign > 0 ? ctx.bmax[eAxis]! : ctx.bmin[eAxis]!;
      const holeEdge = Math.max(
        eSign > 0 ? pLoSrc.point[eAxis]! : -pLoSrc.point[eAxis]!,
        eSign > 0 ? pHiSrc.point[eAxis]! : -pHiSrc.point[eAxis]!,
      ) * eSign;
      const candidateExtCoords: { coord: number; local: boolean; mult: number }[] = [
        { coord: edgeLimit + eSign * ctx.H * 1.9, local: false, mult: 1.9 },
        { coord: edgeLimit + eSign * ctx.H * 3.4, local: false, mult: 3.4 },
        { coord: edgeLimit + eSign * ctx.H * 5.2, local: false, mult: 5.2 },
        { coord: edgeLimit + eSign * ctx.H * 7.2, local: false, mult: 7.2 },
      ];
      const outwardFromCentre = eSign * (holeEdge - ctx.centre[eAxis]!) >= -1e-3;
      if (holeOpensOnPlane && ctx.hasNative && outwardFromCentre) {
        if (dimSpan > ctx.H * 16) {
          candidateExtCoords.push(
            { coord: holeEdge + eSign * ctx.H * 2.6, local: true, mult: 1.1 },
            { coord: holeEdge + eSign * ctx.H * 4.2, local: true, mult: 1.6 },
          );
        } else {
          candidateExtCoords.push(
            { coord: holeEdge + eSign * ctx.H * 4.2, local: true, mult: 1.5 },
            { coord: holeEdge + eSign * ctx.H * 8.2, local: true, mult: 2.0 },
          );
        }
      }
      const candLoStart = witnessStartPoint(ctx, pLoSrc, loExt, axis, lo, pAxis, planeCoord, eAxis, eSign);
      const candHiStart = witnessStartPoint(ctx, pHiSrc, hiExt, axis, hi, pAxis, planeCoord, eAxis, eSign);
      const sideSigns: readonly (1 | -1)[] = fitsInside ? [1] : [1, -1];
      for (const { coord: extCoord, local, mult } of candidateExtCoords) {
        const pA: Vec3 = [0, 0, 0]; pA[axis] = lo; pA[pAxis] = planeCoord; pA[eAxis] = extCoord;
        const pB: Vec3 = [0, 0, 0]; pB[axis] = hi; pB[pAxis] = planeCoord; pB[eAxis] = extCoord;
        const dimMid = scale(add(pA, pB), 0.5);
        const [mx, my] = project2(dimMid, ctx.cam);
        const dimLineInHull = !local && (
          pointInHull2(mx, my, ctx.partHull, ctx.H * 0.2)
          || pointInHull2(...project2(pA, ctx.cam), ctx.partHull, ctx.H * 0.15)
          || pointInHull2(...project2(pB, ctx.cam), ctx.partHull, ctx.H * 0.15)
        );
        let extCrossPen = 0;
        for (const [sPt, ePt] of [[candLoStart, pA], [candHiStart, pB]] as const) {
          const [sx, sy] = project2(sPt, ctx.cam);
          const [ex, ey] = project2(ePt, ctx.cam);
          for (const occ of ctx.occupied) {
            if (segIntersectsRect2(sx, sy, ex, ey, occ)) extCrossPen += 1800;
          }
        }
        for (const sideSign of sideSigns) {
          let textMid: Vec3;
          if (fitsInside) {
            textMid = [0, 0, 0];
            textMid[axis] = (lo + hi) / 2;
            textMid[pAxis] = planeCoord;
            textMid[eAxis] = extCoord;
          } else {
            const sDir = sideSign > 0 ? dirLoToHi : dirHiToLo;
            const sTip = add(sideSign > 0 ? pB : pA, scale(sDir, ctx.arrLen * 1.9));
            textMid = add(sTip, scale(sDir, textAlongDim / 2 + ctx.H * 0.35));
          }
          const boxPts = [
            add(textMid, add(scale(candBasis.xAxis, -textW / 2), scale(candBasis.yAxis, -boxH / 2))),
            add(textMid, add(scale(candBasis.xAxis, textW / 2), scale(candBasis.yAxis, boxH / 2))),
          ];
          const openingBonus = holeOpensOnPlane ? -2500
            : (pLoSrc.hole?.openings.some((o) => dot(o.normal, pNormal) > 0.9)
              || pHiSrc.hole?.openings.some((o) => dot(o.normal, pNormal) > 0.9)) ? -260 : 0;
          const crossDist = Math.abs(extCoord - candLoStart[eAxis]!) + Math.abs(extCoord - candHiStart[eAxis]!);
          const sc = scorePlacement(ctx, boxPts, [pA, pB], !local)
            + overlapPen
            + wrongHoleEndPen
            + (dimLineInHull ? 2600 : 0)
            + extCrossPen
            + (local ? 40 : 0)
            + openingBonus
            + crossDist * (holeOpensOnPlane && ctx.hasNative ? 3.5 : 0.45)
            + mult * 30
            + (1 - dot(pNormal, ctx.cam.eye)) * 140;
          dimCands.push({ planeNormal: pNormal, planeAxis: pAxis, extAxis: eAxis, extSign: eSign, planeCoord, extCoord, sideSign, score: sc });
        }
      }
    }
  }

  dimCands.sort((a, b) => a.score - b.score);
  const best = dimCands[0];
  if (!best) {
    ctx.nextItems[item] = [g];
    return;
  }
  const { planeNormal, planeAxis, extAxis, extSign, planeCoord, extCoord, sideSign } = best;
  const measRight: Vec3 = dot(uUnit, ctx.cam.right) >= 0 ? uUnit : dirHiToLo;
  const basis = readablePlaneBasis(planeNormal, ctx.cam, measRight);
  const overshoot = ctx.H * 0.7;

  const extLoStart = witnessStartPoint(ctx, pLoSrc, loExt, axis, lo, planeAxis, planeCoord, extAxis, extSign);
  const extHiStart = witnessStartPoint(ctx, pHiSrc, hiExt, axis, hi, planeAxis, planeCoord, extAxis, extSign);
  const extLoEnd: Vec3 = [...extLoStart]; extLoEnd[extAxis] = extCoord + extSign * overshoot;
  const extHiEnd: Vec3 = [...extHiStart]; extHiEnd[extAxis] = extCoord + extSign * overshoot;
  const dimLo: Vec3 = [...extLoStart]; dimLo[extAxis] = extCoord;
  const dimHi: Vec3 = [...extHiStart]; dimHi[extAxis] = extCoord;

  const polylines: DrawnPoint[][] = [
    [extLoStart, extLoEnd],
    [extHiStart, extHiEnd],
  ];
  const polylineRoles: string[] = ['extension', 'extension'];
  const triangles: (readonly [DrawnPoint, DrawnPoint, DrawnPoint])[] = [];

  const textAlongDim = Math.abs(dot(basis.xAxis, uUnit)) > 0.5 ? textW : boxH;
  const fitsInside = dimSpan >= textAlongDim + ctx.arrLen * 2.6;

  let textCenter: Vec3;
  if (fitsInside) {
    const midU = (lo + hi) / 2;
    const gapHalf = textAlongDim / 2 + ctx.H * 0.25;
    const breakLo: Vec3 = [...dimLo]; breakLo[axis] = midU - gapHalf;
    const breakHi: Vec3 = [...dimHi]; breakHi[axis] = midU + gapHalf;
    polylines.push([dimLo, breakLo], [breakHi, dimHi]);
    polylineRoles.push('dimension', 'dimension');
    triangles.push(
      arrowTriangle(dimLo, dirHiToLo, planeNormal, ctx.arrLen, ctx.arrHalfW),
      arrowTriangle(dimHi, dirLoToHi, planeNormal, ctx.arrLen, ctx.arrHalfW),
    );
    textCenter = [...dimLo];
    textCenter[axis] = midU;
  } else {
    const tailLen = ctx.arrLen * 1.9;
    const outerLo = add(dimLo, scale(dirHiToLo, tailLen));
    const outerHi = add(dimHi, scale(dirLoToHi, tailLen));
    polylines.push([outerLo, outerHi]);
    polylineRoles.push('dimension');
    triangles.push(
      arrowTriangle(dimLo, dirLoToHi, planeNormal, ctx.arrLen, ctx.arrHalfW),
      arrowTriangle(dimHi, dirHiToLo, planeNormal, ctx.arrLen, ctx.arrHalfW),
    );
    const sideDir = sideSign > 0 ? dirLoToHi : dirHiToLo;
    const sideTip = sideSign > 0 ? outerHi : outerLo;
    const textDist = textAlongDim / 2 + ctx.H * 0.35;
    const tailEnd = add(sideTip, scale(sideDir, ctx.H * 0.35));
    polylines.push([sideTip, tailEnd]);
    polylineRoles.push('dimension');
    textCenter = add(sideTip, scale(sideDir, textDist));
  }

  if (meta.basic) {
    for (const l of rectPolylines(textCenter, basis.xAxis, basis.yAxis, -textW / 2, -boxH / 2, textW / 2, boxH / 2)) {
      polylines.push(l);
      polylineRoles.push('frame');
    }
  }

  const planeId = `drafting:${item}/plane`;
  registerPlane(ctx, planeId, meta.text, textCenter, basis);
  const tp: TextPlacement = {
    textIndex: 0,
    origin: textCenter,
    xAxis: basis.xAxis,
    normal: basis.normal,
    height: ctx.H,
    alignment: 'centre',
    cellBox: [[-textW / 2, -boxH / 2], [textW / 2, boxH / 2]],
  };
  const room = balloonSlot(basis, add(textCenter, scale(basis.yAxis, boxH / 2)), basis.yAxis, ctx.H);
  ctx.nextItems[item] = [{
    id: `drafting:${item}`,
    source: 'generated',
    name: meta.text,
    polylines,
    polylineRoles,
    triangles,
    text: [meta.text],
    textPlacements: [tp],
    planeId,
    balloonSlot: room.slot,
  }];
  if (!isGeneral) {
    const dimBoxPts = [
      ...room.pts,
      add(textCenter, add(scale(basis.xAxis, -textW / 2), scale(basis.yAxis, -boxH / 2))),
      add(textCenter, add(scale(basis.xAxis, textW / 2), scale(basis.yAxis, boxH / 2))),
    ];
    ctx.occupied.push(rectOfPoints(dimBoxPts, ctx.cam, ctx.H * 0.45));
    ctx.existingLeaders.push([...project2(dimLo, ctx.cam), ...project2(dimHi, ctx.cam)]);
    ctx.existingLeaders.push([...project2(extLoStart, ctx.cam), ...project2(extLoEnd, ctx.cam)]);
    ctx.existingLeaders.push([...project2(extHiStart, ctx.cam), ...project2(extHiEnd, ctx.cam)]);
  }
}

/** Places a datum symbol with its filled triangle flush on the datum face mesh and a perpendicular stem. */
function placeDatumSymbol(ctx: LayoutContext, item: string, g: DrawnGraphic): void {
  const sem = ctx.semantic.get(item);
  const letter = sem?.cells[0] ?? g.name.replace(/^Datum\s+/i, '').trim() ?? 'A';
  const faces = ctx.pmi.links[item]?.faces ?? [];
  const face = faces.map((id) => ctx.faceById.get(id)).find((f) => f?.kind === 'plane');
  const tris = faceTriangles(faces, ctx.mesh);
  const fExt = faceMeshExtent(faces, ctx.mesh);
  const boxSize = ctx.frameH;
  const triH = ctx.H * 0.78;
  const triHalfW = ctx.H * 0.52;

  if (face) {
    const faceNormal = (face.direction ? unit(face.direction) : null) ?? [0, -1, 0];
    const fnAxis = [0, 1, 2].reduce((best, k) => (Math.abs(faceNormal[k]!) > Math.abs(faceNormal[best]!) ? k : best), 0) as Axis;
    const fnSign = faceNormal[fnAxis]! >= 0 ? 1 : -1;
    const outDir: Vec3 = [0, 0, 0];
    outDir[fnAxis] = fnSign;
    const faceCoord = face.centroid[fnAxis]!;
    const perpNormals = ctx.frontNormals
      .filter((n) => Math.abs(dot(n, outDir)) < 0.1)
      .sort((a, b) => dot(b, ctx.cam.eye) - dot(a, ctx.cam.eye));

    const uAxis = ([0, 1, 2] as const).find((k) => k !== fnAxis)!;
    const vAxis = ([0, 1, 2] as const).find((k) => k !== fnAxis && k !== uAxis)!;
    const uMin = fExt ? fExt.min[uAxis]! : ctx.bmin[uAxis]!;
    const uMax = fExt ? fExt.max[uAxis]! : ctx.bmax[uAxis]!;
    const vMin = fExt ? fExt.min[vAxis]! : ctx.bmin[vAxis]!;
    const vMax = fExt ? fExt.max[vAxis]! : ctx.bmax[vAxis]!;
    const faceVisible = dot(outDir, ctx.cam.eye) > 0.15;

    // Sample candidate anchor points strictly on actual mesh triangles of the datum face.
    const onFaceAnchors: { pt: Vec3; centerBias: number }[] = [];
    const fracs = [0.12, 0.24, 0.36, 0.5, 0.64, 0.76, 0.88];
    if (faceVisible && tris.length) {
      for (const uf of fracs) {
        for (const vf of fracs) {
          const cand: Vec3 = [0, 0, 0];
          cand[fnAxis] = faceCoord;
          cand[uAxis] = uMin + (uMax - uMin) * uf;
          cand[vAxis] = vMin + (vMax - vMin) * vf;
          const hit = nearestOnTriangles(cand, tris);
          if (!hit || hit.dist > 0.05) continue;
          onFaceAnchors.push({ pt: hit.point, centerBias: Math.hypot(uf - 0.5, vf - 0.5) });
        }
      }
    }
    if (!onFaceAnchors.length) {
      for (const uf of [0.25, 0.5, 0.75]) {
        for (const vf of [0.0, 0.5, 1.0]) {
          const cand: Vec3 = [0, 0, 0];
          cand[fnAxis] = faceCoord;
          cand[uAxis] = uMin + (uMax - uMin) * uf;
          cand[vAxis] = vMin + (vMax - vMin) * vf;
          const hit = tris.length ? nearestOnTriangles(cand, tris) : null;
          onFaceAnchors.push({ pt: hit ? hit.point : cand, centerBias: Math.hypot(uf - 0.5, vf - 0.5) });
        }
      }
    }

    const stemLens = [ctx.H * 1.6, ctx.H * 2.1, ctx.H * 3.1, ctx.H * 4.3, ctx.H * 5.6, ctx.H * 7.2, ctx.H * 9.2];
    let bestDatum: { basis: PlaneBasis; anchor: Vec3; center: Vec3; stemLen: number; score: number } | null = null;
    const [cx, cy] = project2(ctx.centre, ctx.cam);

    for (const chosenNormal of perpNormals) {
      const basis = readablePlaneBasis(chosenNormal, ctx.cam);
      const perpInPlane = unit(cross(basis.normal, outDir)) ?? basis.xAxis;
      for (const { pt: anchor, centerBias } of onFaceAnchors) {
        let edgePen = 0;
        if (tris.length) {
          const b1 = nearestOnTriangles(add(anchor, scale(perpInPlane, triHalfW)), tris);
          const b2 = nearestOnTriangles(add(anchor, scale(perpInPlane, -triHalfW)), tris);
          const maxBaseOff = Math.max(b1?.dist ?? 0, b2?.dist ?? 0);
          if (maxBaseOff > 0.12) edgePen = 1600 + maxBaseOff * 800;
        }
        const [ax2, ay2] = project2(anchor, ctx.cam);
        for (const stemLen of stemLens) {
          const boxCenter = add(anchor, scale(outDir, stemLen + boxSize / 2));
          const corners = [
            add(boxCenter, add(scale(basis.xAxis, -boxSize / 2), scale(basis.yAxis, -boxSize / 2))),
            add(boxCenter, add(scale(basis.xAxis, boxSize / 2), scale(basis.yAxis, boxSize / 2))),
          ];
          const [bx2, by2] = project2(boxCenter, ctx.cam);
          const inward = (bx2 - ax2) * (cx - ax2) + (by2 - ay2) * (cy - ay2) > 0;
          const slotPts = balloonSlot(basis, add(boxCenter, scale(outDir, boxSize / 2)), outDir, ctx.H).pts;
          const sc = scorePlacement(ctx, [...corners, ...slotPts], [anchor, boxCenter], true)
            + edgePen
            + (inward ? 3500 : 0)
            + stemLen * 18
            + centerBias * 55
            + (1 - dot(chosenNormal, ctx.cam.eye)) * 120;
          if (!bestDatum || sc < bestDatum.score) {
            bestDatum = { basis, anchor, center: boxCenter, stemLen, score: sc };
          }
        }
      }
    }

    if (!bestDatum) {
      ctx.nextItems[item] = [g];
      return;
    }
    const { basis, anchor, center, stemLen } = bestDatum;
    const perpInPlane = unit(cross(basis.normal, outDir)) ?? basis.xAxis;
    const triApex = add(anchor, scale(outDir, triH));
    const tri: readonly [DrawnPoint, DrawnPoint, DrawnPoint] = [
      triApex,
      add(anchor, scale(perpInPlane, triHalfW)),
      add(anchor, scale(perpInPlane, -triHalfW)),
    ];
    const stemEnd = add(anchor, scale(outDir, stemLen));
    const frameLines = rectPolylines(center, basis.xAxis, basis.yAxis, -boxSize / 2, -boxSize / 2, boxSize / 2, boxSize / 2);
    const planeId = `drafting:${item}/plane`;
    registerPlane(ctx, planeId, letter, center, basis);
    const room = balloonSlot(basis, add(center, scale(outDir, boxSize / 2)), outDir, ctx.H);
    const tp: TextPlacement = {
      textIndex: 0,
      origin: center,
      xAxis: basis.xAxis,
      normal: basis.normal,
      height: ctx.H,
      alignment: 'centre',
      cellBox: [[-boxSize / 2, -boxSize / 2], [boxSize / 2, boxSize / 2]],
    };
    ctx.nextItems[item] = [{
      id: `drafting:${item}`,
      source: 'generated',
      name: `Datum ${letter}`,
      polylines: [...frameLines, [triApex, stemEnd]],
      polylineRoles: ['frame', 'frame', 'frame', 'frame', 'leader'],
      triangles: [tri],
      text: [letter],
      textPlacements: [tp],
      planeId,
      balloonSlot: room.slot,
    }];
    ctx.occupied.push(rectOfPoints([...frameLines.flat(), ...room.pts], ctx.cam, ctx.H * 0.35));
    ctx.existingLeaders.push([...project2(anchor, ctx.cam), ...project2(stemEnd, ctx.cam)]);
    return;
  }

  // Non-planar datum (cylinder/hole): anchor on the hole rim or nearest mesh surface point.
  const matchingHole = ctx.holes.find((h) => faces.some((f) => h.faces.includes(f)));
  const fp = featurePoint(faces, ctx.analysis, ctx.mesh, ctx.holes);
  const bestNormal = ctx.frontNormals.slice().sort((a, b) => dot(b, ctx.cam.eye) - dot(a, ctx.cam.eye))[0]!;
  const basis = readablePlaneBasis(bestNormal, ctx.cam);
  const rawTip: Vec3 = matchingHole?.openings[0]
    ? add(matchingHole.openings[0].center, scale(basis.xAxis, matchingHole.openings[0].radius))
    : (tris.length && fp ? nearestOnTriangles(fp.point, tris)?.point ?? fp.point : fp?.point ?? ctx.centre);
  const dir = unit(add(scale(basis.xAxis, 0.7), scale(basis.yAxis, 0.7)))!;
  const stemEnd = add(rawTip, scale(dir, ctx.H * 3.5));
  const center = add(stemEnd, scale(dir, boxSize / 2));
  const frameLines = rectPolylines(center, basis.xAxis, basis.yAxis, -boxSize / 2, -boxSize / 2, boxSize / 2, boxSize / 2);
  const planeId = `drafting:${item}/plane`;
  registerPlane(ctx, planeId, letter, center, basis);
  const awayDir = slotDir(basis, dir);
  const room = balloonSlot(basis, add(center, scale(awayDir, boxSize / 2)), awayDir, ctx.H);
  const tp: TextPlacement = {
    textIndex: 0, origin: center, xAxis: basis.xAxis, normal: basis.normal,
    height: ctx.H, alignment: 'centre', cellBox: [[-boxSize / 2, -boxSize / 2], [boxSize / 2, boxSize / 2]],
  };
  ctx.nextItems[item] = [{
    id: `drafting:${item}`, source: 'generated', name: `Datum ${letter}`,
    polylines: [...frameLines, [rawTip, stemEnd]],
    polylineRoles: ['frame', 'frame', 'frame', 'frame', 'leader'],
    triangles: surfaceDot(rawTip, basis.xAxis, basis.yAxis, ctx.H * 0.24),
    text: [letter], textPlacements: [tp], planeId, balloonSlot: room.slot,
  }];
  ctx.occupied.push(rectOfPoints([...frameLines.flat(), ...room.pts], ctx.cam, ctx.H * 0.35));
  ctx.existingLeaders.push([...project2(rawTip, ctx.cam), ...project2(stemEnd, ctx.cam)]);
}

interface CalloutEntry {
  item: string;
  g: DrawnGraphic;
  sem: SceneSemanticLabel;
  isPrimary: boolean;
  isGeneral: boolean;
  cells: string[];
  isFrame: boolean;
  width: number;
  height: number;
  cellWidths: number[];
}

interface FeatureCalloutGroup {
  faces: number[];
  isGeneral: boolean;
  entries: CalloutEntry[];
}

/** The far corners of the room kept for a feature callout stack's balloons, past its end away from the leader. */
function groupSlotPts(basis: PlaneBasis, bodyLeft: Vec3, dirSign: 1 | -1, maxW: number, totalH: number, H: number): Vec3[] {
  const x0 = dirSign > 0 ? maxW + BALLOON_SLOT_W * H : -BALLOON_SLOT_W * H;
  return [add(bodyLeft, scale(basis.xAxis, x0)), add(add(bodyLeft, scale(basis.xAxis, x0)), scale(basis.yAxis, -totalH))];
}

/** Places a stack of feature callouts sharing a single leader anchored to the hole rim or face mesh. */
function placeFeatureCalloutGroup(ctx: LayoutContext, grp: FeatureCalloutGroup): void {
  // Thread/size callouts stack directly above their FCFs.
  grp.entries.sort((a, b) => (a.isPrimary === b.isPrimary ? a.item.localeCompare(b.item) : a.isPrimary ? -1 : 1));
  const faces = grp.faces;
  const matchingHole = ctx.holes.find((h) => h.faces.length > 0 && h.faces.every((f) => faces.includes(f)) && faces.every((f) => h.faces.includes(f)))
    ?? ctx.holes.find((h) => faces.some((f) => h.faces.includes(f)));

  const totalH = grp.entries.reduce((s, e) => s + e.height, 0);
  const maxW = Math.max(...grp.entries.map((e) => e.width));
  const shelfLen = ctx.H * 1.25;
  const shelfGap = grp.entries[0]!.isFrame ? 0 : ctx.H * 0.25;

  interface CandidateCallout {
    basis: PlaneBasis;
    tip: Vec3;
    elbow: Vec3;
    bodyLeft: Vec3;
    dirSign: 1 | -1;
    isDot: boolean;
    score: number;
  }
  const candidates: CandidateCallout[] = [];

  if (matchingHole) {
    const visOpenings = matchingHole.openings
      .filter((op) => dot(op.normal, ctx.cam.eye) > 0.15)
      .sort((a, b) => dot(b.normal, ctx.cam.eye) - dot(a.normal, ctx.cam.eye));
    const useOpenings = visOpenings.length ? visOpenings : [matchingHole.openings[0]!];

    for (const op of useOpenings) {
      const opAxis = [0, 1, 2].reduce((best, k) => (Math.abs(op.normal[k]!) > Math.abs(op.normal[best]!) ? k : best), 0) as Axis;
      const nSign = op.normal[opAxis]! >= 0 ? 1 : -1;
      const planeNormal: Vec3 = [0, 0, 0];
      planeNormal[opAxis] = nSign;
      const candidateNormals: Vec3[] = [planeNormal];
      if (!ctx.hasNative) {
        for (const fn of ctx.frontNormals) {
          if (Math.abs(dot(fn, planeNormal)) < 0.1 && dot(fn, ctx.cam.eye) > 0.55) candidateNormals.push(fn);
        }
      }

      for (const pNormal of candidateNormals) {
        const basis = readablePlaneBasis(pNormal, ctx.cam);
        const sameAsOpening = Math.abs(dot(pNormal, planeNormal)) > 0.9;
        const rimAxis = sameAsOpening ? null : unit(cross(op.normal, pNormal));
        const dirs: [number, number][] = [
          [0.7, 0.7], [-0.7, 0.7], [0.7, -0.7], [-0.7, -0.7],
          [0, 1], [0, -1], [1, 0], [-1, 0],
        ];
        for (const [du, dv] of dirs) {
          const d3 = unit(add(scale(basis.xAxis, du), scale(basis.yAxis, dv)))!;
          const tips: Vec3[] = [];
          if (sameAsOpening) {
            tips.push(add(op.center, scale(d3, op.radius)));
          } else if (rimAxis) {
            // Leader tip sits strictly on the circular rim where the annotation plane cuts the opening.
            if (dot(d3, op.normal) < -0.35) continue;
            const rimDot = dot(d3, rimAxis);
            if (Math.abs(rimDot) < 0.15) {
              tips.push(add(op.center, scale(rimAxis, op.radius)), add(op.center, scale(rimAxis, -op.radius)));
            } else {
              tips.push(add(op.center, scale(rimAxis, (rimDot >= 0 ? 1 : -1) * op.radius)));
            }
          } else {
            tips.push(op.center);
          }
          for (const tip of tips) {
            let distToEdge = Infinity;
            for (let k = 0; k < 3; k++) {
              if (Math.abs(d3[k]!) < 1e-4) continue;
              const lim = d3[k]! > 0 ? ctx.bmax[k]! : ctx.bmin[k]!;
              const t = (lim - tip[k]!) / d3[k]!;
              if (t > 0) distToEdge = Math.min(distToEdge, t);
            }
            if (!Number.isFinite(distToEdge)) distToEdge = 0;
            const clampedEdge = Math.min(distToEdge, ctx.diag * 0.35);
            const leaderLens = [
              Math.max(ctx.H * 2.4, clampedEdge + ctx.H * 1.6),
              Math.max(ctx.H * 4.2, clampedEdge + ctx.H * 3.4),
              Math.max(ctx.H * 6.0, clampedEdge + ctx.H * 5.2),
              ...(ctx.hasNative && distToEdge > ctx.H * 9 ? [ctx.H * 12, ctx.H * 16] : []),
            ];
            for (const lLen of leaderLens) {
              const elbow = add(tip, scale(d3, lLen));
              const dirSign: 1 | -1 = du >= 0 ? 1 : -1;
              const attach = add(elbow, scale(basis.xAxis, dirSign * shelfLen));
              const bodyEdge = add(attach, scale(basis.xAxis, dirSign * shelfGap));
              const topMidY = grp.entries[0]!.height / 2;
              const bodyLeft = dirSign > 0
                ? add(bodyEdge, scale(basis.yAxis, topMidY))
                : add(bodyEdge, add(scale(basis.xAxis, -maxW), scale(basis.yAxis, topMidY)));
              const boxPts = [
                bodyLeft,
                add(bodyLeft, scale(basis.xAxis, maxW)),
                add(bodyLeft, scale(basis.yAxis, -totalH)),
                add(bodyLeft, add(scale(basis.xAxis, maxW), scale(basis.yAxis, -totalH))),
                ...groupSlotPts(basis, bodyLeft, dirSign, maxW, totalH, ctx.H),
              ];
              const planePen = sameAsOpening ? 0 : (ctx.hasNative ? 120 : 30);
              const lenPen = lLen * 3.2;
              const facingPen = (1 - dot(pNormal, ctx.cam.eye)) * (ctx.hasNative ? 180 : 340);
              let crowdPen = 0;
              const [tx, ty] = project2(tip, ctx.cam);
              for (const [cx, cy, dx, dy] of ctx.existingLeaders) {
                if (Math.hypot(tx - cx, ty - cy) < ctx.H * 2.5 || Math.hypot(tx - dx, ty - dy) < ctx.H * 2.5) crowdPen += 900;
              }
              const sc = scorePlacement(ctx, boxPts, [tip, elbow], true) + planePen + lenPen + facingPen + crowdPen;
              candidates.push({ basis, tip, elbow, bodyLeft, dirSign, isDot: false, score: sc });
            }
          }
        }
      }
    }
  } else {
    const fp = featurePoint(faces, ctx.analysis, ctx.mesh, ctx.holes);
    const tris = faceTriangles(faces, ctx.mesh);
    const rawTip: Vec3 = fp?.point ?? ctx.centre;
    const tip: Vec3 = tris.length ? (nearestOnTriangles(rawTip, tris)?.point ?? rawTip) : rawTip;
    const bestNormal = ctx.frontNormals.slice().sort((a, b) => dot(b, ctx.cam.eye) - dot(a, ctx.cam.eye))[0]!;
    const basis = readablePlaneBasis(bestNormal, ctx.cam);
    for (const [du, dv] of [[0.7, 0.7], [-0.7, 0.7], [0.7, -0.7], [-0.7, -0.7]] as [number, number][]) {
      const d3 = unit(add(scale(basis.xAxis, du), scale(basis.yAxis, dv)))!;
      const elbow = add(tip, scale(d3, ctx.H * 4.5));
      const dirSign: 1 | -1 = du >= 0 ? 1 : -1;
      const attach = add(elbow, scale(basis.xAxis, dirSign * shelfLen));
      const bodyEdge = add(attach, scale(basis.xAxis, dirSign * shelfGap));
      const topMidY = grp.entries[0]!.height / 2;
      const bodyLeft = dirSign > 0
        ? add(bodyEdge, scale(basis.yAxis, topMidY))
        : add(bodyEdge, add(scale(basis.xAxis, -maxW), scale(basis.yAxis, topMidY)));
      const boxPts = [
        bodyLeft,
        add(bodyLeft, scale(basis.xAxis, maxW)),
        add(bodyLeft, scale(basis.yAxis, -totalH)),
        add(bodyLeft, add(scale(basis.xAxis, maxW), scale(basis.yAxis, -totalH))),
        ...groupSlotPts(basis, bodyLeft, dirSign, maxW, totalH, ctx.H),
      ];
      const sc = scorePlacement(ctx, boxPts, [tip, elbow], true);
      candidates.push({ basis, tip, elbow, bodyLeft, dirSign, isDot: true, score: sc });
    }
  }

  candidates.sort((a, b) => a.score - b.score);
  const bestCallout = candidates[0];
  if (!bestCallout) {
    for (const entry of grp.entries) ctx.nextItems[entry.item] = [entry.g];
    return;
  }
  const { basis, tip, elbow, bodyLeft, dirSign, isDot } = bestCallout;
  const attach = add(elbow, scale(basis.xAxis, dirSign * shelfLen));
  const leaderDir = sub(tip, elbow);

  let yCursor = 0;
  for (const [entryIdx, entry] of grp.entries.entries()) {
    const polylines: DrawnPoint[][] = [];
    const polylineRoles: string[] = [];
    const triangles: (readonly [DrawnPoint, DrawnPoint, DrawnPoint])[] = [];
    const text: string[] = [];
    const textPlacements: TextPlacement[] = [];

    // Emit the one shared leader on the top entry only.
    if (entryIdx === 0) {
      polylines.push([tip, elbow, attach]);
      polylineRoles.push('leader');
      if (isDot) triangles.push(...surfaceDot(tip, basis.xAxis, basis.yAxis, ctx.H * 0.22));
      else triangles.push(arrowTriangle(tip, leaderDir, basis.normal, ctx.arrLen, ctx.arrHalfW));
    }

    const rowTop = add(bodyLeft, scale(basis.yAxis, -yCursor));
    const rowLeft = dirSign > 0 ? rowTop : add(rowTop, scale(basis.xAxis, maxW - entry.width));

    if (entry.isFrame) {
      for (const l of rectPolylines(rowLeft, basis.xAxis, basis.yAxis, 0, -entry.height, entry.width, 0)) {
        polylines.push(l);
        polylineRoles.push('frame');
      }
      let xOff = 0;
      for (let ci = 0; ci < entry.cells.length; ci++) {
        const cw = entry.cellWidths[ci]!;
        if (ci > 0) {
          const topDiv = add(rowLeft, scale(basis.xAxis, xOff));
          const botDiv = add(topDiv, scale(basis.yAxis, -entry.height));
          polylines.push([topDiv, botDiv]);
          polylineRoles.push('frame');
        }
        const cellMid = add(rowLeft, add(scale(basis.xAxis, xOff + cw / 2), scale(basis.yAxis, -entry.height / 2)));
        text.push(entry.cells[ci]!);
        textPlacements.push({
          textIndex: ci,
          origin: cellMid,
          xAxis: basis.xAxis,
          normal: basis.normal,
          height: ctx.H,
          alignment: 'centre',
          cellBox: [[-cw / 2, -entry.height / 2], [cw / 2, entry.height / 2]],
        });
        xOff += cw;
      }
    } else {
      const labelText = entry.cells.join(' ');
      const mid = add(rowLeft, add(scale(basis.xAxis, entry.width / 2), scale(basis.yAxis, -entry.height / 2)));
      text.push(labelText);
      textPlacements.push({
        textIndex: 0,
        origin: mid,
        xAxis: basis.xAxis,
        normal: basis.normal,
        height: ctx.H,
        alignment: 'centre',
        cellBox: [[-entry.width / 2, -entry.height / 2], [entry.width / 2, entry.height / 2]],
      });
    }

    const planeId = `drafting:${entry.item}/plane`;
    registerPlane(ctx, planeId, entry.g.name, rowLeft, basis);
    const rowMid = add(rowLeft, scale(basis.yAxis, -entry.height / 2));
    const room = balloonSlot(
      basis, dirSign > 0 ? add(rowMid, scale(basis.xAxis, entry.width)) : rowMid, scale(basis.xAxis, dirSign), ctx.H,
    );
    ctx.nextItems[entry.item] = [{
      id: `drafting:${entry.item}`,
      source: 'generated',
      name: entry.g.name,
      polylines,
      polylineRoles,
      triangles,
      text,
      textPlacements,
      planeId,
      balloonSlot: room.slot,
    }];
    yCursor += entry.height;
  }

  if (!grp.isGeneral) {
    const blockPts = [
      bodyLeft,
      add(bodyLeft, scale(basis.xAxis, maxW)),
      add(bodyLeft, scale(basis.yAxis, -totalH)),
      add(bodyLeft, add(scale(basis.xAxis, maxW), scale(basis.yAxis, -totalH))),
      ...groupSlotPts(basis, bodyLeft, dirSign, maxW, totalH, ctx.H),
    ];
    ctx.occupied.push(rectOfPoints(blockPts, ctx.cam, ctx.H * 0.4));
    ctx.existingLeaders.push([...project2(tip, ctx.cam), ...project2(elbow, ctx.cam)]);
  }
}

/**
 * Rebuilds Specify-created graphics in `pmi` onto clean, part-axis-aligned 3D annotation planes
 * following ASME Y14.5 drafting conventions, while preserving all file-native CAD graphics
 * untouched. Produces identical 3D geometry before and after Save.
 */
export function normalizeDraftingPmi(
  pmi: DrawnPmi,
  rawScene: unknown,
  analysis: PmiAnalysis,
  mesh: PmiMesh,
  semantic?: ReadonlyMap<string, SceneSemanticLabel>,
): DrawnPmi {
  if (pmi.draftingNormalized) return pmi;
  const { specifyItems, nativeItems } = partitionGraphics(pmi);
  if (!specifyItems.length) return pmi;

  const resolvedSemantic: ReadonlyMap<string, SceneSemanticLabel> = semantic
    ?? (pmi.sceneSemantic ? new Map(Object.entries(pmi.sceneSemantic)) : readSceneSemanticLabels(rawScene));
  const cam = bestReferenceView(pmi, analysis, mesh);
  const bmin: Vec3 = [mesh.bbox[0][0], mesh.bbox[0][1], mesh.bbox[0][2]];
  const bmax: Vec3 = [mesh.bbox[1][0], mesh.bbox[1][1], mesh.bbox[1][2]];
  const span: Vec3 = [bmax[0] - bmin[0], bmax[1] - bmin[1], bmax[2] - bmin[2]];
  const centre: Vec3 = [(bmin[0] + bmax[0]) / 2, (bmin[1] + bmax[1]) / 2, (bmin[2] + bmax[2]) / 2];
  const diag = Math.hypot(span[0], span[1], span[2]) || 1;
  const maxSpan = Math.max(span[0], span[1], span[2]) || 1;
  // Cap height H proportional to part scale.
  const H = Math.min(diag * 0.022, maxSpan * 0.023);
  const frameH = H * 1.85;
  const arrLen = H * 1.65;
  const arrHalfW = H * 0.30;

  const visibleSigns: [number, number, number] = [
    cam.eye[0] >= 0 ? 1 : -1,
    cam.eye[1] >= 0 ? 1 : -1,
    cam.eye[2] >= 0 ? 1 : -1,
  ];
  const frontNormals: Vec3[] = [
    [visibleSigns[0], 0, 0],
    [0, visibleSigns[1], 0],
    [0, 0, visibleSigns[2]],
  ];

  const partCorners: Vec3[] = [];
  for (const x of [bmin[0], bmax[0]]) for (const y of [bmin[1], bmax[1]]) for (const z of [bmin[2], bmax[2]]) {
    partCorners.push([x, y, z]);
  }
  const partHull = convexHull2(partCorners.map((p) => project2(p, cam)));
  const occupied: Rect2[] = [];
  const existingLeaders: [number, number, number, number][] = [];
  for (const gs of Object.values(nativeItems)) {
    for (const g of gs) {
      // Bound native CAD graphics with glyph triangles around their glyphs rather than long leaders.
      const occPts = g.triangles.length > 16 ? g.triangles.flat() : graphicPoints(g);
      if (occPts.length) occupied.push(rectOfPoints(occPts, cam, H * 0.45));
      for (const pl of g.polylines) {
        if (pl.length >= 2) {
          const [ax, ay] = project2(pl[0]!, cam);
          const [bx, by] = project2(pl[pl.length - 1]!, cam);
          existingLeaders.push([ax, ay, bx, by]);
        }
      }
    }
  }

  const hasNative = Object.keys(nativeItems).length > 0;
  const partRect2 = rectOfPoints(partCorners, cam, 0);
  const partSpanX = partRect2.x1 - partRect2.x0;
  const partSpanY = partRect2.y1 - partRect2.y0;
  const notesKeepOut: Rect2 | null = !hasNative ? {
    x0: partRect2.x0 - partSpanX * 0.35,
    y0: partRect2.y0 - partSpanY * 0.55,
    x1: partRect2.x0 + partSpanX * 0.32,
    y1: partRect2.y0 - H * 2.2,
  } : null;

  const ctx: LayoutContext = {
    pmi, analysis, mesh, semantic: resolvedSemantic, cam, bmin, bmax, centre, diag, H, frameH, arrLen, arrHalfW,
    visibleSigns, frontNormals, partHull, hasNative,
    holes: readHoleGeoms(analysis),
    faceById: new Map(analysis.faces.map((f) => [f.id, f])),
    occupied, existingLeaders, notesKeepOut,
    nextItems: { ...nativeItems },
    nextPlanes: [...pmi.presentation.planes.filter((p) => !p.id.startsWith('generated:'))],
  };

  // Separate Specify items into datums, linear/basic dimensions, and feature callouts.
  // Keep `general: true` items isolated so hidden general dimensions never shift visible stacks across Save.
  const dimMetas = pmi.sceneDimensions ? new Map<string, DimMeta>(Object.entries(pmi.sceneDimensions)) : parseDimMeta(rawScene);
  const datumItems: [string, DrawnGraphic][] = [];
  const dimItems: [string, DrawnGraphic, DimMeta, boolean][] = [];
  const calloutItems: [string, DrawnGraphic, boolean][] = [];

  for (const [item, g] of specifyItems) {
    const sem = resolvedSemantic.get(item);
    const dMeta = dimMetas.get(item);
    const isGeneral = pmi.links[item]?.general === true;
    if (item.startsWith('datum:') || (sem?.kind === 'datum' && !sem.basic)) {
      datumItems.push([item, g]);
    } else if (dMeta) {
      dimItems.push([item, g, dMeta, isGeneral]);
    } else {
      calloutItems.push([item, g, isGeneral]);
    }
  }

  datumItems.sort(([a], [b]) => a.localeCompare(b));
  dimItems.sort(([a, , , ga], [b, , , gb]) => (ga === gb ? a.localeCompare(b) : ga ? 1 : -1));
  calloutItems.sort(([a, , ga], [b, , gb]) => (ga === gb ? a.localeCompare(b) : ga ? 1 : -1));

  // 1. Fixed-endpoint linear/basic dimensions first, then datums, then stacked feature callouts.
  for (const [item, g, meta, isGeneral] of dimItems) placeLinearDimension(ctx, item, g, meta, isGeneral);
  for (const [item, g] of datumItems) placeDatumSymbol(ctx, item, g);

  const featureGroups = new Map<string, FeatureCalloutGroup>();
  for (const [item, g, isGeneral] of calloutItems) {
    const sem = resolvedSemantic.get(item);
    if (!sem) {
      ctx.nextItems[item] = [g];
      continue;
    }
    const faces = [...(pmi.links[item]?.faces ?? [])].sort((a, b) => a - b);
    const matchingHole = ctx.holes.find((h) => h.faces.length > 0 && h.faces.every((f) => faces.includes(f)) && faces.every((f) => h.faces.includes(f)))
      ?? ctx.holes.find((h) => faces.some((f) => h.faces.includes(f)));
    const baseKey = matchingHole ? `hole:${matchingHole.id}` : faces.length ? `faces:${faces.join(',')}` : `item:${item}`;
    const groupKey = isGeneral ? `general:${item}` : baseKey;

    const isFrame = sem.kind === 'frame';
    let cells = [...sem.cells];
    if (!isFrame && sem.tolerance) {
      const tolText = sem.tolerance.style === 'symmetric'
        ? sem.tolerance.upper
        : `${sem.tolerance.upper}/${sem.tolerance.lower}`;
      cells = cells.length ? [`${cells[0]} ${tolText}`, ...cells.slice(1)] : [tolText];
    }
    if (isFrame && matchingHole && matchingHole.count > 1 && !/^\d+×/.test(cells[0] ?? '')) {
      cells = [`${matchingHole.count}× ${cells[0] ?? ''}`, ...cells.slice(1)];
    }
    const padX = H * 0.85;
    const cellWidths = isFrame
      ? cells.map((c, idx) => Math.max(idx === 0 ? frameH : frameH * 0.95, textWidthMm(c, H) + padX))
      : [textWidthMm(cells.join(' '), H) + H * 0.6];
    const width = cellWidths.reduce((s, w) => s + w, 0);
    const height = isFrame ? frameH : H * 1.45;
    const grp = featureGroups.get(groupKey) ?? { faces, isGeneral, entries: [] };
    grp.entries.push({ item, g, sem, isPrimary: !isFrame, isGeneral, cells, isFrame, width, height, cellWidths });
    featureGroups.set(groupKey, grp);
  }
  for (const grp of featureGroups.values()) placeFeatureCalloutGroup(ctx, grp);

  return {
    ...pmi,
    draftingNormalized: true,
    presentation: {
      ...pmi.presentation,
      items: ctx.nextItems,
      planes: ctx.nextPlanes,
    },
  };
}
