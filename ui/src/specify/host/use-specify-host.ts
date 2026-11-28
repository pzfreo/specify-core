import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import type { EditOutcome } from '../../engine/save.ts';
import { SpecifyHost, type ReviewPropsFromHost, type SpecifyHostOptions, type SpecifyHostState } from './specify-host.ts';

export interface UseSpecifyHost {
  /** Spread into SpecifyReview; null until the file's table is read (and while another file is being read). */
  reviewProps: ReviewPropsFromHost | null;
  /** Composes answers, removals and call-outs into one edit list, applies it and reads the saved file again. null: nothing to save. */
  save(): Promise<EditOutcome | null>;
  saving: boolean;
  /** What the last written save reported. */
  lastReport: SpecifyHostState['lastReport'];
  /** The last save's refusal, naming each entry. */
  refused: SpecifyHostState['refused'];
  loading: boolean;
  error: string | null;
  /** Reads the file's table again. */
  refresh(): Promise<void>;
  /** The edit list Save would send now. */
  compose(): Promise<import('../edits/types.ts').EditList | null>;
}

/** The host side of SpecifyReview in one hook: see SpecifyHostOptions. Props the host owns (engine, session, drawn view, slots) stay its own. */
export function useSpecifyHost(options: SpecifyHostOptions): UseSpecifyHost {
  const [host] = useState(() => new SpecifyHost(options));
  host.options = options;
  const { file } = options;
  useEffect(() => { void host.setFile(file); }, [host, file.bytes, file.name, file.sourceId, file.part]);
  const state = useSyncExternalStore(host.subscribe, host.getSnapshot);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const reviewProps = useMemo(() => host.reviewProps(), [host, state, options.answers, options.drawn]);
  return {
    reviewProps, saving: state.saving, lastReport: state.lastReport, refused: state.refused, loading: state.loading, error: state.error,
    save: () => host.save(), refresh: () => host.refresh(), compose: () => host.compose(),
  };
}
