// The part's 3D annotation-plane PMI, from Specify's pmiScene: polylines as Line2-family lines
// and filled triangles as meshes, both drawn without depth testing so the part never hides a
// callout; text the strokes do not draw as DOM text, set as text, never HTML.
//
// Specify-generated graphics use crisp drafting line weights and plane-aligned IBM Plex Mono text
// centred in its cell; file-native CAD graphics draw as the file draws them. Unsaved pending
// callouts render in burnt amber/orange, stated callouts in dark blueprint blue, and open gap
// proposals in muted blueprint blue. An optional RGB x/y/z triad marks an explicit file origin.
//
// The /diff page draws a revision's PMI with this same layer, supplying per-item change colours
// (``colourOf``), highlight colour (``litColour``), and ghost opacity (``alpha``).
import * as THREE from 'three';
import { CSS2DObject } from 'three/examples/jsm/renderers/CSS2DRenderer.js';
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js';
import { LineSegments2 } from 'three/examples/jsm/lines/LineSegments2.js';
import { LineSegmentsGeometry } from 'three/examples/jsm/lines/LineSegmentsGeometry.js';

import type { AnnotationHost } from './annotations';
import type { LabelGroup } from './labels';
import { ranges } from './characteristics';
import {
  drawnGroup, drawnShown, facesAway, graphicAxes, type DrawnGraphic, type DrawnPmi, type DrawnPoint, type DrawnShow,
  type PartOrigin, type TextPlacement,
} from './drawn';

/** Standard drafting ink colours for the single PMI view. */
export const STANDARD_INK = '#0c36c0';
export const STANDARD_GAP_INK = '#4b70cc';
export const STANDARD_UNSAVED_INK = '#b84a00';
export const STANDARD_LIT_INK = '#007a8c';

const FILE_LINE_PX = 1.25;
const GEN_LINE_PX = 1.5;
const HIT_PX = 6;
const MIN_TEXT_PX = 7;
const SET_TEXT_PX = 8.5;
const CAP_HEIGHT = 0.7;
// How far a plane's x or up may shrink as seen, against a millimetre across the view, before its
// set text is hidden: near edge-on it cannot be read.
const EDGE_ON = 0.2;
const ORIGIN_UP = new THREE.Vector3(0, 1, 0);

interface Drawn {
  /** The semantic item, or null for an unlinked graphic. */
  item: string | null;
  name: string;
  source: DrawnGraphic['source'];
  /** Its item's label group (``drawnGroup``): its colour and Show-menu kind. */
  group: LabelGroup | undefined;
  colour: string;
  width: number;
  /** The text set for it in the labels' face, hit where it is seen. */
  set: HTMLElement[];
  /** What it draws, for placing a balloon on it. */
  graphic: DrawnGraphic;
  /** The side it is read from, when that can be told. */
  normal: DrawnPoint | null;
  /** Whether the Show menu shows its kind. */
  shown: boolean;
  /** Whether its plane faces away from the camera. */
  away: boolean;
  polylines: number;
  /** Its lines as drawn, in pairs: what a pointer hits. */
  segments: THREE.Vector3[];
  /** Where all its strokes and text cells are: what a view frames. */
  extent: THREE.Box3;
  triangles: THREE.Vector3[];
  objects: THREE.Object3D[];
}

export interface DrawnLayer {
  /** Sets the drawn PMI scene to display, or clears it when null. */
  set(pmi: DrawnPmi | null): void;
  setVisible(on: boolean): void;
  /**
   * The kinds the Show menu shows, and whether callouts facing away are hidden; items in
   * ``reveal`` are drawn whatever their kind and whichever way they face.
   */
  filter(show: DrawnShow, farSide: boolean, reveal?: ReadonlySet<string>): void;
  /** Draws an item's graphics in the highlight colour, or none. */
  light(item: string | null): void;
  /** Inspection balloon numbers by item, drawn in each item's annotation plane, beside its callout. */
  setBalloons(balloons: ReadonlyMap<string, readonly number[]>): void;
  /** Marks pending (unsaved) scene items in burnt amber/orange ink. */
  setUnsaved(items: ReadonlySet<string>): void;
  /** Draws a small RGB x/y/z triad at the file's stated origin, excluded from bounds and hit-testing. */
  setOrigin(origin: PartOrigin | null): void;
  /** The linked item drawn within a few pixels of (x, y) in the view, if any. */
  hit(x: number, y: number): string | null;
  /** Where the graphics now shown are, for a view that frames them with the part; null if none. */
  bounds(): THREE.Box3 | null;
  /** Sampled world-space points of all currently shown graphics (tighter than a single outer AABB). */
  tightPoints(): THREE.Vector3[];
  dispose(): void;
}

