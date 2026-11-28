import { readCarried } from './carried';
import { withCharacteristicNumbers, type NumberScene } from './characteristic-numbers';
import type { IntentReview, PartOrigin } from '../types/pmi';
import { findStepOrigin } from '../specify/drafting-layout';
import { inspectionFromStep } from '../specify/saved-inspection';

interface IntentModule {
  default(): Promise<unknown>;
  review(analysis: string, answers: string): string;
  pmiScene(bytes: Uint8Array, part?: number): string;
  interpret(analysis: string, answers: string, question: string, text: string): string;
  acceptDefaults(analysis: string, answers: string): string;
  answerEdit(state: string, answers: string): string;
  tolerancingEdit(state: string, mode: string | undefined, answers: string): string;
  openedTolerancing(state: string, resume?: string): string;
  diff(oldName: string, old: Uint8Array, newName: string, next: Uint8Array, title: string, scenes?: boolean): string;
  IntentEngine: {
    new (name: string, bytes: Uint8Array): IntentInstance;
    openPart(name: string, bytes: Uint8Array, part: number): IntentInstance;
    parts(name: string, bytes: Uint8Array): string;
  };
}
interface IntentInstance { free(): void; snapshot(): string; review(answers: string): string; settle(state: string): string;
    evidence(): string; payload(name: string): string | undefined; original(): Uint8Array | undefined; pmiScene(state: string): string; checkEdits(edits: string): string }

let engine: IntentInstance | undefined;
// The open engine's analysis as JSON, for the free acceptDefaults (made once per engine).
let analysisJson: string | undefined;
let stepOrigin: PartOrigin | null = null;
let sourceId = '';
// The part of an assembly the engine is of, if its file is one.
let part: number | undefined;

// 'review' previews defaults as suggestions, marking unanswered intent items as defaults and keeping
// their questions open; nothing is written. 'review-given' and 'review-analysis-given' review only
// the answers supplied. Writing is an edit list applied by specify-pmi (its session's Save, or the
// engine's `edit`), never here.
// The loaded engine module, for its free functions.
let globalModule: IntentModule | undefined;
const analysisOf = () => (analysisJson ??= JSON.stringify(JSON.parse(engine!.snapshot()).analysis));
const accepted = (answers: unknown) => globalModule!.acceptDefaults(analysisOf(), JSON.stringify(answers));
function reviewed(answers: Record<string, unknown>) {
  const full = accepted(answers);
  const review = markDefaults(JSON.parse(engine!.review(full)), answers) as IntentReview;
  // Numbers as the writer will allocate them, asked of the engine (docs/characteristic-numbers.md):
  // an unsaved requirement shows the number Save will write. The defaults accepted here are
  // suggestions, not answers Save writes: they take no number.
  const all = JSON.parse(full) as Record<string, unknown>;
  const suggested = Object.keys(all).filter((id) => !(id in answers));
  const scene = JSON.parse(engine!.pmiScene(JSON.stringify({ answers: all, suggested }))) as NumberScene;
  return { ...review, intent: withCharacteristicNumbers(review.intent, scene), open: JSON.parse(engine!.review(JSON.stringify(answers))).open };
}
function inspectionReview(answers: Record<string, unknown>): IntentReview {
  const review = JSON.parse(engine!.review(JSON.stringify(answers))) as IntentReview;
  const scene = JSON.parse(engine!.pmiScene(JSON.stringify({ answers }))) as NumberScene;
  return { ...review, intent: withCharacteristicNumbers(review.intent, scene) };
}
function markDefaults<T>(review: T, answers: Record<string, unknown>): T {
  const mark = (value: unknown): void => {
    if (Array.isArray(value)) value.forEach(mark);
    else if (value && typeof value === 'object') {
      const item = value as Record<string, unknown>;
      if (typeof item.question === 'string' && 'default' in item) item.default = !(item.question in answers);
      Object.values(item).forEach(mark);
    }
  };
  mark((review as { intent?: unknown }).intent);
  return review;
}

let modulePromise: Promise<IntentModule> | undefined;
// Where the Specify WASM glue is, when the host serves its own (a `{ wasmUrl }` message before any
// work). Otherwise the dependency @draftwright/specify-pmi-wasm, which the host's bundler resolves;
// its glue finds its .wasm beside it (new URL(..., import.meta.url)).
let configuredUrl: string | undefined;
function load() {
  modulePromise ??= (async () => {
    const module: IntentModule = configuredUrl
      ? await import(/* @vite-ignore */ configuredUrl)
      : await import('@draftwright/specify-pmi-wasm') as unknown as IntentModule;
    await module.default();
    return module;
  })();
  return modulePromise;
}

