// The bar at the bottom of the local app: every change held in the page, until Save writes them
// together, all or nothing (0.3: "edits collect; Save writes them together"). Discard drops them.
// Moved from the rc.1 panel unchanged, less its "Add all suggestions": the review's own Keep all
// does that.
import type { AnswerDraft } from './local.ts';
import { describe, type Pending } from './edits.ts';

export function EditBar({ items, drafts, saving, busy, blocked, savedNote, onSave, onDiscard, onUndoDraft }: {
  items: { part: number; name?: string; edit: Pending }[];
  drafts: Readonly<Record<string, AnswerDraft>>;
  saving: boolean; busy: boolean; blocked: string | undefined; savedNote: string | null;
  onSave(): void; onDiscard(): void; onUndoDraft(part: number, id: string): void;
}) {
  return (
    <div className="spec-bar" role="region" aria-label="Changes to save">
      <ul>
        {items.length ? items.map(({ part, name, edit }) => (
          <li key={`${part}:${edit.id}`} data-part={part} data-pending={edit.id}>
            {name ? `${name} · ` : ''}{describe(edit, drafts[part])}
            {edit.draft && <>{' '}<button type="button" disabled={busy || Boolean(blocked)}
              aria-label={`Undo ${name ?? ''} ${edit.name}`.replace(/ +/g, ' ')} onClick={() => onUndoDraft(part, edit.id)}>Undo</button></>}
          </li>
        )) : <li className="muted">{savedNote ?? 'No changes yet. Nothing is written until Save.'}</li>}
        {items.length > 0 && items.every(({ edit }) => edit.kind === 'refused') && savedNote
          && <li className="muted">{savedNote}</li>}
      </ul>
      <button type="button" className="app-discard" onClick={onDiscard} disabled={!items.length || busy}>Discard</button>
      <button type="button" className="app-save" onClick={onSave} disabled={busy || Boolean(blocked)} title={blocked}>
        {saving ? 'Saving…' : 'Save'}
      </button>
    </div>
  );
}
