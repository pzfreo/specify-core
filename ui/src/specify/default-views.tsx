// The package's own 3D view, as the components reach it. The components draw through a HostView and
// make the package's own (on three-cad-viewer) only when given none; the factories for it arrive through
// a context that the entry's exported components provide explicitly (withDefaultViews), so the `/host`
// entry, which binds none, reaches no three-cad-viewer, and nothing depends on a module's side effect.
// Types only here: nothing of three-cad-viewer's is imported.
import { createContext, useContext, type ComponentType, type ReactElement } from 'react';

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

const DefaultViewsContext = createContext<DefaultViews | null>(null);

/** The component's default-view factories: none on the `/host` entry. */
export const useDefaultViews = (): DefaultViews | null => useContext(DefaultViewsContext);

/** ``views``, or an error that says to pass a `view`. */
export function needViews(views: DefaultViews | null | undefined): DefaultViews {
  if (!views) {
    throw new Error('No view was given. Pass `view` (a HostView): the `@draftwright/specify-ui/host` entry has no built-in view; use the main entry for the package\'s own.');
  }
  return views;
}

/** ``Component``, with ``views`` as its default view. */
export function withDefaultViews<P extends object>(Component: ComponentType<P>, views: DefaultViews): (props: P) => ReactElement {
  return function WithDefaultViews(props: P) {
    return <DefaultViewsContext.Provider value={views}><Component {...props} /></DefaultViewsContext.Provider>;
  };
}
