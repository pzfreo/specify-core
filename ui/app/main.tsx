// Specify on this computer: the app the page server serves (specify-pmi docs/open.md), started by
// `specify-pmi open` or by an agent's `show` (docs/session.md). The review screens (SpecifyReview, through adapter.ts) show
// the file's feature table; the person's edits are held in the page and Save sends them to the
// command as an edit list per part, which it applies to the file all or nothing. The session's answers (an
// agent's) come down as they land. Nothing is stored in the browser and nothing is fetched from
// anywhere but the page's own origin.
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/600.css';
import '../src/specify/PmiLab.css';
import './app.css';
import { createPortal } from 'react-dom';
import { createRoot } from 'react-dom/client';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  as9102, createIntentEngine, SpecifyReview, StepDiff, toCsv, xlsx,
  type DrawnPmi, type IntentReview, type PmiAnswers, type PmiLabSession, type PmiQuestion, type SpecifyEngine, type SpecifySlots, type WorkerClient,
} from '../src/index';
import type { PmiAnalysis, PmiExtra } from '../src/types/pmi';
import { stepBytes } from '../src/model/diff';
import { SessionApiMismatch } from './session-api';
import { saveFile, type CalledOut } from '../src/specify/SpecifyReview';
import { describe as describeExtra } from '../src/specify/added';
import { describeFaces } from '../src/specify/datums';
import type { StoredInspection } from '../src/specify/saved-inspection';
import { changedIds, createSession, sameAnswers, type AnswersSession, type AnswersStore } from './answers';
import { decisions, type Decision } from './diff-report';
import { discardAnswers, getAnswers, getConfig, getDiff, getFile, isStepName, keepAlive, listen, openFile, type DiffBody, type EditFailure, type EditList, type LocalConfig, type AnswerDraft } from './local';
import { EditsRefused, editsSaver, FileChanged, NotShown, Stale, waitingOf, type Saver } from './save';
import { PendingEdits, rejectedNoneByPart, removalTargets, withDrafts, withoutSent, type Pending } from './edits';
import { EditBar } from './EditBar';
import { answerEdit, asEngineTakes, valueText, callOutEdit, forEngine, forScreens, isDimensionQuestion, nextExtraKey, removeEdit, statedRows, type StatedRows } from './adapter';
import { labelStated } from '../src/specify/units';
import { findStatedOrigin, normalizeDraftingPmi } from '../src/specify/drafting-layout';
import { readSceneSemanticLabels, reviewDrawnFromScene, type OneViewPending, type OneViewRow, type SceneSemanticLabel } from '../src/specify/one-view';
import { answerText, bareAnalysis, fileLengthUnit, panelModel, tableOf, type FeatureTable, type PanelModel } from './table';

declare const __APP_VERSION__: string;
const DOCS = 'https://github.com/pzfreo/specify-pmi#readme';
const ISSUES = 'https://github.com/pzfreo/specify-pmi/issues';
const EMPTY_SEMANTIC: ReadonlyMap<string, SceneSemanticLabel> = new Map();

/** Builds the exact scene review state passed to `engine.pmiScene`, keeping un-asked general
 * length/location defaults out of stacking so before-Save and after-Save layouts match. The
 * defaults shown are suggestions Save does not write (`suggested`): the engine numbers none of
 * them, so every number shown is the one Save writes. */
function sceneStateFor(
  answers: PmiAnswers,
  questions: readonly PmiQuestion[],
  statedIds: ReadonlySet<string>,
  readOnly: boolean,
  pending?: PendingEdits,
): { answers: PmiAnswers; auto: string[]; suggested: string[] } {
  if (readOnly) return { answers, auto: [], suggested: [] };
  const given: Record<string, unknown> = { ...answers };
  if (pending) {
    for (const edit of pending.values()) {
      if (statedIds.has(edit.id)) continue;
      if (edit.kind === 'fill') given[edit.id] = edit.value;
      else if (edit.kind === 'none') given[edit.id] = 'none';
      else if (edit.kind === 'clear') delete given[edit.id];
    }
  }
  const sceneAnswers: Record<string, unknown> = { ...given };
  const auto: string[] = [];
  for (const q of questions) {
    if (isDimensionQuestion(q.id) || statedIds.has(q.id) || q.id in given) continue;
    if (q.default === null || q.default === undefined || q.default === '') continue;
    if (Array.isArray(q.default) && q.default.length === 0) continue;
    sceneAnswers[q.id] = q.default;
    auto.push(q.id);
  }
  return { answers: sceneAnswers, auto, suggested: auto };
}
/** A part of the file, as the engine's `parts` lists them; the session keeps answers by its index.
 * ``written``: the file stores Specify's answers for it. */
interface PartRow { part: number; name: string; instances: number; written: boolean }

/** Part ``part`` of the file, as the engine opens it: a file of one part whole, a part of several as that part.
 * ``generation``: how many saves the bytes are on from the file as opened (each save's file is a new source). */
const fileOf = (name: string, bytes: Uint8Array, part: number, several: boolean, generation = 0) => ({
  name, bytes, sourceId: `${several ? `${name}#${part}` : name}${generation ? `@${generation}` : ''}`, ...(several ? { part } : {}),
});

interface Opened {
  /** The part shown: its index in the session, and whether the file has others. */
  part: number; several: boolean;
  /** How many saves ``bytes`` are on from the file as opened. */
  generation: number;
  session: PmiLabSession & { answers: PmiAnswers }; review: IntentReview; engine: SpecifyEngine; drawn: DrawnPmi | null;
  semantic: ReadonlyMap<string, SceneSemanticLabel>;
  sceneKey: string;
  bytes: Uint8Array; store: AnswersStore; saver: Saver;
  /** The review panel's model: the file's feature table, named. */
  model: PanelModel;
  /** The file's feature table, and its stated rows as the review shows them (adapter.ts). */
  table: FeatureTable; stated: StatedRows;
  /** The questions asked of the bare part: every row's question, stated or not. */
  bare: PmiQuestion[];
  /** The general dimensions' questions, as the engine last asked them (not shown: Call Out answers them). */
  dimensions(): PmiQuestion[];
  /** The file's length unit, for a new general tolerance block (#228 decision 3). */
  units: 'in' | 'mm';
  /** Working answers used for agent notices and revision tracking. */
  external: { answers: PmiAnswers; review: IntentReview };
  /** The working answers processed by the page at the last shown session revision; ``all``: every
   * answer the review last showed, what the file states included (each change is from these). */
  shown: { answers: PmiAnswers; all: PmiAnswers; generation: number };
  /** Specify's review of ``answers``, for answers that arrive from outside the page. */
  reviewOf(answers: PmiAnswers): Promise<IntentReview>;
}
/** The revision comparison the command gave (``body``: WASM `diff()`'s, with the PMI scenes), drawn as it is. */
interface Compared { old: { name: string; bytes: Uint8Array }; next: { name: string; bytes: Uint8Array }; body: DiffBody; decisions: Decision[]; verdict: string; summary: string }

/** Opens the part on the engine with the session's answers: its analysis and mesh, their review,
 * and the PMI as drawn. ``edited``: called on each change the person makes. */
