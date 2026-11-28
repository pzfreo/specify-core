// Ported from Draftwright.io's PmiLabReview; see third_party/README.md.
import { useCallback, useEffect, useMemo, useRef, useState, type ComponentProps, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import type { IntentReview, PartTint, PmiAnswers, PmiIntent, PmiInterpretation, PmiLabError, PmiLabSession, PmiQuestion, PmiLabEngine } from '../types/pmi';
import { editorDefault } from '../model/intent';
import { addedOf, CALL_OUTS, describe, kindsFor, MAY_HAVE_DATUM, nextKey } from './added';
import { codeOf } from './choose';
import { appliedByRule, chosenForYou, type Chosen } from './chosen';
import { datumChips, freeLetter, removal } from './datums';
import { bestReferenceView, normalizeDraftingPmi, resolveStatedOrigin } from './drafting-layout';
import { bestDrawnView, drawnGroup, drawnShown, drawnViews, partCentre, splitDrawnViews, withoutNotesBlock, type DrawnPmi, type PartOrigin } from './drawn';
import { optionHint } from './hints';
import { balloonsBySource, type Characteristic, byBalloon, characteristics, noteStatements, ranges, toCsv } from './characteristics';
import { as9102 } from './as9102';
import { xlsx } from '../model/xlsx';
import { readOnlyEngine } from './read-only';
import { inspectionAnswers } from './inspection';
import { CharacteristicsTable, ExtraEditor, PurposeChoice, SpecBox, SuggestBox, SuggestionCard, TOLERANCE_PICKS, type ExtraDraft } from './inputs';
import { fmt, type PmiLabel } from './labels';
import { drawnBalloons, oneViewFile, oneViewLabels, scenePendingState, type OneViewPending, type OneViewRow, type SceneSemanticLabel } from './one-view';
import { labelName, labelRow, unitLine, withUnits, type LengthUnit } from './units';
import { compose, emptyBlock, parse, placesText, type GeneralTolerance, type Units } from './general-tolerance';
import type { Picker } from './picker';
import type { HostView } from './host-view';
import { createReviewView } from './review-view';
import { useCadViewer } from './use-cad-viewer';
import { answerOf, confirmRow, display, groupQuestions, needs, nextNeeding, questionForFace, rowState, summary, type QuestionGroup } from './review';
import { MODES, type PickMode } from './selection';
import './PmiLab.css';
export type SpecifyEngine = Pick<PmiLabEngine, 'questions' | 'interpret' | 'preview'> & {
  /** Optional host-specific review requested only when an inspection export runs.
   * Display always uses preview; the original three-method host contract remains sufficient. */
  inspection?(sessionId: string, answers: PmiAnswers): Promise<PmiIntent>;
};
/** Where the workspace shows Specify: the part in the 3D pane, the questions in the edit pane. */
export interface SpecifySlots {
  view: HTMLElement; panel: HTMLElement; exports: HTMLElement | null;
  /** Where the exports for specialists go (under the menu's More); with none, they join ``exports``. */
  more?: HTMLElement | null;
  /** The selected queue row's: the current question, answered where it is listed. */
  question?: HTMLElement | null;
}
const STATE_TEXT = { required: 'needs an answer', check: 'please check', set: 'answered', ok: 'default' } as const;
/** Short names for the part's own fields; any other part question is shown by its prompt. */
const PART_FIELD: Record<string, string> = {
  'part.material': 'Material', 'part.process': 'Process', 'part.general_tolerance': 'General tolerance', 'part.locating': 'Holes located by',
  'part.surface_finish': 'Surface finish', 'part.coating': 'Coating or treatment',
  'part.heat_treatment': 'Heat treatment', 'part.edges': 'Edges',
};
/** The part's notes, folded under "Finish and notes". */
const NOTE_FIELDS = new Set(['part.surface_finish', 'part.coating', 'part.heat_treatment', 'part.edges']);
const MODE_TEXT: Record<PickMode, string> = { face: 'Face', feature: 'Hole / feature', coplanar: 'Coplanar faces' };
const EMPTY_ONE_PENDING: ReadonlyMap<string, OneViewPending> = new Map();

function asError(error: unknown): PmiLabError {
  if (error && typeof error === 'object' && 'message' in error) return error as PmiLabError;
  return { code: 'failed', message: String(error) };
}

function SpecifyReviewInner({ engine, session, slots, slotQuestion, onSaved, onExport, inspectionIntent, inspectionExports = true, gated, after, onPick, showFaces, tints, externalAnswers, externalReview, disabled, focusRequest, toReview, onKeep, kept, drawn: drawnPmi = null, datums = true, readOnly = false, oneView, sceneSemantic, partOrigin = null, view: hostView = null, stated: statedRows, addedByYou = true, callOutDimension, otherPmi, lengthUnit = 'mm', calledOutDimensions, calledOutNumbers, takenKeys }: {
  engine: SpecifyEngine; session: PmiLabSession; slots: SpecifySlots;
  /** The question the queue's selected row is about: what the question slot shows. */
  slotQuestion?: string | null;
  /** A file handed to the browser to save, by name. */
  onSaved?: (name: string) => void;
  onExport: (action: () => void, format?: 'csv' | 'xlsx') => void;
  /** Gets given/kept intent, or intent whose defaults the host explicitly approved.
   * Null cancels the export. Optional engine.inspection reviews given/kept answers.
   * Hosts implementing the original three methods retain their preview behaviour. */
  inspectionIntent?: () => Promise<PmiIntent | null>;
  /** Hide preview-download controls when the host owns saved-file inspection exports. */
  inspectionExports?: boolean;
  /** Structured exports ask for sign-in first. */
  gated?: boolean;
  /** Shown after the questions: the drawing's own issues, in the same list. */
  after?: ReactNode;
  /** Faces picked on the part, by a face or a label: the drawing locates what measures them. */
  onPick?: (faces: readonly number[]) => void;
  /** Faces selected elsewhere (the drawing), lit on the part; a new ``n`` each time. */
  showFaces?: { faces: readonly number[]; n: number };
  /** The queue's state on the part, by face; a new map when it changes. */
  tints?: ReadonlyMap<number, PartTint>;
  externalAnswers: PmiAnswers; externalReview: IntentReview; disabled: boolean; focusRequest?: { questionId: string; faces?: readonly number[]; scroll?: boolean; focusInput?: boolean } | null;
  /** The editor's decisions to review, by question: the suggestions are exactly these (agreed 2026-10-16). */
  toReview?: readonly string[];
  /** Keeps suggestions as they stand, the editor's way: accepted, as in its list, not answered. */
  onKeep?: (ids: readonly string[]) => void;
  /** Questions kept as suggested without being answered (the editor's accepted): exported as approved. */
  kept?: readonly string[];
  /** The PMI as drawn (Specify's pmiScene, less its notes block), when it has any graphics. */
  drawn?: DrawnPmi | null;
  /** Datum labels shown on the part: not until tolerancing is chosen (drawing-first plan). */
  datums?: boolean;
  /**
   * A PMI viewer: the 3D view and a list of the PMI that is set. Questions, answers, suggestions and the exports
   * are not rendered. Nothing reaches the host's ``engine.questions``, ``engine.interpret`` or ``onKeep``;
   * ``engine.preview`` supplies labels. Default false.
   */
  readOnly?: boolean;
  /** Feature-table rows and pending edits for the single PMI view. */
  oneView?: { readonly rows: readonly OneViewRow[]; readonly pending: ReadonlyMap<string, OneViewPending> };
  /** Structured semantic labels parsed from the active PMI scene. */
  sceneSemantic?: ReadonlyMap<string, SceneSemanticLabel>;
  /** Explicit coordinate system or datum origin stated by the file, rendered as an x/y/z triad. */
  partOrigin?: PartOrigin | null;
  /** The host's own 3D view to draw the part in (docs/host-view.md); by default the package's. */
  view?: HostView | null;
  /**
   * 0.3 (ADR P11; change list item 2): what the file states, shown where its questions are, as their
   * answers. Each such item has a pencil (open it to change) and a bin (remove); a removed one shows
   * as removed, with Undo. ``values``: the file's value by question. Without it nothing is stated.
   */
  stated?: {
    values: Readonly<PmiAnswers>; removed: ReadonlySet<string>;
    /** How each reads, as the file states it (the 0.3 engine's text, not an answer to format). */
    texts?: Readonly<Record<string, string>>;
    /** Which are a stored "none needed" (shown as decided; the bin takes the decision back). */
    decided?: ReadonlySet<string>;
    onRemove(id: string): void; onRestore(id: string): void;
  };
  /** "Added by you" is shown (default). 0.3 hosts hide it until the engine saves free-form requirements. */
  addedByYou?: boolean;
  /**
   * 0.3 (change list item 1): general dimensions are one row in the review, "N dimensions under
   * <general tolerance>", whose lines each have Call Out: a tolerance of its own, given here. False
   * when the host cannot call that dimension out. Without it, Call Out adds a requirement as before.
   */
  callOutDimension?: (dimension: CalledOut, tolerance: string) => boolean;
  /**
   * 0.3: requirements the file states on no feature Specify asks about ("Other PMI in the file"):
   * each with its bin (remove, confirmed; Undo). The engine cannot change them in place.
   */
  /** General dimensions the host holds called out, by characteristic, with their tolerance (it
   * outlasts the review, a part switch say); without it the review keeps its own. */
  calledOutDimensions?: ReadonlyMap<string, string>;
  /** The number Save writes for each general dimension called out, by characteristic: the engine's
   * for the call-out that replaces it (its drawn callout's), shown on its row until Save. */
  calledOutNumbers?: ReadonlyMap<string, number>;
  /** "Added by you" keys (extra.N) the host already uses, which a new requirement must not take. */
  takenKeys?: readonly string[];
  /** The file's length unit: the units a general tolerance by decimal places starts in (#228). */
  lengthUnit?: Units;
  otherPmi?: {
    items: readonly { key: string; label: string; value: string; faces: readonly number[]; removable?: boolean }[];
    removed: ReadonlySet<string>; onRemove(key: string): void; onRestore(key: string): void;
  };
}) {
  const host = engine;
  const readOnlyHost = useMemo(() => readOnlyEngine(host), [host]);
  engine = readOnly ? readOnlyHost : host;
  const stage = useRef<HTMLDivElement>(null);
  const picker = useRef<Picker | null>(null);
  const viewBarRef = useRef<HTMLDivElement>(null);
  const [moreTools, setMoreTools] = useState(false);
  const [moreViewsOpen, setMoreViewsOpen] = useState(false);
  const [questions, setQuestions] = useState<readonly PmiQuestion[]>(session.questions);
  const [answers, setAnswers] = useState<PmiAnswers>(session.answers ?? {});
  const [resumed, setResumed] = useState(false);
  const [current, setCurrent] = useState<string | null>(null);
  // A refusal is shown by what was refused: a question's id, or null for the panel.
  const [error, setError] = useState<{ at: string | null; problem: PmiLabError } | null>(null);
  const [preview, setPreview] = useState<PmiIntent | null>(externalReview.intent);
  const [selection, setSelection] = useState<number[]>([]);
  const [mode, setMode] = useState<PickMode>('face');
  const [status, setStatus] = useState('');
  // What a label clicked says that the part cannot: until the next question or view.
  const [note, setNote] = useState('');
  // The general dimension a label clicked is, offered to be called out.
  const [noteGeneral, setNoteGeneral] = useState<{ i: number; what: string } | null>(null);
  const keepCalloutNote = useRef(false);
  const [show, setShow] = useState({
    // What the general tolerance covers can be a hundred callouts: drawn only when chosen.
    defaults: true, existing: true, general: false, farSide: true, balloons: true,
  });
  // A face clicked that no question is about shows what the general tolerance says of it.
  const [peek, setPeek] = useState<number | null>(null);
  // Set by a label clicked: the question it opens is turned to without moving the view.
  const stayPut = useRef(false);
  const calloutFaces = useRef<{ id: string; faces: readonly number[] } | null>(null);
  const [selectedCallout, selectCallout] = useState<string | null>(null);
  // The question or added requirement a focus request opened without asking to be scrolled to.
  const quiet = useRef<string | null>(null);
  const [transparency, setTransparency] = useState(0);
  // Specs typed and read, by question: their callouts, which no offered choice has.
  const [typed, setTyped] = useState<Record<string, PmiInterpretation>>({});
  // A datum being added: picked on the part before pmi-assist knows of it.
  const [adding, setAdding] = useState<PmiQuestion | null>(null);
  // A requirement being added or changed, its faces picked on the part.
  const [draft, setDraft] = useState<ExtraDraft | null>(null);
  const [problems, setProblems] = useState(session.problems ?? []);
  // Which one-line row is unfolded: a part setting, the notes, the datums or the added.
  const [unfolded, setUnfolded] = useState<string | null>(null);
  // Show only the rows with something to review: a suggestion, a doubt, a problem.
  const [onlySuggestions, setOnlySuggestions] = useState(false);
  // The review, or every characteristic numbered as its balloon.
  const [view, setView] = useState<'review' | 'table'>('review');
  // General dimensions called out (0.3, change list item 1), by characteristic, with the tolerance given.
  const [calledOutHere, setCalledOut] = useState<ReadonlyMap<string, string>>(new Map());
  const calledOut = calledOutDimensions ?? calledOutHere;
  // A new "Added by you" key, past any the host holds (a call-out held in the bar, say).
  const freeKey = () => nextKey(answers, takenKeys);
  const [callingOut, setCallingOut] = useState<string | null>(null);
  const [selectedChar, setSelectedChar] = useState<string | null>(null);
  // What is left out of the inspection list: the CSV's, not the PMI's.
  const [skipped, setSkipped] = useState<ReadonlySet<string>>(new Set());
  const toggle = (key: string) => setUnfolded((open) => (open === key ? null : key));
  const rowsBox = useRef<HTMLFieldSetElement>(null);
  const turned = useRef(false);
  const groups = useMemo(() => groupQuestions(questions), [questions]);
  const partGroup = groups.find((g) => g.key === 'part');
  const rows = groups.filter((g) => g !== partGroup);
  // The datums row is always shown, last: it can be added to when nothing needs one.
  const datumGroup = rows.find((g) => g.key === 'datums') ?? { key: 'datums', title: 'Datums', questions: [] };
  const featureRows = rows.filter((g) => g.key !== 'datums');
  // Settings the file already states are not asked; they are shown as read.
  const stated = Object.entries(session.analysis.part ?? {}).filter(([key]) => !statedRows || !(`part.${key}` in statedRows.values));
  const notes = partGroup?.questions.filter((q) => NOTE_FIELDS.has(q.id)) ?? [];
  const notesSummary = notes.map((q) => String(answerOf(q, answers) ?? ''))
    .filter((v) => v && !/^(none|no edge note)/i.test(v)).join(' · ');
  const draftQuestion = useMemo<PmiQuestion | null>(() => (draft ? {
    id: draft.key, kind: 'faces', prompt: 'Pick faces', faces: draft.faces ?? [], default: draft.faces ?? [],
    why: '', attention: 'routine',
  } : null), [draft?.key, draft?.faces]);
  const currentQuestion = questions.find((q) => q.id === current)
    ?? (adding && adding.id === current ? adding : null)
    ?? (draftQuestion && draftQuestion.id === current ? draftQuestion : null);
  const faces = useMemo(() => new Map(session.analysis.faces.map((f) => [f.id, f])), [session]);
  const titles = useMemo(() => new Map(groups.map((g) => [g.key, g.title])), [groups]);
  const chars = useMemo(
    () => characteristics(preview, session.analysis, titles), [preview, session.analysis, titles],
  );
  // The part's notes as a drawing's notes block states them, then the notes written in words, as
  // written: on the view, where they stay put.
  const freeNotes = answers['part.notes'];
  // Read only: a part setting is set when the file states it or an answer gives it, not by a default.
  const partSet = (key: string) => key in (session.analysis.part ?? {}) || (() => {
    const given = answers[`part.${key}`];
    return Array.isArray(given) ? given.length > 0 : typeof given === 'string' ? given.trim() !== '' : given != null;
  })();
  const partNotes = [
    ...noteStatements(preview, session.analysis).filter((n) => !readOnly || partSet(n.key.slice('part:'.length))),
    ...(Array.isArray(freeNotes) ? freeNotes : []).filter((n): n is string => typeof n === 'string' && n.trim() !== '')
      .map((text, i) => ({ key: `notes:${i}`, text, question: 'part.notes' })),
  ];
  const allBalloons = useMemo(() => balloonsBySource(chars), [chars]);

  // The slot shows the selected row's question whatever else moved ``current``: the session
  // reloading, or an answer opening the next question down the list.
  useEffect(() => {
    if (!readOnly && slots.question && slotQuestion && slotQuestion !== current && questions.some((q) => q.id === slotQuestion)) setCurrent(slotQuestion);
  }, [slots.question, slotQuestion, current, questions]);

  // Only a banner or blank-cell request moves focus off the drawing into its answer input.
  useEffect(() => {
    if (readOnly || !focusRequest?.focusInput || focusRequest.questionId !== slotQuestion || !slots.question) return;
    const timer = window.setTimeout(() => slots.question?.querySelector<HTMLElement>('input, select, button')?.focus({ preventScroll: true }), 50);
    return () => window.clearTimeout(timer);
  }, [slots.question, slotQuestion, focusRequest]);

  useEffect(() => {
    const id = focusRequest?.questionId;
    if (readOnly || !id) return;
    // Opened from the queue above, it is not followed down the list unless asked.
    quiet.current = focusRequest.scroll ? null : id;
    // A problem about an added requirement is not a question: it opens as the problem's line does.
    if (!questions.some(question => question.id === id)) { setView('review'); openProblem(id, focusRequest.faces ?? []); return; }
    setView('review'); setOnlySuggestions(false); setCurrent(id);
    setUnfolded(NOTE_FIELDS.has(id) ? `note:${id}` : id.startsWith('part.') ? id : id.startsWith('datum.') ? 'datums' : null);
    // Brought into view only when asked: the queue that asked may be above it in the same column.
    if (focusRequest.scroll) window.setTimeout(() => {
      const field = rowsBox.current?.querySelector<HTMLElement>(`[data-question="${CSS.escape(id)}"]`);
      field?.scrollIntoView({ block: 'center' });
      field?.querySelector<HTMLElement>('input, select')?.focus({ preventScroll: true });
    }, 50);
  }, [focusRequest]);

  // The picker lives as long as the session; React state reaches it through refs.
  const handlers = useRef({ face: (_face: number) => {}, label: (_label: PmiLabel) => {} });
  const cad = useCadViewer(hostView);
  useEffect(() => {
    if (!stage.current || !cad.ready) return undefined;
    const entities = session.provenance?.face_entities;
    const created = createReviewView(stage.current, {
      mesh: session.mesh, analysis: session.analysis, view: hostView, part: session.provenance?.part ?? 0,
      ...(entities ? { entities } : {}),
    });
    created.on('change', setSelection);
    created.on('face', (face) => handlers.current.face(face));
    created.on('label', (label) => handlers.current.label(label));
    created.on('hover', (face) => {
      if (face === null) return setStatus('');
      const f = faces.get(face);
      const size = f?.radius ? ` Ø${Number((2 * f.radius).toFixed(3))}` : '';
      setStatus(`face ${face} · ${f?.kind ?? ''}${size}`);
    });
    picker.current = created;
    return () => {
      created.dispose();
      picker.current = null;
    };
  }, [session, faces, hostView, cad.ready]);

  useEffect(() => { picker.current?.tint(tints ?? new Map()); }, [tints, session, faces]);
  // The view's tree lists the rows by feature, as the panel does.
  useEffect(() => {
    picker.current?.features?.(groups.filter((g) => g.key !== 'part' && g.key !== 'datums').map((g) => ({
      title: g.title, faces: [...new Set(g.questions.flatMap((q) => q.faces))],
    })));
  }, [groups, session]);

  useEffect(() => {
    if (!showFaces) return;
    // Lit, not turned to: selecting on the drawing, often to drag, should not swing the part round.
    // Nothing selected there gives the part back to the open question.
    picker.current?.highlight(showFaces.faces.length ? showFaces.faces : currentQuestion?.faces ?? []);
  }, [showFaces?.n]);

  // Point the picker at the current question -- only when the question changes:
  // a pick must not be reset by its own answer. Turning to it shows what is asked.
  useEffect(() => {
    const p = picker.current;
    if (!p) return;
    p.highlight(currentQuestion?.faces && calloutFaces.current?.id === currentQuestion.id
      ? calloutFaces.current.faces : currentQuestion?.faces ?? []);
    calloutFaces.current = null;
    // A call-out's faces are the dimension's: a click chooses another, not other faces for it.
    p.setPickable(!readOnly && currentQuestion?.kind === 'faces' && !draft?.callOut);
    // A datum's answer is its faces; an added requirement's holds them.
    const given = currentQuestion?.kind === 'faces' ? answerOf(currentQuestion, answers) : [];
    const value = Array.isArray(given) ? given as number[]
      : given && typeof given === 'object' && Array.isArray((given as { faces?: unknown }).faces)
        ? (given as { faces: number[] }).faces : [];
    p.setSelection(value);
    setSelection(value);
    setMode('face');
    p.setMode('face');
    // The part opens at home; from then on it turns to each thing asked about.
    if (turned.current && currentQuestion?.faces.length && !stayPut.current) p.lookAt(currentQuestion.faces);
    stayPut.current = false;
    if (currentQuestion) turned.current = true;
  }, [currentQuestion?.id]);
  // A face clicked that no question is about is lit, as what its general dimensions are on.
  useEffect(() => {
    if (peek !== null && !currentQuestion) picker.current?.highlight([peek]);
  }, [peek, currentQuestion?.id]);

  useEffect(() => {
    if (!keepCalloutNote.current) { setNote(''); setNoteGeneral(null); }
    keepCalloutNote.current = false;
  }, [current, view]);

  // The general dimensions chosen -- in the table, or on a face clicked -- drawn though the rest are not.
  const reveal = useMemo(() => {
    const ids = new Set<string>();
    for (const r of chars) {
      if (r.source.startsWith('general:') && selectedChar !== null && (r.id === selectedChar || `${r.n}` === selectedChar)) {
        ids.add(r.source);
      }
    }
    (preview?.general ?? []).forEach((g, i) => {
      const reference = Array.isArray(g['reference']) ? g['reference'] as number[] : [];
      if (peek !== null && (g.faces.includes(peek) || reference.includes(peek))) ids.add(`general:${i}`);
    });
    return ids;
  }, [chars, selectedChar, preview, peek]);

  // Read only shows what is set: not the defaults to check.
  const seen = useMemo(() => (readOnly ? { ...show, defaults: false } : show), [show, readOnly]);
  // The Show menu's balloons switch governs the drawn callouts and the fallback labels alike.
  const oneRows = useMemo<readonly OneViewRow[]>(() => {
    if (oneView) return oneView.rows;
    const rows: OneViewRow[] = [
      ...questions.map((q): OneViewRow => ({
        id: q.id,
        state: statedRows && q.id in statedRows.values
          ? (statedRows.decided?.has(q.id) ? 'decided_none' : 'stated') : 'gap',
        open: Boolean(needs(q, answers)),
        faces: q.faces,
        label: q.label ?? q.prompt,
        question: q,
      })),
      ...(otherPmi?.items ?? []).map((item): OneViewRow => ({
        id: item.key, state: 'stated', open: false, faces: item.faces, label: item.label, other: true,
      })),
    ];
    if (readOnly) {
      const have = new Set(rows.map((r) => r.id));
      const push = (row: OneViewRow) => { if (!have.has(row.id)) { have.add(row.id); rows.push(row); } };
      for (const d of preview?.datums ?? []) {
        push({ id: `datum.${d.letter}`, state: 'stated', open: false, faces: d.faces, label: `Datum ${d.letter}` });
      }
      session.analysis.existing.forEach((item, i) => {
        const faces = item.faces ?? [];
        if (item.kind === 'datum') {
          push({ id: `datum.${item.type}`, state: 'stated', open: false, faces, label: `Datum ${item.type}` });
        }
        push({ id: `existing:${i}`, state: 'stated', open: false, faces, label: item.text ?? item.type });
      });
      for (const r of chars) {
        if (r.question) push({ id: r.question, state: 'stated', open: false, faces: r.faces, label: r.text });
        push({ id: r.source, state: 'stated', open: false, faces: r.faces, label: r.text });
      }
    }
    return rows;
  }, [oneView, questions, statedRows, answers, otherPmi, readOnly, preview, session.analysis, chars]);
  const onePending = oneView?.pending ?? EMPTY_ONE_PENDING;
  const normalizedDrawn = useMemo(() => {
    if (!drawnPmi) return null;
    const cleaned = withoutNotesBlock(drawnPmi);
    if (cleaned.draftingNormalized) return cleaned;
    const sem = sceneSemantic ?? (cleaned.sceneSemantic ? new Map(Object.entries(cleaned.sceneSemantic)) : undefined);
    return normalizeDraftingPmi(cleaned, null, session.analysis, session.mesh, sem);
  }, [drawnPmi, sceneSemantic, session.analysis, session.mesh]);
  const resolvedOrigin = useMemo(
    () => partOrigin ?? resolveStatedOrigin(drawnPmi?.stepOrigin ?? session.mesh.origin ?? null, session.analysis, normalizedDrawn),
    [partOrigin, drawnPmi?.stepOrigin, session.mesh.origin, session.analysis, normalizedDrawn],
  );
  const oneFile = useMemo(
    () => oneViewFile(normalizedDrawn, session.analysis, oneRows, onePending),
    [normalizedDrawn, session.analysis, oneRows, onePending],
  );
  const openQuestions = useMemo(
    () => new Set(readOnly ? [] : questions.filter((q) => needs(q, answers)).map((q) => q.id)),
    [readOnly, questions, answers],
  );
  const oneLabels = useMemo(
    () => oneViewLabels(preview, session.analysis, oneRows, oneFile, onePending, answers, openQuestions, readOnly),
    [preview, session.analysis, oneRows, oneFile, onePending, answers, openQuestions, readOnly],
  );
  const savedNumbers = useMemo(() => new Set((session.analysis.characteristics ?? []).map((c) => c.number)), [session.analysis]);
  const { unsavedItems, coveredQuestions } = useMemo(
    () => scenePendingState(oneFile, onePending, savedNumbers),
    [oneFile, onePending, savedNumbers],
  );
  // Mark open unanswered gap items on 3D planes as 'decided_none' (group 'default') so they render
  // in muted blueprint-blue before Keep all and switch to dark blue (or burnt amber while unsaved).
  const visibleDrawn = useMemo(() => {
    const base = oneFile.drawn;
    if (!base) return null;
    const rowsById = new Map(oneRows.map((row) => [row.id, row]));
    const links: Record<string, (typeof base.links)[string]> = {};
    for (const [item, link] of Object.entries(base.links)) {
      const q = link.question;
      const row = q ? rowsById.get(q) : undefined;
      const edit = q ? onePending.get(q) : undefined;
      const isPending = Boolean(edit && (edit.kind === 'fill' || edit.kind === 'change' || edit.kind === 'clear'));
      const isOpenGap = !readOnly && !isPending && !link.general && Boolean(row && row.state === 'gap' && row.open && (!edit || edit.kind === 'refused'));
      const origStatus = normalizedDrawn?.links[item]?.status;
      const isUnsetDefault = readOnly && !link.general && origStatus !== undefined && origStatus !== 'stated' && (!row || row.state === 'gap');
      links[item] = isOpenGap || isUnsetDefault ? { ...link, status: 'decided_none' } : link;
    }
    return { ...base, links };
  }, [oneFile, oneRows, onePending, readOnly, normalizedDrawn]);
  const drawnBalloonNumbers = useMemo(
    () => (seen.balloons ? drawnBalloons(oneFile.drawn) : new Map<string, number[]>()),
    [seen.balloons, oneFile],
  );
  // Numbers the saved file does not have yet: the engine's plan for what Save will write.
  const plannedNumbers = useMemo(() => {
    const saved = new Set((session.analysis.characteristics ?? []).map((c) => c.number));
    return new Set([...chars.filter((c) => c.n && !saved.has(c.n)).map((c) => c.n), ...calledOutNumbers?.values() ?? []]);
  }, [chars, session.analysis, calledOutNumbers]);
  const drawnReveal = useMemo(() => {
    const items = new Set<string>();
    for (const [item, label] of oneFile.labels) {
      if (reveal.has(`general:${label.id}`) || reveal.has(label.id)) items.add(item);
    }
    return items;
  }, [oneFile, reveal]);
  // Only items not represented on 3D annotation planes (unpresented file requirements, pending
  // changes to stated rows, real unpreviewed gaps, or hosts without a 3D scene) use fallback labels.
  const fallbackLabels = useMemo(() => {
    if (!visibleDrawn) return oneLabels.labels;
    return oneLabels.labels.filter((l) =>
      l.id.startsWith('gap:')
      || (l.id.startsWith('pending:') && (!l.question || !coveredQuestions.has(l.question)))
      || (l.group === 'existing' && (!l.question || !coveredQuestions.has(l.question))),
    );
  }, [visibleDrawn, oneLabels, coveredQuestions]);
  // Labels that stand for a question (a pending call-out) carry its characteristics' numbers.
  const balloons = useMemo(() => {
    const out = new Map<string, number[]>();
    if (!seen.balloons) return out;
    for (const [source, numbers] of allBalloons) out.set(source, numbers);
    for (const label of fallbackLabels) {
      const key = label.id.split(':').slice(0, 2).join(':');
      if (out.has(key) || !label.question) continue;
      // The engine's number for the drawn callout that answers it, else its characteristics'.
      const linked = Object.values(oneFile.drawn?.links ?? {})
        .filter((l) => l.number && l.question === label.question).map((l) => l.number!);
      const numbers = linked.length ? [...new Set(linked)] : [...new Set(chars.filter((c) => c.n && c.question === label.question).map((c) => c.n))];
      if (numbers.length) out.set(key, numbers);
    }
    return out;
  }, [seen.balloons, allBalloons, fallbackLabels, chars, oneFile]);
  useEffect(() => {
    picker.current?.annotate(preview, {
      current: readOnly ? null : selectedCallout ?? current,
      show: { default: seen.defaults, existing: seen.existing, general: seen.general }, balloons,
      pending: openQuestions, reveal, datums,
      labels: fallbackLabels,
    });
    picker.current?.setFarSideHidden(seen.farSide);
  }, [preview, current, selectedCallout, seen, balloons, openQuestions, reveal, datums, fallbackLabels, readOnly]);
  useEffect(() => {
    picker.current?.drawn?.(visibleDrawn, true, oneFile.labels, drawnBalloonNumbers);
  }, [visibleDrawn, session, oneFile, drawnBalloonNumbers]);
  useEffect(() => {
    picker.current?.setDrawnUnsaved?.(unsavedItems);
  }, [unsavedItems, visibleDrawn, session]);
  useEffect(() => {
    picker.current?.setDrawnOrigin?.(resolvedOrigin);
  }, [resolvedOrigin, session]);
  const hasUnsavedInView = Boolean(unsavedItems.size)
    || Boolean(oneLabels.notes.length)
    || fallbackLabels.some((l) => l.unsaved);
  // The show toggles' kinds and far side, for the drawn PMI as for the fallback labels.
  const drawnShow = useMemo(() => ({ default: seen.defaults, existing: seen.existing, general: seen.general }), [seen]);
  useEffect(() => {
    picker.current?.drawnFilter?.(drawnShow, seen.farSide, drawnReveal);
  }, [drawnShow, seen.farSide, visibleDrawn, session, drawnReveal]);
  // Ways to look square at the drawn PMI shown: the file's saved views, and each plane's front.
  const squares = useMemo(() => (visibleDrawn
    ? drawnViews(visibleDrawn, (g, item) => drawnShown(g.source, item === null ? undefined : drawnGroup(visibleDrawn.links[item]), drawnShow), partCentre(session.mesh.positions))
    : []), [visibleDrawn, drawnShow, session]);
  const bestView = useMemo(() => (visibleDrawn
    ? bestDrawnView(
      visibleDrawn,
      (g, item) => drawnShown(g.source, item === null ? undefined : drawnGroup(visibleDrawn.links[item]), drawnShow),
      partCentre(session.mesh.positions),
      bestReferenceView(visibleDrawn, session.analysis, session.mesh),
    )
    : null), [visibleDrawn, drawnShow, session]);
  const { primary: primaryViews, overflow: overflowViews } = useMemo(
    () => splitDrawnViews(bestView, squares), [bestView, squares],
  );
  useEffect(() => {
    if (overflowViews.length === 0) setMoreViewsOpen(false);
  }, [overflowViews.length]);
  useEffect(() => {
    if (!moreViewsOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!viewBarRef.current?.contains(event.target as Node)) setMoreViewsOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setMoreViewsOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [moreViewsOpen]);
  useEffect(() => { picker.current?.setTools?.(moreTools); }, [moreTools, session]);
  useEffect(() => {
    picker.current?.setDefaultView?.(bestView);
  }, [bestView, session]);
  useEffect(() => {
    picker.current?.setTransparency(transparency / 100);
  }, [transparency, session]);

  const previewing = useRef(0);
  const committedAnswers = useRef(answers);
  const committing = useRef(Promise.resolve(false));
  const sessionId = useRef(session.sessionId);
  const live = useCallback(<T,>(call: (id: string) => Promise<T>): Promise<T> => call(sessionId.current), []);
  // Global project undo/redo and reopen feed the original UI's local review state.
  useEffect(() => {
    ++previewing.current; // An older preview must not overwrite a host undo/redo.
    committedAnswers.current = externalAnswers;
    setAnswers(externalAnswers);
    setQuestions(externalReview.questions);
    setProblems(externalReview.problems);
    setPreview(externalReview.intent);
    setTyped((typed) => Object.fromEntries(Object.entries(typed).filter(([id, read]) => externalAnswers[id] === read.value)));
  }, [externalAnswers, externalReview]);

  // Previews answer in any order; only the latest asked for is shown.
  const relabel = useCallback(async (next: PmiAnswers) => {
    const mine = ++previewing.current;
    try {
      // Keep display traffic on the original preview contract. Inspection is lazy below.
      const shown = await live((id) => engine.preview(id, next));
      if (mine === previewing.current) setPreview(shown);
    } catch {
      // Labels are a view: a failed refresh keeps the last ones.
    }
  }, [engine, live]);

  // Once per part opened -- never again on a re-render, which would throw away
  // what has been answered since. Reached through ``startWith``, set below.
  const startWith = useRef<{ commit: typeof commit; relabel: typeof relabel } | null>(null);
  useEffect(() => {
    const start = startWith.current;
    if (!start) return;
    // A STEP Specify wrote carries on from the answers it was written with.
    const saved = session.answers ?? session.analysis.resume?.answers;
    if (readOnly) {
      // Nothing is asked: the answers it was saved with are only read, to judge what is set.
      setCurrent(null);
      if (saved && Object.keys(saved).length > 0) setAnswers(saved);
      void start.relabel(saved ?? {});
      return;
    }
    if (saved && Object.keys(saved).length > 0) {
      setResumed(true);
      void start.commit(null, () => saved, (asked, next) =>
        nextNeeding(asked.filter((g) => g.key !== 'part'), next));
      return;
    }
    // The part's own fields are always in view; start at the first thing on the part.
    const first = groupQuestions(session.questions).filter((g) => g.key !== 'part');
    setCurrent(nextNeeding(first, {}) ?? first[0]?.questions[0]?.id ?? null);
    void start.relabel({});
  }, [session]);

  // pmi-assist decides what follows from the answers, and may refuse them.
  // ``focus`` is the question to show afterwards; by default the one answered
  // stays open, so a choice is seen to have been made.
  const commit = useCallback((
    at: string | null, update: (answers: PmiAnswers) => PmiAnswers,
    focus: (asked: QuestionGroup[], next: PmiAnswers) => string | null,
  ) => {
    if (readOnly) return Promise.resolve(false);
    // The host can save answers in questions(). Apply each edit to the last accepted
    // answers, even before React has rendered them.
    const pending = committing.current.then(async () => {
      try {
        const next = update(committedAnswers.current);
        const asked = await live((id) => engine.questions(id, next));
        committedAnswers.current = next;
        setError(null);
        setAnswers(next);
        setQuestions(asked.questions);
        setProblems(asked.problems ?? []);
        setCurrent(focus(groupQuestions(asked.questions), next));
        void relabel(next);
        return true;
      } catch (failure) {
        setError({ at, problem: asError(failure) });
        return false;
      }
    });
    committing.current = pending;
    return pending;
  }, [engine, relabel, live]);
  startWith.current = { commit, relabel };

  const answer = (q: PmiQuestion, value: unknown, remove = false) => {
    if (adding && q.id === adding.id) setAdding(null);
    return commit(q.id, (latest) => {
      const next = { ...latest };
      if (remove) delete next[q.id];
      else next[q.id] = value;
      return next;
    }, (asked, next) => {
      if (q.id.startsWith('part.')) return current;
      // A face pick takes the whole box; once used it closes, and what still
      // needs a person opens -- or nothing, and the row shrinks to its summary.
      // An answer folds its row to its summary; what still needs a person opens.
      return nextNeeding(asked.filter((g) => g.key !== 'part'), next);
    });
  };

  // A typed spec: pmi-assist reads it against the feature, and it is answered
  // as what it means; a refusal comes back as the reason, beside the box.
  const spec = async (q: PmiQuestion, text: string): Promise<string | null> => {
    if (readOnly) return null;
    try {
      const read = await live((id) => engine.interpret(id, answers, q.id, text));
      setTyped((t) => ({ ...t, [q.id]: read }));
      answer(q, read.value);
      return null;
    } catch (failure) {
      return asError(failure).message;
    }
  };

  const chips = useMemo(() => datumChips(preview, session.analysis), [preview, session.analysis]);
  const nextLetter = freeLetter(chips);
  // Read only: the datums and characteristics that are set, not defaults to check. What the general
  // tolerance covers stays, folded under it as in the table: it is set by that tolerance.
  const setChips = readOnly
    ? chips.filter((c) => c.existing || preview?.datums.find((d) => d.letter === c.letter)?.default !== true) : [];
  const setRows = readOnly ? chars.filter((r) => {
    if (r.source.startsWith('datum:')) return true;
    if (r.source.startsWith('req:')) return preview?.requirements[Number(r.source.slice('req:'.length))]?.default !== true;
    if (r.source.startsWith('existing:') || r.source.startsWith('general:')) return true;
    if (r.source.startsWith('part:')) return partSet(r.source.slice('part:'.length));
    return false;
  }) : [];
  const [selectedDatum, setSelectedDatum] = useState<string | null>(null);

  // Adding a datum is picking it: the letter's question if pmi-assist already
  // asks it (one removed earlier), else a pick it learns of once answered.
  const addDatum = () => {
    if (readOnly || !nextLetter) return;
    const id = `datum.${nextLetter}`;
    if (!questions.some((q) => q.id === id)) {
      setAdding({
        id, kind: 'faces', faces: [], default: [], attention: 'routine',
        prompt: `Pick datum ${nextLetter} on the part: a face, coplanar faces, or a hole or boss.`,
        why: 'Tolerances can then be measured from it.',
      });
    }
    setCurrent(id);
  };

  // Added requirements: picked, then saved as an answer pmi-assist checks.
  const added = addedOf(answers);
  const startExtra = () => {
    if (readOnly) return;
    const key = freeKey();
    setDraft({ key, kind: '', value: '', datums: [], reference: [] });
    setCurrent(key);
  };
  // A dimension the general tolerance covers, called out to be toleranced apart:
  // its kind, faces and reference given, its tolerance to say.
  const callOut = (i: number, what: string, value = '') => {
    // 0.3: a general dimension is called out on the dimensions row (change list item 1).
    if (callOutDimension) {
      const row = chars.find((r) => r.source === `general:${i}`);
      if (!row) return;
      setNoteGeneral(null);
      switchView('review'); setUnfolded('dims'); setCallingOut(row.id); showChar(row.id, row.faces);
      return;
    }
    if (readOnly) return;
    const g = preview?.general?.[i];
    if (!g) return;
    const key = freeKey();
    const reference = Array.isArray(g['reference']) ? g['reference'] as number[] : [];
    setDraft({
      key, kind: g.kind === 'size' ? 'size' : 'distance', value, datums: [], reference, faces: g.faces,
      callOut: what,
    });
    setNoteGeneral(null);
    if (view === 'table') switchView('review');
    setCurrent(key);
    // Its key may be an open draft's, which turning to it would not reset: its faces are set here.
    picker.current?.setPickable(false);
    picker.current?.setSelection([...g.faces]);
    setSelection([...g.faces]);
  };
  // Called out where it was clicked, with one of the usual tolerances: no editor,
  // no scrolling. One pmi-assist refuses opens in the editor, to say why.
  const quickCallOut = async (i: number, what: string, value: string) => {
    const g = preview?.general?.[i];
    if (!g) return;
    // 0.3: through the host, as the dimensions row calls out (one key, shown on its line).
    if (callOutDimension) {
      const row = chars.find((r) => r.source === `general:${i}`);
      if (!row) return;
      const ok = callOutDimension({
        id: row.id, what: `${row.feature} ${row.text}`, kind: g.kind === 'size' ? 'size' : 'distance', faces: g.faces,
        reference: Array.isArray(g['reference']) ? g['reference'] as number[] : [],
      }, value);
      setNoteGeneral(null);
      if (ok) setNote(`Called out ${what} ${value}.`);
      return;
    }
    const key = freeKey();
    const reference = Array.isArray(g['reference']) ? g['reference'] as number[] : [];
    const extra = g.kind === 'size'
      ? { faces: [...g.faces], kind: 'size' as const, value }
      : { faces: [...g.faces], kind: 'distance' as const, value, reference };
    setNoteGeneral(null);
    if (await commit(key, (latest) => ({ ...latest, [key]: extra }), () => null)) {
      setNote(`Called out ${what} ${value}.`);
    } else {
      callOut(i, what, value);
    }
  };
  const pickFrom = () => {
    if (!draft) return;
    setDraft({ ...draft, reference: selection });
    picker.current?.setSelection([]);
    setSelection([]);
  };
  const editExtra = (key: string) => {
    if (readOnly) return;
    const extra = added.find(([k]) => k === key)?.[1];
    if (!extra) return;
    setDraft({ key, kind: extra.kind, value: extra.value, datums: extra.datums ?? [], reference: extra.reference ?? [] });
    setCurrent(key);
  };
  const saveExtra = () => {
    if (!draft || !draft.kind) return;
    const oriented = MAY_HAVE_DATUM.includes(draft.kind) && draft.datums.length > 0;
    const extra = {
      // A call-out keeps its dimension's faces, whatever is lit on the part.
      faces: draft.callOut && draft.faces ? [...draft.faces] : selection, kind: draft.kind, value: draft.value.trim(), ...(oriented ? { datums: draft.datums } : {}),
      ...(draft.kind === 'distance' ? { reference: draft.reference } : {}),
    };
    // A new datum beyond C has no proposal: its question opens to pick it.
    const unpicked = (extra.datums ?? []).find((x) => !'ABC'.includes(x) && !chips.some((c) => c.letter === x));
    void commit(draft.key, (latest) => ({ ...latest, [draft.key]: extra }), () => {
      setDraft(null);
      return unpicked ? `datum.${unpicked}` : null;
    });
  };
  const removeExtra = (key: string) => {
    void commit(key, (latest) => {
      const next = { ...latest };
      delete next[key];
      return next;
    }, () => { setDraft(null); return null; });
  };

  // What pmi-assist chose, to keep (answered as it stands) or change (opened).
  const chosen = toReview ? chosenForYou(questions, answers, session.analysis, toReview).filter((c) => toReview.includes(c.id))
    : chosenForYou(questions, answers, session.analysis);
  const chosenById = new Map(chosen.map((c) => [c.id, c]));
  const applied = appliedByRule(preview);
  const card = (c: Chosen) => (
    <SuggestionCard
      key={c.id} c={c} onKeep={() => keepChosen([c.id])} onPick={(value) => pickChosen(c.id, value)}
      onMore={() => changeChosen(c.id)}
    />
  );
  // Each problem shown in the row it is about.
  const rowOf = (about: string) => (about.startsWith('extra.') ? 'added'
    : about.startsWith('datum.') ? 'datums'
      : groups.find((g) => g.questions.some((q) => q.id === about))?.key ?? null);
  const problemsIn = (key: string) => problems.filter((p) => rowOf(p.about) === key);
  const generalProblems = problems.filter((p) => rowOf(p.about) === null);
  const problemLines = (key: string) => problemsIn(key).map((p) => (
    <button
      key={`${p.about}:${p.message}`} type="button" className={`problem ${p.severity}`}
      onClick={() => openProblem(p.about, p.faces)}
    >
      {p.message}
    </button>
  ));
  const keepChosen = (ids: readonly string[]) => {
    if (readOnly) return;
    if (onKeep) { onKeep(ids); return; }
    const keptAnswers: Record<string, unknown> = {};
    for (const id of ids) {
      const q = questions.find((x) => x.id === id);
      if (q) keptAnswers[id] = q.default;
    }
    void commit(ids[0] ?? null, (latest) => ({ ...latest, ...keptAnswers }), (asked, next) => settled(ids, asked, next));
  };
  // After keeping or choosing: the open question stays open unless it was the
  // one settled, which folds, and what still needs a person opens.
  const settled = (ids: readonly string[], asked: QuestionGroup[], next: PmiAnswers) =>
    (current && ids.includes(current) ? nextNeeding(asked.filter((x) => x.key !== 'part'), next) : current);
  // Choosing an alternative answers it there and then; it leaves the list.
  const pickChosen = (id: string, value: string) => {
    void commit(id, (latest) => ({ ...latest, [id]: value }), (asked, next) => settled([id], asked, next));
  };
  // Opens the question in full and brings it into view -- every time, even
  // when it is already the open one, or the tap would seem to do nothing.
  const changeChosen = (id: string) => {
    if (id.startsWith('part.')) {
      setUnfolded(NOTE_FIELDS.has(id) ? `note:${id}` : id);
      rowsBox.current?.scrollTo({ top: 0, behavior: 'smooth' });
      return;
    }
    setCurrent(id);
    window.setTimeout(() => {
      rowsBox.current?.querySelector('.pmi-lab-item.on')?.scrollIntoView({ block: 'center', behavior: 'smooth' });
    }, 60);
  };

  // A problem opens what it is about and turns to it.
  const openProblem = (about: string, faces: readonly number[]) => {
    if (about.startsWith('extra.')) editExtra(about);
    else if (questions.some((q) => q.id === about)) setCurrent(about);
    if (faces.length) picker.current?.lookAt(faces);
  };

  const removeDatum = (letter: string) => {
    const { warning } = removal(letter, chips, answers, preview);
    if (warning && !window.confirm(`${warning}\n\nRemove datum ${letter}?`)) return;
    void commit(`datum.${letter}`, (latest) => removal(letter, chips, latest, preview).answers, (asked) =>
      (asked.some((g) => g.questions.some((x) => x.id === current)) ? current : null));
  };

  // "Looks right" on a row: its checks are accepted and the next row needing a person opens.
  const confirm = (g: QuestionGroup) => {
    // Confirmed, the row folds; what still needs a person opens, if anything.
    void commit(g.questions[0]?.id ?? null, (latest) => confirmRow(g, latest), (asked, next) =>
      nextNeeding(asked.filter((x) => x.key !== 'part'), next));
  };

  // What the general tolerance covers, one line per dimension (its first instance).
  const generalDims = byBalloon(chars.filter((r) => r.source.startsWith('general:'))).flatMap((g) => g.slice(0, 1));
  const lightFor = (id: string | null) => {
    const match = id ? [...oneFile.labels.values()].find((label) => label.question === id) : undefined;
    picker.current?.lightDrawn?.(match?.sceneItem ?? null);
  };
  const selectCalloutRow = (id: string, kind: 'question' | 'other' | 'characteristic' = 'question') => {
    selectCallout(id);
    const attr = kind === 'other' ? 'data-other' : kind === 'characteristic' ? 'data-characteristic' : 'data-question';
    window.requestAnimationFrame(() => {
      rowsBox.current?.querySelector(`[${attr}="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'nearest' });
    });
  };
  const openQuestionRow = (id: string) => {
    selectCallout(id);
    lightFor(id);
    setCurrent(id);
  };

  handlers.current = {
    face: (face) => {
      selectCallout(null);
      picker.current?.lightDrawn?.(null);
      if (readOnly) {
        picker.current?.highlight([face]);
        const row = setRows.find((r) => r.faces.includes(face));
        setSelectedDatum(null);
        setSelectedChar(row ? (row.n ? `${row.n}` : row.id) : null);
        onPick?.([face]);
        return;
      }
      // A call-out not yet given a tolerance gives way to whatever is clicked next.
      if (draft?.callOut && !draft.value.trim()) {
        setDraft(null);
        setCurrent(null);
      } else if (currentQuestion?.kind === 'faces') return;
      const id = questionForFace(groups, face);
      setPeek(id ? null : face);
      setNote('');
      setNoteGeneral(null);
      if (id) setCurrent(id);
      onPick?.([face]);
    },
    // A callout clicked is already in sight: the view stays put, its faces (for a dimension,
    // both ends) and drawn ink are lit, and its row in the panel is opened/selected.
    label: (label) => {
      const on = [...label.faces, ...(label.span?.reference ?? [])];
      picker.current?.highlight(on);
      picker.current?.lightDrawn?.(label.sceneItem ?? null);
      onPick?.(label.faces);
      const id = label.question;
      const charList = readOnly ? setRows : chars;
      const sourcePrefix = label.id.split(':').slice(0, 2).join(':');
      const characteristic = charList.find((row) =>
        row.source === label.sceneItem
        || row.source === label.id
        || row.source === `general:${label.id}`
        || row.source === sourcePrefix);
      const datum = id?.startsWith('datum.') ? id.slice(6) : label.kind === 'datum' ? label.cells[0] : undefined;
      if (readOnly && datum && setChips.some((chip) => chip.letter === datum)) {
        setSelectedDatum(datum);
        setSelectedChar(null);
        selectCallout(null);
        return;
      }
      if ((readOnly || view === 'table') && characteristic) {
        setSelectedChar(characteristic.n ? `${characteristic.n}` : characteristic.id);
        setSelectedDatum(null);
        selectCallout(null);
        return;
      }
      if (id) {
        if (readOnly) {
          selectCalloutRow(id, 'question');
          setSelectedChar(null);
          setSelectedDatum(null);
          return;
        }
        setView('review');
        setOnlySuggestions(false);
        if (id.startsWith('extra.') && added.some(([k]) => k === id)) {
          stayPut.current = true;
          calloutFaces.current = { id, faces: on };
          selectCalloutRow(id, 'question');
          editExtra(id);
          return;
        }
        if (questions.some((q) => q.id === id)) {
          if (id !== current) {
            stayPut.current = true;
            calloutFaces.current = { id, faces: on };
          }
          selectCalloutRow(id, 'question');
          setCurrent(id);
          if (id.startsWith('part.')) setUnfolded(NOTE_FIELDS.has(id) ? `note:${id}` : id);
          else if (id.startsWith('datum.')) setUnfolded('datums');
          return;
        }
        if (otherPmi?.items.some((item) => item.key === id)) {
          setUnfolded('other');
          selectCalloutRow(id, 'other');
          return;
        }
        const dimRow = generalDims.find((r) => r.source === `general:${label.id}` || r.source === label.id);
        if (callOutDimension && dimRow) {
          setUnfolded('dims');
          setSelectedChar(dimRow.id);
          selectCalloutRow(dimRow.id, 'characteristic');
          return;
        }
        selectCalloutRow(id, 'question');
        return;
      }
      if (label.group === 'general' && !readOnly) {
        if (view === 'table') keepCalloutNote.current = true;
        setView('review');
        setOnlySuggestions(false);
        const general = chars.find((r) => r.source === 'part:general_tolerance')?.text ?? 'General tolerance';
        const tol = label.tolerance ? `, ${label.tolerance.upper}` : '';
        setNote(`${general}${tol} on ${label.cells[0] ?? 'this'}.`);
        const i = Number(label.id.split(':')[1]);
        const row = chars.find((r) => r.source === `general:${i}`);
        setNoteGeneral(Number.isInteger(i) ? { i, what: row?.text ?? label.cells[0] ?? '' } : null);
        if (callOutDimension && row) {
          setUnfolded('dims');
          setSelectedChar(row.id);
          selectCalloutRow(row.id, 'characteristic');
        }
      }
    },
  };

  const here = rows.find((g) => g.questions.some((q) => q.id === current));
  const errorAt = (id: string) => (error?.at === id ? error.problem : null);
  const shown = new Set(questions.map((q) => q.id));
  const looseError = error && (error.at === null || !shown.has(error.at)) ? error.problem : null;

  // Keep the open row in view as it moves -- but not at the start, when the
  // part's own fields, above the rows, come first.
  const scrolled = useRef(false);
  useEffect(() => {
    if (!here) return;
    if (scrolled.current && quiet.current !== current) rowsBox.current?.querySelector('.pmi-lab-row.current')?.scrollIntoView({ block: 'nearest' });
    scrolled.current = true;
  }, [here?.key]);
  // An added requirement or call-out opened is brought into sight, wherever it was opened from.
  useEffect(() => {
    if (draft && quiet.current !== draft.key) rowsBox.current?.querySelector('.pmi-lab-extra')?.scrollIntoView?.({ block: 'nearest' });
  }, [draft?.key]);
  // Quiet lasts one visit: once the person moves on, rows are followed again.
  useEffect(() => {
    if (quiet.current !== current && quiet.current !== draft?.key) quiet.current = null;
  }, [current, draft?.key]);

  // The balloon numbers of what a row specifies, beside its title.
  const rowBalloons = (g: QuestionGroup) => {
    const numbers = chars.filter((c) => c.n && g.questions.some((q) => q.id === c.question)).map((c) => c.n);
    return numbers.length ? <span className="bal" title="Balloon numbers">{ranges(numbers)}</span> : null;
  };
  const skip = (keys: readonly string[], out: boolean) => setSkipped((was) => {
    const next = new Set(was);
    for (const k of keys) if (out) next.add(k); else next.delete(k);
    return next;
  });
  // Where a characteristic's PMI is changed: its question, or for a part's
  // note, the part's. What the general tolerance covers has none of its own.
  const editChar = (row: Characteristic) => {
    const id = row.question ?? (row.source.startsWith('part:') ? `part.${row.source.slice(5)}` : null);
    if (!id || !(id.startsWith('extra.') || questions.some((q) => q.id === id))) return null;
    return () => {
      switchView('review');
      if (id.startsWith('extra.')) editExtra(id);
      else setCurrent(id);
    };
  };
  // A characteristic chosen in the table is shown on the part.
  const showChar = (id: string, faces: readonly number[]) => {
    setSelectedChar(id);
    selectCallout(id);
    const charRow = (readOnly ? setRows : chars).find((r) => r.id === id || `${r.n}` === id);
    const match = charRow ? [...oneFile.labels.values()].find((l) =>
      charRow.source === l.sceneItem || charRow.source === l.id || charRow.source === `general:${l.id}`) : undefined;
    picker.current?.lightDrawn?.(match?.sceneItem ?? null);
    picker.current?.highlight(faces);
    picker.current?.lookAt(faces);
  };
  const switchView = (next: 'review' | 'table') => {
    setView(next);
    if (next === 'review') picker.current?.highlight(currentQuestion?.faces ?? []);
  };
  // Read when the file is written: an export resumed after sign-in carries edits made meanwhile.
  const exportRows = { chars, skipped, titles, answers, questions, kept, toReview,
    notes: noteStatements(preview, session.analysis) };
  const csvRows = useRef(exportRows);
  csvRows.current = exportRows;
  const inspection = async () => {
    const { chars, skipped, titles, answers, questions, kept, toReview, notes } = csvRows.current;
    try {
      // Match the original three-method host: read the latest displayed rows AND notes
      // when a deferred download runs, including edits made while signing in.
      let intent: PmiIntent | null;
      if (inspectionIntent) intent = await inspectionIntent();
      else if (engine.inspection) intent = await engine.inspection(session.sessionId, inspectionAnswers(answers, questions, kept, toReview));
      else return { chars, skipped, notes };
      return intent && { chars: characteristics(intent, session.analysis, titles), skipped, notes: noteStatements(intent, session.analysis) };
    } catch (failure) {
      setError({ at: null, problem: asError(failure) });
      return null;
    }
  };
  const downloadCsv = () => onExport(async () => {
    const data = await inspection();
    if (!data) return;
    const stem = session.filename.replace(/\.(step|stp|p21)$/i, '');
    saveFile(new File([toCsv(data.chars, data.skipped)], `${stem}-characteristics.csv`, { type: 'text/csv' }));
    onSaved?.(`${stem}-characteristics.csv`);
  });
  // The same list as AS9102's Forms 1–3, for a first article inspection report.
  const downloadFai = () => onExport(async () => {
    const data = await inspection();
    if (!data) return;
    const stem = session.filename.replace(/\.(step|stp|p21)$/i, '');
    const book = xlsx(as9102({ name: stem }, data.chars, data.skipped, data.notes));
    saveFile(new File([book as BlobPart], `${stem}-AS9102.xlsx`, { type: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet' }));
    onSaved?.(`${stem}-AS9102.xlsx`);
  }, 'xlsx');

  // What the file states (0.3, change list item 2): its own pencil and bin, or removed with Undo.
  const statedOf = (id: string): StatedState | null => (statedRows && !readOnly && id in statedRows.values ? {
    removed: statedRows.removed.has(id),
    changed: JSON.stringify(answers[id]) !== JSON.stringify(statedRows.values[id]),
    onRemove: () => statedRows.onRemove(id), onRestore: () => statedRows.onRestore(id),
    decided: Boolean(statedRows.decided?.has(id)),
  } : null);
  // A row with something the file states: each stated item as the file states it, the rest as
  // the row's summary would say them; null when nothing in it is stated (the usual summary).
  const statedSummary = (g: QuestionGroup): string | null => {
    if (!statedRows?.texts || !g.questions.some((q) => q.id in statedRows.values)) return null;
    return g.questions.flatMap((q) => {
      if (statedRows.removed.has(q.id)) return [];
      if (q.id in statedRows.values && JSON.stringify(answers[q.id]) === JSON.stringify(statedRows.values[q.id])) return [statedRows.texts?.[q.id] ?? ''];
      return q.id in answers ? [inMm(q, answerOf(q, answers))] : [];
    }).filter(Boolean).join(' · ');
  };
  // A row: its head and summary, a check to confirm, and when open its questions.
  const questionItem = (q: PmiQuestion, g: QuestionGroup, titled = true) => {
    const st = statedOf(q.id);
    if (st && q.id !== current) {
      return (
        <div key={q.id} className={`pmi-lab-stated${st.removed ? ' removed' : ''}`} data-question={q.id} data-state={st.removed ? 'removed' : st.changed ? 'changed' : statedRows?.decided?.has(q.id) ? 'decided_none' : 'stated'} data-callout-selected={selectedCallout === q.id}>
          {st.removed
            ? <span className="pmi-lab-item"><span className="prompt">{labelName(q.label ?? q.prompt, lengthUnit)}</span><span className="value"><s>{statedRows?.texts?.[q.id] ?? inMm(q, statedRows!.values[q.id])}</s> removed</span></span>
            : <button type="button" className="pmi-lab-item" onClick={() => openQuestionRow(q.id)}>
                <span className="prompt">{labelName(q.label ?? q.prompt, lengthUnit)}</span>
                <span className="value">{st.changed ? inMm(q, answerOf(q, answers)) : statedRows?.texts?.[q.id] ?? inMm(q, answerOf(q, answers))} <small>{st.changed ? '(changed)' : statedRows?.decided?.has(q.id) ? '(decided)' : '(from the file)'}</small></span>
              </button>}
          <StatedTools name={q.label ?? q.prompt} state={st} onChange={() => openQuestionRow(q.id)} />
        </div>
      );
    }
    return questionItemOf(q, g, titled);
  };
  const questionItemOf = (q: PmiQuestion, g: QuestionGroup, titled = true) => (
    <QuestionItem titled={titled}
      key={`${q.id}:${JSON.stringify(answers[q.id])}`} q={q} answers={answers} active={q.id === current}
      selection={selection} mode={mode} error={errorAt(q.id)} typed={typed[q.id] ?? null}
      locked={readOnly} onSpec={(text) => spec(q, text)} resettable={!statedOf(q.id)}
      selected={selectedCallout === q.id}
      onOpen={() => openQuestionRow(q.id)}
      onAnswer={(value) => answer(q, value)}
      onConfirm={() => confirm(g)}
      onReset={() => answer(q, undefined, true)}
      onMode={(m) => { setMode(m); picker.current?.setMode(m); }}
      onDefaultSelection={() => {
        const value = Array.isArray(q.default) ? q.default as number[] : [];
        picker.current?.setSelection(value);
        setSelection(value);
      }}
    />
  );
  // A part setting folded to one line; what the file states with its pencil and bin.
  const partLine = (q: PmiQuestion, onOpen: () => void) => {
    const st = statedOf(q.id);
    const name = PART_FIELD[q.id] ?? q.prompt;
    if (!st) return <FoldedLine key={q.id} name={name} value={partValue(q, answers)} onOpen={onOpen} question={q.id} />;
    return (
      <div key={q.id} className={`pmi-lab-stated${st.removed ? ' removed' : ''}`} data-question={q.id} data-state={st.removed ? 'removed' : st.changed ? 'changed' : statedRows?.decided?.has(q.id) ? 'decided_none' : 'stated'} data-callout-selected={selectedCallout === q.id}>
        {st.removed
          ? <span className="pmi-lab-line"><span className="name">{name}</span><span className="value"><s>{partValue(q, statedRows!.values)}</s> removed</span></span>
          : <FoldedLine name={name} value={`${st.changed || !statedRows?.decided?.has(q.id) ? partValue(q, answers) : statedRows.texts?.[q.id] ?? partValue(q, answers)} ${st.changed ? '(changed)' : statedRows?.decided?.has(q.id) ? '(decided)' : '(from the file)'}`} onOpen={onOpen} />}
        <StatedTools name={name} state={st} onChange={onOpen} />
      </div>
    );
  };
  // ``extra`` goes under the head, open or not (the datums row's chips).
  // ``sum`` and ``onHead`` replace the row's own summary and open/close, for
  // rows that are more than their questions (the datums).
  const renderRow = (
    g: QuestionGroup, extra?: ReactNode, sum?: string, onHead?: () => void, unfoldedHere = false,
  ) => {
    // A doubted default the editor has recorded as reviewed is not "please check" any more.
    const own = rowState(g, answers);
    const state = own === 'check' && toReview && !g.questions.some((q) => toReview.includes(q.id)) ? 'ok' : own;
    const open = g === here;
    const expanded = open || unfoldedHere;
    const firstOpen = () => (g.questions.find((q) => needs(q, answers)) ?? g.questions[0])?.id ?? null;
    // Folded, a row shows its choices still standing as cards, to keep or change.
    const cards = open || readOnly ? [] : g.questions.flatMap((q) => chosenById.get(q.id) ?? []);
    const rowProblems = problemLines(g.key);
    if (onlySuggestions && !open && !cards.length && !rowProblems.length && state !== 'required') return null;
    return (
      <section key={g.key} className={`pmi-lab-row ${state}${open ? ' current' : ''}`}>
        <button
          type="button" className="pmi-lab-row-head" aria-expanded={expanded} data-questions={g.questions.map((q) => q.id).join(' ')}
          onClick={() => {
            if (!onHead) {
              const next = open ? null : firstOpen();
              selectCallout(next);
              lightFor(next);
              setCurrent(next);
            } else { if (open) setCurrent(null); onHead(); }
          }}
        >
          {/* Only what needs a person, or was changed, is marked. */}
          {state !== 'ok' && <span className="st">{STATE_TEXT[state]}</span>}
          <Chevron open={expanded} />
          <b>{labelName(g.title, lengthUnit)}</b>
          {rowBalloons(g)}
          <span className="sum">{typeof sum === 'string' ? withUnits(sum, 'mm') : sum ?? statedSummary(g) ?? withUnits(summary(g, answers), 'mm')}</span>
        </button>
        {rowProblems}
        {extra}
        {cards.length > 0 && <div className="pmi-lab-cards">{cards.map(card)}</div>}
        {/* Not twice: the current question shown in its queue row is answered there. */}
        {open && g.questions.filter((q) => !(slots.question && q.id === current)).map((q) => questionItem(q, g))}
        {g.key === 'datums' && adding && (
          <>
            <QuestionItem
        file={lengthUnit}
              q={adding} locked={readOnly} answers={answers} active selection={selection} mode={mode} error={null} typed={null}
              onSpec={async () => null} onOpen={() => {}} onAnswer={(value) => answer(adding, value)}
              onConfirm={() => {}} onReset={() => {}}
              onMode={(m) => { setMode(m); picker.current?.setMode(m); }}
              onDefaultSelection={() => {}}
            />
            <button type="button" className="pmi-lab-cancel" onClick={() => { setAdding(null); setCurrent(null); }}>
              Cancel
            </button>
          </>
        )}
      </section>
    );
  };

  const drawingAnnotations = oneLabels.extras.size > 0 && <>
    <h2 className="pmi-lab-rows-title">Drawing annotations</h2>
    <section className="pmi-lab-row">
      {[...oneLabels.extras].map(([id, label]) => (
        <div key={id} data-question={id} data-callout-selected={selectedCallout === id}>
          <button type="button" className="pmi-lab-line" onClick={() => {
            handlers.current.label(label);
            if (label.faces.length) picker.current?.lookAt(label.faces);
          }}>{label.cells[0]}</button>
        </div>
      ))}
    </section>
  </>;

  return (<>
    {createPortal(<div className="pmi-lab pmi-lab-pane">
      <div className="pmi-lab-view">
        <div className="pmi-lab-stage" ref={stage} />
        {cad.error && <p role="alert" className="pmi-lab-error">{cad.error}</p>}
        {visibleDrawn && primaryViews.length > 0 && (
          <div className={`pmi-lab-view-bar${moreTools ? ' with-tools' : ''}`} role="group" aria-label="Look square at" ref={viewBarRef}>
            <div className="pmi-lab-view-pills">
              {primaryViews.map((v) => (
                <button key={v.key} type="button" title={v.title} onClick={() => { setMoreViewsOpen(false); picker.current?.lookSquare?.(v); }}>{v.name}</button>
              ))}
              {overflowViews.length > 0 && (
                <button
                  type="button"
                  className="pmi-lab-view-more-toggle"
                  aria-expanded={moreViewsOpen}
                  onClick={() => setMoreViewsOpen((open) => !open)}
                >
                  More views ▾
                </button>
              )}
              {moreViewsOpen && overflowViews.length > 0 && (
                <div className="pmi-lab-view-more-menu" role="group" aria-label="More views">
                  {overflowViews.map((v) => (
                    <button key={v.key} type="button" title={v.title} onClick={() => { setMoreViewsOpen(false); picker.current?.lookSquare?.(v); }}>{v.name}</button>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
        {/* The view's navigation controls, together at its bottom right. */}
        <div className="pmi-lab-pad">
          <button type="button" className="pmi-lab-more-tools" aria-pressed={moreTools} title="The 3D viewer's own toolbar and tree: measuring, clipping, zebra" onClick={() => setMoreTools(on => !on)}>More view tools</button>
          <div>
            <button type="button" title="Roll a quarter turn anticlockwise" onClick={() => picker.current?.navigator.roll(-1)}>↺</button>
            <button type="button" title="Roll a quarter turn clockwise" onClick={() => picker.current?.navigator.roll(1)}>↻</button>
          </div>
          <div>
            <button type="button" title="Zoom in" onClick={() => picker.current?.navigator.dolly(1 / 1.25)}>+</button>
            <button type="button" title="Zoom out" onClick={() => picker.current?.navigator.dolly(1.25)}>−</button>
            <button type="button" className="wide" title="Fit the part" onClick={() => picker.current?.navigator.fit()}>fit</button>
            <button type="button" title="The opening view" onClick={() => picker.current?.navigator.home()}>⌂</button>
          </div>
          <label title="Transparency">
            <span aria-hidden="true">◐</span>
            <input
              type="range" min={0} max={90} value={transparency} aria-label="Transparency"
              onChange={(event) => setTransparency(event.target.valueAsNumber)}
            />
          </label>
        </div>
        {(partNotes.length > 0 || oneLabels.notes.length > 0) && (
          <details className="pmi-lab-partnotes" open>
            <summary title="The part's notes">Notes (unless otherwise specified):</summary>
            <ol>
              {partNotes.filter((n) => !oneLabels.notes.some((label) => label.question === n.question)).map((n) => (
                <li key={n.key}>
                  {!readOnly && questions.some((q) => q.id === n.question)
                    ? <button type="button" title="Open its question" onClick={() => {
                      setView('review');
                      setCurrent(n.question);
                      if (n.question.startsWith('part.')) {
                        setUnfolded(NOTE_FIELDS.has(n.question) ? `note:${n.question}` : n.question);
                      }
                    }}>{n.text}</button>
                    : n.text}
                </li>
              ))}
              {oneLabels.notes.map((label) => (
                <li key={label.id} className="unsaved">
                  <button type="button" onClick={() => handlers.current.label(label)}>
                    {label.cells[0]}
                  </button>
                </li>
              ))}
            </ol>
          </details>
        )}
        <div className="pmi-lab-bottom">
          {hasUnsavedInView && (
            <div className="pmi-lab-legend" role="note" aria-label="PMI colour key">
              <span className="pmi-lab-legend-item">
                <span className="pmi-lab-legend-swatch unsaved" aria-hidden="true" />
                Unsaved
              </span>
              <span className="pmi-lab-legend-item">
                <span className="pmi-lab-legend-swatch infile" aria-hidden="true" />
                In file
              </span>
            </div>
          )}
          <div className="pmi-lab-show-bar" role="group" aria-label="Show in 3D view">
            {(readOnly ? ['existing', 'general', 'farSide', 'balloons'] as const : ['defaults', 'existing', 'general', 'farSide', 'balloons'] as const).map((key) => (
              <label key={key}>
                <input
                  type="checkbox" checked={show[key]}
                  onChange={(event) => setShow({ ...show, [key]: event.target.checked })}
                />{' '}
                {{
                  defaults: 'Suggested by Specify',
                  existing: 'In the file',
                  general: 'General dimensions',
                  farSide: 'Hide callouts facing away',
                  balloons: 'Inspection balloons',
                }[key]}
              </label>
            ))}
          </div>
        </div>
        <div className="pmi-lab-status">
          {status || (picker.current?.missing.length ? `faces ${picker.current.missing.join(', ')} could not be shown` : '')}
        </div>
      </div>
    </div>, slots.view)}
    {createPortal(readOnly ? (<div className="pmi-lab pmi-lab-pane">
      <aside className="pmi-lab-panel">
        <header>
          <div className="title">
            <strong>{session.filename}</strong>
            <span className="pmi-lab-unit">{unitLine(lengthUnit)}</span>
          </div>
          <p className="pmi-lab-note" role="status">PMI (view only)</p>
        </header>
        <fieldset className="pmi-lab-rows">
          {setChips.length > 0 && (<>
            <h2 className="pmi-lab-rows-title">Datums</h2>
            <div className="pmi-lab-datums">
              {setChips.map((c) => (
                <span key={c.letter} className={`chip${c.existing ? ' existing' : ''}${selectedDatum === c.letter ? ' on' : ''}`}>
                  <button
                    type="button" className="letter" aria-label={`Datum ${c.letter}`} aria-pressed={selectedDatum === c.letter}
                    onClick={() => {
                      const at = preview?.datums.find((d) => d.letter === c.letter)?.faces ?? [];
                      setSelectedDatum(c.letter);
                      setSelectedChar(null);
                      selectCallout(`datum.${c.letter}`);
                      lightFor(`datum.${c.letter}`);
                      picker.current?.highlight(at);
                      picker.current?.lookAt(at);
                    }}
                  >
                    {c.letter}
                  </button>
                  <span className="what">{c.existing ? `${c.text}, from the file` : c.text}</span>
                </span>
              ))}
            </div>
          </>)}
          {setRows.length > 0 && (
            <CharacteristicsTable
              file={lengthUnit}
              readOnly rows={setRows} selected={selectedChar}
              onSelect={(id, at) => { setSelectedDatum(null); showChar(id, at); }}
              onDownload={() => {}} skipped={skipped} onSkip={() => {}} onEdit={() => null} onCallOut={() => null}
              generalTolerance={preview?.part['general_tolerance'] ?? session.analysis.part?.['general_tolerance'] ?? ''} calledOut={0}
            />
          )}
          {drawingAnnotations}
          {setChips.length === 0 && setRows.length === 0 && !drawingAnnotations && <p className="pmi-lab-none">No PMI is set on this part.</p>}
          {after}
        </fieldset>
      </aside>
    </div>) : (<div className="pmi-lab pmi-lab-pane">
      <aside className="pmi-lab-panel">
        <header>
          <div className="title">
            <strong>{session.filename}</strong>
            <span className="pmi-lab-unit">{unitLine(lengthUnit)}</span>
          </div>
          {note && <p className="pmi-lab-note" role="status">{note}</p>}
          {note && noteGeneral && !readOnly && (
            <div className="pmi-lab-quick" role="group" aria-label={`Call out ${noteGeneral.what}`}>
              <span>Call out {noteGeneral.what}:</span>
              {[...TOLERANCE_PICKS, ...(preview?.general?.[noteGeneral.i]?.kind === 'size' ? ['H7', 'g6'] : [])].map((t) => (
                <button key={t} type="button" onClick={() => void quickCallOut(noteGeneral.i, noteGeneral.what, t)}>
                  {withUnits(t, 'mm')}
                </button>
              ))}
              <button type="button" onClick={() => callOut(noteGeneral.i, noteGeneral.what)}>Other…</button>
            </div>
          )}
          {readOnly && <p className="pmi-lab-note" role="status">Read only</p>}
          {resumed && !statedRows && (
            <p className="pmi-lab-resumed" role="status">
              Opened with the answers this STEP was saved with: change any of them below.
            </p>
          )}
          <div className="pmi-lab-views" role="tablist">
            <button
              type="button" role="tab" aria-selected={view === 'review'} className={view === 'review' ? 'on' : ''}
              onClick={() => switchView('review')}
            >
              {readOnly ? 'Review' : 'Review/Edit'}
            </button>
            <button
              type="button" role="tab" aria-selected={view === 'table'} className={view === 'table' ? 'on' : ''}
              onClick={() => switchView('table')}
            >
              Characteristics ({byBalloon(chars).length})
            </button>
          </div>
          {view === 'review' && (chosen.length > 0 || problems.length > 0) && (
            <div className="pmi-lab-review">
              {chosen.length > 0 && !readOnly && (
                <span>{chosen.length} suggestion{chosen.length === 1 ? '' : 's'}</span>
              )}
              {chosen.length > 1 && !readOnly && (
                <button type="button" className="ok" onClick={() => keepChosen(chosen.map((c) => c.id))}>
                  ✓ Keep all
                </button>
              )}
              {problems.length > 0 && (
                <button
                  type="button" className="warn"
                  onClick={() => openProblem(problems[0]?.about ?? '', problems[0]?.faces ?? [])}
                >
                  {problems.length} problem{problems.length === 1 ? '' : 's'}
                </button>
              )}
              <label>
                <input type="checkbox" checked={onlySuggestions} onChange={(e) => setOnlySuggestions(e.target.checked)} />
                {' '}only these
              </label>
            </div>
          )}
        </header>
        <fieldset className="pmi-lab-rows" ref={rowsBox} disabled={disabled}>
          {view === 'table' ? (
            <CharacteristicsTable
              file={lengthUnit}
              rows={chars} selected={selectedChar} onSelect={showChar} onDownload={downloadCsv} planned={plannedNumbers} calledOutNumbers={calledOutNumbers}
              exportable={inspectionExports}
              skipped={skipped} onSkip={skip} onEdit={readOnly ? () => null : editChar}
              onCallOut={(row) => (!readOnly && row.source.startsWith('general:') && (callOutDimension || !row.source.startsWith('general:existing:'))
                ? (callOutDimension
                  ? () => { switchView('review'); setUnfolded('dims'); setCallingOut(row.id); showChar(row.id, row.faces); }
                  : () => callOut(Number(row.source.slice('general:'.length)), row.text))
                : null)}
              generalTolerance={preview?.part['general_tolerance'] ?? session.analysis.part?.['general_tolerance'] ?? ''}
              calledOut={callOutDimension ? calledOut.size : added.filter(([, extra]) => CALL_OUTS.includes(extra.kind)).length}
            />
          ) : (<>
          {generalProblems.length > 0 && (
            <div className="pmi-lab-problems" role="status">
              {generalProblems.map((p) => (
                <div key={`${p.about}:${p.message}`} className={`problem ${p.severity}`}>
                  {p.message}
                </div>
              ))}
            </div>
          )}
          {(partGroup || stated.length > 0) && (
            <section className="pmi-lab-fields">
              {stated.map(([key, value]) => (
                <div key={key} className="pmi-lab-line stated">
                  <span className="name">{PART_FIELD[`part.${key}`] ?? key}</span>
                  <span className="value">{value} <small>(from the file)</small></span>
                </div>
              ))}
              {partGroup?.questions.filter((q) => !NOTE_FIELDS.has(q.id)).map((q) => {
                const c = chosenById.get(q.id);
                // Shown in its queue row instead while it is being answered there.
                if (slots.question && q.id === current) return null;
                if (needs(q, answers) === 'required' || unfolded === q.id) {
                  return (
                    <PartField locked={readOnly} units={lengthUnit}
                      key={`${q.id}:${JSON.stringify(answers[q.id])}`} q={q} answers={answers} error={errorAt(q.id)}
                      onAnswer={(value) => { setUnfolded(null); return answer(q, value); }}
                    />
                  );
                }
                if (c && !readOnly) return card(c);
                if (onlySuggestions) return null;
                return partLine(q, () => toggle(q.id));
              })}
              {notes.length > 0 && (unfolded === 'notes' || unfolded?.startsWith('note:') ? (
                <div className="pmi-lab-notes">
                  <button type="button" className="pmi-lab-line open" aria-expanded onClick={() => setUnfolded(null)}>
                    <span className="name"><Chevron open />Finish and notes</span><span className="value">fold</span>
                  </button>
                  {notes.filter((q) => !(slots.question && q.id === current)).map((q) => (unfolded === `note:${q.id}` ? (
                    <PartField locked={readOnly} units={lengthUnit}
                      key={`${q.id}:${JSON.stringify(answers[q.id])}`} q={q} answers={answers} error={errorAt(q.id)}
                      onAnswer={(value) => { setUnfolded('notes'); return answer(q, value); }}
                    />
                  ) : (
                    partLine(q, () => setUnfolded(`note:${q.id}`))
                  )))}
                </div>
              ) : (
                <>
                  {!onlySuggestions && (
                    <FoldedLine name="Finish and notes" value={notesSummary || 'none'} onOpen={() => setUnfolded('notes')} questions={notes.map((q) => q.id)} />
                  )}
                  {!readOnly && notes.flatMap((q) => chosenById.get(q.id) ?? []).map(card)}
                </>
              ))}
            </section>
          )}
          <h2 className="pmi-lab-rows-title">On the part</h2>
          {featureRows.map((g) => renderRow(g))}
          {callOutDimension && !readOnly && generalDims.length > 0 && (
            <section className={`pmi-lab-row ${calledOut.size ? 'set' : 'ok'}`} data-row="dimensions">
              <button type="button" className="pmi-lab-row-head" aria-expanded={unfolded === 'dims'} onClick={() => toggle('dims')}>
                <Chevron open={unfolded === 'dims'} />
                <b>{generalDims.length} dimension{generalDims.length === 1 ? '' : 's'} under {preview?.part['general_tolerance'] || (typeof statedRows?.values['part.general_tolerance'] === 'string' ? statedRows.values['part.general_tolerance'] as string : '') || 'the general tolerance'}</b>
                <span className="sum">{calledOut.size ? `${calledOut.size} called out` : ''}</span>
              </button>
              {unfolded === 'dims' && (
                <div className="pmi-lab-dims">
                  {generalDims.map((r) => (
                    <DimensionLine
                      key={r.id} row={r} file={lengthUnit} selected={selectedChar === r.id} calledOut={calledOut.get(r.id) ?? null}
                      planned={calledOutNumbers?.get(r.id)}
                      startOpen={callingOut === r.id}
                      onSelect={() => showChar(r.id, r.faces)}
                      onCallOut={(tolerance) => {
                        const stated = r.source.startsWith('general:existing:');
                        const g = stated ? undefined : preview?.general?.[Number(r.source.slice('general:'.length))];
                        // As the editor's Call out: the dimension with its kind, faces and reference; one
                        // the file states, by its number, to be toleranced in place.
                        const ok = callOutDimension({
                          id: r.id, what: `${r.feature} ${labelRow(r)}`, kind: g?.kind === 'size' ? 'size' : 'distance', faces: g?.faces ?? r.faces,
                          reference: Array.isArray(g?.['reference']) ? g['reference'] as number[] : [],
                          ...(stated && r.n > 0 ? { number: `#${r.n}` } : {}),
                        }, tolerance);
                        if (ok) setCalledOut((m) => { const next = new Map(m); if (tolerance.trim()) next.set(r.id, tolerance.trim()); else next.delete(r.id); return next; });
                        return ok;
                      }}
                    />
                  ))}
                </div>
              )}
            </section>
          )}
          {otherPmi && !readOnly && otherPmi.items.length > 0 && !onlySuggestions && (
            <section className="pmi-lab-row ok" data-row="other-pmi">
              <button type="button" className="pmi-lab-row-head" aria-expanded={unfolded === 'other'} onClick={() => toggle('other')}>
                <Chevron open={unfolded === 'other'} />
                <b>Other PMI in the file</b>
                <span className="sum">{otherPmi.items.map((i) => i.value).join(' · ')}</span>
              </button>
              {unfolded === 'other' && otherPmi.items.map((item) => {
                const removed = otherPmi.removed.has(item.key);
                return (
                  <div key={item.key} className={`pmi-lab-stated${removed ? ' removed' : ''}`} data-other={item.key} data-callout-selected={selectedCallout === item.key}>
                    <button type="button" className="pmi-lab-item" onClick={() => {
                      selectCallout(item.key);
                      lightFor(item.key);
                      picker.current?.highlight(item.faces);
                      picker.current?.lookAt(item.faces);
                    }}>
                      <span className="prompt">{item.label}</span>
                      <span className="value">{removed ? <><s>{item.value}</s> removed</> : <>{item.value} <small>(from the file)</small></>}</span>
                    </button>
                    {item.removable !== false && (
                      <StatedTools name={`${item.label} ${item.value}`} state={{ removed, changed: false, onRemove: () => otherPmi.onRemove(item.key), onRestore: () => otherPmi.onRestore(item.key) }} />
                    )}
                  </div>
                );
              })}
            </section>
          )}
          {addedByYou && (!onlySuggestions || draft || problemsIn('added').length > 0) && (
          <section className={`pmi-lab-row ${added.length ? 'set' : 'ok'}${draft ? ' current' : ''}`}>
            <button
              type="button" className="pmi-lab-row-head" aria-expanded={unfolded === 'added' || Boolean(draft)}
              onClick={() => (draft ? undefined : toggle('added'))}
            >
              <Chevron open={unfolded === 'added' || Boolean(draft)} />
              <b>Added by you</b>
              <span className="sum">
                {added.length ? added.map(([, extra]) => withUnits(describe(extra), 'mm')).join(' · ') : 'a flatness, orientation, fit, finish or called-out dimension on any face'}
              </span>
            </button>
            {(unfolded === 'added' || draft) && (
              <div className="pmi-lab-datums">
                {added.map(([key, extra]) => (
                  <span key={key} className="chip">
                    <button type="button" className="what-button" disabled={readOnly} onClick={() => editExtra(key)}>
                      {withUnits(describe(extra), 'mm')}
                    </button>
                    {!readOnly && (
                      <button type="button" className="x" aria-label={`Remove ${describe(extra)}`} onClick={() => removeExtra(key)}>
                        ✕
                      </button>
                    )}
                  </span>
                ))}
                {!draft && !readOnly && <button type="button" className="add" onClick={startExtra}>+ Add a requirement</button>}
              </div>
            )}
            {draft && (
              <ExtraEditor
                draft={draft} selection={selection} kinds={kindsFor(selection, session.analysis)}
                letters={chips.map((c) => c.letter)} newLetter={nextLetter}
                error={errorAt(draft.key)?.message ?? null}
                editing={added.some(([k]) => k === draft.key)}
                onChange={setDraft} onSave={saveExtra} onRemove={() => removeExtra(draft.key)}
                onCancel={() => { setDraft(null); setCurrent(null); }}
                onPickFrom={pickFrom}
              />
            )}
            {problemLines('added')}
          </section>
          )}
          {renderRow(datumGroup, unfolded === 'datums' || here === datumGroup || adding ? (
            <div className="pmi-lab-datums">
              {chips.map((c) => (
                <span key={c.letter} className={`chip${c.existing ? ' existing' : ''}`}>
                  <button
                    type="button" className="letter" disabled={!questions.some((q) => q.id === `datum.${c.letter}`)}
                    onClick={() => openQuestionRow(`datum.${c.letter}`)} aria-label={`Datum ${c.letter}`}
                  >
                    {c.letter}
                  </button>
                  <span className="what">{c.existing ? `${c.text}, from the file` : c.text}</span>
                  {!c.existing && !(`datum.${c.letter}` in answers) && <small className="sg">suggested</small>}
                  {!c.existing && !readOnly && (
                    <button
                      type="button" className="x" aria-label={`Remove datum ${c.letter}`}
                      onClick={() => removeDatum(c.letter)}
                    >
                      ✕
                    </button>
                  )}
                </span>
              ))}
              {!chips.length && <span className="none">None needed: no tolerance is measured from one.</span>}
              {nextLetter && !adding && !readOnly && (
                <button type="button" className="add" onClick={addDatum}>+ Add datum</button>
              )}
            </div>
          ) : null, chips.length ? chips.map((c) => `${c.letter} ${c.text}`).join(' · ') : 'none needed', () => toggle('datums'),
          unfolded === 'datums' || Boolean(adding))}
          {applied.length > 0 && (
            <div className="pmi-lab-rules">
              <b>Applied by rule</b>
              {applied.map((line) => <div key={line}>{line}</div>)}
            </div>
          )}
          {drawingAnnotations}
          {after}
          </>)}
        </fieldset>
        {looseError && <footer><p role="alert" className="pmi-lab-error">{looseError.message}</p></footer>}
      </aside>
    </div>), slots.panel)}
    {/* The current question again in the selected queue row, so it is answered where it is listed. */}
    {!readOnly && slots.question && (() => {
      const g = groups.find((x) => x.questions.some((q) => q.id === current));
      const q = g?.questions.find((x) => x.id === current);
      // The part's own fields (material, tolerance, finish and notes) in the same box as in
      // the list, with its suggestions; a feature's question as its chips.
      return g && q && createPortal(<div className="pmi-lab pmi-lab-inline">{g === partGroup
        ? <PartField locked={readOnly} units={lengthUnit} key={`${q.id}:${JSON.stringify(answers[q.id])}`} q={q} answers={answers} error={errorAt(q.id)} onAnswer={(value) => answer(q, value)} />
        : questionItem(q, g, false)}</div>, slots.question);
    })()}
    {inspectionExports && !readOnly && slots.exports && <>
      {createPortal(<button type="button" onClick={downloadCsv} disabled={!chars.length} title="Every dimension and tolerance, numbered, with its nominal and limits">
        {gated ? 'Inspection list (CSV) · Sign in' : 'Inspection list (CSV)'}
      </button>, slots.exports)}
      {createPortal(<button type="button" onClick={downloadFai} disabled={!chars.length} title="AS9102 first article inspection Forms 1–3 as an Excel workbook: the part, its materials and processes, and each characteristic with its requirement, for the results">
        {gated ? 'AS9102 FAI (Excel) · Sign in' : 'AS9102 FAI (Excel)'}
      </button>, slots.more ?? slots.exports)}
    </>}
  </>);
}

