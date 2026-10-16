export class IntentEngine {
  [Symbol.dispose](): void;
  constructor(name: string, bytes: Uint8Array);
  free(): void;
  review(answers: string): string;
  snapshot(): string;
  writePmi(answers: string, project?: string | null): string;
}

export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;

export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

export function interpret(analysis: string, answers: string, question_id: string, text: string): string;

export function review(analysis: string, answers: string): string;

export function start(): void;

export interface InitOutput {
  readonly __externref_table_dealloc: (a: number) => void;
  readonly __wbg_intentengine_free: (a: number, b: number) => void;
  readonly __wbindgen_externrefs: WebAssembly.Table;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_start: () => void;
  readonly intentengine_open: (a: number, b: number, c: number, d: number) => [number, number, number];
  readonly intentengine_review: (a: number, b: number, c: number) => [number, number, number, number];
  readonly intentengine_snapshot: (a: number) => [number, number];
  readonly intentengine_writePmi: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
  readonly interpret: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number, number, number];
  readonly memory: WebAssembly.Memory;
  readonly review: (a: number, b: number, c: number, d: number) => [number, number, number, number];
  readonly start: () => void;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export type SyncInitInput = BufferSource | WebAssembly.Module;