async function openPart(worker: WorkerClient, answersSession: AnswersSession, name: string, bytes: Uint8Array, at: { part: number; several: boolean }, readOnly: boolean, edited: () => void, generation = 0,
  answered: (part: number, id: string, value: unknown) => void = () => {}): Promise<Opened> {
  const file = fileOf(name, bytes, at.part, at.several, generation);
  const { sourceId } = file;
  const snapshot = await worker.request<{ analysis: PmiLabSession['analysis']; mesh: PmiLabSession['mesh']; provenance?: PmiLabSession['provenance'] }>('open', file);
  const analysis = snapshot.analysis as PmiAnalysis;
  // Every row named as Specify asks it of the bare part (the table's rows are those questions).
  const bare = await worker.request<IntentReview>('review-analysis-given', { analysis: bareAnalysis(analysis), answers: {} });
  const table = tableOf(analysis);
  // What the file states: shown as answered rows, kept out of the engine's review (adapter.ts).
  const stated = statedRows(table, bare.questions, analysis);
  // The session's answers with the page's unsaved changes to this part (kept while another part was shown).
  const answers = answersSession.shown(at.part);
  // The general dimensions' questions, from the engine's latest review: what Call Out answers.
  let dimensionQuestions: PmiQuestion[] = [];
  const screened = (raw: IntentReview) => {
    dimensionQuestions = raw.questions.filter((q) => isDimensionQuestion(q.id));
    return forScreens(raw, stated);
  };
  const reviewOf = async (given: PmiAnswers) => screened(
    await worker.request<IntentReview>(readOnly ? 'review-given' : 'review', { ...file, answers: forEngine(given, stated) }));
  const review = await reviewOf(answers);
  const stored = snapshot.analysis.resume?.answers ?? {};
  const fileReview = sameAnswers(stored, answers) ? review : await reviewOf(stored);
  const store = answersSession.store(at.part, stored);
  // ``generation``: Discard starts the review again; a commit begun before it is stale. (A save
  // reopens the part with a new ``shown``: a commit in flight then is an edit not yet saved.)
  const shown = { answers, all: { ...stated.values, ...answers }, generation: 0 };
  const engine: SpecifyEngine = {
    // Each commit of the page's answers comes through here: what the session gets once Specify
    // accepts them. The change is the one from the answers the page showed when it was made, so an
    // agent's answer that arrived while Specify reviewed it is kept.
    questions: async (_id, typed) => {
      const shownAnswers = asEngineTakes(typed);
      const given = forEngine(shownAnswers, stated);
      const started = shown.generation;
      const base = shown.answers;
      // What the review showed when this commit began: an agent's answer landing meanwhile is not
      // taken for the person's change.
      const was = shown.all;
      const asked = screened(await worker.request<IntentReview>('review', { ...file, answers: given }));
      // Discard started the review again meanwhile: this commit's answers are gone.
      if (shown.generation !== started) return asked;
      // Each question whose answer the person changed since the review last showed it is an edit,
      // held in the page until Save; set back to the file's value, its edit goes.
      for (const id of new Set([...Object.keys(shownAnswers), ...Object.keys(was)])) {
        if (id.startsWith('_') || JSON.stringify(shownAnswers[id]) === JSON.stringify(was[id])) continue;
        answered(at.part, id, shownAnswers[id]);
      }
      shown.all = shownAnswers;
      shown.answers = given;
      store.set(given, base);
      if (!sameAnswers(given, base)) edited();
      return asked;
    },
    interpret: (_id, given, question, text) => worker.request('interpret', { analysis: snapshot.analysis, answers: forEngine(asEngineTakes(given), stated), question, text }),
    preview: async (_id, given) => (await reviewOf(asEngineTakes(given))).intent,
  };
  const session: Opened['session'] = {
    sessionId: sourceId, filename: name, analysis: snapshot.analysis, mesh: snapshot.mesh, answers: { ...stated.values, ...stored },
    ...(snapshot.provenance ? { provenance: snapshot.provenance } : {}),
    questions: fileReview.questions, open: fileReview.open, problems: fileReview.problems,
  };
  const state = sceneStateFor(answers, fileReview.questions, stated.ids, readOnly);
  const sceneKey = JSON.stringify(state);
  const raw = await worker.request('scene', { ...file, state, exact: true }).catch(() => null);
  const read = raw ? reviewDrawnFromScene(raw, analysis) : null;
  const semantic = raw ? readSceneSemanticLabels(raw) : EMPTY_SEMANTIC;
  const drawn = read?.ok ? normalizeDraftingPmi(read.value, raw, analysis, snapshot.mesh, semantic) : null;
  const model = panelModel(table, bare.questions, analysis.features);
  return {
    ...at, generation, session, review: fileReview, engine, drawn, semantic, sceneKey, bytes, store, shown,
    saver: editsSaver(store, answersSession, () => shown.answers), external: { answers, review }, reviewOf, model,
    units: fileLengthUnit(bytes), table, stated, bare: bare.questions, dimensions: () => dimensionQuestions,
  };
}

/** How the bar names row ``id`` of the part ``o`` shows ("Ø1.6 hole position", "Material"). */
function rowName(o: Opened, id: string): string {
  const rows = [...(o.model.part?.rows ?? []), ...o.model.features.flatMap((g) => g.rows), ...(o.model.datums?.rows ?? [])];
  const row = rows.find((r) => r.id === id);
  if (row) return row.name;
  const q = o.bare.find((x) => x.id === id);
  return q?.label ?? q?.prompt ?? id;
}

