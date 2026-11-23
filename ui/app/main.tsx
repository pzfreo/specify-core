// Specify on this computer: the app the page server serves (specify-core docs/open.md), started by
// `specify-core open` or by an agent's `show` (docs/session.md). SpecifyReview shows
// the file's feature table; the person's edits are held in the page and Save sends them to the
// command as an edit list per part, which it applies to the file all or nothing. The session's answers (an
// agent's) come down as they land. Nothing is stored in the browser and nothing is fetched from
// anywhere but the page's own origin.
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/600.css';
import '../src/specify/PmiLab.css';
import './app.css';
import { createRoot } from 'react-dom/client';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  createIntentEngine, drawnFromScene, hasGraphics, SpecifyReview, StepDiff, withoutNotesBlock,
  type DrawnPmi, type IntentReview, type PmiAnswers, type PmiLabSession, type PmiQuestion, type SpecifyEngine, type SpecifySlots, type WorkerClient,
} from '../src/index';
import { stepBytes } from '../src/model/diff';
import { SessionApiMismatch } from './session-api';
import { fileRows, requirementName, type ReviewFile, type ReviewFocus, type ReviewPart, type ReviewSaveResult } from '../src/specify/file-review';
import { answerText, bareAnalysis } from '../src/specify/feature-table';
import { EMPTY_QUEUE } from '../src/specify/review-assembly';
import { EditBar } from '../src/specify/EditBar';
import { changedIds, createSession, sameAnswers, type AnswersSession, type AnswersStore } from './answers';
import { decisions, type Decision } from './diff-report';
import { discardAnswers, getAnswers, getConfig, getDiff, getFile, keepAlive, listen, type DiffBody, type SaveEdits, type LocalConfig, type AnswerDraft } from './local';
import { EditsRefused, editsSaver, FileChanged, NotShown, Stale, type Saver } from './save';
import { withDrafts } from './edits';

declare const __APP_VERSION__: string;
const DOCS = 'https://github.com/pzfreo/specify-core#readme';
const ISSUES = 'https://github.com/pzfreo/specify-core/issues';
/** A part of the file, as the engine's `parts` lists them; the session keeps answers by its index.
 * ``written``: the file stores Specify's answers for it. */
interface PartRow extends ReviewPart { written: boolean }

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
  bytes: Uint8Array; file: ReviewFile; labels: PmiQuestion[]; store: AnswersStore; saver: Saver;
  /** Working answers used for agent notices and revision tracking. */
  external: { answers: PmiAnswers; review: IntentReview };
  /** The working answers processed by the page at the last shown session revision. */
  shown: { answers: PmiAnswers };
  /** Specify's review of ``answers``, for answers that arrive from outside the page. */
  reviewOf(answers: PmiAnswers): Promise<IntentReview>;
}
/** The revision comparison the command gave (``body``: WASM `diff()`'s, with the PMI scenes), drawn as it is. */
interface Compared { old: { name: string; bytes: Uint8Array }; next: { name: string; bytes: Uint8Array }; body: DiffBody; decisions: Decision[]; verdict: string; summary: string }

/** Opens the part on the engine with the session's answers: its analysis and mesh, their review,
 * and the PMI as drawn. */
