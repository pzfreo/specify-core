// The package's public surface.
// three-cad-viewer's stylesheet belongs to the main entry, not to cad-view.ts: a host that bundles /view's
// helpers (the picker, look) without a viewer need not have a loader for it. A host of /view alone, drawing
// createCadView, loads 'three-cad-viewer/css' itself.
import 'three-cad-viewer/css';
import { createCadView, createDefaultView } from './specify/cad-view';
import { wireDefaultViews } from './specify/default-views';

// The package's own 3D view (three-cad-viewer) is the components' default here. Wired in this module's
// body, not a module of its own: the package is flagged side-effect free, so a module with no used export is dropped.
wireDefaultViews({ part: createDefaultView, assembly: (container, nodes, options) => createCadView(container, nodes, options) });

export { default as StepDiff, type StepDiffProps } from './components/StepDiff';
export { default as AssemblyDiffView, type AssemblyRow, type StepFile } from './components/AssemblyDiffView';
export { SpecifyReview, type CalledOut, type SpecifyEngine, type SpecifySlots } from './specify/SpecifyReview';
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
export * from './specify/characteristics';
export { as9102, type FaiPart } from './specify/as9102';
export { xlsx, workbookFiles, zip, type Cell, type Sheet } from './model/xlsx';
export * from './model/diff-pmi';
export * from './model/diff';
export type * from './types/pmi';
export type {
  Box, CameraState, FaceIds, FaceRef, FaceStyle, FaceSample, HostCallout, HostPointerEvent, HostView, HostViewInfo, Mat4, MeshOptions, Vec3,
} from './specify/host-view';
