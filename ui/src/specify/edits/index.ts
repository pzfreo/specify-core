// The save side of the review, for a host that embeds SpecifyReview ('@draftwright/specify-ui/edits'):
// the answers-to-edit-list translation, the feature table's panel model and the pending-edit queue.
// A host builds an edit list with `editList(pending)` and saves it with the engine's `edit` operation.
export * from './types';
export * from './table';
export * from './adapter';
export * from './pending';
export type * from '../../engine/save';
