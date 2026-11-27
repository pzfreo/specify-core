import { WorkerClient } from './rpc.ts';

/** The Specify intent engine in a worker, on the dependency @draftwright/specify-pmi-wasm. `wasmUrl`, for a host that
 * serves the engine itself, is the URL of its `specify_pmi_wasm.js` module, resolved against the page; a cross-origin
 * URL needs the host's CSP `script-src` (and `connect-src` for the .wasm) to allow it.
 * It never writes a STEP (specify-pmi 0.3.0: the `write` and `rematch` operations are gone; a file
 * changes only by an edit list, applied by specify-pmi's session Save or the engine's `edit`).
 * `review-given` accepts no defaults and supplies the writer's characteristic numbers from the
 * STEP geometry and history. */
export function createIntentEngine({ wasmUrl }: { wasmUrl?: string } = {}): WorkerClient {
  return new WorkerClient(() => {
    const worker = new Worker(new URL('./intent.worker.ts', import.meta.url), { type: 'module' });
    if (wasmUrl) worker.postMessage({ wasmUrl: new URL(wasmUrl, location.href).href });
    return worker;
  });
}
