// The Specify review's view of a part, with the face picker's interface (picker.ts), so the
// review is unchanged by the move. The part is drawn by a HostView (host-view.ts): the host's,
// or by default the package's own on three-cad-viewer (default-view.ts). A face's state --
// plain, a tint from the queue, the current question, picked, under the pointer -- is a layer of
// its feature, each one colour; a change restyles only the layers it touches.
// Labels and drawn PMI are drawn over the default view's canvas, in a scene and renderer of
// their own, from its camera; over a host's view they are not drawn yet (docs/host-view.md).
import * as THREE from 'three';

import type { PmiAnalysis, PmiMesh, PmiVec } from '../types/pmi';

import { createAnnotations } from './annotations';
import { createDefaultView } from './cad-view';
import { HOME, type DefaultView } from './default-view';
import { drawnGroup, partCentre, type DrawnPmi, type DrawnPoint, type DrawnView } from './drawn';
import { createDrawnLayer } from './drawn-layer';
import { chooseFaceRoute, faceSamples, withFaceRoute, type Box, type HostPointerEvent, type HostView, type Vec3 } from './host-view';
import { toLabels, type PmiLabel } from './labels';
import { createLook, viewpointOf } from './look';
import type { Picker, PickerEvents } from './picker';
import { expand, faceOfTriangle, indexAnalysis, sameBinding, toggle, type PickMode } from './selection';

// The editor's colour scheme (docs/WORKFLOW.md), chosen to stay apart for colour-blind eyes,
// in the order a face takes them: the first that applies.
const STATES = [
  ['Picked', '#003b73'], // faces picked: chosen, navy
  ['Under the pointer', '#f4f8fb'], // what a click would pick: a slight whitening
  ['Current question', '#1ec8ff'], // active, cyan
  ['Needs an answer', '#eda07a'], // required: vermillion, pale
  ['Default to check', '#f3dc6a'], // check: yellow, pale
  ['Problem', '#8f7fe0'], // a problem on the drawing: violet
  ['Faces', '#c9ced4'],
] as const;
type State = (typeof STATES)[number][0];
const TINT: Record<'required' | 'check' | 'problem', State> = {
  required: 'Needs an answer', check: 'Default to check', problem: 'Problem',
};
const colourOf = (state: State) => STATES.find(([s]) => s === state)![1];
const boxOf = (box: THREE.Box3): Box => ({ min: box.min.toArray() as Vec3, max: box.max.toArray() as Vec3 });

/**
 * ``mesh`` and ``analysis`` must be pmi-assist's for the same file (format 2, same binding);
 * a mismatched pair is refused rather than mislabelled. ``view`` is the host's, if it gives
 * one; ``part`` is Specify's index of the part and ``entities`` its faces' STEP entities.
 */
