// The package's own 3D view, as the components reach it. The components draw through a HostView and
// make the package's own (on three-cad-viewer) only when given none; they get its factories from here,
// so that the `/host` entry, which wires none, reaches no three-cad-viewer. The main entry wires them
// (src/index.ts). Types only here: nothing of three-cad-viewer's is imported.
import type { PmiMesh, PmiVec } from '../types/pmi';

import type { CadNode, CadView } from './cad-view';
import type { DefaultView } from './default-view';

export interface DefaultViews {
  /** A part's view (createDefaultView). */
  part(container: HTMLElement, options: {
    part: number; mesh: PmiMesh; bounds?: readonly [PmiVec, PmiVec];
    onPick?: (path: string) => void; control?: 'orbit' | 'trackball'; tools?: boolean;
  }): DefaultView;
  /** An assembly's view (createCadView). */
  assembly(container: HTMLElement, nodes: CadNode[], options: { onPick?: (path: string) => void }): CadView;
}

let wired: DefaultViews | null = null;

export function wireDefaultViews(views: DefaultViews): void { wired = views; }

export function defaultViews(): DefaultViews {
  if (!wired) {
    throw new Error('No view was given. Pass `view` (a HostView): the `@draftwright/specify-ui/host` entry has no built-in view; use the main entry for the package\'s own.');
  }
  return wired;
}
