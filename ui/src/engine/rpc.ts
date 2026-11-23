export class WorkerClient {
  private worker: Worker;
  private sequence = 0;
  generation = 0;
  private pending = new Map<number, {
    resolve: (value: unknown) => void;
    reject: (reason: unknown) => void;
    timer: ReturnType<typeof setTimeout>;
    cleanup: () => void;
  }>();

  private factory: () => Worker;
  constructor(factory: () => Worker) {
    this.factory = factory;
    this.worker = factory();
    this.attach();
  }

  private attach() {
    const worker = this.worker;
    worker.onmessage = ({ data }) => {
      if (worker !== this.worker) return;
      const pending = this.take(data.id);
      if (pending) {
        if (data.ok) pending.resolve(data.result);
        // An engine's refusal code, where it gives one, rides on the error (counted, never shown).
        else pending.reject(Object.assign(new Error(data.error), typeof data.code === 'string' ? { code: data.code } : {},
          data.specRefused === true ? { specRefused: true } : {}));
      }
      // Even a cancelled request can panic in the worker.
      if (data.fatal) this.restart(new Error('The engine panicked and was restarted.'));
    };
    worker.onerror = event => { if (worker === this.worker) this.restart(new Error(event.message || 'The engine worker stopped.')); };
    worker.onmessageerror = () => { if (worker === this.worker) this.fail(new Error('The engine response could not be read.')); };
  }

  /** Aborting rejects this caller immediately. Work already posted may still finish in the worker. */
  request<Result>(operation: string, args: unknown = {}, signal?: AbortSignal): Promise<Result> {
    return new Promise((resolve, reject) => {
      signal?.throwIfAborted();
      const id = ++this.sequence;
      const timer = setTimeout(() => {
        this.restart(new Error('The engine took more than three minutes and was restarted. Try a smaller part.'));
      }, 180000);
      const abort = () => this.take(id)?.reject(signal?.reason);
      this.pending.set(id, { resolve: value => resolve(value as Result), reject, timer,
        cleanup: () => signal?.removeEventListener('abort', abort) });
      signal?.addEventListener('abort', abort, { once: true });
      try { this.worker.postMessage({ id, operation, args }); }
      catch (error) { this.fail(error instanceof Error ? error : new Error(String(error))); }
    });
  }

  private take(id: number) {
    const pending = this.pending.get(id);
    if (pending) {
      this.pending.delete(id);
      clearTimeout(pending.timer);
      pending.cleanup();
    }
    return pending;
  }

  private fail(error: Error) {
    for (const id of this.pending.keys()) this.take(id)?.reject(error);
  }

  private restart(error: Error) {
    this.fail(error);
    this.worker.terminate();
    this.generation++;
    this.worker = this.factory();
    this.attach();
  }

  dispose() {
    this.fail(new Error('The engine was closed.'));
    this.worker.terminate();
  }
}