export function createReviewView(
  container: HTMLElement,
  { mesh, analysis, view: given, part = 0, entities }: {
    mesh: PmiMesh; analysis: PmiAnalysis; view?: HostView | null; part?: number; entities?: readonly number[];
  },
): Picker {
  if (mesh.format !== 'pmi-assist-mesh' || mesh.version !== 2) {
    throw new Error(`unsupported mesh ${mesh.format} v${mesh.version}`);
  }
  if (!sameBinding(mesh.binding, analysis.binding)) {
    throw new Error('mesh and analysis are of different files');
  }
  const index = indexAnalysis(analysis);
  const all = mesh.faces.map((_, f) => f);
  // The tree (the default view's) is the review's features (its rows, as the panel names
  // them), each feature's faces in the layer of their state's colour; faces no row is about
  // are the part's others.
  const OTHER = 'Other faces';
  let featureOf = new Map<number, string>();
  const groupOf = (face: number) => featureOf.get(face) ?? OTHER;
  const own: DefaultView | null = given ? null : createDefaultView(container, {
    part, mesh, control: 'trackball',
    // A calm first look: the viewer's toolbar and tools panel come with "More view tools".
    tools: false,
    // A feature picked in the tree: its question, as picking one of its faces would.
    onPick: (path) => {
      const name = path.split('/')[2];
      const face = all.find((f) => groupOf(f) === name && name !== OTHER);
      if (face !== undefined) for (const f of listeners.face) f(face);
    },
  });
  const host = (given ?? own)!;
  const view = withFaceRoute(host, part, mesh);
  view.showParts([part]);
  view.home();
  let disposed = false;
  // The check on load: the host's faces against Specify's, before they are trusted.
  const diagonal = new THREE.Vector3(...mesh.bbox[0]).distanceTo(new THREE.Vector3(...mesh.bbox[1])) || 1;
  void (async () => {
    const info = await host.info();
    if (disposed) return;
    if (entities) host.partFaces?.(part, { entities });
    const route = await chooseFaceRoute(host, info, part, faceSamples(analysis.faces, entities), diagonal);
    if (!disposed) view.route(route);
  })();

  let mode: PickMode = 'face';
  let selection = new Set<number>();
  let focus = new Set<number>();
  let tints = new Map<number, 'required' | 'check' | 'problem'>();
  let hovered = new Set<number>();
  let pickable = true;
  const listeners: { [K in keyof PickerEvents]: PickerEvents[K][] } = { change: [], hover: [], face: [], label: [] };

  // Each face's layer as styled, its feature and state; a paint restyles only the layers whose
  // faces changed.
  const key = (group: string, state: State) => `${group}/${state}`;
  const stateIn = (k: string) => k.slice(k.lastIndexOf('/') + 1) as State;
  const drawnKey = new Map<number, string>();
  function stateOf(face: number): State {
    if (selection.has(face)) return 'Picked';
    if (hovered.has(face)) return 'Under the pointer';
    if (focus.has(face)) return 'Current question';
    const tint = tints.get(face);
    return tint ? TINT[tint] : 'Faces';
  }
  const style = (k: string) => {
    const faces = all.filter((f) => drawnKey.get(f) === k).map((face) => ({ part, face }));
    if (faces.length) view.styleFaces(k, [{ faces, colour: colourOf(stateIn(k)) }]);
    else view.clearStyles(k);
  };
  function paint() {
    const touched = new Set<string>();
    for (const face of all) {
      const next = key(groupOf(face), stateOf(face));
      const was = drawnKey.get(face);
      if (next === was) continue;
      touched.add(next);
      if (was !== undefined) touched.add(was);
      drawnKey.set(face, next);
    }
    for (const k of touched) style(k);
  }
  paint();

  // What hides a label or a feature: the part, as the package's own copy of it.
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute([...mesh.positions], 3));
  geometry.setIndex([...mesh.indices]);
  const occluder = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide }));
  geometry.computeBoundingBox();
  const partBox = geometry.boundingBox!.clone();
  let seeThrough = false;
  let moved = false;
  const stopMoved = view.onUserMoved(() => { moved = true; });

  // Labels and the file's own graphics, over the default view only.
  const overlays = own ? overlaysOver(own, {
    mesh, analysis, occluders: () => (seeThrough ? [] : [occluder]),
    onLabel: (label) => { for (const f of listeners.label) f(label); },
  }) : null;
  const drawnBounds = () => overlays?.drawnBounds() ?? null;
  let defaultView: DrawnView | null = null;
  let defaultViewKey = '';
  // Exact orthographic fit computed directly from world-space points along the camera's (right, up)
  // axes so the part and shown callouts sit centred and fill ~84% of the 3D viewport, independent
  // of three-cad-viewer's async frame update.
  const tightenFit = (eyeDir: DrawnPoint, upDir: DrawnPoint | undefined, around: THREE.Box3) => {
    if (!own) return;
    const cam = own.cad.camera as THREE.OrthographicCamera;
    if (!cam.isOrthographicCamera) return;
    const frustumW = Math.abs(cam.right - cam.left);
    const frustumH = Math.abs(cam.top - cam.bottom);
    if (frustumW < 1e-6 || frustumH < 1e-6) return;
    const e = new THREE.Vector3(...eyeDir).normalize();
    const look = e.clone().negate();
    let u = upDir ? new THREE.Vector3(...upDir).normalize() : new THREE.Vector3(0, 0, 1);
    if (Math.abs(e.dot(u)) > 0.99) u = new THREE.Vector3(0, 0, 1);
    const right = new THREE.Vector3().crossVectors(look, u).normalize();
    const camUp = new THREE.Vector3().crossVectors(right, look).normalize();
    const target = around.getCenter(new THREE.Vector3());

    let uMin = Infinity, uMax = -Infinity, vMin = Infinity, vMax = -Infinity;
    const visit = (x: number, y: number, z: number) => {
      const dx = x - target.x;
      const dy = y - target.y;
      const dz = z - target.z;
      const pu = dx * right.x + dy * right.y + dz * right.z;
      const pv = dx * camUp.x + dy * camUp.y + dz * camUp.z;
      if (pu < uMin) uMin = pu;
      if (pu > uMax) uMax = pu;
      if (pv < vMin) vMin = pv;
      if (pv > vMax) vMax = pv;
    };
    const stride = Math.max(1, Math.floor(mesh.positions.length / 3 / 600)) * 3;
    for (let i = 0; i + 2 < mesh.positions.length; i += stride) {
      visit(mesh.positions[i]!, mesh.positions[i + 1]!, mesh.positions[i + 2]!);
    }
    for (const q of overlays?.drawnLayer.tightPoints() ?? []) {
      visit(q.x, q.y, q.z);
    }
    const uSpan = uMax - uMin;
    const vSpan = vMax - vMin;
    if (!Number.isFinite(uSpan) || uSpan < 1e-6 || vSpan < 1e-6) return;
    const wantZoom = Math.min(8, Math.max(0.2, 0.84 * Math.min(frustumW / uSpan, frustumH / vSpan)));
    const shiftU = (uMin + uMax) / 2;
    const shiftV = (vMin + vMax) / 2;
    const nextTarget = target.clone().addScaledVector(right, shiftU).addScaledVector(camUp, shiftV);
    const dist = new THREE.Vector3(...own.cad.viewer.getCameraPosition())
      .distanceTo(new THREE.Vector3(...own.cad.viewer.getCameraTarget()));
    const nextEye = nextTarget.clone().addScaledVector(e, dist);
    own.cad.viewer.setCameraTarget(nextTarget.toArray() as [number, number, number], false);
    own.cad.viewer.setCameraPosition(nextEye.toArray() as [number, number, number], false, false);
    own.cad.viewer.setCameraZoom(wantZoom, false);
  };
  let lastFitSize = '';
  let lastFitAround: THREE.Box3 | null = null;
  const fitKey = () => {
    if (!own) return '';
    const [w, h] = own.cad.size();
    const cam = own.cad.camera as THREE.OrthographicCamera;
    const fw = cam.isOrthographicCamera ? Math.abs(cam.right - cam.left) : 0;
    const fh = cam.isOrthographicCamera ? Math.abs(cam.top - cam.bottom) : 0;
    return `${Math.round(w)}x${Math.round(h)}|${fw.toFixed(4)}x${fh.toFixed(4)}`;
  };
  if (own) {
    own.cad.onFrame(() => {
      if (moved || !defaultView || !lastFitAround) return;
      const key = fitKey();
      if (key === lastFitSize) return;
      lastFitSize = key;
      tightenFit(defaultView.eye, defaultView.up, lastFitAround);
    });
  }
  // The part with ``also``, framed from Best view when available.
  const homeView = () => {
    const also = drawnBounds();
    if (defaultView) {
      const v = (p: DrawnPoint) => new THREE.Vector3(...p);
      const around = partBox.clone().union(also ?? new THREE.Box3(v(defaultView.min), v(defaultView.max)));
      view.lookFrom([...defaultView.eye] as Vec3, { frame: boxOf(around), ...(defaultView.up ? { up: [...defaultView.up] as Vec3 } : {}) });
      lastFitAround = around;
      lastFitSize = fitKey();
      tightenFit(defaultView.eye, defaultView.up, around);
      return;
    }
    if (also) view.lookFrom(HOME as Vec3, { frame: boxOf(partBox.clone().union(also)) });
    else view.home();
  };

  const report = (face: number | null) => {
    // What a click would pick is lit only while picking; the face under the pointer is
    // reported either way.
    const next = new Set(face === null || !pickable ? [] : expand(index, face, mode));
    if ([...next].join() !== [...hovered].join()) { hovered = next; paint(); }
    if (face !== hoverFace) { hoverFace = face; for (const f of listeners.hover) f(face); }
  };
  const faceOf = (event: HostPointerEvent) => (event.hit && event.hit.part === part && event.mesh === undefined ? event.hit.face : null);
  let hoverFace: number | null = null;
  const stopPointer = view.onPointer((event) => {
    if (event.type === 'leave') { report(null); return; }
    if (event.type === 'hover') {
      overlays?.pointAt(event.x, event.y);
      report(faceOf(event));
      return;
    }
    if (event.type !== 'click') return;
    // A drawn callout clicked is its semantic item, as its label would be.
    if (overlays?.clickAt(event.x, event.y)) return;
    const face = faceOf(event);
    if (face === null) return;
    for (const f of listeners.face) f(face);
    if (!pickable) return;
    // Shift adds to the selection; a plain click starts a new one.
    selection = toggle(selection, expand(index, face, mode), !event.shift);
    paint();
    const sorted = [...selection].sort((a, b) => a - b);
    for (const f of listeners.change) f(sorted);
  });
  let framedLabels = false;

  const viewpoint = viewpointOf(host, diagonal);
  const lookAt = createLook(viewpoint, {
    mesh, occluder, faceOf: (triangle) => faceOfTriangle(mesh.faces, triangle),
    faceInfo: (face) => index.faces.get(face),
  });

  const navigator = {
    fit: () => { const also = drawnBounds(); view.fit(also ? { also: boxOf(also) } : undefined); },
    home: homeView,
    roll: (quarters: number) => view.roll?.(quarters),
    dolly: (factor: number) => view.dolly?.(factor),
  };

  return {
    navigator: navigator as unknown as Picker['navigator'],
    missing: mesh.missing,
    annotate(intent, { current = null, show = {}, balloons = new Map(), pending, reveal, datums = true, labels } = {}) {
      if (!overlays) return;
      const shown = (labels ?? toLabels(intent, analysis, pending))
        .filter((label) => datums || label.kind !== 'datum' || label.group === 'existing')
        .filter((label) => show[label.group] !== false || reveal?.has(label.id));
      overlays.annotations.set(shown, { current, anchors: intent?.anchors ?? {}, balloons });
      // The opening view is chosen before the labels arrive; the first time they do, it is
      // chosen again -- unless the view has been moved.
      if (shown.length && !framedLabels) {
        framedLabels = true;
        if (!moved) homeView();
      }
    },
    features(rows) {
      // A face in several rows (a hole and its pattern) is in the first; names are made unique
      // and plain, as the viewer's tree finds nodes by name and shows them as HTML.
      const used = new Map<string, number>();
      const next = new Map<number, string>();
      for (const row of rows) {
        const base = row.title.replace(/[/<>&"]/g, ' ').replace(/\s+/g, ' ').trim() || 'Feature';
        const n = (used.get(base) ?? 0) + 1;
        used.set(base, n);
        const name = n > 1 ? `${base} (${n})` : base;
        for (const f of row.faces) if (!next.has(f)) next.set(f, name);
      }
      if ([...next].join() === [...featureOf].join()) return;
      // The layers again, in the panel's order: its features, then the part's other faces.
      for (const k of new Set(drawnKey.values())) view.clearStyles(k);
      drawnKey.clear();
      featureOf = next;
      for (const group of [...new Set([...next.values()]), OTHER]) {
        for (const [state] of STATES) {
          const k = key(group, state);
          for (const f of all) if (groupOf(f) === group && stateOf(f) === state) drawnKey.set(f, k);
          if (all.some((f) => drawnKey.get(f) === k)) style(k);
        }
      }
      own?.collapse();
    },
    setLabelsVisible: (on) => overlays?.annotations.setVisible(on),
    drawn(pmi, visible, labels) {
      if (!overlays) return;
      // Turned on: framed once with its callouts, so none starts out of view.
      if (overlays.drawn(pmi, visible, labels) && !moved) homeView();
    },
    lightDrawn: (item) => overlays?.drawnLayer.light(item),
    drawnFilter: (show, farSide, reveal) => overlays?.drawnLayer.filter(show, farSide, reveal),
    setDrawnUnsaved: (items) => overlays?.drawnLayer.setUnsaved(items),
    setDrawnOrigin: (origin) => overlays?.drawnLayer.setOrigin(origin),
    setDefaultView(next) {
      defaultView = next;
      const nextKey = next ? `${next.eye.join(',')}|${next.up?.join(',') ?? ''}|${next.min.join(',')}|${next.max.join(',')}` : '';
      if (nextKey !== defaultViewKey) {
        defaultViewKey = nextKey;
        if (!moved) homeView();
      }
    },
    lookSquare(drawnView) {
      // Square on, framing its graphics with the part: a callout is read where it is on the part.
      const v = (p: DrawnPoint) => new THREE.Vector3(...p);
      const around = partBox.clone().union(new THREE.Box3(v(drawnView.min), v(drawnView.max)));
      view.lookFrom([...drawnView.eye] as Vec3, { frame: boxOf(around), ...(drawnView.up ? { up: [...drawnView.up] as Vec3 } : {}) });
      if (drawnView.key === 'best') tightenFit(drawnView.eye, drawnView.up, around);
    },
    setFarSideHidden: (on) => overlays?.annotations.setFarSideHidden(on),
    setTools: (on) => {
      own?.showTools(on);
      if (!moved && defaultView && lastFitAround) {
        lastFitSize = fitKey();
        tightenFit(defaultView.eye, defaultView.up, lastFitAround);
      }
    },
    setTransparency(amount) {
      const a = Math.min(Math.max(amount, 0), 0.95);
      seeThrough = a > 0;
      view.setOpacity(1 - a);
      overlays?.annotations.recheck();
    },
    setMode(next) { mode = next; hovered = new Set(); paint(); },
    setPickable(on) { pickable = on; hovered = new Set(); paint(); },
    setSelection(faces) { selection = new Set(faces); paint(); },
    getSelection: () => [...selection].sort((a, b) => a - b),
    highlight(faces) { focus = new Set(faces); paint(); },
    tint(states) { tints = new Map(states); paint(); },
    lookAt,
    on(name, listener) { (listeners[name] as typeof listener[]).push(listener); },
    dispose() {
      disposed = true;
      overlays?.dispose();
      stopPointer();
      stopMoved();
      viewpoint.dispose();
      geometry.dispose();
      // The host's view is the host's: only what the review put on it is taken off.
      if (own) own.dispose();
      else for (const k of new Set(drawnKey.values())) view.clearStyles(k);
    },
  };
}

/** The review's labels and the file's own graphics, drawn over the default view's canvas. */
function overlaysOver(own: DefaultView, { mesh, analysis, occluders, onLabel }: {
  mesh: PmiMesh; analysis: PmiAnalysis; occluders: () => THREE.Object3D[]; onLabel: (label: PmiLabel) => void;
}) {
  const cad = own.cad;
  // The labels' own scene, drawn by a renderer of their own over the canvas.
  const scene = new THREE.Scene();
  const ink = new THREE.WebGLRenderer({ alpha: true, antialias: true });
  ink.setPixelRatio(window.devicePixelRatio);
  ink.domElement.style.cssText = 'position:absolute;left:0;top:0;pointer-events:none';
  cad.overlayHost.append(ink.domElement);
  const labels = document.createElement('div');
  labels.style.cssText = 'position:absolute;left:0;top:0;pointer-events:none';
  cad.overlayHost.append(labels);
  const host = {
    scene, diagonal: cad.diagonal,
    get camera() { return cad.camera; },
    size: () => cad.size(),
    onFrame: (listener: () => void) => cad.onFrame(listener),
  };
  cad.onFrame(() => {
    const [w, h] = cad.size();
    const canvas = cad.canvas as HTMLElement;
    for (const el of [ink.domElement, labels]) {
      el.style.left = `${canvas.offsetLeft}px`;
      el.style.top = `${canvas.offsetTop}px`;
      el.style.width = `${w}px`;
      el.style.height = `${h}px`;
    }
    ink.setSize(w, h, false);
    ink.domElement.style.width = `${w}px`;
    ink.domElement.style.height = `${h}px`;
    ink.render(scene, cad.camera);
  });
  const annotations = createAnnotations(host, labels, {
    mesh, analysis, onClick: onLabel,
    // A label behind the part is what someone turns transparency up to read.
    occluders,
  });

  // The file's own graphics, in the labels' scene; a linked one clicked is its item's label.
  let drawnPmi: DrawnPmi | null = null;
  let reviewLabels: ReadonlyMap<string, PmiLabel> | undefined;
  let drawnShowing = false;
  // The part, and the file's callouts when they are drawn: a notes block off the part is in view.
  const drawnBounds = () => (drawnShowing ? drawnLayer.bounds() : null);
  // The orientation cube frames them too, as home and fit do.
  cad.alsoFrame(drawnBounds);
  const drawnLabel = (item: string): PmiLabel => {
    const reviewLabel = reviewLabels?.get(item);
    if (reviewLabel) return reviewLabel;
    const link = drawnPmi?.links[item];
    return {
      id: `drawn:${item}`, kind: 'note', group: drawnGroup(link) ?? 'existing',
      cells: [drawnPmi?.presentation.items[item]?.[0]?.name ?? item],
      faces: (link?.faces ?? []).filter((f) => f >= 0 && f < mesh.faces.length),
      ...(link?.question ? { question: link.question } : {}),
    };
  };
  const drawnLayer = createDrawnLayer(host, labels, {
    onClick: (item) => onLabel(drawnLabel(item)),
    titleOf: (item) => reviewLabels?.get(item)?.title,
    centre: partCentre(mesh.positions),
  });
  return {
    annotations, drawnLayer, drawnBounds,
    /** Shows the file's graphics, or not; whether they were just turned on. */
    drawn(pmi: DrawnPmi | null, visible: boolean, nextLabels?: ReadonlyMap<string, PmiLabel>) {
      reviewLabels = nextLabels;
      if (pmi !== drawnPmi) { drawnPmi = pmi; drawnLayer.set(pmi); }
      const showing = visible && pmi !== null;
      drawnLayer.setVisible(showing);
      const turnedOn = showing && !drawnShowing;
      drawnShowing = showing;
      return turnedOn;
    },
    /** Over a drawn callout: a hand, and its notation as haecceity names it. */
    pointAt(x: number, y: number) {
      const item = drawnLayer.hit(x, y);
      cad.canvas.style.cursor = item === null ? '' : 'pointer';
      cad.canvas.title = item === null ? '' : reviewLabels?.get(item)?.title ?? drawnPmi?.presentation.items[item]?.[0]?.name ?? '';
    },
    /** A drawn callout clicked is its semantic item, as its label would be: whether one was. */
    clickAt(x: number, y: number) {
      const item = drawnLayer.hit(x, y);
      if (item === null) return false;
      onLabel(drawnLabel(item));
      return true;
    },
    dispose() {
      drawnLayer.dispose();
      annotations.dispose();
      ink.dispose();
    },
  };
}