export function createDrawnLayer(
  nav: AnnotationHost,
  container: HTMLElement,
  { onClick, centre, colourOf, litColour = STANDARD_LIT_INK, lineWeight, minTextPx = SET_TEXT_PX, rank, titleOf, alpha = 1 }: {
    onClick: (item: string) => void;
    centre?: DrawnPoint;
    /** An item's colour (an unlinked graphic's is asked with null), or null to leave it out. */
    colourOf?: (item: string | null, group: LabelGroup | undefined) => string | null;
    /** Highlight colour when an item is lit (diff uses its own blue). */
    litColour?: string;
    /** Stroke widths in pixels for file-native and generated polylines (diff uses heavier weights). */
    lineWeight?: { readonly file: number; readonly generated: number };
    /** Minimum rendered font size in pixels for plane-aligned generated text. */
    minTextPx?: number;
    /** Which of several items sharing a graphic it is drawn as: the lowest rank, before the item Specify knows. */
    rank?: (item: string) => number;
    /** Hover tooltip text for a linked item's file text element. */
    titleOf?: (item: string) => string | undefined;
    /** Below 1, everything is drawn see-through. */
    alpha?: number;
  },
): DrawnLayer {
  const { scene } = nav;
  const fileLinePx = lineWeight?.file ?? FILE_LINE_PX;
  const genLinePx = lineWeight?.generated ?? GEN_LINE_PX;
  const inks = new Map<string, { line: LineMaterial; fill: THREE.MeshBasicMaterial }>();
  const ink = (colour: string, width = fileLinePx) => {
    let made = inks.get(`${colour} ${width}`);
    if (!made) {
      made = {
        line: new LineMaterial({
          color: new THREE.Color(colour).getHex(), linewidth: width, worldUnits: false,
          depthTest: false, depthWrite: false, alphaToCoverage: alpha >= 1,
          ...(alpha < 1 ? { transparent: true, opacity: alpha } : {}),
        }),
        fill: new THREE.MeshBasicMaterial({
          color: colour, side: THREE.DoubleSide, depthTest: false, depthWrite: false,
          ...(alpha < 1 ? { transparent: true, opacity: alpha } : {}),
        }),
      };
      inks.set(`${colour} ${width}`, made);
    }
    return made;
  };

  let drawn: Drawn[] = [];
  let placed: { inner: HTMLElement; at: TextPlacement; set: boolean; cell?: number }[] = [];
  let visible = false;
  let show: DrawnShow = { default: true, existing: true, general: true };
  let farSide = false;
  let reveal: ReadonlySet<string> = new Set();
  let lit: string | null = null;
  let pmiNow: DrawnPmi | null = null;
  let unsavedItems: ReadonlySet<string> = new Set();
  let balloons: ReadonlyMap<string, readonly number[]> = new Map();
  let balloonKey = '';
  let originNow: PartOrigin | null = null;
  let originObjects: THREE.Object3D[] = [];
  let originMaterials: LineMaterial[] = [];

  const revealed = (d: { item: string | null }) => d.item !== null && reveal.has(d.item);
  const shownBy = (d: { item: string | null; source: Drawn['source']; group: LabelGroup | undefined }) =>
    revealed(d) || drawnShown(d.source, d.group, show);
  const on = (d: Drawn) => visible && d.shown && !(farSide && d.away && !revealed(d));
  const colourOfDrawn = (d: Drawn) => {
    if (d.item !== null && d.item === lit) return litColour;
    if (!colourOf && d.item !== null && unsavedItems.has(d.item)) return STANDARD_UNSAVED_INK;
    return d.colour;
  };

  function clearOrigin() {
    if (originObjects.length) {
      scene.remove(...originObjects);
      for (const o of originObjects) {
        if (o instanceof CSS2DObject) o.element.remove();
        else if (o instanceof THREE.Mesh) {
          o.geometry.dispose();
          (o.material as THREE.Material).dispose();
        } else if (o instanceof LineSegments2) {
          o.geometry.dispose();
        }
      }
      originObjects = [];
    }
    for (const m of originMaterials) m.dispose();
    originMaterials = [];
  }

  function rebuildOrigin() {
    clearOrigin();
    if (!originNow) return;
    const axisLen = nav.diagonal * 0.028;
    const coneR = axisLen * 0.095;
    const coneH = axisLen * 0.28;
    const pos = new THREE.Vector3(...originNow.position);
    const axes: readonly { label: string; dir: DrawnPoint; colour: string }[] = [
      { label: 'x', dir: originNow.xAxis, colour: '#d62828' },
      { label: 'y', dir: originNow.yAxis, colour: '#1e8e3e' },
      { label: 'z', dir: originNow.zAxis, colour: STANDARD_INK },
    ];
    const [w, h] = nav.size();
    for (const { label, dir, colour } of axes) {
      const u = new THREE.Vector3(...dir);
      if (u.lengthSq() < 1e-12) continue;
      u.normalize();
      const tip = pos.clone().addScaledVector(u, axisLen);
      const shaftEnd = pos.clone().addScaledVector(u, Math.max(0, axisLen - coneH * 0.6));
      const lineMat = new LineMaterial({
        color: new THREE.Color(colour).getHex(),
        linewidth: 1.6,
        worldUnits: false,
        depthTest: false,
        depthWrite: false,
        alphaToCoverage: true,
      });
      lineMat.resolution.set(w, h);
      originMaterials.push(lineMat);
      const lineGeo = new LineSegmentsGeometry();
      lineGeo.setPositions([pos.x, pos.y, pos.z, shaftEnd.x, shaftEnd.y, shaftEnd.z]);
      const line = new LineSegments2(lineGeo, lineMat);
      line.renderOrder = 12;
      originObjects.push(line);

      const cone = new THREE.Mesh(
        new THREE.ConeGeometry(coneR, coneH, 10),
        new THREE.MeshBasicMaterial({ color: colour, depthTest: false, depthWrite: false }),
      );
      cone.quaternion.setFromUnitVectors(ORIGIN_UP, u);
      cone.position.copy(tip).addScaledVector(u, -coneH * 0.5);
      cone.renderOrder = 12;
      originObjects.push(cone);

      const el = document.createElement('div');
      el.className = 'pmi-origin-axis';
      el.textContent = label;
      el.style.color = colour;
      const tag = new CSS2DObject(el);
      tag.position.copy(pos).addScaledVector(u, axisLen * 1.24);
      originObjects.push(tag);
    }
    if (originObjects.length) {
      scene.add(...originObjects);
      for (const o of originObjects) o.visible = visible;
    }
  }

  function counts() {
    // What is drawn, for tests to compare against the scene: nothing while hidden.
    const shownNow = drawn.filter(on);
    const n = (f: (d: Drawn) => number) => shownNow.reduce((sum, d) => sum + f(d), 0);
    container.dataset['drawnPolylines'] = String(n((d) => d.polylines));
    container.dataset['drawnTriangles'] = String(n((d) => d.triangles.length / 3));
    container.dataset['drawnTexts'] = String(n((d) => d.objects.filter((o) => o instanceof CSS2DObject).length));
    container.dataset['drawnNames'] = shownNow.map((d) => d.name).join('\n');
    const by = (f: (d: Drawn) => boolean) => String(shownNow.filter(f).length);
    container.dataset['drawnGraphics'] = String(shownNow.length);
    container.dataset['drawnGeneral'] = by((d) => d.group === 'general');
    container.dataset['drawnDefault'] = by((d) => d.group === 'default');
    container.dataset['drawnFile'] = by((d) => d.source === 'file' || d.group === 'existing');
    container.dataset['drawnItems'] = [...new Set(shownNow.filter((d) => d.item !== null)
      .map((d) => `${d.item} ${colourOfDrawn(d)}`))].join('\n');
    updateHitPoints(shownNow);
  }

  const probeA = new THREE.Vector2();
  const probeB = new THREE.Vector2();
  const probeC = new THREE.Vector2();
  const probeMid = new THREE.Vector3();
  function updateHitPoints(shownNow = drawn.filter(on)) {
    const [w, h] = nav.size();
    if (!visible || w <= 0 || h <= 0) {
      container.dataset['drawnPoints'] = '';
      return;
    }
    const inView = (x: number, y: number) => x >= 2 && x <= w - 2 && y >= 2 && y <= h - 2;
    const lines: string[] = [];
    const listed = new Set<string>();
    for (const d of shownNow) {
      if (d.item === null || listed.has(d.item)) continue;
      let found: [number, number] | null = null;
      for (let k = 0; k + 1 < d.segments.length && !found; k += 2) {
        if (!screen(d.segments[k]!, probeA) || !screen(d.segments[k + 1]!, probeB)) continue;
        const mx = (probeA.x + probeB.x) / 2;
        const my = (probeA.y + probeB.y) / 2;
        if (inView(mx, my) && hitAt(mx, my) === d.item) found = [mx, my];
      }
      for (let k = 0; k + 2 < d.triangles.length && !found; k += 3) {
        if (!screen(d.triangles[k]!, probeA) || !screen(d.triangles[k + 1]!, probeB) || !screen(d.triangles[k + 2]!, probeC)) continue;
        const cx = (probeA.x + probeB.x + probeC.x) / 3;
        const cy = (probeA.y + probeB.y + probeC.y) / 3;
        if (inView(cx, cy) && hitAt(cx, cy) === d.item) found = [cx, cy];
      }
      if (!found && !d.extent.isEmpty()) {
        d.extent.getCenter(probeMid);
        if (screen(probeMid, probeA) && inView(probeA.x, probeA.y) && hitAt(probeA.x, probeA.y) === d.item) {
          found = [probeA.x, probeA.y];
        }
      }
      if (found) {
        listed.add(d.item);
        lines.push(`${d.item} ${Math.round(found[0])},${Math.round(found[1])}`);
      }
    }
    container.dataset['drawnPoints'] = lines.join('\n');
  }

  function paint(d: Drawn) {
    const colour = colourOfDrawn(d);
    const { line, fill } = ink(colour, d.width);
    for (const o of d.objects) {
      if (o instanceof LineSegments2) o.material = line;
      else if (o instanceof THREE.Mesh) o.material = fill;
      else if (o instanceof CSS2DObject) (o.element.firstElementChild as HTMLElement | null)?.style.setProperty('color', colour);
    }
  }

  function apply() {
    for (const d of drawn) {
      const vis = on(d);
      for (const o of d.objects) o.visible = vis;
    }
    for (const o of originObjects) o.visible = visible;
    counts();
  }

  const look = new THREE.Vector3();
  function turn(): boolean {
    nav.camera.getWorldDirection(look);
    const direction: DrawnPoint = [look.x, look.y, look.z];
    let changed = false;
    for (const d of drawn) {
      const away = d.normal !== null && facesAway(d.normal, direction);
      if (away !== d.away) { d.away = away; changed = true; }
    }
    return changed;
  }

  function clear() {
    for (const d of drawn) {
      scene.remove(...d.objects);
      for (const o of d.objects) {
        if (o instanceof CSS2DObject) o.element.remove();
        else if (o instanceof THREE.Mesh) o.geometry.dispose();
        else if (o instanceof LineSegments2) o.geometry.dispose();
      }
    }
    drawn = [];
    placed = [];
  }

  function place(graphic: DrawnGraphic, item: string | null, group: LabelGroup | undefined, colour: string): Drawn {
    const axes = graphicAxes(graphic, pmiNow?.presentation.planes ?? [], centre);
    const roles = graphic.polylineRoles;
    const setting = graphic.source === 'generated' && roles !== undefined;
    const width = setting ? genLinePx : fileLinePx;
    const { line: material, fill } = ink(colour, width);
    const lines = graphic.polylines.filter((_, k) => !setting || roles[k] !== 'text');
    const segments: THREE.Vector3[] = [];
    for (const line of lines) {
      for (let k = 1; k < line.length; k++) segments.push(new THREE.Vector3(...line[k - 1]!), new THREE.Vector3(...line[k]!));
    }
    const triangles = graphic.triangles.flatMap((t) => t.map((p) => new THREE.Vector3(...p)));
    const objects: THREE.Object3D[] = [];
    if (segments.length) {
      const geometry = new LineSegmentsGeometry();
      geometry.setPositions(segments.flatMap((p) => [p.x, p.y, p.z]));
      const strokes = new LineSegments2(geometry, material);
      strokes.renderOrder = 10;
      objects.push(strokes);
    }
    if (triangles.length) {
      const geometry = new THREE.BufferGeometry().setFromPoints(triangles);
      const meshObj = new THREE.Mesh(geometry, fill);
      meshObj.renderOrder = 10;
      objects.push(meshObj);
    }
    const texts = graphic.source === 'file' || setting ? graphic.text : [];
    const block = setting && graphic.text.length > 1 && !roles.includes('frame');
    const box = new THREE.Box3().setFromPoints([...segments, ...triangles]);
    const fullExtent = new THREE.Box3().setFromPoints(
      [...graphic.polylines.flat(), ...graphic.triangles.flat(), ...(graphic.textPlacements ?? []).map((t) => t.origin)]
        .map((q) => new THREE.Vector3(...q)),
    );
    const set: HTMLElement[] = [];
    const heights = (graphic.textPlacements ?? []).flatMap((t) => (t.cellBox ? [Math.abs(t.cellBox[1][1] - t.cellBox[0][1])] : []));
    const pitch = block && heights.length ? Math.min(...heights) : undefined;
    for (const [i, text] of texts.entries()) {
      const at = graphic.textPlacements?.find((t) => t.textIndex === i);
      if (setting && !at) continue;
      if (!at && box.isEmpty()) continue;
      const el = document.createElement('div');
      el.className = `pmi-drawn${at ? ' pmi-drawn-placed' : ''}${setting ? ' pmi-drawn-set' : ''}`;
      const inner = document.createElement('span');
      inner.textContent = text;
      inner.style.color = colour;
      if (alpha < 1) inner.style.opacity = String(alpha);
      if (!at && i) inner.style.transform = `translateY(${1.2 * i}em)`;
      if (setting) {
        inner.style.fontFamily = "var(--specify-mono-font, 'IBM Plex Mono', ui-monospace, monospace)";
        inner.style.fontWeight = '600';
        inner.style.fontVariantNumeric = 'tabular-nums';
        set.push(inner);
      } else if (item !== null) {
        const id = item;
        inner.style.pointerEvents = 'auto';
        inner.style.cursor = 'pointer';
        const title = titleOf?.(id);
        if (title) inner.title = title;
        inner.addEventListener('pointerdown', (event) => event.stopPropagation());
        inner.addEventListener('click', (event) => { event.stopPropagation(); onClick(id); });
      }
      el.append(inner);
      const tag = new CSS2DObject(el);
      if (at && setting && at.cellBox) {
        const [[x0, y0], [x1, y1]] = at.cellBox;
        const x = new THREE.Vector3(...at.xAxis).normalize();
        const y = new THREE.Vector3(...at.normal).cross(x).normalize();
        const mid = new THREE.Vector3(...at.origin).addScaledVector(x, block ? x0 : (x0 + x1) / 2).addScaledVector(y, (y0 + y1) / 2);
        tag.position.copy(mid);
        placed.push({ inner, at: { ...at, origin: [mid.x, mid.y, mid.z], alignment: block ? 'middle left' : 'centre' }, set: true, cell: pitch ?? Math.abs(y1 - y0) });
        // Expand fullExtent with the cell corners so framing includes text-only callouts (e.g. threads).
        fullExtent.expandByPoint(new THREE.Vector3(...at.origin).addScaledVector(x, x0).addScaledVector(y, y0));
        fullExtent.expandByPoint(new THREE.Vector3(...at.origin).addScaledVector(x, x1).addScaledVector(y, y1));
      } else if (at) {
        tag.position.set(...at.origin);
        placed.push({ inner, at, set: setting });
      } else tag.position.copy(box.getCenter(new THREE.Vector3()));
      objects.push(tag);
    }
    scene.add(...objects);
    return {
      item, name: graphic.name, source: graphic.source, group, colour, width, set, graphic,
      normal: axes?.normal ?? null,
      shown: shownBy({ item, source: graphic.source, group }), away: false,
      polylines: lines.filter((l) => l.length > 1).length, segments, triangles, objects,
      extent: fullExtent,
    };
  }

  const a = new THREE.Vector2();
  const b = new THREE.Vector2();
  const c = new THREE.Vector2();
  const p = new THREE.Vector2();
  const v = new THREE.Vector3();
  function screen(point: THREE.Vector3, into: THREE.Vector2): boolean {
    const [w, h] = nav.size();
    v.copy(point).project(nav.camera);
    into.set((v.x * 0.5 + 0.5) * w, (-v.y * 0.5 + 0.5) * h);
    return v.z <= 1;
  }
  function nearSegment(): number {
    const ab = b.clone().sub(a);
    const t = ab.lengthSq() ? Math.min(Math.max(p.clone().sub(a).dot(ab) / ab.lengthSq(), 0), 1) : 0;
    return a.clone().addScaledVector(ab, t).distanceTo(p);
  }
  function inside(): boolean {
    const cross2 = (u: THREE.Vector2, w2: THREE.Vector2) => (w2.x - u.x) * (p.y - u.y) - (w2.y - u.y) * (p.x - u.x);
    const [d1, d2, d3] = [cross2(a, b), cross2(b, c), cross2(c, a)];
    return !((d1 < 0 || d2 < 0 || d3 < 0) && (d1 > 0 || d2 > 0 || d3 > 0));
  }

  const shift = (alignment = '') => {
    const words = alignment.toLowerCase().split(/[\s_-]+/);
    const has = (...w: string[]) => w.some((x) => words.includes(x));
    const mid = has('centre', 'center');
    const x = has('left') ? '0' : has('right') ? '-100%' : mid ? '-50%' : '0';
    const y = has('top') ? '0' : has('bottom') ? '-100%' : has('middle') || (mid && !has('baseline')) ? '-50%' : '-80%';
    return `translate(${x}, ${y})`;
  };
  const o = new THREE.Vector2();
  const ex = new THREE.Vector2();
  const up = new THREE.Vector2();
  const along = new THREE.Vector3();
  const across = new THREE.Vector2();
  const right = new THREE.Vector3();
  function orient() {
    right.set(1, 0, 0).applyQuaternion(nav.camera.quaternion);
    for (const { inner, at, set, cell: height } of placed) {
      const origin = new THREE.Vector3(...at.origin);
      if (!screen(origin, o)) continue;
      const x = new THREE.Vector3(...at.xAxis).normalize();
      const y = new THREE.Vector3(...at.normal).cross(x).normalize();
      const size = at.height ?? 0;
      if (set) {
        screen(along.copy(origin).add(x), ex);
        screen(along.copy(origin).add(y), up);
        screen(along.copy(origin).add(right), across);
        const [ax, ay, ux, uy] = [ex.x - o.x, ex.y - o.y, up.x - o.x, up.y - o.y];
        const mm = o.distanceTo(across);
        const edgeOn = mm < 1e-6 || Math.min(Math.hypot(ax, ay), Math.hypot(ux, uy)) / mm < EDGE_ON;
        inner.style.visibility = edgeOn ? 'hidden' : '';
        if (edgeOn) continue;
        const flip = ax * -uy - ay * -ux < 0 ? -1 : 1;
        const m = [flip * ax / mm, flip * ay / mm, -ux / mm, -uy / mm].map((n) => n.toFixed(4)).join(', ');
        const own = (mm * size) / CAP_HEIGHT;
        const cell = height ? mm * height * 0.88 : Infinity;
        inner.style.fontSize = `${Math.min(Math.max(own, minTextPx), cell).toFixed(1)}px`;
        inner.style.transform = `matrix(${m}, 0, 0) ${shift(at.alignment)}`;
        continue;
      }
      screen(along.copy(origin).addScaledVector(x, size || 1), ex);
      screen(along.copy(origin).addScaledVector(y, size || 1), up);
      const angle = Math.atan2(ex.y - o.y, ex.x - o.x);
      const px = size ? Math.max(o.distanceTo(up), MIN_TEXT_PX) : 11.5;
      inner.style.fontSize = `${px.toFixed(1)}px`;
      inner.style.transform = `rotate(${angle.toFixed(4)}rad) ${shift(at.alignment)}`;
    }
  }

  const upward = new THREE.Vector3();
  const fixed = (u: THREE.Vector3) => u.toArray().map((x) => (Math.abs(x) < 5e-4 ? 0 : x).toFixed(3)).join(',');
  let lastProjKey = '';
  nav.onFrame(() => {
    const [w, h] = nav.size();
    for (const { line } of inks.values()) line.resolution.set(w, h);
    for (const line of originMaterials) line.resolution.set(w, h);
    nav.camera.getWorldDirection(look);
    upward.set(0, 1, 0).applyQuaternion(nav.camera.quaternion);
    const camera = `${fixed(look)} ${fixed(upward)}`;
    if (container.dataset['camera'] !== camera) container.dataset['camera'] = camera;
    if (!visible) return;
    const zoom = 'zoom' in nav.camera ? (nav.camera as THREE.OrthographicCamera).zoom.toFixed(4) : '1';
    const projKey = `${camera}|${fixed(nav.camera.position)}|${zoom}|${Math.round(w)}x${Math.round(h)}`;
    const projChanged = projKey !== lastProjKey;
    if (projChanged) lastProjKey = projKey;
    if (farSide && turn()) apply();
    else if (projChanged) updateHitPoints();
    if (placed.length) orient();
  });

  /**
   * An inspection balloon in a callout's own annotation plane: a ring (a stadium, for a range) with
   * its number, as tall as the callout's text allows, touching the callout's frame on the side away
   * from its leader. It slides along that edge until it is clear of every other callout and balloon.
   * View only: the callout is not moved and nothing is written.
   */
  function addBalloon(d: Drawn, numbers: readonly number[], taken: THREE.Box3[], all: readonly Drawn[]) {
    const g = d.graphic;
    const axes = graphicAxes(g, pmiNow?.presentation.planes ?? [], centre);
    if (!axes?.xAxis || !axes.up) return;
    const x = new THREE.Vector3(...axes.xAxis);
    const y = new THREE.Vector3(...axes.up);
    const n = new THREE.Vector3(...axes.normal);
    const o = d.extent.getCenter(new THREE.Vector3());
    const local = (q: THREE.Vector3): [number, number] => [q.clone().sub(o).dot(x), q.clone().sub(o).dot(y)];
    const world = (u: number, v: number) => o.clone().addScaledVector(x, u).addScaledVector(y, v);
    const bounds = (pts: THREE.Vector3[]) => {
      const r = { x0: Infinity, x1: -Infinity, y0: Infinity, y1: -Infinity };
      for (const q of pts) {
        const [u, v] = local(q);
        r.x0 = Math.min(r.x0, u); r.x1 = Math.max(r.x1, u); r.y0 = Math.min(r.y0, v); r.y1 = Math.max(r.y1, v);
      }
      return r;
    };
    const roles = g.polylineRoles;
    const text = g.balloonSlot?.height ?? g.textPlacements?.[0]?.height ?? Math.max(...[...d.extent.max.clone().sub(d.extent.min).toArray()]) * 0.04;
    const h = 0.95 * text;
    const label = ranges(numbers);
    const w = Math.max(2 * h, label.length * 0.86 * text + 0.9 * text);
    const slot = g.balloonSlot;
    let cx: number;
    let cy: number;
    if (slot) {
      // Room the layout kept: touching its edge, whatever the balloon says (a longer label than the
      // slot overflows outward, along the side away from the leader).
      const [eu, ev] = local(new THREE.Vector3(...slot.edge));
      const dir = new THREE.Vector3(...slot.dir);
      const [du, dv] = [dir.dot(x), dir.dot(y)];
      [cx, cy] = Math.abs(du) >= Math.abs(dv) ? [eu + Math.sign(du) * w / 2, ev] : [eu, ev + Math.sign(dv) * h];
    } else {
      // Another tool's callout: it is not moved. The balloon stands beside its text and frame on the
      // side away from its leader, and slides along that edge, then tries the other sides, until it
      // clears every dimension, extension and leader line, every text and every balloon placed.
      const near = (q: THREE.Vector3) => Math.abs(q.clone().sub(o).dot(n)) < text * 1.5;
      const em = 0.86 * text;
      const textRect = (t: TextPlacement, str: string | undefined) => {
        const origin = local(new THREE.Vector3(...t.origin));
        const tx = new THREE.Vector3(...t.xAxis).normalize();
        const flip = tx.dot(x) < 0 ? -1 : 1;
        const cell = t.cellBox ? { w: t.cellBox[1][0] - t.cellBox[0][0], h: t.cellBox[1][1] - t.cellBox[0][1] } : null;
        const width = Math.max(cell?.w ?? 0, (str?.length ?? 1) * em * ((t.height ?? text) / text));
        const height = Math.max(cell?.h ?? 0, (t.height ?? text) * 1.4);
        const al = (t.alignment ?? '').toLowerCase();
        const shiftX = al.includes('left') ? width / 2 : al.includes('right') ? -width / 2 : 0;
        const shiftY = al.includes('top') ? -height / 2 : al.includes('bottom') ? height / 2 : 0;
        const [u, v] = [origin[0] + flip * shiftX, origin[1] + shiftY];
        return { x0: u - width / 2, x1: u + width / 2, y0: v - height / 2, y1: v + height / 2 };
      };
      const textsOf = (e: Drawn) => (e.graphic.textPlacements ?? []).filter((t) => near(new THREE.Vector3(...t.origin)))
        .map((t) => textRect(t, e.graphic.text[t.textIndex ?? 0]));
      const own = textsOf(d);
      const pts = [...own.flatMap((r) => [[r.x0, r.y0], [r.x1, r.y1]]), ...(g.textPlacements ?? []).flatMap((t) => (t.cellBox
        ? [[t.cellBox[0][0], t.cellBox[0][1]], [t.cellBox[1][0], t.cellBox[1][1]]].map(([u, v]) => local(new THREE.Vector3(...t.origin)
          .addScaledVector(new THREE.Vector3(...t.xAxis).normalize(), u!).addScaledVector(y, v!))) : []))];
      const frame = g.polylines.filter((_, k) => roles?.[k] === 'frame').flat().map((q) => local(new THREE.Vector3(...q)));
      const all2 = [...pts, ...frame];
      const body = all2.length
        ? { x0: Math.min(...all2.map((p) => p[0]!)), x1: Math.max(...all2.map((p) => p[0]!)), y0: Math.min(...all2.map((p) => p[1]!)), y1: Math.max(...all2.map((p) => p[1]!)) }
        : (() => { const [a1, a2] = [local(d.extent.min), local(d.extent.max)]; return { x0: Math.min(a1[0], a2[0]), x1: Math.max(a1[0], a2[0]), y0: Math.min(a1[1], a2[1]), y1: Math.max(a1[1], a2[1]) }; })();
      const pad = 0.35 * text;
      const inBody = (p: [number, number]) => p[0] >= body.x0 - pad && p[0] <= body.x1 + pad && p[1] >= body.y0 - pad && p[1] <= body.y1 + pad;
      // Every line within reach of this plane that the balloon must keep off: the callout's own frame
      // (inside its body) excepted.
      const lines: [number, number, number, number][] = [];
      for (const e of all) {
        for (let k = 0; k + 1 < e.segments.length; k += 2) {
          const [p, q] = [e.segments[k]!, e.segments[k + 1]!];
          if (!near(p) || !near(q)) continue;
          const [lp, lq] = [local(p), local(q)];
          if (e === d && inBody(lp) && inBody(lq)) continue;
          lines.push([lp[0], lp[1], lq[0], lq[1]]);
        }
      }
      const texts = all.filter((e) => e !== d).flatMap(textsOf);
      const leaders = g.polylines.filter((_, k) => roles?.[k] === 'leader').flat();
      const lx = leaders.length ? local(new THREE.Vector3(...leaders[0]!))[0] : null;
      const away = lx !== null && lx > (body.x0 + body.x1) / 2 ? -1 : 1;
      const gap = 0.2 * text;
      const hitsLine = (r: { x0: number; x1: number; y0: number; y1: number }, [ax, ay, bx, by]: [number, number, number, number]) => {
        let t0 = 0;
        let t1 = 1;
        const [dx, dy] = [bx - ax, by - ay];
        for (const [pp, qq] of [[-dx, ax - r.x0], [dx, r.x1 - ax], [-dy, ay - r.y0], [dy, r.y1 - ay]] as const) {
          if (Math.abs(pp) < 1e-12) { if (qq < 0) return false; continue; }
          const t = qq / pp;
          if (pp < 0) { if (t > t1) return false; if (t > t0) t0 = t; } else { if (t < t0) return false; if (t < t1) t1 = t; }
        }
        return true;
      };
      const overlap = (a2: { x0: number; x1: number; y0: number; y1: number }, b2: { x0: number; x1: number; y0: number; y1: number }) =>
        a2.x0 < b2.x1 && a2.x1 > b2.x0 && a2.y0 < b2.y1 && a2.y1 > b2.y0;
      const hits = (u: number, v: number) => {
        const r = { x0: u - w / 2, x1: u + w / 2, y0: v - h, y1: v + h };
        return lines.filter((l) => hitsLine(r, l)).length + texts.filter((t) => overlap(r, t)).length
          + own.filter((t) => overlap(r, t)).length
          + taken.filter((t) => t.intersectsBox(new THREE.Box3().setFromPoints([world(r.x0, r.y0), world(r.x1, r.y1)]))).length;
      };
      const mid = [(body.x0 + body.x1) / 2, (body.y0 + body.y1) / 2] as const;
      const step = 1.2 * h;
      const tries: [number, number][] = [];
      for (const sideSign of [away, -away]) {
        const u = sideSign > 0 ? body.x1 + gap + w / 2 : body.x0 - gap - w / 2;
        for (let k = 0; k <= 8; k++) for (const sign of k ? [1, -1] : [1]) tries.push([u, mid[1] + sign * k * step]);
      }
      for (const sign of [1, -1]) {
        const v = sign > 0 ? body.y1 + gap + h : body.y0 - gap - h;
        for (let k = 0; k <= 8; k++) for (const s2 of k ? [1, -1] : [1]) tries.push([mid[0] + s2 * k * step, v]);
      }
      let best = tries[0]!;
      let fewest = Infinity;
      for (const t of tries) {
        const count = hits(t[0], t[1]);
        if (count < fewest) { fewest = count; best = t; }
        if (!count) break;
      }
      [cx, cy] = best;
    }
    taken.push(new THREE.Box3().setFromPoints([world(cx - w / 2, cy - h), world(cx + w / 2, cy + h)]));
    // The outline: a stadium of straight sides and half-circle ends.
    const ring: THREE.Vector3[] = [];
    const arc = (centreU: number, from: number) => {
      for (let k = 0; k <= 10; k++) {
        const t = from + (Math.PI * k) / 10;
        ring.push(world(centreU + h * Math.cos(t), cy + h * Math.sin(t)));
      }
    };
    arc(cx + w / 2 - h, -Math.PI / 2);
    arc(cx - w / 2 + h, Math.PI / 2);
    ring.push(ring[0]!.clone());
    const segs: THREE.Vector3[] = [];
    for (let k = 1; k < ring.length; k++) segs.push(ring[k - 1]!, ring[k]!);
    const geometry = new LineSegmentsGeometry();
    geometry.setPositions(segs.flatMap((q) => [q.x, q.y, q.z]));
    const strokes = new LineSegments2(geometry, ink(d.colour, d.width).line);
    strokes.renderOrder = 10;
    const el = document.createElement('div');
    el.className = 'pmi-drawn pmi-drawn-placed pmi-drawn-set';
    const inner = document.createElement('span');
    inner.textContent = label;
    inner.style.color = d.colour;
    inner.style.fontFamily = "var(--specify-mono-font, 'IBM Plex Mono', ui-monospace, monospace)";
    inner.style.fontWeight = '600';
    inner.style.fontVariantNumeric = 'tabular-nums';
    // Not a target: a click through it reaches the callout under it, as the strokes do.
    inner.style.pointerEvents = 'none';
    el.append(inner);
    const tag = new CSS2DObject(el);
    const mid = world(cx, cy);
    tag.position.copy(mid);
    placed.push({
      inner, set: true, cell: 2 * h,
      at: { textIndex: 0, origin: [mid.x, mid.y, mid.z], xAxis: axes.xAxis, normal: axes.normal, height: text, alignment: 'centre' },
    });
    d.objects.push(strokes, tag);
    scene.add(strokes, tag);
  }

  function rebuild() {
    const pmi = pmiNow;
    clear();
    pmiNow = pmi;
    if (pmi) {
      // Items Specify knows first, so a graphic under several is drawn as the one it knows.
      const known = (item: string) => (pmi.links[item]?.status ? 0 : 1);
      const ranked = (item: string) => rank?.(item) ?? 0;
      const items = Object.entries(pmi.presentation.items)
        .sort(([a], [b]) => ranked(a) - ranked(b) || known(a) - known(b));
      const seen = new Set<string>();
      const once = (g: DrawnGraphic) => {
        if (g.id === undefined) return true;
        if (seen.has(g.id)) return false;
        seen.add(g.id);
        return true;
      };
      const colourFor = (item: string | null, group: LabelGroup | undefined) => {
        if (colourOf) return colourOf(item, group);
        return group === 'default' ? STANDARD_GAP_INK : STANDARD_INK;
      };
      for (const [item, graphics] of items) {
        const group = drawnGroup(pmi.links[item]);
        const colour = colourFor(item, group);
        for (const g of graphics) if (once(g) && colour !== null) drawn.push(place(g, item, group, colour));
      }
      const loose = colourFor(null, undefined);
      for (const g of pmi.presentation.unlinked) if (once(g) && loose !== null) drawn.push(place(g, null, undefined, loose));
      // One balloon per item, on its first graphic; each clear of what is already drawn.
      const ballooned = new Set<string>();
      const taken: THREE.Box3[] = [];
      for (const d of [...drawn]) {
        const numbers = d.item === null ? undefined : balloons.get(d.item);
        if (d.item === null || !numbers?.length || ballooned.has(d.item) || d.extent.isEmpty()) continue;
        ballooned.add(d.item);
        addBalloon(d, numbers, taken, drawn);
      }
      for (const d of drawn) paint(d);
    }
    turn();
    apply();
  }

  function hitAt(x: number, y: number): string | null {
    if (!visible) return null;
    p.set(x, y);
    let best: string | null = null;
    let reach = HIT_PX;
    for (const d of drawn) {
      if (d.item === null || !on(d)) continue;
      for (const el of d.set) {
        const r = el.getBoundingClientRect();
        const view = el.parentElement?.parentElement?.getBoundingClientRect();
        if (!r.width || !view || el.style.visibility === 'hidden') continue;
        if (x >= r.left - view.left && x <= r.right - view.left && y >= r.top - view.top && y <= r.bottom - view.top) return d.item;
      }
      for (let k = 0; k + 1 < d.segments.length; k += 2) {
        if (!screen(d.segments[k]!, a) || !screen(d.segments[k + 1]!, b)) continue;
        const dist = nearSegment();
        if (dist < reach) { reach = dist; best = d.item; }
      }
      for (let k = 0; k + 2 < d.triangles.length; k += 3) {
        if (screen(d.triangles[k]!, a) && screen(d.triangles[k + 1]!, b) && screen(d.triangles[k + 2]!, c) && inside()) {
          return d.item;
        }
      }
    }
    return best;
  }

  return {
    set(pmi) {
      pmiNow = pmi;
      rebuild();
    },
    setVisible(next) {
      visible = next;
      if (next) { turn(); orient(); }
      apply();
    },
    filter(next, hideFar, chosen = new Set()) {
      show = next;
      farSide = hideFar;
      reveal = chosen;
      for (const d of drawn) d.shown = shownBy(d);
      turn();
      apply();
    },
    light(item) {
      if (item === lit) return;
      const was = lit;
      lit = item;
      for (const d of drawn) if (d.item !== null && (d.item === was || d.item === item)) paint(d);
      counts();
    },
    setBalloons(next) {
      const key = [...next].map(([item, n]) => `${item}=${n.join(',')}`).sort().join(';');
      if (key === balloonKey) return;
      balloonKey = key;
      balloons = next;
      if (pmiNow) rebuild();
      lastProjKey = '';
    },
    setUnsaved(items) {
      unsavedItems = items;
      for (const d of drawn) paint(d);
      apply();
    },
    setOrigin(origin) {
      const prevKey = originNow ? `${originNow.position.join(',')}|${originNow.xAxis.join(',')}|${originNow.yAxis.join(',')}|${originNow.zAxis.join(',')}` : '';
      const nextKey = origin ? `${origin.position.join(',')}|${origin.xAxis.join(',')}|${origin.yAxis.join(',')}|${origin.zAxis.join(',')}` : '';
      if (prevKey === nextKey && (origin === null) === (originNow === null)) return;
      originNow = origin;
      rebuildOrigin();
    },
    bounds() {
      const box = new THREE.Box3();
      for (const d of drawn) {
        if (!on(d)) continue;
        box.union(d.extent);
        for (const o of d.objects) if (o instanceof CSS2DObject) box.expandByPoint(o.position);
      }
      return box.isEmpty() ? null : box;
    },
    tightPoints() {
      if (turn()) apply();
      const pts: THREE.Vector3[] = [];
      for (const d of drawn) {
        if (!on(d)) continue;
        if (d.segments.length) {
          const step = Math.max(1, Math.floor(d.segments.length / 24));
          for (let i = 0; i < d.segments.length; i += step) pts.push(d.segments[i]!);
        }
        if (d.triangles.length) {
          const step = Math.max(1, Math.floor(d.triangles.length / 24));
          for (let i = 0; i < d.triangles.length; i += step) pts.push(d.triangles[i]!);
        }
        if (!d.segments.length && !d.triangles.length && !d.extent.isEmpty()) {
          pts.push(d.extent.min, d.extent.max);
        }
      }
      return pts;
    },
    hit(x, y) {
      return hitAt(x, y);
    },
    dispose() {
      clear();
      clearOrigin();
      for (const { line, fill } of inks.values()) { line.dispose(); fill.dispose(); }
    },
  };
}
