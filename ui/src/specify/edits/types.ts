// The 0.3 engine's edit list and its refusals (specify-pmi docs/session.md): what a host saves with.

/** An edit list (specify-pmi docs/session.md, the 0.3.0 edit list): ``fill`` gaps with a value (a
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

/** The session's working answers kept for a row, by part (an agent's, shown until the person's page edit replaces them). */
export type AnswerDraft = Pick<EditList, 'fill' | 'change' | 'none' | 'clear'> & { refused?: Record<string, string> };

