// three-cad-viewer, loaded on demand: a host that gives the components its own view (HostView)
// never loads it, and need not install it (it is an optional peer dependency). The package's own
// view (cad-view.ts) reads it through ``cadViewer()`` once ``loadCadViewer()`` has resolved.
type Lib = typeof import('three-cad-viewer');

const MISSING = 'three-cad-viewer is needed for the package\'s own 3D view, and could not be loaded. '
  + 'Install it (npm install three-cad-viewer@~5.1.3), or pass your own view to the component (the `view` prop).';

let lib: Lib | null = null;
let loading: Promise<void> | null = null;

/** Loads three-cad-viewer and its stylesheet; rejects with an error naming the package if it is missing. */
export function loadCadViewer(): Promise<void> {
  loading ??= (async () => {
    try {
      lib = await import('three-cad-viewer');
      await import('three-cad-viewer/css');
    } catch (cause) {
      loading = null;
      lib = null;
      throw new Error(MISSING, { cause });
    }
  })();
  return loading;
}

/** Whether ``loadCadViewer()`` has resolved. */
export const cadViewerLoaded = () => lib !== null;

/** The loaded module; ``loadCadViewer()`` first. */
export function cadViewer(): Lib {
  if (!lib) throw new Error(`${MISSING} (loadCadViewer() has not resolved.)`);
  return lib;
}
