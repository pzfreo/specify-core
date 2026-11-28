// The package's public surface.
// three-cad-viewer's stylesheet belongs to the main entry, not to cad-view.ts: a host that bundles /view's
// helpers (the picker, look) without a viewer need not have a loader for it. A host of /view alone, drawing
// createCadView, loads 'three-cad-viewer/css' itself.
import 'three-cad-viewer/css';
import { createCadView, createDefaultView } from './specify/cad-view';
import { withDefaultViews, type DefaultViews } from './specify/default-views';
import StepDiffBase from './components/StepDiff';
import AssemblyDiffViewBase from './components/AssemblyDiffView';
import { SpecifyReview as SpecifyReviewBase } from './specify/SpecifyReview';

// The components, with the package's own 3D view (three-cad-viewer) as their default when no `view` is given.
// (Typed as the components are, so the declarations point at them and not at wherever a prop's type is re-exported.)
// The `/host` entry (src/host.ts) has the same components bound to none.
const own: DefaultViews = { part: createDefaultView, assembly: (container, nodes, options) => createCadView(container, nodes, options) };
export const StepDiff: typeof StepDiffBase = withDefaultViews(StepDiffBase, own);
export const AssemblyDiffView: typeof AssemblyDiffViewBase = withDefaultViews(AssemblyDiffViewBase, own);
export const SpecifyReview: typeof SpecifyReviewBase = withDefaultViews(SpecifyReviewBase, own);
export type { StepDiffProps } from './components/StepDiff';
export type { AssemblyRow, StepFile } from './components/AssemblyDiffView';
export type { CalledOut, SpecifyEngine, SpecifySlots } from './specify/SpecifyReview';
export {
  drawnFromScene, withoutNotesBlock, hasGraphics,
  type DrawnGraphic, type DrawnPlane, type DrawnPmi, type DrawnPoint, type DrawnStatus, type TextPlacement,
} from './specify/drawn';
// Helpers the editor uses outside the review (the editor imports them from the package); the 3D
// view's are in '@draftwright/specify-ui/view' (src/view.ts).
export { needs } from './specify/review';
export { chosenForYou } from './specify/chosen';
export { CHARACTERISTIC } from './specify/labels';
export { sameBinding } from './specify/selection';
export { readOnlyEngine } from './specify/read-only';
export { createIntentEngine } from './engine/intent-engine';
export { WorkerClient } from './engine/rpc';
export type * from './engine/save';
export * from './specify/characteristics';
export { as9102, type FaiPart } from './specify/as9102';
export { xlsx, workbookFiles, zip, type Cell, type Sheet } from './model/xlsx';
export * from './model/diff-pmi';
export * from './model/diff';
export type * from './types/pmi';
export type {
  Box, CameraState, FaceIds, FaceRef, FaceStyle, FaceSample, HostCallout, HostPointerEvent, HostView, HostViewInfo, Mat4, MeshOptions, Vec3,
} from './specify/host-view';
export { useSpecifyHost, type UseSpecifyHost } from './specify/host/use-specify-host';
export { SpecifyHost, type SavedElsewhere, type SpecifyHostOptions, type SpecifyHostState, type ReviewPropsFromHost } from './specify/host/specify-host';