const stem = (name: string) => name.replace(/\.gz$/i, '').replace(/\.(step|stp)$/i, '');
const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`;

interface InspectionStep { path: string; name: string; bytes: Uint8Array }
const inspectionName = (path: string) => path.split(/[\\/]/).pop() ?? path;
const SAVE_FIRST = 'Save first: the inspection files describe the saved STEP';
const NO_EDITS = new PendingEdits();

/** What the last save wrote, and the questions still open in it, by their short labels. */
interface SavedLine { path: string; open: string[] | null; byAgent: boolean; waiting?: string[] }

function App() {
  const [config, setConfig] = useState<LocalConfig | null>(null);
  const [liveScene, setLiveScene] = useState<{ sessionId: string; key: string; drawn: DrawnPmi | null; semantic: ReadonlyMap<string, SceneSemanticLabel> } | null>(null);
  const sceneReq = useRef(0);
  const [opened, setOpened] = useState<Opened | null>(null);
  const [parts, setParts] = useState<PartRow[]>([]);
  const [switching, setSwitching] = useState(false);
  // The session (every part's answers, and what is on disk) and the part shown, for its events.
  const sessionRef = useRef<AnswersSession | null>(null);
  const shownRef = useRef<Opened | null>(null);
  const [compared, setCompared] = useState<Compared | null>(null);
  const [tab, setTab] = useState<'changes' | 'answers'>('changes');
  const [problem, setProblem] = useState<string | null>(null);
  const [openingError, setOpeningError] = useState<string | null>(null);
  const [apiProblem, setApiProblem] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<SavedLine | null>(null);
  const [dirty, setDirty] = useState(false);
  // Edits not saved yet, held here or in the session: read by the heartbeat.
  const unsavedRef = useRef(false);
  // The event stream: null until it first opens, false while it is lost (retrying).
  const [live, setLive] = useState<boolean | null>(null);
  const [stopped, setStopped] = useState(false);
  // An in-place save refused because another program changed the file: Save cannot succeed again.
  const [fileChanged, setFileChanged] = useState(false);
  // The review panel's edits, held in the page until Save sends them (specify-panel.md decision 3).
  const [localPending, setPending] = useState(() => new Map<number, PendingEdits>());
  const [draftSnapshot, setDraftSnapshot] = useState<{ rev: number; drafts: Record<string, AnswerDraft> }>({ rev: 0, drafts: {} });
  const [discarding, setDiscarding] = useState(false);
  const [rejectedNone, setRejectedNone] = useState(() => new Map<number, Map<string, string>>());
  const [checking, setChecking] = useState(false);
  const [reopening, setReopening] = useState(false);
  const [syncing, setSyncing] = useState(false);
  // What the bar says after a save, until the next edit.
  const [savedNote, setSavedNote] = useState<string | null>(null);
  // The characteristics table (the review's, read only) is drawn into this, under the panel's tab.
  const charsElement = useRef<HTMLDivElement>(document.createElement('div'));
  // Answers that arrived from outside the page but could not be shown (their review failed): Save
  // is refused until they are, so the page offers to show them again.
  const [unshown, setUnshown] = useState<string | null>(null);
  const reshow = useRef<() => void>(() => {});
  const refreshConfig = useRef<() => void>(() => {});
  const streamLostRef = useRef<() => void>(() => {});
  const unlistenRef = useRef<() => void>(() => {});
  const pageClosedRef = useRef(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const [uploading, setUploading] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const worker = useRef<WorkerClient | null>(null);
  const view = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const [inspectionStep, setInspectionStep] = useState<InspectionStep | null>(null);
  const [inspectionData, setInspectionData] = useState<(StoredInspection & { part: number }) | null>(null);
  const [inspectionProblem, setInspectionProblem] = useState<string | null>(null);
  const inspectionLoad = useRef(0);
  const savedRevision = useRef(-1);
  // The bytes the session's last save wrote: the 3D view draws their PMI, for every part shown since.
  const savedSource = useRef<{ bytes: Uint8Array; generation: number; name: string } | null>(null);
  // The file shown once the session has saved: the copy it wrote (or the input, in place), not the input opened.
  const [savedAs, setSavedAs] = useState<{ name: string; path: string } | null>(null);
  const heardSaves = useRef(new Set<string>());
  const pageSaves = useRef(new Set<string>());
  const reopenPromise = useRef<Promise<void>>(Promise.resolve());
  const captureSaved = useRef<(saved: { path: string; rev: number }) => void>(() => {});
  const [slots, setSlots] = useState<SpecifySlots | null>(null);
  useEffect(() => setSlots({ view: view.current!, panel: panel.current!, exports: null }), []);
  // Answers from outside the page (an agent's, source "mcp") are shown as they land: the review
  // takes them, the first changed question opens, its faces are lit, and a note says what the
  // agent changed. The note stays until it is dismissed, the person changes something, or the
  // agent changes something else.
  const [agent, setAgent] = useState<{ ids: string[]; n: number } | null>(null);
  const [other, setOther] = useState<{ name: string; part: number; n: number; joined: boolean } | null>(null);
  const [focus, setFocus] = useState<{ questionId: string; faces?: readonly number[]; scroll?: boolean; n?: number } | null>(null);
  const [lit, setLit] = useState<{ faces: readonly number[]; n: number } | undefined>(undefined);
  const edited = () => setAgent(null);
  // Answers that landed from an in-flight review while Save ran: rebuilt against the saved file
  // once it reopens (including an answer back to the pre-save value, which cleared against the old file).
  const inFlightAnswers = useRef<Map<string, { part: number; id: string; value: unknown }> | null>(null);
  const buildAnswerEdit = (now: Opened, id: string, value: unknown): Pending | null => {
    const row = now.table.rows.find((r) => r.id === id);
    const q = now.bare.find((x) => x.id === id) ?? now.review.questions.find((x) => x.id === id);
    // "Added by you": named and worded as the review lists it ("Flatness 0.05" on its faces).
    const added = id.startsWith('extra.') && value && typeof value === 'object' ? value as PmiExtra : null;
    const name = added ? 'Added by you' : rowName(now, id);
    const edit = answerEdit(id, value, row, q, name);
    if (edit?.kind === 'fill' && added) edit.text = describeExtra(added);
    // Faces by what they are ("bottom face"), never by their numbers.
    const faces = (v: unknown) => Array.isArray(v) && v.length > 0 && v.every((f) => typeof f === 'number');
    if (edit && (edit.kind === 'fill' || edit.kind === 'change') && faces(value)) edit.text = describeFaces(value as number[], now.session.analysis as PmiAnalysis);
    if (edit?.kind === 'change' && faces(edit.expect)) edit.was = describeFaces(edit.expect as number[], now.session.analysis as PmiAnalysis);
    return edit;
  };
  // The review's answers as the page's held edits (adapter.ts): each changed question is its row's
  // edit, replacing any it had; an answer taken back drops it. Read through a ref, as the part's
  // engine is made once and outlives renders.
  const answeredRef = useRef<(part: number, id: string, value: unknown) => void>(() => {});
  answeredRef.current = (part, id, value) => {
    inFlightAnswers.current?.set(`${part}:${id}`, { part, id, value });
    const now = shownRef.current;
    if (!now || now.part !== part) return;
    const edit = buildAnswerEdit(now, id, value);
    setPending((all) => {
      const updated = new Map(all);
      const list = updated.get(part) ?? new PendingEdits();
      const next = edit ? list.with(edit) : list.without(id);
      if (next.size) updated.set(part, next); else updated.delete(part);
      return updated;
    });
    setSavedNote(null);
  };
  // The "Added by you" key each called-out general dimension is written under, by characteristic.
  const callOutKeys = useRef(new Map<string, string>());
  // The edit a bin replaced on an item, by part and item, for its Undo.
  const displaced = useRef(new Map<string, Pending>());
  // A reset of the review to the file (Discard, or a save that reopened the part).
  const [reviewGeneration, setReviewGeneration] = useState(0);

  useEffect(() => {
    let stop = () => {};
    let cancelled = false;
    (async () => {
      const first = await getConfig();
      if (cancelled) return;
      setConfig(first);
      document.title = first.name ? `${first.name} — Specify` : 'Specify';
      const heartbeat = keepAlive(setConfig, setStopped, () => {
        pageClosedRef.current = true;
        unlistenRef.current();
        sessionRef.current?.close();
      }, () => unsavedRef.current);
      stop = heartbeat.stop;
      refreshConfig.current = heartbeat.refresh;
      streamLostRef.current = heartbeat.streamLost;
    })().catch((error: unknown) => {
      if (cancelled) return;
      if (error instanceof SessionApiMismatch) setApiProblem(error.message);
      else setOpeningError(message(error));
    });
    return () => { cancelled = true; stop(); };
  }, []);

  const empty = Boolean(config && !config.name);
  useEffect(() => {
    if (!empty) return undefined;
    const stopDefault = (event: DragEvent) => { event.preventDefault(); };
    window.addEventListener('dragover', stopDefault);
    window.addEventListener('drop', stopDefault);
    return () => {
      window.removeEventListener('dragover', stopDefault);
      window.removeEventListener('drop', stopDefault);
    };
  }, [empty]);

  const pickFile = async (file: File | undefined) => {
    if (!file || uploading || stopped || config?.name) return;
    if (!isStepName(file.name)) {
      setProblem(`${file.name} is not a STEP file: Specify opens .step, .stp, .step.gz or .stp.gz.`);
      return;
    }
    setProblem(null);
    setOpeningError(null);
    setUploading(file.name);
    try {
      const next = await openFile(file);
      setConfig(next);
    } catch (error: unknown) {
      setProblem(`Could not open ${file.name}: ${message(error)}`);
    } finally {
      setUploading(null);
    }
  };

  useEffect(() => {
    if (!config || !config.name) return undefined;
    const first = { ...config, name: config.name };
    let unlisten = () => {};
    let unwatch = () => {};
    let ended = false;
    document.title = `${first.name} — Specify`;
    (async () => {
      worker.current = createIntentEngine({ wasmUrl: first.wasmUrl });
      const [bytes, answers, old, diff] = await Promise.all([
        getFile('new').then(stepBytes), getAnswers(),
        first.mode === 'diff' ? getFile('old').then(stepBytes) : null,
        first.mode === 'diff' ? getDiff() : null,
      ]);
      if (ended || pageClosedRef.current) return;
      const answersSession = createSession(answers.rev, answers.parts, answers.drafts);
      if (first.readOnly) setInspectionStep({ path: first.path ?? first.name, name: first.name, bytes });
      captureSaved.current = (event) => {
        if (event.rev < savedRevision.current || ended) return;
        savedRevision.current = event.rev;
        const load = ++inspectionLoad.current;
        setReopening(true);
        setInspectionStep(null);
        setInspectionData(null);
        setInspectionProblem(null);
        // The exports come from the file as saved, a copy or the input in place (`file?rev=saved`).
        reopenPromise.current = (async () => {
          const written = await getFile('saved').then(stepBytes);
          if (ended || load !== inspectionLoad.current) return;
          setInspectionStep({ path: event.path, name: inspectionName(event.path), bytes: written });
          const name = inspectionName(event.path);
          savedSource.current = { bytes: written, generation: load, name };
          // The session works on the file as saved from now on (ADR P11: the file is the only
          // copy): the part shown opens again from it, so what the save wrote reads as the file's
          // and the next edits apply to it. The 3D view draws the PMI as the save wrote it
          // (specify-ui#14); a scene that fails leaves the view as the opening drew it.
          const shown = shownRef.current;
          if (!shown || !worker.current) return;
          let next = await openPart(worker.current, answersSession, name, written, { part: shown.part, several: shown.several }, first.readOnly, edited, load, answeredRef.current);
          if (ended || load !== inspectionLoad.current || shownRef.current?.store !== shown.store) { next.store.close(); return; }
          shown.store.close();
          // What was saved is what the file now holds, as the engine wrote it (a "None" written as
          // "None (as machined)", say): the session takes the file's answers for this part, unless
          // a newer session revision has already landed while the saved file was reopening.
          if (answersSession.rev() <= event.rev) {
            const fileAnswers = forEngine(next.session.answers, next.stated);
            next.store.set(fileAnswers, next.shown.answers);
            next.shown.answers = fileAnswers;
            next.shown.all = { ...next.stated.values, ...fileAnswers };
            next = { ...next, external: { answers: fileAnswers, review: next.review } };
          }
          shownRef.current = next;
          setOpened(next);
          setSavedAs({ name, path: event.path });
        })().catch((error: unknown) => {
          if (!ended && load === inspectionLoad.current) setInspectionProblem(message(error));
        }).finally(() => {
          if (!ended && load === inspectionLoad.current) setReopening(false);
        });
      };
      sessionRef.current = answersSession;
      if (ended || pageClosedRef.current) answersSession.close();
      const showDrafts = () => {
        setDirty(answersSession.dirty());
        setDraftSnapshot(answersSession.draftSnapshot());
      };
      unwatch = answersSession.subscribe(showDrafts);
      showDrafts();
      // An assembly's parts: the page shows one at a time, first the first the session has answers for.
      // The session has saved already (the page was reloaded, or opened again): it works on what it wrote.
      // A copy changed or removed since cannot be shown: said, and the file as opened is.
      const held = first.savedPath ? await getFile('saved').then(stepBytes).catch((error: unknown) => {
        setProblem(`The file saved earlier could not be shown, so this is the file as opened: ${message(error)}`);
        return null;
      }) : null;
      const now = held && first.savedPath ? { name: inspectionName(first.savedPath), path: first.savedPath, bytes: held } : { name: first.name, path: first.path ?? first.name, bytes };
      if (held) {
        const load = ++inspectionLoad.current;
        savedSource.current = { bytes: now.bytes, generation: load, name: now.name };
        setInspectionStep(now);
        setSavedAs({ name: now.name, path: now.path });
      }
      const listed = (await worker.current.request<PartRow[]>('parts', { name: now.name, bytes: now.bytes })).map(({ part, name, instances, written }) => ({ part, name, instances, written: written === true }));
      const several = listed.length > 1 || listed[0]?.instances > 1;
      let rows = listed;
      const withAnswers = rows.find((row) => Object.keys(answers.parts[String(row.part)] ?? {}).some((id) => !id.startsWith('_')));
      // The part the agent's `show` named (config.part), else the first with answers.
      const asked = rows.find((row) => row.part === first.part);
      const at = { part: several ? (asked ?? withAnswers ?? rows[0]!).part : 0, several };
      setParts(rows);
      const part = await openPart(worker.current, answersSession, now.name, now.bytes, at, first.readOnly, edited, savedSource.current?.generation ?? 0, answeredRef.current);
      if (ended || pageClosedRef.current) { part.store.close(); return; }
      if (old && diff) {
        const r = diff.report as { verdict?: string; summary?: string };
        setCompared({
          old: { name: first.oldName ?? 'old.step', bytes: old }, next: { name: first.name, bytes }, body: diff,
          decisions: decisions(diff.report), verdict: r.verdict ?? '', summary: r.summary ?? '',
        });
      }
      shownRef.current = part;
      let lost = false;
      if (!ended && !pageClosedRef.current) {
        unlisten = listen((event) => {
          // One rev can carry an event per part; a repeat of one already received is ignored.
          if (!answersSession.receive(event)) return;
          // A part joining the session is added to the page's model: the picker lists it, and the
          // session holds it, with the answers it has.
          if ('part' in event && several && !rows.some((r) => r.part === event.part)) {
            rows = [...rows, { part: event.part, name: `Part ${event.part}`, instances: 1, written: false }];
            setParts(rows);
          }
          // An agent's change to another part than the one shown: said, by name.
          if (event.source === 'mcp' && 'part' in event && event.part !== shownRef.current?.part) {
            const row = rows.find((r) => r.part === event.part);
            const joined = event.joined === true && !Object.keys(event.answers).some((id) => !id.startsWith('_'));
            setOther({ name: row?.name ?? `part ${event.part}`, part: event.part, n: Date.now(), joined });
          }
        }, (event) => {
          answersSession.markSaved(event.rev);
          const key = `${event.rev}:${event.path}`;
          heardSaves.current.add(key);
          if (!(event.source === 'page' && pageSaves.current.delete(key))) captureSaved.current(event);
          if (event.source === 'mcp') void agentSaved(event.path);
          refreshConfig.current();
        }, (up) => {
          setLive(up);
          if (!up) { lost = true; streamLostRef.current(); return; }
          // Back after a loss: what changed meanwhile, as the session has it now.
          if (lost) getAnswers().then((now) => answersSession.receive({ ...now, source: 'start' }), () => {});
          lost = false;
        });
        unlistenRef.current = unlisten;
      }
      setOpened(part);
      // The other parts the session holds answers for: what the file stores for each, so answers an
      // agent gave them before the page opened count as unsaved. A part Specify did not write
      // stores none; one it wrote is read from the file, after the part shown is up.
      for (const row of rows) {
        if (ended || pageClosedRef.current) break;
        if (row.part === at.part || !(String(row.part) in answers.parts)) continue;
        if (!row.written) { answersSession.fileAnswers(row.part, {}); continue; }
        const snapshot = await worker.current.request<{ analysis: PmiLabSession['analysis'] }>('open', fileOf(now.name, now.bytes, row.part, true)).catch(() => null);
        if (snapshot) answersSession.fileAnswers(row.part, snapshot.analysis.resume?.answers ?? {});
      }
    })().catch((error: unknown) => {
      if (ended) return;
      if (error instanceof SessionApiMismatch) setApiProblem(error.message);
      else setOpeningError(message(error));
    });
    return () => {
      ended = true;
      unlisten();
      unlistenRef.current = () => {};
      unwatch();
      sessionRef.current?.close();
      worker.current?.dispose();
      worker.current = null;
    };
  }, [config?.name]);

  useEffect(() => {
    if (!opened) return;
    shownRef.current = opened;
    opened.store.onProblem((error) => setProblem(`Your change did not reach the session: ${error.message}`));
  }, [opened?.store]);

  const busy = switching || saving || checking || reopening || discarding;

  /** Shows another part of the file, with the session's answers for it now. */
  const showPart = async (index: number) => {
    const answersSession = sessionRef.current;
    if (busy || !opened || !config || !worker.current || !answersSession || index === opened.part) return;
    setSwitching(true); setProblem(null);
    try {
      // The part left keeps its unsaved answers: in the session, and counted as unsaved.
      await opened.store.settled();
      // If another page or the agent saved while this part was opening, open the saved version.
      for (;;) {
        const source = savedSource.current;
        const next = await openPart(worker.current, answersSession, source?.name ?? config.name ?? opened.session.filename, source?.bytes ?? opened.bytes,
          { part: index, several: true }, config.readOnly, edited, source?.generation ?? opened.generation, answeredRef.current);
        if (source !== savedSource.current) { next.store.close(); continue; }
        shownRef.current?.store.close();
        shownRef.current = next;
        setOpened(next);
        setOther(null); setAgent(null);
        break;
      }
    } catch (error) {
      setProblem(`That part could not be opened: ${message(error)}`);
    } finally { setSwitching(false); }
  };

  const current = useRef(opened); current.current = opened;
  useEffect(() => {
    if (!opened) return undefined;
    const { store, shown } = opened;
    // The questions the agent changed that are not shown yet.
    const fromAgent = new Set<string>();
    let latest = 0;
    const show = (answers: PmiAnswers, source: string, previous: PmiAnswers) => {
      if (source === 'mcp') for (const id of changedIds(previous, answers)) fromAgent.add(id);
      // The page's own change, as SpecifyReview already shows it (or answers back to what it shows).
      if (sameAnswers(answers, shown.answers)) { setUnshown(null); return; }
      // Answers from before a Discard are not shown when their review lands after it.
      const started = shown.generation;
      const token = ++latest;
      setSyncing(true);
      void opened.reviewOf(answers).then((review) => {
        // Another part shown since, or a newer change on its way (shown when it lands).
        if (current.current?.store !== store || !sameAnswers(store.get(), answers) || shown.generation !== started) return;
        shown.answers = answers;
        // What the review now shows: these answers, with what the file states and the page's
        // changes to it (which the session's answers leave out) as they were.
        const keptStated = Object.fromEntries(Object.entries(shown.all).filter(([id]) => opened.stated.ids.has(id)));
        shown.all = { ...keptStated, ...answers };
        setUnshown(null);
        setOpened((now) => (now?.store === store ? { ...now, external: { answers, review } } : now));
        const ids = [...fromAgent];
        fromAgent.clear();
        if (!ids.length) return;
        const faces = review.questions.filter((q) => ids.includes(q.id)).flatMap((q) => q.faces);
        const n = Date.now();
        setAgent({ ids, n });
        setTab('answers');
        setFocus({ questionId: ids[0]!, faces, scroll: true, n });
        setLit({ faces, n });
      }, (error: unknown) => {
        if (current.current?.store !== store || !sameAnswers(store.get(), answers)) return;
        // Not shown, so not saved: said, with a way to try again (a later change also tries again).
        setUnshown(message(error));
      }).finally(() => {
        if (token === latest) setSyncing(false);
      });
    };
    // The session's answers, shown again if the page does not show them yet.
    const again = () => { if (current.current?.store === store && !sameAnswers(store.get(), shown.answers)) show(store.get(), 'start', shown.answers); };
    reshow.current = again;
    const unsubscribe = store.subscribe(show);
    // A change that landed while the part was opening, before this listened.
    again();
    return () => { latest += 1; setSyncing(false); unsubscribe(); setUnshown(null); };
  }, [opened?.store]);
  // The questions the agent changed are marked in the panel while its note shows: marked now and
  // again whenever the panel re-renders (the review opens the question after this runs).
  useEffect(() => {
    const root = panel.current;
    if (!agent || !root) return undefined;
    const selector = agent.ids.map((id) => `[data-question="${CSS.escape(id)}"]`).join(', ');
    const mark = () => root.querySelectorAll(selector).forEach((el) => { if (!el.classList.contains('app-agent-lit')) el.classList.add('app-agent-lit'); });
    mark();
    const observer = new MutationObserver(mark);
    observer.observe(root, { childList: true, subtree: true });
    return () => { observer.disconnect(); root.querySelectorAll('.app-agent-lit').forEach((el) => el.classList.remove('app-agent-lit')); };
  }, [agent]);

  const mode = config?.mode;
  useEffect(() => { if (savedAs) document.title = `${savedAs.name} — Specify`; }, [savedAs]);
  const readOnly = !config || config.readOnly;

  /** The short labels of questions ``ids`` (the part shown's; an id Specify does not ask here is kept as it is). */
  const prompts = (ids: string[], ...reviews: { questions: PmiQuestion[] }[]) => {
    const prompt = new Map(reviews.flatMap((r) => r.questions.map((q) => [q.id, q.label ?? q.prompt] as const)));
    // General dimensions are covered by the general tolerance, not questions to answer (change list item 1).
    return ids.filter((id) => !isDimensionQuestion(id)).map((id) => prompt.get(id) || id);
  };
  // An agent's save says no more than where it wrote: what is still open is what the session's
  // answers for the part shown leave unanswered.
  const agentSaved = async (path: string) => {
    const now = shownRef.current;
    const line = { path, open: null, byAgent: true };
    setSaved(line);
    const review = await now?.reviewOf(now.store.get()).catch(() => null);
    if (review) setSaved((saved) => saved === line ? { ...line, open: prompts(review.open, review) } : saved);
  };

  // Save sends every part's list in one request, applied to the file all or nothing, whatever is
  // still open, and says what is.
  const onSave = async () => {
    if (busy || stopped || !opened) return;
    // Nothing listed: nothing is written (the mock-up's "Nothing to save.").
    const lists = [...pending].map(([part, list]) => ({ part, edits: list.list() }))
      .filter(({ edits }) => Object.keys(edits).length > 0);
    if (!lists.length) { setProblem(null); setSavedNote('Nothing to save.'); return; }
    const sentParts = new Set(opened.several ? lists.map((entry) => entry.part) : [opened.part]);
    const sent = new Map<number, PendingEdits>(
      [...pending].filter(([part]) => sentParts.has(part)),
    );
    const sentCallOuts = new Map(callOutKeys.current);
    const sentDisplaced = new Map(displaced.current);
    const answeredDuringSave = new Map<string, { part: number; id: string; value: unknown }>();
    inFlightAnswers.current = answeredDuringSave;
    setSaving(true); setProblem(null); setRejectedNone(new Map());
    const count = items.filter(({ edit }) => edit.kind !== 'refused').length;
    try {
      const result = await opened.saver.save(opened.several ? lists : partPending.list(), draftSnapshot.rev);
      // The saved events are ordered by the command. A delayed HTTP reply must not replace a
      // later save at the same answer revision (for example an agent saving again meanwhile).
      const saveKey = `${result.rev}:${result.path}`;
      if (!heardSaves.current.has(saveKey)) {
        pageSaves.current.add(saveKey);
        captureSaved.current(result);
      }
      for (let active = reopenPromise.current; ; active = reopenPromise.current) {
        await active;
        if (active === reopenPromise.current) break;
      }
      if (sessionRef.current?.signal.aborted) return;
      // An answer that waits on another (a hole left to the general tolerance before the part states one)
      // was not written: it stays pending, and the line says what it waits for.
      const waiting = waitingOf(result.report, opened.part);
      for (const w of waiting) {
        const left = sent.get(w.part)?.without(w.id);
        if (left) sent.set(w.part, left);
      }
      setSaved({ path: result.path, open: prompts(result.missing, opened.external.review, opened.review), byAgent: false, waiting: waiting.map((w) => w.part === opened.part ? `${rowName(opened, w.id)}: ${w.message.startsWith(`${w.id}: `) ? w.message.slice(w.id.length + 2) : w.message}` : w.message) });
      // The sent edits are in the file now: remove them, and rebuild any made while the save ran
      // against the saved file (including an in-flight answer back to what the file held before).
      const reopened = shownRef.current && shownRef.current !== opened ? shownRef.current : null;
      setPending((all) => {
        const remaining = withoutSent(all, sent);
        for (const { part, id, value } of answeredDuringSave.values()) {
          const savedEdit = sent.get(part)?.get(id);
          if (!savedEdit || savedEdit.kind === 'refused' || !reopened || reopened.part !== part) continue;
          const matchesSaved = (savedEdit.kind === 'fill' || savedEdit.kind === 'change')
            && JSON.stringify(value) === JSON.stringify(savedEdit.value);
          let rebuilt: Pending | null = null;
          if (!matchesSaved) {
            if (value === undefined) {
              const row = reopened.table.rows.find((r) => r.id === id);
              const q = reopened.bare.find((x) => x.id === id) ?? reopened.review.questions.find((x) => x.id === id);
              const name = rowName(reopened, id);
              if (row?.state === 'stated') rebuilt = removeEdit(row, q, name, reopened.session.analysis as PmiAnalysis);
              else if (row?.state === 'decided_none') rebuilt = { kind: 'clear', id, name };
            } else {
              rebuilt = buildAnswerEdit(reopened, id, value);
            }
          }
          const list = remaining.get(part) ?? new PendingEdits();
          const next = rebuilt ? list.with(rebuilt) : list.without(id);
          if (next.size) remaining.set(part, next); else remaining.delete(part);
        }
        for (const [at, key] of sentCallOuts) {
          const part = Number(at.split(':')[0]);
          if (callOutKeys.current.get(at) === key && !remaining.get(part)?.get(key)) callOutKeys.current.delete(at);
        }
        for (const [key, edit] of sentDisplaced) {
          if (displaced.current.get(key) === edit) displaced.current.delete(key);
        }
        return remaining;
      });
      setSavedNote(`Saved ${inspectionName(result.path)} · ${plural(count, 'change')} written, all together.`);
      refreshConfig.current();
    } catch (error) {
      if (sessionRef.current?.signal.aborted) return;
      // The session moved on: the page takes it as it is now (the panel's edits stay).
      if (error instanceof Stale) {
        const now = await getAnswers().catch(() => null);
        if (now) sessionRef.current?.receive({ ...now, source: 'start' });
      }
      if (error instanceof FileChanged) {
        setFileChanged(true);
        setProblem('Not saved: the file changed on disk since it was opened (another program wrote it), so Save will not write over it. '
          + 'Your changes are still listed below; open the file again to make them.');
      } else if (error instanceof EditsRefused) {
        setRejectedNone(rejectedNoneByPart(pending, error.failures, opened.several));
        // All or nothing: the command names every edit it cannot apply, and wrote nothing.
        setProblem(`Not saved: nothing was written, because ${plural(error.failures.length, 'change')} cannot be applied: `
          + error.failures.map((f) => `${f.entry}: ${f.reason}`).join('; '));
      } else if (error instanceof NotShown && unshown) {
        setProblem('Not saved: the latest answers in the session are not shown yet (see below). Trying to show them again.');
        reshow.current();
      } else if (error instanceof Stale || error instanceof NotShown) {
        reshow.current();
        setProblem('Not saved: the session changed while you saved (your agent answered). What is now shown is the latest; check it and Save again.');
      } else setProblem(`Not saved: ${message(error)}`);
    } finally {
      if (inFlightAnswers.current === answeredDuringSave) inFlightAnswers.current = null;
      setSaving(false);
    }
  };


  const onDiscard = async (target?: { part: number; id: string }) => {
    const session = sessionRef.current;
    if (busy || stopped || !session) return;
    setProblem(null);
    setDiscarding(true);
    try {
      // The page's own answers still on their way to the session land first: Discard then clears
      // them with the rest, and none comes back afterwards as an edit to save.
      if (!target) await shownRef.current?.store.settled();
      if (target || Object.keys(draftSnapshot.drafts).length) {
        const snapshot = await discardAnswers(draftSnapshot.rev, target, session.signal);
        session.receive({ ...snapshot, source: 'start' });
      }
      if (!target) {
        setPending(new Map());
        callOutKeys.current.clear();
        displaced.current.clear();
        // The review starts again from the file: its answers, and what the page last showed.
        const now = shownRef.current;
        if (now) {
          const file = forEngine(now.session.answers, now.stated);
          now.shown.generation += 1;
          now.store.set(file, now.shown.answers);
          now.shown.answers = file;
          now.shown.all = { ...now.stated.values, ...file };
          now.external = { answers: file, review: now.review };
        }
        setReviewGeneration((n) => n + 1);
      }
      setRejectedNone(new Map());
      setAgent(null);
    } catch (error) {
      if (!session.signal.aborted) {
        const latest = await getAnswers().catch(() => null);
        if (latest) session.receive({ ...latest, source: 'start' });
        setProblem(`Could not discard: ${message(error)}`);
      }
    } finally { setDiscarding(false); }
  };

  // A part's exports are derived only from the retained saved/opened STEP. Live answers and
  // subsequent edits cannot refresh this snapshot. A different part reads the same saved bytes.
  useEffect(() => {
    if (!inspectionStep || !opened || !worker.current) return;
    let stale = false;
    setInspectionData(null);
    setInspectionProblem(null);
    const file = fileOf(inspectionStep.name, inspectionStep.bytes, opened.part, opened.several);
    void worker.current.request<StoredInspection>('file-inspection', file).then((data) => {
      if (!stale) setInspectionData({ ...data, part: opened.part });
    }, (error: unknown) => { if (!stale) setInspectionProblem(message(error)); });
    return () => { stale = true; };
  }, [inspectionStep, opened?.part]);
  const inspectionReady = inspectionStep !== null && inspectionData !== null && inspectionData.part === opened?.part;
  const downloadCsv = () => {
    if (!inspectionReady || !inspectionStep || !inspectionData) return;
    saveFile(new File([toCsv(inspectionData.chars)], `${stem(inspectionStep.name)}-characteristics.csv`, { type: 'text/csv' }));
  };
  const downloadFai = () => {
    if (!inspectionReady || !inspectionStep || !inspectionData) return;
    const book = xlsx(as9102({ name: stem(inspectionStep.name) }, inspectionData.chars, new Set(), inspectionData.notes));
    saveFile(new File([book as BlobPart], `${stem(inspectionStep.name)}-AS9102.xlsx`, { type: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet' }));
  };
  const inspectionTitle = inspectionProblem ?? (!inspectionStep && savedRevision.current < 0 ? SAVE_FIRST : !inspectionReady ? 'Reading the saved STEP…' : undefined);
  /** A decision picked in the diff: its question, in the answers. */
  const decide = (d: Decision) => {
    setTab('answers');
    if (d.question) setFocus({ questionId: d.question, scroll: true, n: Date.now() });
  };

  const showingChanges = mode === 'diff' && tab === 'changes';
  const saveBlocked = apiProblem ?? (openingError ? 'Save is disabled until the file and its answers can be loaded. Correct the session answers, then reload this page.'
    : stopped ? 'Specify has stopped' : fileChanged ? 'The file changed on disk since it was opened: open it again to save' : undefined);
  // The panel edits in specify and diff mode; read-only shows the review's PMI viewer instead.
  const editing = Boolean(config && !config.readOnly);
  const pending = useMemo(() => {
    const model = opened?.model;
    const rows = model ? [...(model.part?.rows ?? []), ...model.features.flatMap((group) => group.rows), ...(model.datums?.rows ?? [])] : [];
    return withDrafts(localPending, draftSnapshot.drafts, (part, id, value) => {
      const row = part === opened?.part ? rows.find((row) => row.id === id) : undefined;
      const words = id.startsWith('part.') ? id.slice(5).replace(/_/g, ' ') : id;
      return { name: row?.name ?? words.charAt(0).toUpperCase() + words.slice(1), text: valueText(value, row?.question, opened?.session.analysis as PmiAnalysis | undefined) };
    });
  }, [localPending, draftSnapshot.drafts, opened?.model, opened?.part]);
  const partPending = pending.get(opened?.part ?? 0) ?? NO_EDITS;
  // Keep the 3D annotation-plane scene in sync with pending edits so before-Save and after-Save
  // draw identical callout geometry and placements.
  useEffect(() => {
    if (!editing || !opened || !worker.current) return;
    const state = sceneStateFor(opened.shown.answers, opened.external.review.questions, opened.stated.ids, false, partPending);
    const key = JSON.stringify(state);
    if (liveScene?.sessionId === opened.session.sessionId && liveScene.key === key) return;
    if (liveScene?.sessionId !== opened.session.sessionId && opened.sceneKey === key) return;
    const token = ++sceneReq.current;
    const file = fileOf(opened.session.filename, opened.bytes, opened.part, opened.several, opened.generation);
    const analysis = opened.session.analysis as PmiAnalysis;
    const sid = opened.session.sessionId;
    void worker.current.request('scene', { ...file, state, exact: true }).then((raw) => {
      if (token !== sceneReq.current) return;
      const read = reviewDrawnFromScene(raw, analysis);
      const semantic = readSceneSemanticLabels(raw);
      const drawn = read.ok ? normalizeDraftingPmi(read.value, raw, analysis, opened.session.mesh, semantic) : null;
      setLiveScene({ sessionId: sid, key, drawn, semantic });
    }, () => {});
  }, [editing, opened, partPending, opened?.external.answers, liveScene?.sessionId, liveScene?.key]);
  const activeDrawn = opened && liveScene?.sessionId === opened.session.sessionId ? liveScene.drawn : opened?.drawn ?? null;
  const activeSemantic = opened
    ? (liveScene?.sessionId === opened.session.sessionId ? liveScene.semantic : opened.semantic)
    : EMPTY_SEMANTIC;
  const partOrigin = useMemo(
    () => (opened ? findStatedOrigin(opened.bytes, opened.session.analysis as PmiAnalysis, activeDrawn) : null),
    [opened, activeDrawn],
  );
  const oneView = useMemo(() => {
    if (!opened) return undefined;
    const bareById = new Map(opened.bare.map((q) => [q.id, q]));
    const rows: OneViewRow[] = [
      ...opened.table.rows.map((row): OneViewRow => {
        const q = bareById.get(row.id);
        return {
          id: row.id, state: row.state, open: row.open, faces: row.faces,
          label: rowName(opened, row.id), ...(q ? { question: q } : {}),
        };
      }),
      ...opened.model.other.map((o): OneViewRow => ({
        id: o.key, state: 'stated', open: false, faces: o.faces, label: o.label, other: true,
      })),
    ];
    const pendingMap = new Map<string, OneViewPending>();
    for (const edit of partPending.values()) {
      pendingMap.set(edit.id, {
        kind: edit.kind, id: edit.id,
        ...('value' in edit ? { value: edit.value } : {}),
        ...('text' in edit ? { text: edit.text } : {}),
        ...('was' in edit ? { was: edit.was } : {}),
        ...('targets' in edit ? { targets: edit.targets } : {}),
      });
    }
    return { rows, pending: pendingMap };
  }, [opened, partPending]);
  // What the review opens on: what the file states, the session's answers (an agent's too) and the
  // page's held changes to stated items, which the engine's answers leave out (adapter.ts).
  const shownSession = useMemo(() => {
    if (!opened) return null;
    const changed = Object.fromEntries(partPending.values().filter((e) => e.kind === 'change' && !e.draft).map((e) => [e.id, (e as { value: unknown }).value]));
    return { ...opened.session, answers: { ...opened.stated.values, ...opened.external.answers, ...changed } };
    // Only when the part, its answers from outside or the review's start change: not on each edit.
  }, [opened?.session, opened?.external, reviewGeneration]);
  const items = [...pending].flatMap(([part, list]) => list.values().map((edit) => {
    const name = opened?.several || edit.draft ? parts.find((row) => row.part === part)?.name || `Part ${part}` : undefined;
    return { part, ...(name ? { name } : {}), edit };
  }));
  const unsaved = dirty || items.length > 0;
  // What Save would have sent, a part's list to a file, for `specify-pmi write`: the command has stopped.
  const heldLists = [...pending].map(([part, list]) => ({ part, edits: list.list() })).filter(({ edits }) => Object.keys(edits).length > 0);
  const downloadEdits = ({ part, edits }: { part: number; edits: EditList }) => {
    const url = URL.createObjectURL(new Blob([`${JSON.stringify(edits, null, 1)}\n`], { type: 'application/json' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = `${inspectionName(savedAs?.name ?? config?.name ?? 'part.step').replace(/\.(step|stp)(\.gz)?$/i, '')}${parts.length > 1 ? `.part-${part}` : ''}.edits.json`;
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  useEffect(() => {
    unsavedRef.current = unsaved;
    if (!unsaved) return;
    // Said at once, not at the next heartbeat: the agent may exit before it.
    refreshConfig.current();
    const guard = (event: BeforeUnloadEvent) => { event.preventDefault(); };
    window.addEventListener('beforeunload', guard);
    return () => window.removeEventListener('beforeunload', guard);
  }, [unsaved]);
  const editPending = (next: PendingEdits) => {
    if (!opened) return;
    setPending((all) => {
      const updated = new Map(all);
      const local = new PendingEdits(next.values().filter((edit) => !edit.draft));
      if (local.size) updated.set(opened.part, local);
      else updated.delete(opened.part);
      return updated;
    });
    if (rejectedNone.has(opened.part)) {
      setRejectedNone((all) => {
        const updated = new Map(all);
        updated.delete(opened.part);
        return updated;
      });
    }
    setSavedNote(null); setAgent(null);
  };
  const check = async (edits: EditList): Promise<EditFailure[]> => {
    const whole = (reason: string) => [{ entry: '(the whole list)', reason }];
    if (!opened || !worker.current || !config) return whole('The part is not open.');
    setChecking(true);
    try {
      const result = await worker.current.request<{ applicable: boolean; failures?: EditFailure[]; message?: string }>(
        'check-edits', { ...fileOf(opened.session.filename, opened.bytes, opened.part, opened.several, opened.generation), edits });
      return result.applicable ? [] : result.failures?.length ? result.failures : whole(result.message || 'It cannot be applied.');
    } catch (error) { return whole(message(error)); }
    finally { setChecking(false); }
  };
  return (
    <div className="app">
      <header className="app-header">
        <div className="app-title">
          <strong className="app-brand">Specify</strong>
          {config && mode === 'diff' && <span className="app-file app-old" title={config.oldPath ?? ''}>{config.oldName}</span>}
          {config && mode === 'diff' && <span aria-hidden="true">→</span>}
          {config?.name && <span className="app-file">{savedAs?.name ?? config.name}</span>}
          {config?.name && (mode === 'diff' || savedAs?.path || config.path) && (
            <code className="app-path">{mode === 'diff' ? `${config.oldPath} → ${savedAs?.path ?? config.path}` : savedAs?.path ?? config.path}</code>
          )}
          {parts.length > 1 && opened && (
            <label className="app-part">Part{' '}
              <select value={opened.part} disabled={busy} onChange={(event) => void showPart(Number(event.target.value))}>
                {parts.map((row) => <option key={row.part} value={row.part}>{row.name}</option>)}
              </select>
            </label>
          )}
          {readOnly && config && <span className="app-badge">View only</span>}
          {unsaved && <span className="app-dirty">Unsaved changes</span>}
        </div>
        {!empty && (
          <div className="app-actions">
            <div className="app-exports">
              <button type="button" onClick={downloadCsv} disabled={!inspectionReady} title={inspectionTitle}>Inspection list (CSV)</button>
              <button type="button" onClick={downloadFai} disabled={!inspectionReady} title={inspectionTitle}>AS9102 FAI (Excel)</button>
              {inspectionStep && <span className="app-inspection-source">
                from <code>{inspectionStep.name}</code>
              </span>}
              {inspectionProblem && <span className="app-problem" role="alert">Inspection exports unavailable. {inspectionProblem}</span>}
            </div>
          </div>
        )}
        {config?.name && (
          <div className="app-paths">
            {saved && (
              <div className="app-saved" role="status">
                {saved.byAgent ? 'Your agent saved' : 'Saved'} <code>{saved.path}</code>{saved.open !== null && `: ${saved.open.length
                  ? `${plural(saved.open.length, 'question')} ${saved.byAgent ? 'still unanswered' : 'still open'}: ${saved.open.slice(0, 6).join('; ')}${saved.open.length > 6 ? '; …' : ''}`
                  : 'complete'}`}
                {!!saved.waiting?.length && <> Not written yet, they wait on another answer: {saved.waiting.join('; ')}</>}
              </div>
            )}
            {!stopped && config.saveTarget && <span>Save writes <code className="app-target">{config.saveTarget}</code></span>}
          </div>
        )}
      </header>
      {apiProblem && <p className="app-problem app-api-mismatch" role="alert">{apiProblem}</p>}
      {openingError && <p className="app-problem app-open-failed" role="alert">
        Could not open {config?.name ?? 'the part'}: {openingError}{' '}
        Save is disabled until the file and its answers can be loaded. Correct the session answers, then reload this page.
      </p>}
      {problem && <p className="app-problem" role="alert">{problem}</p>}
      {unshown && (
        <p className="app-problem app-unshown" role="alert">
          The latest answers in the session (from your agent or another window) could not be shown, so Save
          waits until they are: {unshown}{' '}
          <button type="button" onClick={() => reshow.current()}>Show the latest answers</button>
        </p>
      )}
      {stopped && config ? (
        <p className="app-offline app-stopped" role="status">
          Specify has stopped. Run <code>specify-pmi open …</code> again (or ask your agent to show it).
          {(saved?.path ?? savedAs?.path ?? config.path) && (
            <>{' '}Your saved file is at <code>{saved?.path ?? savedAs?.path ?? config.path}</code>.</>
          )}
          {heldLists.length > 0 && <span className="app-held">
            {' '}Your unsaved changes were not written.
            {heldLists.map((held) => (
              <button key={held.part} type="button" onClick={() => downloadEdits(held)}>
                Download {heldLists.length > 1 ? `the changes to ${parts.find((row) => row.part === held.part)?.name || `part ${held.part}`}` : 'them'}
              </button>
            ))}
            {' '}Apply with <code>specify-pmi write FILE EDITS.json -o OUT{parts.length > 1 ? ' --part N' : ''}</code>.
          </span>}
        </p>
      ) : live === false && (
        <p className="app-offline" role="status">
          Not receiving live updates from the session: reconnecting. Changes your agent makes meanwhile show when it is back.
        </p>
      )}
      {agent && (
        <p className="app-agent" role="status">
          Updated by your agent: {prompts(agent.ids, ...(opened ? [opened.external.review, opened.review] : [])).join('; ')} · pending review{' '}
          <button type="button" className="app-dismiss" onClick={() => setAgent(null)}>Dismiss</button>
        </p>
      )}
      {other && (
        <p className="app-agent" role="status">
          {other.joined ? 'Your agent opened' : 'Updated by your agent:'} {other.name}{other.joined ? '' : ' · pending review'}{' '}
          <button type="button" disabled={busy} onClick={() => void showPart(other.part)}>Show it</button>
        </p>
      )}
      {mode === 'diff' && (
        <nav className="app-tabs" role="tablist">
          <button type="button" role="tab" aria-selected={tab === 'changes'} onClick={() => setTab('changes')}>What changed</button>
          <button type="button" role="tab" aria-selected={tab === 'answers'} onClick={() => setTab('answers')}>
            Requirements for {savedAs?.name ?? config?.name}{compared?.decisions.length ? ` (${plural(compared.decisions.length, 'decision')})` : ''}
          </button>
          {compared && compared.decisions.length > 0 && (
            <div className="app-decide" aria-label="To decide">
              <span>To decide:</span>
              {compared.decisions.map((d, i) => (
                <button key={i} type="button" className="app-decision" title={d.why} onClick={() => decide(d)}>{d.what}</button>
              ))}
            </div>
          )}
        </nav>
      )}
      <main className="app-main">
        {empty && !apiProblem && !stopped && (
          <div
            className={`app-drop-area${dragOver ? ' drag-over' : ''}`}
            onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; setDragOver(true); }}
            onDragEnter={(event) => { event.preventDefault(); setDragOver(true); }}
            onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDragOver(false); }}
            onDrop={(event) => { event.preventDefault(); setDragOver(false); void pickFile(event.dataTransfer.files[0]); }}
          >
            <div className="app-drop-card">
              <p className="app-drop-prompt">
                {uploading ? `Opening ${uploading}…` : (
                  <>
                    Drop a STEP file here, or{' '}
                    <button type="button" className="app-choose-file" disabled={Boolean(uploading) || stopped} onClick={() => fileInput.current?.click()}>
                      Choose file…
                    </button>
                  </>
                )}
              </p>
              <p className="app-drop-hint">Accepts <code>.step</code>, <code>.stp</code>, <code>.step.gz</code> and <code>.stp.gz</code> · runs locally on this computer</p>
              <input
                ref={fileInput}
                className="app-file-input"
                type="file"
                accept=".step,.stp,.step.gz,.stp.gz"
                aria-label="Choose a STEP file"
                disabled={Boolean(uploading) || stopped}
                onChange={(event) => {
                  const file = event.target.files?.[0];
                  event.target.value = '';
                  void pickFile(file);
                }}
              />
            </div>
          </div>
        )}
        {!empty && !opened && !problem && !openingError && !apiProblem && !stopped && <p className="app-loading">Opening {config?.name ?? 'the part'}…</p>}
        {compared && (
          <div className="app-diff diff-page" hidden={!showingChanges}>
            <StepDiff before={compared.old} after={compared.next} compared={compared.body} wasmUrl={config?.wasmUrl} />
          </div>
        )}
        <div className={`app-slot app-view${showingChanges ? ' app-under' : ''}`} ref={view} />
        <div className={`app-slot app-panel${showingChanges ? ' app-under' : ''}`} ref={panel} />
        {opened && slots && shownSession && (
          // The editor's review screens: the 3D view, the questions and the characteristics. On the
          // 0.3 engine through adapter.ts: what the file states has a pencil and a bin; changes are
          // held in the bar below until Save writes them together.
          <SpecifyReview
            key={`${opened.session.sessionId}:${reviewGeneration}`}
            engine={opened.engine} session={shownSession} readOnly={!editing} drawn={activeDrawn} oneView={oneView}
            sceneSemantic={activeSemantic} partOrigin={partOrigin} slots={slots}
            inspectionExports={false}
            externalAnswers={shownSession.answers} externalReview={opened.external.review} disabled={busy || syncing || stopped}
            focusRequest={focus} showFaces={lit}
            onExport={(_action, format) => { if (format === 'xlsx') downloadFai(); else downloadCsv(); }}
            lengthUnit={opened.units}
            {...(editing ? {
              // Called out, by characteristic: from the held edits, so it outlasts a part switch.
              calledOutDimensions: new Map([...callOutKeys.current].flatMap(([at, key]) => {
                const [part, ...rest] = at.split(':');
                const edit = partPending.get(key);
                const tolerance = edit?.kind === 'fill' ? edit.text.replace(/ mm$/, '') : edit?.kind === 'change' ? String(edit.value) : null;
                return Number(part) === opened.part && tolerance !== null ? [[rest.join(':'), tolerance.replace(/^±/, '')] as const] : [];
              })),
              // The number Save writes for each, called out: the engine's for its call-out's drawn callout.
              calledOutNumbers: new Map([...callOutKeys.current].flatMap(([at, key]) => {
                const [part, ...rest] = at.split(':');
                const number = Object.values(activeDrawn?.links ?? {}).find((l) => l.question === key)?.number;
                return Number(part) === opened.part && number ? [[rest.join(':'), number] as const] : [];
              })),
              // "Added by you" keys already taken (the page's call-outs among them): no two share one.
              takenKeys: [...partPending.values().map((e) => e.id), ...callOutKeys.current.values()],
              otherPmi: {
                // Named as the file names it ("Flatness"), else by its kind.
                items: opened.model.other.map((o) => {
                  const item = opened.table.file_pmi.find((r) => (o.number && r.number === o.number) || (o.id && r.id === o.id));
                  const label = item?.type ? item.type.replace(/([a-z])([A-Z])/g, '$1 $2').replace(/_/g, ' ') : o.label;
                  return { key: o.key, label: label.charAt(0).toUpperCase() + label.slice(1).toLowerCase(), value: labelStated(o.value, opened.session.analysis.existing), faces: o.faces,
                    // With no number and no id the engine has no way to remove it: no bin.
                    removable: Boolean(o.number || o.id) };
                }),
                removed: new Set(partPending.values().filter((e) => e.kind === 'remove').map((e) => e.id)),
                onRemove: (key: string) => {
                  const o = opened.model.other.find((x) => x.key === key);
                  if (o) editPending(partPending.with({ kind: 'remove', id: key, name: o.label, targets: removalTargets(o.number ? [o.number] : [], o.id), was: o.value }));
                },
                onRestore: (key: string) => editPending(partPending.without(key)),
              },
              stated: {
                values: opened.stated.values, texts: opened.stated.texts, decided: opened.stated.decided,
                removed: new Set(partPending.values().filter((e) => e.kind === 'remove' || e.kind === 'clear').map((e) => e.id)),
                onRemove: (id: string) => {
                  const row = opened.table.rows.find((r) => r.id === id);
                  if (!row) return;
                  // The edit the bin replaces (a change, a value for a decision) comes back with Undo.
                  const before = partPending.get(id);
                  if (before && before.kind !== 'remove' && before.kind !== 'clear') displaced.current.set(`${opened.part}:${id}`, before);
                  // A stored "none needed" taken back: the item is open again (clear); a stated one removed.
                  editPending(partPending.with(row.state === 'decided_none'
                    ? { kind: 'clear', id, name: rowName(opened, id) }
                    : removeEdit(row, opened.bare.find((q) => q.id === id), rowName(opened, id), opened.session.analysis as PmiAnalysis)));
                },
                onRestore: (id: string) => {
                  const before = displaced.current.get(`${opened.part}:${id}`);
                  displaced.current.delete(`${opened.part}:${id}`);
                  editPending(before ? partPending.with(before) : partPending.without(id));
                },
              },
              callOutDimension: (dimension: CalledOut, tolerance: string) => {
                // One key per dimension called out, kept while the part is shown (the same edit is replaced).
                const keys = callOutKeys.current;
                const at = `${opened.part}:${dimension.id}`;
                // One the file states: its feature-and-kind (the table's row stating that number) is the key.
                const row = dimension.number === undefined ? undefined
                  : opened.table.rows.find((r) => r.requirements?.some((q) => q.number === dimension.number));
                if (dimension.number !== undefined) {
                  if (!row || typeof row.value !== 'string') return false;
                  keys.set(at, row.id);
                  const edit = callOutEdit(dimension, tolerance, row.id, { id: row.id, value: row.value, label: labelStated(row.value, opened.session.analysis.existing) });
                  editPending(edit ? partPending.with(edit) : partPending.without(row.id));
                  return true;
                }
                const key = keys.get(at) ?? nextExtraKey([...opened.table.rows.map((r) => r.id), ...partPending.values().map((e) => e.id),
                  ...Object.keys(opened.shown.all), ...keys.values()]);
                keys.set(at, key);
                const edit = callOutEdit(dimension, tolerance, key);
                editPending(edit ? partPending.with(edit) : partPending.without(key));
                return true;
              },
            } : {})}
          />
        )}
      </main>
      {editing && !empty && (
        <EditBar items={items} drafts={draftSnapshot.drafts} saving={saving && !stopped} busy={busy} blocked={(!opened ? 'Opening…' : undefined) ?? saveBlocked} savedNote={savedNote}
          onSave={() => void onSave()} onDiscard={() => void onDiscard()} onUndoDraft={(part, id) => void onDiscard({ part, id })} />
      )}
      <footer className="app-about">
        Specify {config?.specifyCore ?? ''} · app {__APP_VERSION__} · runs on this computer; the file is not uploaded ·{' '}
        <a href={DOCS} target="_blank" rel="noreferrer">Docs</a> ·{' '}
        <a href={ISSUES} target="_blank" rel="noreferrer">Report a problem</a>
      </footer>
    </div>
  );
}

createRoot(document.getElementById('root')!).render(<App />);
