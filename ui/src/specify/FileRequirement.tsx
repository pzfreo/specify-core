import { useState, type ReactNode } from 'react';
import { IconButton, RemoveLine } from './row-actions';
import { answerText, isNoneValue } from './feature-table';
import { rowText, type FileRow } from './file-review';
import type { Pending } from './edit-list';

interface Props {
  row: FileRow;
  name: string;
  pending?: Pending;
  open: boolean;
  disabled: boolean;
  problem?: string;
  card?: ReactNode;
  children?: ReactNode;
  onOpen(): void;
  onClose(): void;
  onNone(): void;
  onKeep(): void;
  onUndo(): void;
  onRemove(): Promise<boolean>;
  removalWarning?: string | null;
}

/** A file row in the old review's groups. A gap opens the original question controls. */
export function FileRequirement({ row, name, pending, open, disabled, problem, card, children, onOpen, onClose, onNone, onKeep, onUndo, onRemove, removalWarning }: Props) {
  const [removing, setRemoving] = useState(false);
  const { question } = row;
  const numbers = row.requirements?.flatMap(r => r.number ? [r.number] : []) ?? [];
  const edit = pending?.kind === 'refused' ? undefined : pending;
  const removed = edit?.kind === 'remove';
  const entered = edit?.kind === 'fill' || edit?.kind === 'change' || edit?.kind === 'accept' ? edit : undefined;
  const state = edit?.kind === 'clear' ? 'gap'
    : edit?.kind === 'none' ? 'decided_none'
      : entered ? 'stated' : row.state;
  const value = state === 'decided_none' ? 'none needed' : entered?.text ?? rowText(row);
  const suggested = question.default == null ? '' : isNoneValue(question.default) ? 'none' : answerText(question.default, question);
  const notes = row.id === 'part.notes' ? entered?.value ?? row.value : null;
  return (
    <div className={`pmi-lab-file-row ${state}${edit ? ' changed' : ''}${removed ? ' removed' : ''}`}
      data-row={row.id} data-question={row.id} data-state={state} data-pending-row={edit ? '' : undefined}>
      <div className="pmi-lab-line">
        <button type="button" className="name" onClick={onOpen}>{name}</button>
        <span className="value">
          {state === 'gap' ? (suggested && <span className="sugg">suggested: {suggested}</span>)
            : Array.isArray(notes) ? notes.map((note, i) => <span className="spec-note" key={i}>{String(note)}</span>) : value}
          {state === 'stated' && numbers.length > 0 && <span className="bal" title="Balloon numbers">{numbers.join(', ')}</span>}
        </span>
        <span className="acts">
          {removed ? <button type="button" aria-label="Undo removal" onClick={onUndo}>Undo</button>
            : state === 'gap' ? <>
              <button type="button" aria-expanded={open} onClick={onOpen}>Add</button>
              <button type="button" onClick={onNone}>None needed</button>
              {edit && <button type="button" onClick={onUndo}>Undo</button>}
            </> : state === 'decided_none' ? <button type="button" onClick={onUndo}>Undo</button>
              : row.state === 'stated' && !row.bom ? <>
                <IconButton label={`Change ${name}`} icon="edit" disabled={disabled} onClick={() => { setRemoving(false); onOpen(); }} />
                <IconButton label={`Remove ${name}`} icon="bin" disabled={disabled} onClick={() => { onClose(); setRemoving(true); }} />
              </> : <>
                <button type="button" onClick={onOpen}>Edit</button>
                {edit && <button type="button" onClick={onUndo}>Undo</button>}
              </>}
        </span>
      </div>
      {state === 'gap' && (card ?? (suggested && (
        <button type="button" className="pmi-lab-keep" onClick={onKeep}>Keep suggestion</button>
      )))}
      {problem && !removing && <div className="pmi-lab-error spec-problem" role="alert">{problem}</div>}
      {open && !removed && !removing && children}
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
