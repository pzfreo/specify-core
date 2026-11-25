// The review panel's pending edits (specify-panel.md decision 3): each change the person makes is
// held in the page, one per row of a part. Save sends the lists together, all or nothing.
// Discard drops them all (specify-core docs/open.md, `POST save`).
import type { AnswerDraft, EditFailure, EditList, RemoveTarget } from './local.ts';
import { withUnits } from '../src/specify/units.ts';

/** One pending edit, by the row it is on (a feature-and-kind id, or an item's `#N` in "Other PMI"). */
export type Pending = PendingEdit & { draft?: true };

type PendingEdit =
  /** A gap given a value: Add. */
  | { kind: 'fill'; id: string; name: string; value: unknown; text: string; clear?: true }
  /* ``clear``: a stored "none needed" replaced by a value, one bar entry that sends clear + fill. */
  /** A stated row given a new value: Change. ``expect`` is the file's value the person saw. */
  | { kind: 'change'; id: string; name: string; value: unknown; text: string; expect: unknown; was: string }
  /** A stated row (or an item of "Other PMI") removed: what `items.remove` names (removalTargets). */
  | { kind: 'remove'; id: string; name: string; targets: RemoveTarget[]; was: string }
  /** A gap decided "none needed". */
  | { kind: 'none'; id: string; name: string }
  /** A stored "none needed" taken back (Undo): the row is a gap again. Never touches PMI. */
  | { kind: 'clear'; id: string; name: string }
  /** An agent's refused answer, shown on its row until it is discarded. */
  | { kind: 'refused'; id: string; name: string; reason: string };

/** The edit list for ``pending``. */
export function editList(pending: Iterable<Pending>): EditList {
  const out: EditList = {};
  for (const p of pending) {
    if (p.kind === 'fill') {
      (out.fill ??= {})[p.id] = p.value;
      if (p.clear) (out.clear ??= []).push(p.id);
    }
    else if (p.kind === 'change') (out.change ??= {})[p.id] = { to: p.value, expect: p.expect };
    else if (p.kind === 'remove') {
      const list = ((out.items ??= {}).remove ??= []);
      for (const t of p.targets) if (!list.some((x) => JSON.stringify(x) === JSON.stringify(t))) list.push(t);
    }
    else if (p.kind === 'none') (out.none ??= []).push(p.id);
    else if (p.kind === 'clear') (out.clear ??= []).push(p.id);
  }
  return out;
}

/** The one route for a removal: a requirement's characteristic numbers when it has them, otherwise
 * the id of the stated row or of the existing-PMI item (the engine's `file_pmi:#<entity>`), as
 * `{"id": …}` in `items.remove`; none when it has neither. */
export function removalTargets(numbers: readonly string[], id?: string): RemoveTarget[] {
  return numbers.length ? [...numbers] : id ? [{ id }] : [];
}

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

function describeEdit(p: Pending): string {
  switch (p.kind) {
    case 'fill': return `Add ${p.name}: ${p.id.startsWith('part.') ? p.text : withUnits(p.text, 'mm')}`;
    // What it was is as the file states it: its unit is said where the page knows it (callOutEdit).
    case 'change': return `Change ${p.name}: ${p.was} → ${p.id.startsWith('part.') || /(?:mm|in)\b/.test(p.text) ? p.text : withUnits(p.text, 'mm')}`;
    case 'remove': {
      const numbers = p.targets.filter((t): t is string => typeof t === 'string');
      return `Remove ${p.name}: ${p.was}${numbers.length ? ` (${numbers.join(', ')})` : ''}`;
    }
    case 'none': return `None needed: ${p.name}`;
    case 'clear': return `Undo none needed: ${p.name}`;
    case 'refused': return `Refused ${p.name}: ${p.reason}`;
  }
}

/** The pending edits, one per row, in the order they were made. */
export class PendingEdits {
  private readonly byRow = new Map<string, Pending>();
  constructor(from?: Iterable<Pending>) { for (const p of from ?? []) this.byRow.set(p.id, p); }
  get size() { return this.byRow.size; }
  get(id: string) { return this.byRow.get(id); }
  values() { return [...this.byRow.values()]; }
  /** A copy with ``p`` in place of whatever its row had (moved to the end). */
  with(p: Pending): PendingEdits {
    const next = new PendingEdits(this.values().filter((q) => q.id !== p.id));
    next.byRow.set(p.id, p);
    return next;
  }
  /** A copy without row ``id``'s edit. */
  without(id: string): PendingEdits { return new PendingEdits(this.values().filter((q) => q.id !== id)); }
  list(): EditList { return editList(this.values()); }
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

const sameValue = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Rebuilds an edit made on a row whose ``saved`` edit was just written to the file (null: already matches). */
function rebaseOnSaved(edit: Pending, saved: Pending): Pending | null {
  if (saved === edit) return null;
  if (saved.kind === 'refused' || edit.kind === 'refused') return edit;
  if (saved.kind === 'fill' || saved.kind === 'change') {
    // The saved file now states ``saved.value`` (shown as ``saved.text``).
    if (edit.kind === 'fill' || edit.kind === 'change') {
      if (sameValue(edit.value, saved.value)) return null;
      return { kind: 'change', id: edit.id, name: edit.name, value: edit.value, text: edit.text, expect: saved.value, was: saved.text };
    }
    if (edit.kind === 'remove') return { ...edit, was: saved.text };
    if (edit.kind === 'none' || edit.kind === 'clear') {
      return { kind: 'remove', id: edit.id, name: edit.name, targets: removalTargets([], edit.id), was: saved.text };
    }
    return edit;
  }
  if (saved.kind === 'none') {
    // The saved file now holds ``decided_none`` for this row.
    if (edit.kind === 'none') return null;
    if (edit.kind === 'fill' || edit.kind === 'change') {
      return { kind: 'fill', id: edit.id, name: edit.name, value: edit.value, text: edit.text, clear: true };
    }
    if (edit.kind === 'remove') return { kind: 'clear', id: edit.id, name: edit.name };
    return edit;
  }
  // ``saved`` is ``clear`` or ``remove``: the row is a gap in the saved file now.
  if (edit.kind === saved.kind) return null;
  if (edit.kind === 'fill') {
    if (!edit.clear) return edit;
    const { clear: _, ...rest } = edit;
    return rest;
  }
  if (edit.kind === 'change') {
    return { kind: 'fill', id: edit.id, name: edit.name, value: edit.value, text: edit.text };
  }
  return edit;
}

/** Pending edits with the edits sent by a save removed; any edit made meanwhile on a saved row
 * is rebuilt against what was saved (or dropped if it already matches it). */
export function withoutSent(
  current: ReadonlyMap<number, PendingEdits>,
  sent: ReadonlyMap<number, PendingEdits>,
): Map<number, PendingEdits> {
  const remaining = new Map<number, PendingEdits>();
  for (const [part, list] of current) {
    const taken = sent.get(part);
    if (!taken) { remaining.set(part, list); continue; }
    const kept: Pending[] = [];
    for (const edit of list.values()) {
      const saved = taken.get(edit.id);
      if (!saved) { kept.push(edit); continue; }
      const next = rebaseOnSaved(edit, saved);
      if (next) kept.push(next);
    }
    if (kept.length) remaining.set(part, new PendingEdits(kept));
  }
  return remaining;
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
