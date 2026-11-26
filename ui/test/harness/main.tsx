// A minimal host for <StepDiff>: two file inputs, and what the component tells its host recorded
// on `window` for the browser test to read. The Specify WASM is the dependency, as a host gets it.
import { createRoot } from 'react-dom/client';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  createIntentEngine, type HostPointerEvent, type HostView, SpecifyReview, StepDiff, type DrawnPmi, type PmiAnswers, type StepFile,
} from '../../src/index';
import { findStatedOrigin, normalizeDraftingPmi } from '../../src/specify/drafting-layout';
import type { PartOrigin } from '../../src/specify/drawn';
import { readSceneSemanticLabels, reviewDrawnFromScene, type SceneSemanticLabel } from '../../src/specify/one-view';
import { stepBytes } from '../../src/model/diff';

declare global {
  interface Window {
    __events: string[];
    /** The Specify host's calls from SpecifyReview ('questions' | 'interpret' | 'preview' | 'inspection', and
     * 'onKeep:<ids>' with &keep=host), when ?specify=1. */
    __calls: string[];
    __questionAnswers: PmiAnswers[];
    /** When set, tests release each held questions reply by calling its function. */
    __questionReplies?: (() => void)[];
    __parts: (string | null)[];
    __setAnswers(answers: unknown): void;
    __setPart(name: string | null): void;
    /** With ?host=1: what the component asked of the stand-in host view, and a way to send it pointer events. */
    __host: { calls: [string, unknown][]; pointer(event: HostPointerEvent): void };
  }
}
window.__events = [];
window.__parts = [];
window.__calls = [];
window.__questionAnswers = [];

function Slot({ label, file, pick }: { label: string; file: StepFile | null; pick: (file: StepFile) => void }) {
  return (
    <label className="diff-slot">
      <span className="diff-slot-label">{label}</span>
      <input type="file" onChange={async event => {
        const chosen = event.target.files?.[0];
        if (chosen) pick({ name: chosen.name.replace(/\.gz$/, ''), bytes: await stepBytes(chosen) });
      }} />
      <span className="diff-slot-file">{file ? file.name : 'Choose a STEP file'}</span>
    </label>
  );
}

// ?host=1: a stand-in for a host's own 3D view, drawing nothing and recording every call, as a
// host such as Amy's viewer would receive them (docs/host-view.md).
function recordingHost(): HostView {
  const calls: [string, unknown][] = [];
  const pointers = new Set<(e: HostPointerEvent) => void>();
  window.__host = { calls, pointer: e => { for (const l of pointers) l(e); } };
  const rec = (name: string) => (...args: unknown[]) => { calls.push([name, JSON.parse(JSON.stringify(args.length === 1 ? args[0] : args))]); };
  return {
    info: async () => ({ faceIds: 'mapshapes', canDrawMeshes: true }),
    showParts: rec('showParts'), styleFaces: rec('styleFaces'), clearStyles: rec('clearStyles'), setOpacity: rec('setOpacity'),
    addMesh: (id, _mesh, opts) => calls.push(['addMesh', { id, opts }]), removeMesh: rec('removeMesh'),
    lookFrom: rec('lookFrom'), fit: rec('fit'), home: rec('home'),
    onCamera: () => () => {}, onPointer: l => { pointers.add(l); return () => pointers.delete(l); }, onUserMoved: () => () => {},
    rect: async () => ({ left: 0, top: 0, width: 800, height: 600 }), dispose: rec('dispose'),
    showCallouts: rec('showCallouts'),
  };
}
const hostView = new URLSearchParams(location.search).get('host') === '1' ? recordingHost() : null;

// ?wasmUrl=… : the engine the host serves itself (the override), not the dependency.
const wasmUrl = new URLSearchParams(location.search).get('wasmUrl') ?? undefined;

function Harness() {
  const [before, setBefore] = useState<StepFile | null>(null);
  const [after, setAfter] = useState<StepFile | null>(null);
  const [answers, setAnswers] = useState<unknown>({});
  const [part, setPart] = useState<string | null>(null);
  useEffect(() => { window.__setAnswers = setAnswers; window.__setPart = setPart; }, []);
  return (
    <main className="diff-page">
      <div className="diff-slots">
        <Slot label="Before" file={before} pick={setBefore} />
        <Slot label="After" file={after} pick={setAfter} />
      </div>
      {before && after && (
        <StepDiff
          before={before} after={after} answers={answers} part={part} wasmUrl={wasmUrl} view={hostView}
          onPart={name => { window.__parts.push(name); setPart(name); }}
          onEvent={name => window.__events.push(name)}
        />
      )}
    </main>
  );
}