/** Hands ``file`` to the browser to save, under its own name. */
/**
 * The review. Its own 3D view needs three-cad-viewer (an optional peer), loaded on demand: the review
 * mounts once it has, so the view is made with the rest of the component's state. A host's ``view`` is
 * used at once; when the package is missing, the review mounts with the error in the view's place.
 */
export function SpecifyReview(props: ComponentProps<typeof SpecifyReviewInner>) {
  const cad = useCadViewer(props.view ?? null);
  if (!cad.ready && !cad.error) return <div className="pmi-lab-stage" aria-busy="true" />;
  return <SpecifyReviewInner {...props} />;
}

export function saveFile(file: File): void {
  const url = URL.createObjectURL(file);
  const link = document.createElement('a');
  link.href = url;
  link.download = file.name;
  document.body.append(link);
  link.click();
  link.remove();
  // After the click has handed the data to the download.
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** A setting folded to one line: its name and value; a tap unfolds it. */
function FoldedLine({ name, value, onOpen, question, questions }: {
  name: string; value: string; onOpen: () => void;
  /** Test hooks: the question it is, or the questions folded under it. */
  question?: string; questions?: readonly string[];
}) {
  return (
    <button type="button" className="pmi-lab-line" aria-expanded={false} onClick={onOpen} data-question={question} data-questions={questions?.join(' ')}>
      <span className="name"><Chevron open={false} />{name}</span>
      <span className="value">{value}</span>
    </button>
  );
}

/** A general dimension called out: which one, and what the requirement needs (added.ts's size or distance). */
export interface CalledOut {
  id: string; what: string; kind: 'size' | 'distance'; faces: readonly number[]; reference: readonly number[];
  /** Set for a dimension the file states with its nominal alone: its requirement's number (#N). The
   * host tolerances that dimension in place rather than adding one. */
  number?: string;
}

/** One dimension the general tolerance covers: its feature, characteristic, nominal, ± and number,
 * and Call Out, which takes a tolerance of its own in place (change list item 1). */
function DimensionLine({ row, file, selected, calledOut, planned, startOpen, onSelect, onCallOut }: {
  row: Characteristic; file: LengthUnit; selected: boolean; calledOut: string | null; startOpen: boolean;
  /** The number Save writes for it, called out (the engine's). */
  planned?: number;
  onSelect: () => void; onCallOut: (tolerance: string) => boolean;
}) {
  const [open, setOpen] = useState(startOpen);
  const [text, setText] = useState(calledOut ?? '');
  const [problem, setProblem] = useState<string | null>(null);
  const tol = row.upper !== null && row.lower !== null && row.upper === -row.lower ? `±${fmt(row.upper)}`
    : [row.upper !== null ? `+${fmt(row.upper)}` : '', row.lower !== null ? fmt(row.lower) : ''].filter(Boolean).join(' ');
  const use = () => {
    if (!onCallOut(text)) { setProblem('This dimension cannot be called out here.'); return; }
    setProblem(null); setOpen(false);
  };
  return (
    <div className={`pmi-lab-dim${selected ? ' on' : ''}${calledOut ? ' changed' : ''}`} data-characteristic={row.id}>
      <button type="button" className="pmi-lab-dim-line" onClick={onSelect}>
        <span className={`bal${!row.n && planned ? ' planned' : ''}`}>{row.n ? row.id : planned ?? '—'}</span>
        <span className="feature">{labelName(row.feature, file)}</span>
        <span className="text">{labelRow(row)}</span>
        <span className="tol">{calledOut ? <><s>{tol}</s> ±{calledOut.replace(/^±\s*/, '')} mm</> : withUnits(tol, 'mm')}</span>
      </button>
      {!open && <button type="button" className="call-out" onClick={() => { setOpen(true); onSelect(); }}>{calledOut ? 'Change' : 'Call Out'}</button>}
      {open && (
        <span className="pmi-lab-dim-edit">
          <label>± <input type="text" inputMode="decimal" value={text} autoFocus aria-label={`Tolerance for ${row.feature} ${row.text}`}
            placeholder={tol.replace(/^±/, '')} onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') use(); if (e.key === 'Escape') setOpen(false); }} /> mm</label>
          <button type="button" onClick={use} disabled={!text.trim()}>Use</button>
          {calledOut && <button type="button" onClick={() => { if (onCallOut('')) { setText(''); setOpen(false); } }}>Back to {withUnits(tol, 'mm')}</button>}
          <button type="button" onClick={() => { setOpen(false); setText(calledOut ?? ''); }}>Cancel</button>
        </span>
      )}
      {problem && <p role="alert" className="pmi-lab-error">{problem}</p>}
    </div>
  );
}

/** What an item the file states can do: change (pencil) and remove (bin), or, removed, Undo. */
interface StatedState { removed: boolean; changed: boolean; onRemove(): void; onRestore(): void; decided?: boolean }

const PENCIL = <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M11 2.5l2.5 2.5L6 12.5l-3 .5.5-3z" /></svg>;
const BIN = <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 8.5h6l.5-8.5M6.8 7v4M9.2 7v4" /></svg>;

/** A stated item's pencil and bin: the same size and colour; the bin asks before it removes. */
function StatedTools({ name, state, onChange }: { name: string; state: StatedState; onChange?: () => void }) {
  const [asking, setAsking] = useState(false);
  if (state.removed) {
    return <span className="pmi-lab-stated-tools"><button type="button" onClick={state.onRestore}>Undo</button></span>;
  }
  if (asking) {
    return (
      <span className="pmi-lab-stated-tools asking" role="group" aria-label={`Remove ${name}?`}>
        <span>{state.decided ? 'Take back "none needed"?' : 'Remove from the file?'}</span>
        <button type="button" className="remove" onClick={() => { setAsking(false); state.onRemove(); }}>{state.decided ? 'Take back' : 'Remove'}</button>
        <button type="button" onClick={() => setAsking(false)}>Cancel</button>
      </span>
    );
  }
  return (
    <span className="pmi-lab-stated-tools">
      {onChange && <button type="button" className="icon" title={`Change ${name}`} aria-label={`Change ${name}`} onClick={onChange}>{PENCIL}</button>}
      <button type="button" className="icon" title={state.decided ? `Take back "none needed" for ${name}` : `Remove ${name}`} aria-label={`Remove ${name}`} onClick={() => setAsking(true)}>{BIN}</button>
    </span>
  );
}

/** ▸ folded, ▾ open: every row says which it is and that a tap changes it. */
function Chevron({ open }: { open: boolean }) {
  return <span className={`pmi-lab-chev${open ? ' open' : ''}`} aria-hidden="true">▸</span>;
}

/** A part setting's value in a line: its answer, else its default. */
function partValue(q: PmiQuestion, answers: PmiAnswers): string {
  const value = answerOf(q, answers);
  return typeof value === 'string' && value ? value.replace(/ \(default\)$/, '') : '—';
}

/** One of the part's own settings, unfolded: a text box, suggesting if it can, or a list. */
function PartField({ q, answers, error, onAnswer, locked = false, units = 'mm' }: {
  /** Read only: the control is shown disabled. */
  locked?: boolean;
  /** The file's length unit, for a general tolerance by decimal places. */
  units?: Units;
  q: PmiQuestion; answers: PmiAnswers; error: PmiLabError | null; onAnswer: (value: unknown) => Promise<boolean>;
}) {
  const need = needs(q, answers);
  const label = PART_FIELD[q.id] ?? q.prompt;
  const value = answerOf(q, answers);
  const placeholder = need === 'required' ? 'e.g. aluminium 6082-T6' : String(q.default ?? '');
  const name = (
    <span className="name">
      {label}
      {need === 'required' && <em> required</em>}
    </span>
  );
  const rest = (
    <>
      {need === 'required' && <span className="why">{q.why}</span>}
      {q.kind === 'choice' && typeof value === 'string' && optionHint(q, value) && (
        <span className="why">{optionHint(q, value)}</span>
      )}
      {error && <span role="alert" className="pmi-lab-error">{error.message}</span>}
    </>
  );
  // A suggesting box is not wrapped in a label: a click on a suggestion would
  // focus the box again and reopen the list.
  if (q.kind === 'text' && q.suggestions?.length) {
    return (
      <div className={`pmi-lab-field${need ? ` ${need}` : ''}`} title={q.why} role="group" aria-label={label} data-question={q.id}>
        {name}
        {lock(<SuggestBox
          label={label} value={typeof answers[q.id] === 'string' ? answers[q.id] as string : ''}
          suggestions={q.suggestions} placeholder={placeholder} invalid={need === 'required'}
          onAnswer={onAnswer}
        />, locked)}
        {rest}
      </div>
    );
  }
  // The general tolerance can also be a title block by decimal places (#228, Paul 24 Nov): one
  // more choice, which opens its rows under the field.
  if (q.id === 'part.general_tolerance' && q.kind === 'choice') {
    return <GeneralToleranceField q={q} value={typeof value === 'string' ? value : ''} name={name} rest={rest} need={need}
      locked={locked} units={units} onAnswer={onAnswer} />;
  }
  return (
    <label className={`pmi-lab-field${need ? ` ${need}` : ''}`} title={q.why} data-question={q.id}>
      {name}
      {lock(q.kind === 'choice' ? (
        <select value={typeof value === 'string' ? value : ''} onChange={(event) => onAnswer(event.target.value)}>
          {(q.options ?? []).map((option) => (
            <option key={option} value={option}>{option}{option === editorDefault(q) ? ' (default)' : ''}</option>
          ))}
        </select>
      ) : (
        <input
          type="text" defaultValue={typeof answers[q.id] === 'string' ? answers[q.id] as string : ''}
          placeholder={placeholder}
          onBlur={(event) => {
            const typedText = event.target.value.trim();
            if (typedText && typedText !== answers[q.id]) onAnswer(typedText);
          }}
          onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }}
        />
      ), locked)}
      {rest}
    </label>
  );
}

