// The components for a host that draws the 3D view itself: '@draftwright/specify-ui/host'. Each one takes
// its HostView (`view`, required) and has no built-in viewer, so nothing here reaches three-cad-viewer,
// which a host on this entry need not install. The main entry has the same components with the package's
// own view as the default.
import type { ComponentProps } from 'react';

import AssemblyDiffViewBase from './components/AssemblyDiffView';
import StepDiffBase from './components/StepDiff';
import { SpecifyReview as SpecifyReviewBase } from './specify/SpecifyReview';
import type { HostView } from './specify/host-view';

type WithView<P> = Omit<P, 'view'> & { view: HostView };

export const SpecifyReview = SpecifyReviewBase as (props: WithView<ComponentProps<typeof SpecifyReviewBase>>) => ReturnType<typeof SpecifyReviewBase>;
export const StepDiff = StepDiffBase as (props: WithView<ComponentProps<typeof StepDiffBase>>) => ReturnType<typeof StepDiffBase>;
export const AssemblyDiffView = AssemblyDiffViewBase as (props: WithView<ComponentProps<typeof AssemblyDiffViewBase>>) => ReturnType<typeof AssemblyDiffViewBase>;

export type { StepDiffProps } from './components/StepDiff';
export type { AssemblyRow, StepFile } from './components/AssemblyDiffView';
export type { CalledOut, SpecifyEngine, SpecifySlots } from './specify/SpecifyReview';
export type { HostView } from './specify/host-view';
export type {
  Box, CameraState, FaceIds, FaceRef, FaceStyle, FaceSample, HostCallout, HostPointerEvent, HostViewInfo, Mat4, MeshOptions, Vec3,
} from './specify/host-view';
export type * from './types/pmi';
export { createIntentEngine } from './engine/intent-engine';
export { WorkerClient } from './engine/rpc';
export { useSpecifyHost, type UseSpecifyHost } from './specify/host/use-specify-host';
export { SpecifyHost, type SpecifyHostOptions, type SpecifyHostState, type ReviewPropsFromHost } from './specify/host/specify-host';
