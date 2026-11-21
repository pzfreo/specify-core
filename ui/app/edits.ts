// The review panel's pending edits (specify-panel.md decision 3): each change the person makes is
// held in the page, one per row, listed in the bar, and sent together by Save as one edit list
// (specify-core docs/open.md, `POST save {rev, edits}`), all or nothing. Discard drops them all.
import type { EditList } from './local.ts';

/** One pending edit, by the row it is on (a feature-and-kind id, or an item's `#N` in "Other PMI"). */
export type Pending =
  /** A gap given a value: Add. */
  | { kind: 'fill'; id: string; name: string; value: unknown; text: string }
  /** A stated row given a new value: Change. ``expect`` is the file's value the person saw. */
  | { kind: 'change'; id: string; name: string; value: unknown; text: string; expect: unknown; was: string }
  /** A stated row (or an item of "Other PMI") removed: its characteristic numbers, retired. */
  | { kind: 'remove'; id: string; name: string; numbers: string[]; was: string }
  /** A gap decided "none needed". */
  | { kind: 'none'; id: string; name: string }
  /** A stored "none needed" taken back (Undo): the row is a gap again. Never touches PMI. */
  | { kind: 'clear'; id: string; name: string };

/** The edit list for ``pending``. */
export function editList(pending: Iterable<Pending>): EditList {
  const out: EditList = {};
  for (const p of pending) {
    if (p.kind === 'fill') (out.fill ??= {})[p.id] = p.value;
    else if (p.kind === 'change') (out.change ??= {})[p.id] = { to: p.value, expect: p.expect };
    else if (p.kind === 'remove') ((out.items ??= {}).remove ??= []).push(...p.numbers.filter((n) => !out.items!.remove!.includes(n)));
    else if (p.kind === 'none') (out.none ??= []).push(p.id);
    else (out.clear ??= []).push(p.id);
  }
  return out;
}

/** How the bar lists ``p``. */
export function describe(p: Pending): string {
  switch (p.kind) {
    case 'fill': return `Add ${p.name}: ${p.text}`;
    case 'change': return `Change ${p.name}: ${p.was} → ${p.text}`;
    case 'remove': return `Remove ${p.name}${p.numbers.length ? ` (${p.numbers.join(', ')})` : ''}`;
    case 'none': return `None needed: ${p.name}`;
    case 'clear': return `Undo none needed: ${p.name}`;
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