const BY_PLACES = 'By decimal places…';

/** The general tolerance: the editor's list of classes, plus a title block by decimal places
 * (".XX ±.01, .XXX ±.005, angles ±1°"), entered row by row under the field and written as one line. */
function GeneralToleranceField({ q, value, name, rest, need, locked, units, onAnswer }: {
  q: PmiQuestion; value: string; name: ReactNode; rest: ReactNode; need: string | null; locked: boolean; units: Units;
  onAnswer: (value: unknown) => Promise<boolean>;
}) {
  const options = q.options ?? [];
  const isBlock = Boolean(value) && !options.includes(value);
  const [editing, setEditing] = useState<GeneralTolerance | null>(null);
  const start = () => setEditing(isBlock ? parse(value, units) : emptyBlock(units));
  return (
    <div className={`pmi-lab-field${need ? ` ${need}` : ''}`} title={q.why} role="group" aria-label={PART_FIELD[q.id] ?? q.prompt} data-question={q.id}>
      {name}
      {lock(
        <select aria-label="General tolerance" value={editing ? BY_PLACES : value}
          onChange={(event) => { if (event.target.value === BY_PLACES) start(); else { setEditing(null); void onAnswer(event.target.value); } }}>
          {isBlock && <option value={value}>{value}</option>}
          {options.map((option) => (
            <option key={option} value={option}>{option}{option === editorDefault(q) ? ' (default)' : ''}</option>
          ))}
          <option value={BY_PLACES}>{BY_PLACES}</option>
        </select>, locked)}
      {editing?.form === 'block' && (
        <DecimalPlaces block={editing} onChange={setEditing} onCancel={() => setEditing(null)}
          onUse={(text) => { void onAnswer(text).then((ok) => { if (ok) setEditing(null); }); }} />
      )}
      {rest}
    </div>
  );
}

