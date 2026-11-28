import type { EngineFile, EditOutcome, EditRequest, RematchRequest, Rematched } from './save';
import type { FeatureTable } from '../specify/edits/table';
import type { PmiAnswers } from '../types/pmi';

export class WorkerClient {
  private worker: Worker;
  private sequence = 0;
  generation = 0;
  private pending = new Map<number, {
    resolve: (value: unknown) => void;
    reject: (reason: Error) => void;
    timer: ReturnType<typeof setTimeout>;
  }>();

  private factory: () => Worker;
  constructor(factory: () => Worker) {
    this.factory = factory;
    this.worker = factory();
    this.attach();
  }

  private attach() {
    this.worker.onmessage = ({ data }) => {
      const pending = this.pending.get(data.id);
      if (!pending) return;
      clearTimeout(pending.timer);
      this.pending.delete(data.id);
      if (data.ok) pending.resolve(data.result);
      // An engine's refusal code, where it gives one, rides on the error (counted, never shown).
      else pending.reject(Object.assign(new Error(data.error), typeof data.code === 'string' ? { code: data.code } : {},
        data.specRefused === true ? { specRefused: true } : {}));
      if (data.fatal) this.restart(new Error('The engine panicked and was restarted.'));
    };
    this.worker.onerror = event => this.restart(new Error(event.message || 'The engine worker stopped.'));
    this.worker.onmessageerror = () => this.fail(new Error('The engine response could not be read.'));
  }

  request<Result>(operation: string, args: unknown = {}): Promise<Result> {
    return new Promise((resolve, reject) => {
      const id = ++this.sequence;
      const timer = setTimeout(() => {
        this.restart(new Error('The engine took more than three minutes and was restarted. Try a smaller part.'));
      }, 180000);
      this.pending.set(id, { resolve: value => resolve(value as Result), reject, timer });
      try { this.worker.postMessage({ id, operation, args }); }
      catch (error) { this.fail(error instanceof Error ? error : new Error(String(error))); }
    });
  }

  /** Applies an edit list (or a 0.2 answers file) to ``file`` in memory, all or nothing: the new bytes, or every entry
   * refused. Nothing is written to disk; the host stores ``bytes``. */
  edit(file: EngineFile, request: EditRequest): Promise<EditOutcome> { return this.request('edit', { ...file, ...request }); }
  /** Answers the host saved outside the STEP, matched to ``file`` (a new revision of the part they were made on). */
  rematch(file: EngineFile, request: RematchRequest): Promise<Rematched> { return this.request('rematch', { ...file, ...request }); }
  /** ``answers`` with Specify's defaults accepted for ``file`` (the answers its default path gives): hand them to ``edit``
   * as a 0.2 answers file, `edit(file, { edits: await acceptDefaults(file, answers) })`. Reads only. */
  acceptDefaults(file: EngineFile, answers: PmiAnswers): Promise<PmiAnswers> { return this.request('accept-defaults', { ...file, answers }); }
  /** The feature table of ``file``: each row's state, value and suggestion; row ids are the ids an edit list names. */
  table(file: EngineFile): Promise<FeatureTable> { return this.request('table', file); }

  private fail(error: Error) {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
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
