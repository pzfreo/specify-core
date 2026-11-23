// The review panel's pending edits (specify-panel.md decision 3): each change the person makes is
// held in the page, one per row of a part. Save sends the lists together, all or nothing.
// Discard drops them all (specify-core docs/open.md, `POST save`).
import type { AnswerDraft, EditFailure } from './local.ts';

import { describeEdit, PendingEdits, type Pending } from '../src/specify/edit-list.ts';
export { editList, PendingEdits, removalTargets, type Pending } from '../src/specify/edit-list.ts';

/** "Add all suggestions": every gap of ``rows`` with a suggestion and no edit yet, as the edit Add or
 * None needed would make: a value suggested is a fill of it, a suggestion of none is a none. */
export function suggestionEdits(rows: readonly { id: string; name: string; state: string; suggestion?: unknown; suggestionText?: string }[],
  pending: PendingEdits, isNone: (value: unknown) => boolean): Pending[] {
  return rows.filter((r) => r.state === 'gap' && r.suggestion !== undefined && !pending.get(r.id))
    .map((r): Pending => (isNone(r.suggestion) ? { kind: 'none', id: r.id, name: r.name }
      : { kind: 'fill', id: r.id, name: r.name, value: r.suggestion, text: r.suggestionText ?? String(r.suggestion) }));
}

/** How the bar lists ``p``. */
export function describe(p: Pending, draft?: AnswerDraft): string {
  const text = describeEdit(p);
  if (!draft || p.draft) return text;
  const replaces = Object.hasOwn(draft.fill ?? {}, p.id) || Object.hasOwn(draft.change ?? {}, p.id)
    || draft.none?.includes(p.id) || draft.clear?.includes(p.id);
  return replaces ? `${text} (replaces the agent’s answer)` : text;
}

/** Working answers from the session, with page edits taking precedence on the same row. */
export function withDrafts(
  local: ReadonlyMap<number, PendingEdits>,
  drafts: Readonly<Record<string, AnswerDraft>>,
  describeRow: (part: number, id: string, value?: unknown) => { name: string; text: string },
): ReadonlyMap<number, PendingEdits> {
  if (!Object.keys(drafts).length) return local;
  const pending = new Map(local);
  for (const [key, list] of Object.entries(drafts)) {
    const part = Number(key);
    const staged: Pending[] = [
      ...Object.entries(list.refused ?? {}).map(([id, reason]): Pending => ({
        kind: 'refused', id, name: describeRow(part, id).name, reason, draft: true,
      })),
      ...Object.entries(list.fill ?? {}).map(([id, value]): Pending => ({
        kind: 'fill', id, value, ...describeRow(part, id, value), draft: true,
      })),
      ...Object.entries(list.change ?? {}).map(([id, { to, expect }]): Pending => ({
        kind: 'change', id, value: to, expect, was: describeRow(part, id, expect).text,
        ...describeRow(part, id, to), draft: true,
      })),
      ...(list.none ?? []).map((id): Pending => ({ kind: 'none', id, name: describeRow(part, id).name, draft: true })),
      ...(list.clear ?? []).map((id): Pending => ({ kind: 'clear', id, name: describeRow(part, id).name, draft: true })),
    ];
    pending.set(part, new PendingEdits([...staged, ...(local.get(part)?.values() ?? [])]));
  }
  return pending;
}

/** Matches save refusals to the queued None needed on the part they identify. */
export function rejectedNoneByPart(pending: ReadonlyMap<number, PendingEdits>, failures: readonly EditFailure[], assembly: boolean) {
  const rejected = new Map<number, Map<string, string>>();
  for (const [part, list] of pending) {
    const rows = new Map<string, string>();
    for (const edit of list.values()) {
      if (edit.kind !== 'none') continue;
      const entry = `none[${JSON.stringify(edit.id)}]`;
      const reasons = failures.filter((failure) =>
        (failure.part === part || (!assembly && failure.part === undefined))
        && (failure.editEntry ?? failure.entry) === entry);
      if (reasons.length) rows.set(edit.id, reasons.map((f) => f.reason).join('; '));
    }
    if (rows.size) rejected.set(part, rows);
  }
  return rejected;
}
