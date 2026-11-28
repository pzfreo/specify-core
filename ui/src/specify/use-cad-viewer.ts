import { useEffect, useState } from 'react';

import { cadViewerLoaded, loadCadViewer } from './cad-lib';

/**
 * Whether the package's own 3D view can be made: true at once when the host gives a view (nothing is
 * loaded), else when three-cad-viewer has loaded. ``error`` names the package to install when it
 * could not be.
 */
export function useCadViewer(hostView: unknown): { ready: boolean; error: string } {
  const [state, setState] = useState<{ loaded: boolean; error: string }>({ loaded: cadViewerLoaded(), error: '' });
  useEffect(() => {
    if (hostView || cadViewerLoaded()) return undefined;
    let live = true;
    loadCadViewer().then(
      () => { if (live) setState({ loaded: true, error: '' }); },
      (e: unknown) => { if (live) setState({ loaded: false, error: e instanceof Error ? e.message : String(e) }); },
    );
    return () => { live = false; };
  }, [hostView]);
  return { ready: Boolean(hostView) || state.loaded, error: hostView ? '' : state.error };
}