async function openPart(worker: WorkerClient, answersSession: AnswersSession, name: string, bytes: Uint8Array, at: { part: number; several: boolean }, readOnly: boolean, generation = 0): Promise<Opened> {
  const file = fileOf(name, bytes, at.part, at.several, generation);
  const { sourceId } = file;
  const snapshot = await worker.request<{ analysis: PmiLabSession['analysis']; mesh: PmiLabSession['mesh']; provenance?: PmiLabSession['provenance'] }>('open', file);
  // The session's answers with the page's unsaved changes to this part (kept while another part was shown).
  const answers = answersSession.shown(at.part);
  const reviewOf = (given: PmiAnswers) => worker.request<IntentReview>(
    readOnly ? 'review-given' : 'review-edits', { ...file, answers: given, without: [] });
  const review = await reviewOf(answers);
  const stored = snapshot.analysis.resume?.answers ?? {};
  const fileReview = sameAnswers(stored, answers) ? review : await reviewOf(stored);
  const store = answersSession.store(at.part, stored);
  const shown = { answers };
  const engine: SpecifyEngine = {
    questions: (_id, given) => reviewOf(given),
    interpret: (_id, given, question, text) => worker.request('interpret', { analysis: snapshot.analysis, answers: given, question, text }),
    preview: async (_id, given) => (await reviewOf(given)).intent,
  };
  const session: Opened['session'] = {
    sessionId: sourceId, filename: name, analysis: snapshot.analysis, mesh: snapshot.mesh, answers: stored,
    ...(snapshot.provenance ? { provenance: snapshot.provenance } : {}),
    questions: fileReview.questions, open: fileReview.open, problems: fileReview.problems,
  };
  const scene = drawnFromScene(await worker.request('scene', { ...file, state: { answers: stored } }));
  const drawn = scene.ok ? withoutNotesBlock(scene.value) : null;
  // Every row named as Specify asks it of the bare part (the table's rows are those questions).
  const bare = await worker.request<IntentReview>('review-analysis-given', { analysis: bareAnalysis(snapshot.analysis), answers: {} });
  return {
    ...at, generation, session, review: fileReview, engine, drawn: drawn && hasGraphics(drawn) ? drawn : null, bytes, file, labels: bare.questions, store, shown,
    saver: editsSaver(store, answersSession, () => shown.answers), external: { answers, review }, reviewOf,
  };
}

/** The PMI the saved file ``bytes`` has for the part shown, as drawn: null when it has no graphics,
 * undefined when the scene could not be made (the view is then left as it is). */