// ?specify=1 (and &readOnly=1): <SpecifyReview> on a picked STEP instead. Its engine is the worker's, and
// records each call the component makes of it on window.__calls.
const query = new URLSearchParams(location.search);
const readOnly = query.get('readOnly') === '1';
// &keep=host: the host keeps suggestions itself, as the editor does (onKeep, and kept for the ticks);
// otherwise SpecifyReview keeps one by answering it with its default, as the local app does.
const hostKeeps = query.get('keep') === 'host';

const noAnswers = {};

function SpecifyHost() {
  // Made here, not at module level: the StepDiff page starts no extra worker.
  const worker = useMemo(() => createIntentEngine({ wasmUrl }), []);
  const [open, setOpen] = useState<{ session: any; review: any; engine: any; drawn: DrawnPmi | null; semantic: ReadonlyMap<string, SceneSemanticLabel>; origin: PartOrigin | null } | null>(null);
  const view = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const exports = useRef<HTMLDivElement>(null);
  const [slots, setSlots] = useState<{ view: HTMLElement; panel: HTMLElement; exports: HTMLElement } | null>(null);
  useEffect(() => setSlots({ view: view.current!, panel: panel.current!, exports: exports.current! }), []);
  const [kept, setKept] = useState<readonly string[]>([]);
  // Sign-in gates can hold an export action while the person continues editing.
  const deferred = useRef<(() => void)[]>([]);
  const onKeep = (ids: readonly string[]) => { window.__calls.push(`onKeep:${ids.join(',')}`); setKept((was) => [...was, ...ids]); };
  const pick = async (file: File) => {
    const name = file.name.replace(/\.gz$/, '');
    const bytes = await stepBytes(file);
    const sourceId = name;
    const snapshot: any = await worker.request('open', { name, bytes, sourceId });
    const review: any = await worker.request('review', { name, bytes, sourceId, answers: {} });
    const call = (operation: string, answers: unknown) => worker.request<any>(operation, { name, bytes, sourceId, answers });
    const engine = {
      questions: async (_id: string, answers: PmiAnswers) => {
        window.__calls.push('questions');
        window.__questionAnswers.push(answers);
        const replies = window.__questionReplies;
        if (replies) {
          const release = new Promise<void>((resolve) => replies.push(resolve));
          const [review] = await Promise.all([call('review', answers), release]);
          return review;
        }
        return call('review', answers);
      },
      interpret: async (_id: string, answers: unknown, question: string, text: string) => {
        window.__calls.push('interpret');
        return worker.request<any>('interpret', { analysis: snapshot.analysis, answers, question, text });
      },
      preview: async (_id: string, answers: unknown) => { window.__calls.push('preview'); return (await call('review', answers)).intent; },
      ...(!query.has('legacy') ? { inspection: async (_id: string, answers: unknown) => { window.__calls.push('inspection'); return (await call('review-given', answers)).intent; } } : {}),
    };
    const session = { sessionId: sourceId, filename: name, analysis: snapshot.analysis, mesh: snapshot.mesh, questions: review.questions, problems: review.problems };
    // The PMI as drawn, laid out on 3D annotation planes.
    const saved = snapshot.analysis.resume?.answers ?? {};
    const raw = await worker.request('scene', { name, bytes, sourceId, state: { answers: saved }, exact: true });
    const scene = reviewDrawnFromScene(raw, snapshot.analysis);
    const semantic = readSceneSemanticLabels(raw);
    const drawn = scene.ok ? normalizeDraftingPmi(scene.value, raw, snapshot.analysis, snapshot.mesh, semantic) : null;
    const origin = findStatedOrigin(bytes, snapshot.analysis, drawn);
    setOpen({ session, review, engine, drawn, semantic, origin });
  };
  return (
    <main className="diff-page">
      <input id="specify-file" type="file" onChange={event => { const chosen = event.target.files?.[0]; if (chosen) void pick(chosen); }} />
      <div id="specify-view" style={{ height: 520 }} ref={view} />
      <div id="specify-panel" ref={panel} />
      <div id="specify-actions">
        <div id="specify-exports" ref={exports} />
        {query.has('defer') && <button onClick={() => deferred.current.shift()?.()}>Resume download</button>}
      </div>
      {open && slots && (
        <SpecifyReview
          engine={open.engine} session={open.session} slots={slots} readOnly={readOnly} drawn={open.drawn}
          sceneSemantic={open.semantic} partOrigin={open.origin}
          externalAnswers={noAnswers} externalReview={open.review} disabled={false}
          onExport={action => { if (query.has('defer')) deferred.current.push(action); else action(); }} {...(hostKeeps ? { onKeep, kept } : {})}
        />
      )}
    </main>
  );
}

createRoot(document.getElementById('root')!).render(query.get('specify') === '1' ? <SpecifyHost /> : <Harness />);
