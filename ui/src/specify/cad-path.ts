// A node's path in the package's own view's tree, kept apart from cad-view.ts so the components can
// use it without reaching three-cad-viewer.
export const ROOT = 'Part';
/** A node's path: the viewer's tree finds what to show, hide or light by names. */
export const pathOf = (...names: string[]) => `/${[ROOT, ...names].join('/')}`;