/** A general tolerance by decimal places: units, a tolerance per number of places, angles and notes. */
function DecimalPlaces({ block, onChange, onUse, onCancel }: {
  block: Extract<GeneralTolerance, { form: 'block' }>; onChange(next: GeneralTolerance): void; onUse(text: string): void; onCancel(): void;
}) {
  const notes = block.notes.length ? block.notes : [''];
  const text = compose(block, block.units);
  return (
    <div className="pmi-lab-gt" role="group" aria-label="General tolerance by decimal places">
      <label className="pmi-lab-gt-line">Units{' '}
        <select aria-label="Units" value={block.units} onChange={(e) => onChange({ ...block, units: e.target.value as Units })}>
          <option value="mm">mm</option><option value="in">in</option>
        </select>
      </label>
      {block.rows.map((r, i) => (
        <div key={i} className="pmi-lab-gt-line" data-gt-row={i}>
          <select aria-label={`Row ${i + 1} decimal places`} value={r.places}
            onChange={(e) => onChange({ ...block, rows: block.rows.map((x, j) => (j === i ? { ...x, places: Number(e.target.value) } : x)) })}>
            {[0, 1, 2, 3, 4].map((n) => <option key={n} value={n}>{placesText(n, block.units)}</option>)}
          </select>
          <input aria-label={`Row ${i + 1} tolerance`} placeholder={block.units === 'in' ? '±.005' : '±0.1'} value={r.tolerance}
            onChange={(e) => onChange({ ...block, rows: block.rows.map((x, j) => (j === i ? { ...x, tolerance: e.target.value } : x)) })} />
          <button type="button" aria-label={`Remove row ${i + 1}`} onClick={() => onChange({ ...block, rows: block.rows.filter((_, j) => j !== i) })}>✕</button>
        </div>
      ))}
      <button type="button" onClick={() => {
        const last = block.rows.at(-1)?.places ?? -1;
        onChange({ ...block, rows: [...block.rows, { places: Math.min(4, last + 1), tolerance: '' }] });
      }}>Add row</button>
      <label className="pmi-lab-gt-line">Angles <input aria-label="Angles" placeholder="±1°" value={block.angles} onChange={(e) => onChange({ ...block, angles: e.target.value })} /></label>
      {notes.map((note, i) => (
        <div key={i} className="pmi-lab-gt-line">
          <input aria-label={`Note ${i + 1}`} placeholder="fractions ±1/64" value={note}
            onChange={(e) => onChange({ ...block, notes: notes.map((n, j) => (j === i ? e.target.value : n)) })} />
          <button type="button" aria-label={`Remove note ${i + 1}`} onClick={() => onChange({ ...block, notes: notes.filter((_, j) => j !== i) })}>✕</button>
        </div>
      ))}
      <button type="button" onClick={() => onChange({ ...block, notes: [...notes, ''] })}>Add note</button>
      <div className="pmi-lab-gt-text">Written as: <code>{text || '…'}</code></div>
      <div className="pmi-lab-gt-line">
        <button type="button" className="ok" disabled={!text} onClick={() => onUse(text)}>Use</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}

/** Read only: the controls disabled as one, with no box of their own. */
function lock(controls: ReactNode, locked: boolean): ReactNode {
  return locked ? <fieldset disabled style={{ border: 0, margin: 0, padding: 0, minWidth: 0 }}>{controls}</fieldset> : controls;
}

/** A question's value in a line: as its row summary says it, else as given. */
/** Its value as shown: lengths in mm, which Specify works in; a part setting as it is. */
const inMm = (q: PmiQuestion, value: unknown): string => (q.id.startsWith('part.') ? shortValue(q, value) : withUnits(shortValue(q, value), 'mm'));

function shortValue(q: PmiQuestion, value: unknown): string {
  return display(q, value) || (typeof value === 'string' && value ? value : '—');
}

function QuestionItem({
  q, answers, active, selection, mode, error, typed, onOpen, onAnswer, onSpec, onConfirm, onReset, onMode,
  onDefaultSelection, titled = true, locked = false, resettable = true, file = 'mm', selected,
}: {
  /** The file's length unit, for its prompt (units.ts). */
  file?: LengthUnit;
  /** "Reset to default" offered: not on what the file states, which has its pencil and bin instead. */
  resettable?: boolean;
  /** Read only: the answer controls are shown disabled, the editing ones hidden. */
  locked?: boolean;
  /** Whether it shows its prompt: not under a queue row that already does. */
  titled?: boolean;
  selected?: boolean;
  q: PmiQuestion; answers: PmiAnswers; active: boolean; selection: number[]; mode: PickMode;
  error: PmiLabError | null; typed: PmiInterpretation | null; onOpen: () => void; onAnswer: (value: unknown) => void;
  onSpec: (text: string) => Promise<string | null>; onConfirm: () => void;
  onReset: () => void; onMode: (mode: PickMode) => void; onDefaultSelection: () => void;
}) {
  const need = needs(q, answers);
  const optional = q.attention === 'refine' ? ' (optional)' : '';
  // What an option means: the one pointed at, else the one chosen (a phone cannot point).
  const [hinted, setHinted] = useState<string | null>(null);
  const chosen = answerOf(q, answers);
  const hintFor = hinted ?? (typeof chosen === 'string' ? chosen : '');
  const hint = hintFor ? optionHint(q, hintFor) : null;
  // What the answer puts on the drawing: its choice's callout, or a typed spec's.
  const callout = q.choices?.find((c) => c.value === chosen)?.callout
    ?? (typed && typed.value === chosen ? typed.callout : undefined);
  if (!active) {
    return (
      <button type="button" className={`pmi-lab-item${need ? ` ${need}` : ''}`} onClick={onOpen} data-question={q.id} data-callout-selected={selected}>
        <span className="prompt">{labelName(q.prompt, file)}{optional}</span>
        <span className="value">{inMm(q, answerOf(q, answers))} ›</span>
      </button>
    );
  }
  return (
    <div className={`pmi-lab-item on${need ? ` ${need}` : ''}`} data-question={q.id} data-callout-selected={selected}>
      <div className="prompt">
        {titled && <>{labelName(q.prompt, file)}{optional}</>}
        {q.id in answers && q.default !== null && !locked && resettable && (
          <button type="button" className="reset" onClick={onReset}>Reset to default</button>
        )}
      </div>
      {q.reason && need === 'check' && (
        <div className="reason">
          {q.reason}{' '}
          {!locked && <button type="button" className="ok" onClick={onConfirm}>✓ Looks right</button>}
        </div>
      )}
      <div className="why">{withUnits(q.why, 'mm')}</div>
      {lock(<>
      {q.kind === 'choice' && (
        <>
          {q.choices?.length ? (
            <PurposeChoice q={q} value={chosen} onAnswer={onAnswer} onPoint={setHinted} />
          ) : (
            <div className="opts" onMouseLeave={() => setHinted(null)}>
              {(q.options ?? []).map((option) => (
                <button
                  key={option} type="button" onClick={() => onAnswer(option)}
                  onMouseEnter={() => setHinted(option)} onFocus={() => setHinted(option)}
                  onBlur={() => setHinted(null)}
                  className={`${chosen === option ? 'on' : ''}${option === q.default ? ' default' : ''}`}
                >
                  {q.id.startsWith('part.') ? option : withUnits(option, 'mm')}
                </button>
              ))}
            </div>
          )}
          {callout && <div className="callout"><span>On the drawing</span> {callout}</div>}
          {hint && <div className="hint"><b>{codeOf(q, hintFor)}</b> {hint}</div>}
          <SpecBox q={q} codes={(q.choices ?? []).map((c) => c.code)} onSpec={onSpec} />
        </>
      )}
      {q.kind === 'text' && (
        <input
          type="text" defaultValue={typeof answers[q.id] === 'string' ? answers[q.id] as string : ''}
          aria-label={q.prompt}
          onBlur={(event) => { const value = event.target.value.trim(); if (value) onAnswer(value); }}
          onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }}
        />
      )}
      {q.kind === 'faces' && (
        <>
          <div className="opts">
            {MODES.map((m) => (
              <button key={m} type="button" className={m === mode ? 'on' : ''} onClick={() => onMode(m)}>
                {MODE_TEXT[m]}
              </button>
            ))}
          </div>
          <div className="faces">{selection.length ? `faces ${selection.join(', ')}` : 'nothing selected'}</div>
          <div className="opts">
            <button type="button" onClick={() => onAnswer(selection)}>Use selection</button>
            <button type="button" onClick={() => onAnswer([])}>None</button>
            <button type="button" onClick={onDefaultSelection}>Default</button>
          </div>
          <div className="why">Click to select, shift-click to add. Drag to turn the part.</div>
        </>
      )}
      </>, locked)}
      {error && <p role="alert" className="pmi-lab-error">{error.message}</p>}
    </div>
  );
}