async function savedDrawn(worker: WorkerClient, bytes: Uint8Array, at: { part: number; several: boolean }): Promise<DrawnPmi | null | undefined> {
  const scene = await worker.request('file-scene', { bytes, ...(at.several ? { part: at.part } : {}) }).then(drawnFromScene, () => null);
  if (!scene) return undefined;
  const drawn = scene.ok ? withoutNotesBlock(scene.value) : null;
  return drawn && hasGraphics(drawn) ? drawn : null;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`;

interface InspectionStep { path: string; name: string; bytes: Uint8Array }
const inspectionName = (path: string) => path.split(/[\\/]/).pop() ?? path;
const SAVE_FIRST = 'Save first: the inspection files describe the saved STEP';

/** What the last save wrote, and the questions still open in it, by their short labels. */
interface SavedLine { path: string; open: string[] | null; byAgent: boolean }

function App() {
  const [config, setConfig] = useState<LocalConfig | null>(null);
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
  const [saved, setSaved] = useState<SavedLine | null>(null);
  const [dirty, setDirty] = useState(false);
  // The event stream: null until it first opens, false while it is lost (retrying).
  const [live, setLive] = useState<boolean | null>(null);
  const [stopped, setStopped] = useState(false);
  // An in-place save refused because another program changed the file: Save cannot succeed again.
  const [fileChanged, setFileChanged] = useState(false);
  const [editsPending, setEditsPending] = useState(false);
  const [draftSnapshot, setDraftSnapshot] = useState<{ rev: number; drafts: Record<string, AnswerDraft> }>({ rev: 0, drafts: {} });
  const displayedRevision = useRef(draftSnapshot.rev);
  displayedRevision.current = draftSnapshot.rev;
  const [discarding, setDiscarding] = useState(false);
  const [checking, setChecking] = useState(false);
  const [reopening, setReopening] = useState(false);
  // Answers that arrived from outside the page but could not be shown (their review failed): Save
  // is refused until they are, so the page offers to show them again.
  const [unshown, setUnshown] = useState<string | null>(null);
  const reshow = useRef<() => void>(() => {});
  const refreshConfig = useRef<() => void>(() => {});
  const worker = useRef<WorkerClient | null>(null);
  const view = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const exportsSlot = useRef<HTMLDivElement>(null);
  const editBar = useRef<HTMLDivElement>(null);
  const [inspectionStep, setInspectionStep] = useState<InspectionStep | null>(null);
  const [inspectionProblem, setInspectionProblem] = useState<string | null>(null);
  const inspectionLoad = useRef(0);
  const savedRevision = useRef(-1);
  const lastSaveResponse = useRef(-1);
  // The bytes the session's last save wrote: the 3D view draws their PMI, for every part shown since.
  const savedSource = useRef<{ bytes: Uint8Array; generation: number } | null>(null);
  const heardSaves = useRef(new Set<string>());
  const captureSaved = useRef<(saved: { path: string; rev: number }) => Promise<Uint8Array | null>>(async () => null);
  const savedLoad = useRef<Promise<Uint8Array | null>>(Promise.resolve(null));
  const [slots, setSlots] = useState<SpecifySlots | null>(null);
  useEffect(() => setSlots({ view: view.current!, panel: panel.current!, exports: exportsSlot.current }), []);
  // Answers from outside the page (an agent's, source "mcp") are shown as they land: the review
  // takes them, the first changed question opens, its faces are lit, and a note says what the
  // agent changed. The note stays until it is dismissed, the person changes something, or the
  // agent changes something else.
  const [agent, setAgent] = useState<{ ids: string[]; n: number } | null>(null);
  const [other, setOther] = useState<{ name: string; part: number; n: number; joined: boolean } | null>(null);
  const [focus, setFocus] = useState<ReviewFocus & { n?: number } | null>(null);
  const [lit, setLit] = useState<{ faces: readonly number[]; n: number } | undefined>(undefined);
  const edited = () => setAgent(null);

  useEffect(() => {
    let stop = () => {};
    let unlisten = () => {};
    let unwatch = () => {};
    let ended = false;
    (async () => {
      const first = await getConfig();
      setConfig(first);
      document.title = `${first.name} — Specify`;
      const heartbeat = keepAlive(setConfig, setStopped, () => {
        ended = true;
        unlisten();
        sessionRef.current?.close();
      });
      stop = heartbeat.stop;
      refreshConfig.current = heartbeat.refresh;
      worker.current = createIntentEngine({ wasmUrl: first.wasmUrl });
      const [bytes, answers, old, diff] = await Promise.all([
        getFile('new').then(stepBytes), getAnswers(),
        first.mode === 'diff' ? getFile('old').then(stepBytes) : null,
        first.mode === 'diff' ? getDiff() : null,
      ]);
      const answersSession = createSession(answers.rev, answers.parts, answers.drafts);
      if (first.readOnly) setInspectionStep({ path: first.path, name: first.name, bytes });
      captureSaved.current = (event) => {
        if (event.rev < savedRevision.current || ended) return Promise.resolve(null);
        savedRevision.current = event.rev;
        const load = ++inspectionLoad.current;
        setReopening(true);
        setInspectionStep(null);
        setInspectionProblem(null);
        // The exports come from the file as saved, a copy or the input in place (`file?rev=saved`).
        const loading = (async () => {
          const written = await getFile('saved').then(stepBytes);
          if (ended || load !== inspectionLoad.current) return written;
          setInspectionStep({ path: event.path, name: inspectionName(event.path), bytes: written });
          savedSource.current = { bytes: written, generation: load };
          // The session works on the file as saved from now on (ADR P11: the file is the only
          // copy): the part shown opens again from it, so what the save wrote reads as the file's
          // and the next edits apply to it. The 3D view draws the PMI as the save wrote it
          // (specify-ui#14); a scene that fails leaves the view as the opening drew it.
          const shown = shownRef.current;
          if (!shown || !worker.current) return written;
          let next = await openPart(worker.current, answersSession, first.name, written, { part: shown.part, several: shown.several }, first.readOnly, load);
          const drawn = await savedDrawn(worker.current, written, next);
          const listed = shown.several ? await worker.current.request<PartRow[]>('parts', { name: first.name, bytes: written }) : null;
          if (drawn !== undefined) next = { ...next, drawn };
          if (ended || load !== inspectionLoad.current || shownRef.current?.store !== shown.store) { next.store.close(); return written; }
          shown.store.close();
          shownRef.current = next;
          setOpened(next);
          if (listed) setParts(listed);
          return written;
        })().catch((error: unknown) => {
          if (!ended && load === inspectionLoad.current) setInspectionProblem(message(error));
          return null;
        }).finally(() => {
          if (!ended && load === inspectionLoad.current) setReopening(false);
        });
        savedLoad.current = loading;
        return loading;
      };
      sessionRef.current = answersSession;
      if (ended) answersSession.close();
      const showDrafts = () => {
        setDirty(answersSession.dirty());
        setDraftSnapshot(answersSession.draftSnapshot());
      };
      unwatch = answersSession.subscribe(showDrafts);
      showDrafts();
      // An assembly's parts: the page shows one at a time, first the first the session has answers for.
      const listed = await worker.current.request<PartRow[]>('parts', { name: first.name, bytes });
      const several = listed.length > 1 || listed[0]?.instances > 1;
      let rows = listed;
      const withAnswers = rows.find((row) => Object.keys(answers.parts[String(row.part)] ?? {}).some((id) => !id.startsWith('_')));
      // The part the agent's `show` named (config.part), else the first with answers.
      const asked = rows.find((row) => row.part === first.part);
      const at = { part: several ? (asked ?? withAnswers ?? rows[0]!).part : 0, several };
      setParts(rows);
      const part = await openPart(worker.current, answersSession, first.name, bytes, at, first.readOnly);
      if (old && diff) {
        const r = diff.report as { verdict?: string; summary?: string };
        setCompared({
          old: { name: first.oldName ?? 'old.step', bytes: old }, next: { name: first.name, bytes }, body: diff,
          decisions: decisions(diff.report), verdict: r.verdict ?? '', summary: r.summary ?? '',
        });
      }
      shownRef.current = part;
      let lost = false;
      if (!ended) unlisten = listen((event) => {
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
        heardSaves.current.add(`${event.rev}:${event.path}`);
        void captureSaved.current(event);
        if (event.source === 'mcp') void agentSaved(event.path);
        heartbeat.refresh();
      }, (up) => {
        setLive(up);
        if (!up) { lost = true; heartbeat.streamLost(); return; }
        // Back after a loss: what changed meanwhile, as the session has it now.
        if (lost) getAnswers().then((now) => answersSession.receive({ ...now, source: 'start' }), () => {});
        lost = false;
      });
      setOpened(part);
      // The other parts the session holds answers for: what the file stores for each, so answers an
      // agent gave them before the page opened count as unsaved. A part Specify did not write
      // stores none; one it wrote is read from the file, after the part shown is up.
      for (const row of rows) {
        if (row.part === at.part || !(String(row.part) in answers.parts)) continue;
        if (!row.written) { answersSession.fileAnswers(row.part, {}); continue; }
        const snapshot = await worker.current.request<{ analysis: PmiLabSession['analysis'] }>('open', fileOf(first.name, bytes, row.part, true)).catch(() => null);
        if (snapshot) answersSession.fileAnswers(row.part, snapshot.analysis.resume?.answers ?? {});
      }
    })().catch((error: unknown) => {
      if (ended) return;
      if (error instanceof SessionApiMismatch) setApiProblem(error.message);
      else setOpeningError(message(error));
    });
    return () => { stop(); unlisten(); unwatch(); sessionRef.current?.close(); worker.current?.dispose(); };
  }, []);

  useEffect(() => {
    if (!opened) return;
    shownRef.current = opened;
    opened.store.onProblem((error) => setProblem(`Your change did not reach the session: ${error.message}`));
  }, [opened?.store]);

  const busy = switching || checking || reopening || discarding;

  /** Shows another part of the file, with the session's answers for it now. */
  const showPart = async (index: number) => {
    const answersSession = sessionRef.current;
    if (busy || !opened || !config || !worker.current || !answersSession || index === opened.part) return;
    setFocus(null);
    setLit(undefined);
    setSwitching(true); setProblem(null);
    try {
      // The part left keeps its unsaved answers: in the session, and counted as unsaved.
      await opened.store.settled();
      // If another page or the agent saved while this part was opening, open the saved version.
      for (;;) {
        const source = savedSource.current;
        let next = await openPart(worker.current, answersSession, config.name, source?.bytes ?? opened.bytes,
          { part: index, several: true }, config.readOnly, source?.generation ?? opened.generation);
        const drawn = source ? await savedDrawn(worker.current, source.bytes, next) : undefined;
        if (source !== savedSource.current) { next.store.close(); continue; }
        if (drawn !== undefined) next = { ...next, drawn };
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
    const show = (answers: PmiAnswers, source: string, previous: PmiAnswers) => {
      if (source === 'mcp') for (const id of changedIds(previous, answers)) fromAgent.add(id);
      // The page's own change, as SpecifyReview already shows it (or answers back to what it shows).
      if (sameAnswers(answers, shown.answers)) { setUnshown(null); return; }
      void opened.reviewOf(answers).then((review) => {
        // Another part shown since, or a newer change on its way (shown when it lands).
        if (current.current?.store !== store || !sameAnswers(store.get(), answers)) return;
        shown.answers = answers;
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
      });
    };
    // The session's answers, shown again if the page does not show them yet.
    const again = () => { if (current.current?.store === store && !sameAnswers(store.get(), shown.answers)) show(store.get(), 'start', shown.answers); };
    reshow.current = again;
    const unsubscribe = store.subscribe(show);
    // A change that landed while the part was opening, before this listened.
    again();
    return () => { unsubscribe(); setUnshown(null); };
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
  useEffect(() => {
    if (!dirty && !editsPending) return;
    const guard = (event: BeforeUnloadEvent) => { event.preventDefault(); };
    window.addEventListener('beforeunload', guard);
    return () => window.removeEventListener('beforeunload', guard);
  }, [dirty, editsPending]);

  const mode = config?.mode;
  const readOnly = !config || config.readOnly;

  /** The short labels of questions ``ids`` (the part shown's; an id Specify does not ask here is kept as it is). */
  const prompts = (ids: string[], ...reviews: { questions: PmiQuestion[] }[]) => {
    const prompt = new Map(reviews.flatMap((r) => r.questions.map((q) => [q.id, q.label ?? q.prompt] as const)));
    return ids.map((id) => prompt.get(id) || id);
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
  const onSave = async (edits: SaveEdits): Promise<ReviewSaveResult | null> => {
    if (switching || reopening || discarding || stopped || !opened) return null;
    setProblem(null);
    try {
      const result = await opened.saver.save(edits, Math.max(displayedRevision.current, lastSaveResponse.current));
      lastSaveResponse.current = result.rev;
      // Reuse an SSE reload if it beat the response. The command advances the revision on every write.
      const loading = heardSaves.current.has(`${result.rev}:${result.path}`)
        ? savedLoad.current : captureSaved.current(result);
      setSaved({ path: result.path, open: prompts(result.missing, opened.external.review, opened.review), byAgent: false });
      refreshConfig.current();
      return { bytes: await loading, name: inspectionName(result.path), report: result.report };
    } catch (error) {
      if (sessionRef.current?.signal.aborted) return null;
      if (error instanceof Stale) {
        const now = await getAnswers().catch(() => null);
        if (now) sessionRef.current?.receive({ ...now, source: 'start' });
      }
      if (error instanceof FileChanged) {
        setFileChanged(true);
        setProblem('Not saved: the file changed on disk since it was opened (another program wrote it), so Save will not write over it. '
          + 'Your changes are still listed below; open the file again to make them.');
      } else if (error instanceof EditsRefused) {
        return { failures: error.failures };
      } else if (error instanceof NotShown && unshown) {
        setProblem('Not saved: the latest answers in the session are not shown yet (see below). Trying to show them again.');
        reshow.current();
      } else if (error instanceof Stale || error instanceof NotShown) {
        reshow.current();
        setProblem('Not saved: the session changed while you saved (your agent answered). What is now shown is the latest; check it and Save again.');
      } else setProblem(`Not saved: ${message(error)}`);
      return null;
    }
  };

  const onDiscard = async (target?: { part: number; id: string }): Promise<boolean> => {
    const session = sessionRef.current;
    if (switching || discarding || stopped || !session) return false;
    setProblem(null);
    setDiscarding(true);
    try {
      if (target || Object.keys(session.draftSnapshot().drafts).length) {
        const snapshot = await discardAnswers(Math.max(displayedRevision.current, lastSaveResponse.current), target, session.signal);
        session.receive({ ...snapshot, source: 'start' });
      }
      setAgent(null);
      return true;
    } catch (error) {
      if (!session.signal.aborted) {
        const latest = await getAnswers().catch(() => null);
        if (latest) session.receive({ ...latest, source: 'start' });
        setProblem(`Could not discard: ${message(error)}`);
      }
      return false;
    } finally { setDiscarding(false); }
  };

  const inspectionTitle = inspectionProblem ?? (!inspectionStep && savedRevision.current < 0 && !readOnly ? SAVE_FIRST
    : reopening ? 'Reading the saved STEP…' : undefined);
  /** A decision picked in the diff: its question, in the answers. */
  const decide = (d: Decision) => {
    setTab('answers');
    setFocus({ questionId: d.question, number: d.number, feature: d.feature, requirement: d.requirement, scroll: true, n: Date.now() });
  };

  const showingChanges = mode === 'diff' && tab === 'changes';
  const saveBlocked = apiProblem ?? (openingError ? 'Save is disabled until the file and its answers can be loaded. Correct the session answers, then reload this page.'
    : stopped ? 'Specify has stopped' : fileChanged ? 'The file changed on disk since it was opened: open it again to save' : undefined);
  // The panel edits in specify and diff mode; read-only shows the review's PMI viewer instead.
  const editing = Boolean(config && !config.readOnly);
  const staged = useMemo(() => {
    const rows = opened ? fileRows(opened.session.analysis, opened.review.questions, opened.labels) : [];
    return withDrafts(EMPTY_QUEUE, draftSnapshot.drafts, (part, id, value) => {
      const row = part === opened?.part ? rows.find(one => one.id === id) : undefined;
      const words = id.startsWith('part.') ? id.slice(5).replace(/_/g, ' ') : id;
      return { name: row ? requirementName(row.question) : words.charAt(0).toUpperCase() + words.slice(1),
        text: answerText(value, row?.question) };
    });
  }, [draftSnapshot.drafts, opened]);
  return (
    <div className="app">
      <header className="app-header">
        <div className="app-title">
          <strong className="app-brand">Specify</strong>
          {config && mode === 'diff' && <span className="app-file app-old" title={config.oldPath ?? ''}>{config.oldName}</span>}
          {config && mode === 'diff' && <span aria-hidden="true">→</span>}
          {config && <span className="app-file">{config.name}</span>}
          {config && <code className="app-path">{mode === 'diff' ? `${config.oldPath} → ${config.path}` : config.path}</code>}
          {parts.length > 1 && opened && (
            <label className="app-part">Part{' '}
              <select value={opened.part} disabled={busy} onChange={(event) => void showPart(Number(event.target.value))}>
                {parts.map((row) => <option key={row.part} value={row.part}>{row.name}</option>)}
              </select>
            </label>
          )}
          {readOnly && config && <span className="app-badge">View only</span>}
          {(dirty || editsPending || staged.size > 0) && <span className="app-dirty">Unsaved changes</span>}
        </div>
        <div className="app-actions">
          <div className="app-exports">
            <div className="app-export-buttons" ref={exportsSlot}>
              {!opened && <>
                <button type="button" disabled title={inspectionTitle}>Inspection list (CSV)</button>
                <button type="button" disabled title={inspectionTitle}>AS9102 FAI (Excel)</button>
              </>}
            </div>
            {inspectionStep && <span className="app-inspection-source">
              from <code>{inspectionStep.name}</code>
            </span>}
            {inspectionProblem && <span className="app-problem" role="alert">Inspection exports unavailable. {inspectionProblem}</span>}
          </div>
        </div>
        {config && (
          <div className="app-paths">
            {saved && (
              <div className="app-saved" role="status">
                {saved.byAgent ? 'Your agent saved' : 'Saved'} <code>{saved.path}</code>{saved.open !== null && `: ${saved.open.length
                  ? `${plural(saved.open.length, 'question')} ${saved.byAgent ? 'still unanswered' : 'still open'}: ${saved.open.slice(0, 6).join('; ')}${saved.open.length > 6 ? '; …' : ''}`
                  : 'complete'}`}
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
          Specify has stopped. Run <code>specify-core open …</code> again (or ask your agent to show it).
          {' '}Your saved file is at <code>{saved?.path ?? config.path}</code>.
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
            Requirements for {config?.name}{compared?.decisions.length ? ` (${plural(compared.decisions.length, 'decision')})` : ''}
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
        {!opened && !problem && !openingError && !apiProblem && !stopped && <p className="app-loading">Opening {config?.name ?? 'the part'}…</p>}
        {compared && (
          <div className="app-diff diff-page" hidden={!showingChanges}>
            <StepDiff before={compared.old} after={compared.next} compared={compared.body} wasmUrl={config?.wasmUrl} />
          </div>
        )}
        <div className={`app-slot app-view${showingChanges ? ' app-under' : ''}`} ref={view} />
        <div className={`app-slot app-panel${showingChanges ? ' app-under' : ''}`} ref={panel} />
        {opened && slots && (
          <SpecifyReview
            engine={opened.engine} session={opened.session} readOnly={readOnly} drawn={opened.drawn}
            slots={slots}
            externalAnswers={opened.session.answers} externalReview={opened.review} disabled={busy || stopped}
            focusRequest={focus} showFaces={lit}
            fileReview={worker.current ? {
              file: opened.file, worker: worker.current, questionLabels: opened.labels, staged, parts,
              stagedReplacement: 'replaces the agent’s answer',
              onPending: pending => { setEditsPending(pending.size > 0); edited(); },
              onBusy: setChecking, save: onSave, onDiscard,
              inspectionName: inspectionStep?.name, inspectionBlocked: inspectionTitle,
              saveBlocked, bar: editBar.current,
            } : undefined}
            onExport={action => action()}
          />
        )}
      </main>
      <div ref={editBar}>{editing && (!opened || !slots) && <EditBar items={[]} saving={false}
        busy={false} blocked={saveBlocked ?? 'Opening…'} savedNote={null}
        onSave={() => {}} onDiscard={() => {}} onAddAll={() => {}} onUndoDraft={() => {}} />}</div>
      <footer className="app-about">
        Specify {config?.specifyCore ?? ''} · app {__APP_VERSION__} · runs on this computer; the file is not uploaded ·{' '}
        <a href={DOCS} target="_blank" rel="noreferrer">Docs</a> ·{' '}
        <a href={ISSUES} target="_blank" rel="noreferrer">Report a problem</a>
      </footer>
    </div>
  );
}

createRoot(document.getElementById('root')!).render(<App />);
