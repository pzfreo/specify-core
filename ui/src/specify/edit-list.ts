/** An edit list (specify-core docs/session.md, the 0.3.0 edit list): ``fill`` gaps with a value (a
 * value meaning none records "decided: none"), ``change`` what the file states (``expect``: the value
 * seen), ``items`` by characteristic number, ``none`` decided none, ``accept_suggestions``, ``clear``
 * a stored decision. */
export interface EditList {
  fill?: Record<string, unknown>;
  change?: Record<string, { to: unknown; expect: unknown }>;
  items?: { change?: Record<string, { tolerance: string; expect: string }>; remove?: RemoveTarget[] };
  none?: string[];
  accept_suggestions?: string[];
  /** Stored decisions that leave no PMI to take back (a decided none, a choice such as how holes
   * are located): a gap, or unanswered, again. Never PMI. */
  clear?: string[];
}

/** What `items.remove` names: a characteristic number ("#4"), or, for a requirement without one, the
 * stated row's id or the existing-PMI item's id (`{"id": …}`; the engine route for these is being added). */
export type RemoveTarget = string | { id: string };

/** An edit list scoped to a distinct part index from the file's part picker. */
export interface PartEdits { part: number; edits: EditList }
export type SaveEdits = EditList | PartEdits[];

/** One entry of an edit list that cannot be applied, and why. */
export interface EditFailure { entry: string; reason: string; part?: number; editEntry?: string }

/** One pending edit, by the row it is on (a feature-and-kind id, or an item's `#N` in "Other PMI"). */
export type Pending = PendingEdit & { draft?: true };

type PendingEdit =
  /** A gap given a value: Add. */
  | { kind: 'fill'; id: string; name: string; value: unknown; text: string }
  /** A stated row given a new value: Change. ``expect`` is the file's value the person saw. */
  | { kind: 'change'; id: string; name: string; value: unknown; text: string; expect: unknown; was: string }
  /** A stated row (or an item of "Other PMI") removed: what `items.remove` names (removalTargets). */
  | { kind: 'remove'; id: string; name: string; targets: RemoveTarget[]; was: string }
  /** A gap filled with the file's suggestion. The value shown is also used in the preview. */
  | { kind: 'accept'; id: string; name: string; value: unknown; text: string }
  /** A gap decided "none needed". */
  | { kind: 'none'; id: string; name: string }
  /** A stored "none needed" taken back (Undo): the row is a gap again. Never touches PMI. */
  | { kind: 'clear'; id: string; name: string }
  /** A host-supplied refusal, shown on its row until discarded. */
  | { kind: 'refused'; id: string; name: string; reason: string };

/** The edit list for ``pending``. */
export function editList(pending: Iterable<Pending>): EditList {
  const out: EditList = {};
  for (const p of pending) {
    if (p.kind === 'fill') (out.fill ??= {})[p.id] = p.value;
    else if (p.kind === 'accept') (out.accept_suggestions ??= []).push(p.id);
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

export function describeEdit(p: Pending): string {
  switch (p.kind) {
    case 'accept':
    case 'fill': return `Add ${p.name}: ${p.text}`;
    case 'change': return `Change ${p.name}: ${p.was} → ${p.text}`;
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