let queue = Promise.resolve();
self.onmessage = ({ data }) => {
  if (data.id === undefined && data.operation === undefined && typeof data.wasmUrl === 'string') { configuredUrl = data.wasmUrl; return; }
  const { id, operation, args } = data;
  queue = queue.then(async () => {
    try {
      const module = await load();
      globalModule = module;
      let result: unknown = true;
      if (operation === 'file-scene') {
        // A saved file's own PMI as drawn (the free pmiScene): the cached engine is left as it is.
        const scene = JSON.parse(module.pmiScene(args.bytes, args.part)) as Record<string, unknown>;
        const origin = args.bytes ? findStepOrigin(args.bytes) : null;
        result = origin ? { ...scene, step_origin: origin } : scene;
      } else if (operation === 'file-inspection') {
        const file = args.part === undefined ? new module.IntentEngine(args.name, args.bytes)
          : module.IntentEngine.openPart(args.name, args.bytes, args.part);
        try {
          const { analysis } = JSON.parse(file.snapshot());
          result = inspectionFromStep(analysis, JSON.parse(module.pmiScene(args.bytes, args.part)));
        } finally { file.free(); }
      } else if (operation === 'open' || operation === 'review' || operation === 'review-given' || operation === 'settle' || operation === 'scene' || operation === 'check-edits') {
        if (!engine || sourceId !== args.sourceId || part !== args.part || operation === 'open') {
          engine?.free(); engine = undefined; analysisJson = undefined; stepOrigin = null;
          engine = args.part === undefined ? new module.IntentEngine(args.name, args.bytes)
            : module.IntentEngine.openPart(args.name, args.bytes, args.part);
          sourceId = args.sourceId; part = args.part;
          stepOrigin = args.bytes ? findStepOrigin(args.bytes) : null;
        }
        if (operation === 'review-given') {
          // Inspection files use the given answers and the STEP's PMI directly. Filling defaults
          // even for preview can add part notes, general tolerances and dependent dimensions.
          result = inspectionReview(args.answers);
        } else if (operation === 'settle') {
          const { state } = JSON.parse(engine.settle(JSON.stringify(args.state)));
          result = { review: reviewed(state.answers), state };
        } else if (operation === 'scene') {
          // The PMI as drawn for the review state (specify-pmi 0.1.5 pmiScene). When `exact` is set,
          // pass the review state as given so un-asked length/location defaults do not alter stacking
          // relative to the saved STEP.
          let scene: Record<string, unknown>;
          if (args.exact) {
            scene = JSON.parse(engine.pmiScene(JSON.stringify(args.state))) as Record<string, unknown>;
          } else {
            // The defaults the editor accepts are written, so they are in the scene, as unreviewed (`auto`) unless answered.
            const answers = JSON.parse(accepted(args.state.answers)) as Record<string, unknown>;
            const auto = [...new Set([...args.state.auto ?? [], ...Object.keys(answers).filter(id => !(id in args.state.answers))])];
            scene = JSON.parse(engine.pmiScene(JSON.stringify({ ...args.state, answers, auto }))) as Record<string, unknown>;
          }
          const origin = stepOrigin ?? (args.bytes ? findStepOrigin(args.bytes) : null);
          result = origin ? { ...scene, step_origin: origin } : scene;
        } else if (operation === 'check-edits') {
          // An edit list checked against the file, all or nothing (WASM checkEdits): writes nothing.
          result = JSON.parse(engine.checkEdits(JSON.stringify(args.edits)));
        } else if (operation === 'open') {
          const snapshot = JSON.parse(engine.snapshot());
          analysisJson = JSON.stringify(snapshot.analysis);
          const mesh = stepOrigin ? { ...snapshot.mesh, origin: stepOrigin } : snapshot.mesh;
          result = { ...snapshot, mesh, evidence: JSON.parse(engine.evidence()) };
        } else result = reviewed(args.answers);
      } else if (operation === 'review-analysis-given') {
        // Pure analysis-only review, without allocation geometry. Inspection hosts use
        // review-given with STEP bytes to obtain the writer's stable characteristic numbers.
        result = JSON.parse(module.review(JSON.stringify(args.analysis), JSON.stringify(args.answers)));
      } else if (operation === 'parts') {
        result = JSON.parse(module.IntentEngine.parts(args.name, args.bytes));
      } else if (operation === 'part-mesh') {
        // One part's mesh for the assembly's view, on an engine of its own: the open part's is kept.
        const one = module.IntentEngine.openPart(args.name, args.bytes, args.part);
        try { result = JSON.parse(one.snapshot()).mesh; } finally { one.free(); }
      } else if (operation === 'carried') {
        // What a STEP carries, read on an engine of its own: the open part's is kept.
        result = readCarried(module, args.name, args.bytes, args.payload, args.part);
      } else if (operation === 'answer-edit') {
        result = JSON.parse(module.answerEdit(JSON.stringify(args.state), JSON.stringify(args.answers)));
      } else if (operation === 'tolerancing-edit') {
        result = JSON.parse(module.tolerancingEdit(JSON.stringify(args.state), args.mode, JSON.stringify(args.answers)));
      } else if (operation === 'opened') {
        result = JSON.parse(module.openedTolerancing(JSON.stringify(args.state), args.resume && JSON.stringify(args.resume)));
      } else if (operation === 'diff') {
        // Two revisions of a part compared, as `specify-pmi diff` compares them; with ``scenes``,
        // the two PMI scenes it compared too (`report.pmi.scenes`, specify-pmi 0.1.7).
        result = JSON.parse(module.diff(args.oldName, args.old, args.newName, args.new, args.title, args.scenes === true));
      } else if (operation === 'interpret') {
        result = JSON.parse(module.interpret(
          JSON.stringify(args.analysis), JSON.stringify(args.answers), args.question, args.text,
        ));
      }
      self.postMessage({ id, ok: true, result });
    } catch (error) {
      const fatal = error instanceof WebAssembly.RuntimeError;
      self.postMessage({ id, ok: false, error: error instanceof Error ? error.message : String(error), code: fatal ? 'panic' : undefined, fatal });
    }
  });
};
