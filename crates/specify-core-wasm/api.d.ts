export class IntentEngine {
  [Symbol.dispose](): void;
  acceptDefaults(answers: string): string;
  constructor(name: string, bytes: Uint8Array);
  evidence(): string;
  free(): void;
  original(): Uint8Array | undefined;
  payload(name: string): string | undefined;
  rematchAnswers(answers: string, evidence?: string | null, automatic?: string | null): string;
  review(answers: string): string;
  settle(state: string): string;
  snapshot(): string;
  static openPart(name: string, bytes: Uint8Array, part: number): IntentEngine;
  static parts(name: string, bytes: Uint8Array): string;
  writePmi(answers: string, project?: string | null, payload?: string | null): string;
}

export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;

export function acceptDefaults(analysis: string, answers: string): string;

export function answerEdit(state: string, answers: string): string;

export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

export function interpret(analysis: string, answers: string, question_id: string, text: string): string;

export function openedTolerancing(state: string, resume?: string | null): string;

export function review(analysis: string, answers: string): string;

export function start(): void;

export function tolerancingEdit(state: string, mode: string | null | undefined, answers: string): string;

export interface InitOutput {
  readonly __externref_table_dealloc: (a: number) => void;
  readonly __wbg_intentengine_free: (a: number, b: number) => void;
  readonly __wbindgen_externrefs: WebAssembly.Table;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_start: () => void;
  readonly acceptDefaults: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly answerEdit: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly intentengine_acceptDefaults: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_evidence: (a: number) => [number, number, number, number];
  readonly intentengine_open: (a: number, b: number, c: number, d: number) => [number, number, number];
  readonly intentengine_openPart: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
  readonly intentengine_original: (a: number) => [number, number];
  readonly intentengine_parts: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly intentengine_payload: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_rematchAnswers: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number, number, number];
  readonly intentengine_review: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_settle: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_snapshot: (a: number) => [number, number];
  readonly intentengine_writePmi: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number, number, number];
  readonly interpret: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number, number, number];
  readonly memory: WebAssembly.Memory;
  readonly openedTolerancing: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly review: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly start: () => void;
  readonly tolerancingEdit: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export type SyncInitInput = BufferSource | WebAssembly.Module;
