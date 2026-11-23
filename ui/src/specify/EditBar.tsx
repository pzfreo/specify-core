import { describeEdit, type Pending } from './edit-list';

/** The bar at the bottom (decision 3): every part's pending edits, Save and Discard. */
export function EditBar({ items, saving, busy, blocked, savedNote, onSave, onDiscard, onAddAll, onUndo }: {
  items: { part: number; name?: string; edit: Pending; suffix?: string; target?: { part: number; id: string } }[];
  saving: boolean; busy: boolean; blocked: string | undefined; savedNote: string | null;
  onSave(): void; onDiscard(): void; onAddAll(): void; onUndo(part: number, id: string, draft: boolean): void;
}) {
  return (
    <div className="spec-bar" role="region" aria-label="Changes to save">
      <ul>
        {items.length ? items.map(({ part, name, edit, suffix, target }) => (
          <li key={`${target?.part ?? part}:${target?.id ?? edit.id}`} data-part={part} data-pending={edit.id}>
            {name ? `${name} · ` : ''}{describeEdit(edit)}{suffix}
            {(edit.draft || target) && <>{' '}<button type="button" disabled={busy || Boolean(blocked)}
              aria-label={`Undo ${name ?? ''} ${edit.name}`.replace(/ +/g, ' ')} onClick={() => onUndo(target?.part ?? part, target?.id ?? edit.id, Boolean(edit.draft))}>Undo</button></>}
          </li>
        )) : <li className="muted">{savedNote ?? 'No changes yet. Nothing is written until Save.'}</li>}
        {items.length > 0 && items.every(({ edit }) => edit.kind === 'refused') && savedNote
          && <li className="muted">{savedNote}</li>}
      </ul>
      <button type="button" className="app-add-all" onClick={onAddAll} disabled={busy || Boolean(blocked)}
        title="Queue this part's gap suggestions as edits (a suggestion of none as None needed); nothing is written until Save">
        Add all suggestions
      </button>
      <button type="button" className="app-discard" onClick={onDiscard} disabled={!items.length || busy}>Discard</button>
      <button type="button" className="app-save" onClick={onSave} disabled={busy || Boolean(blocked)} title={blocked}>
        {saving ? 'Saving…' : 'Save'}
      </button>
    </div>
  );
}
