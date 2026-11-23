import { useState, type ReactNode } from 'react';
import { IconButton, RemoveLine } from './row-actions';
import { requirementLabel, rowSuggestionText, rowText, type FileRow } from './file-review';
import { isNoneValue } from './feature-table';
import type { Pending } from './edit-list';

interface Props {
  row: FileRow;
  name: string;
  pending?: Pending;
  open: boolean;
  disabled: boolean;
  problem?: string;
  children?: ReactNode;
  onOpen(): void;
  onClose(): void;
  onUndo(): void;
  onRemove(): Promise<boolean>;
  removalWarning?: string | null;
  copies?: number;
}

export function fileRequirementState(row: FileRow, edit: Pending | undefined): FileRow['state'] {
  if (edit?.kind === 'clear') return 'gap';
  if (edit?.kind === 'none') return 'decided_none';
  if (edit?.kind === 'fill' || edit?.kind === 'accept' || edit?.kind === 'change' || edit?.kind === 'replace') {
    return edit.kind === 'replace' && isNoneValue(edit.value) ? 'decided_none' : 'stated';
  }
  return row.state;
}

/** A file row in the old review's groups. A gap opens the original question controls. */
export function FileRequirement({ row, name, pending, open, disabled, problem, children, onOpen, onClose, onUndo, onRemove, removalWarning, copies }: Props) {
  const [removing, setRemoving] = useState(false);
  const { question } = row;
  const numbers = row.requirements?.flatMap(r => r.number ? [r.number] : []) ?? [];
  const edit = pending?.kind === 'refused' ? undefined : pending;
  const removed = edit?.kind === 'remove';
  const entered = edit?.kind === 'fill' || edit?.kind === 'replace' || edit?.kind === 'change' || edit?.kind === 'accept' ? edit : undefined;
  const decidedNone = edit?.kind === 'none' || (entered ? isNoneValue(entered.value) : isNoneValue(row.value));
  const state = fileRequirementState(row, edit);
  const value = state === 'decided_none' && decidedNone ? 'none needed' : entered?.text ?? rowText(row);
  const suggested = rowSuggestionText(row);
  const notes = row.id === 'part.notes' ? entered?.value ?? row.value : null;
  return (
    <div className={`pmi-lab-file-row${row.other ? ' spec-row' : ''} ${state}${edit ? ' changed' : ''}${removed ? ' removed' : ''}`}
      data-row={row.id} data-question={row.id} data-state={state} data-pending-row={edit ? '' : undefined}>
      <div className="pmi-lab-line">
        <button type="button" className="name" onClick={open ? onClose : onOpen}>{requirementLabel(question)}</button>
        <span className="value">
          {state === 'gap' ? (suggested && !children ? <span className="sugg">suggested: {suggested}</span> : 'not stated')
            : Array.isArray(notes) ? notes.map((note, i) => <span className="spec-note" key={i}>{String(note)}</span>) : value}
          {state === 'stated' && numbers.length > 0 && <span className="bal" title="Balloon numbers">{numbers.join(', ')}</span>}
        </span>
        <span className="acts">
          {removed ? <button type="button" aria-label="Undo removal" onClick={onUndo}>Undo</button>
            : row.other ? (row.other.number || row.other.id) && <IconButton label={`Remove ${value}`} icon="bin" disabled={disabled}
              onClick={() => { onClose(); setRemoving(true); }} />
            : state === 'gap' || state === 'decided_none' ? (state === 'decided_none' || edit) &&
              <button type="button" onClick={onUndo}>Undo</button>
              : row.state === 'stated' && !row.bom ? <>
                <IconButton label={`Change ${name}`} icon="edit" disabled={disabled} onClick={() => { setRemoving(false); onOpen(); }} />
                <IconButton label={`Remove ${name}`} icon="bin" disabled={disabled} onClick={() => { onClose(); setRemoving(true); }} />
                {edit && <button type="button" onClick={onUndo}>Undo</button>}
              </> : <>
                <button type="button" onClick={onOpen}>Edit</button>
                {edit && <button type="button" onClick={onUndo}>Undo</button>}
              </>}
        </span>
      </div>
      {copies !== undefined && copies > 1 && <small className="pmi-lab-copy-count">applies to {copies} identical parts</small>}
      {problem && !removing && <div className="pmi-lab-error spec-problem" role="alert">{problem}</div>}
      {!removed && !removing && children}
      {removing && !removed && (
        <>
          {removalWarning && <p className="why">{removalWarning}</p>}
          <RemoveLine what={rowText(row)} numbers={numbers} problem={problem ?? null} checking={disabled}
            onCancel={() => setRemoving(false)} onRemove={() => { void onRemove().then(ok => { if (ok) setRemoving(false); }); }} />
        </>
      )}
    </div>
  );
}
