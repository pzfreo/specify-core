import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Library build: ES output, with the host's React and three left to the host. The intent worker
// is emitted as its own chunk, referenced from the bundle by new URL(..., import.meta.url). The
// Specify WASM is not bundled: the worker imports the dependency @draftwright/specify-pmi-wasm,
// which the host's bundler resolves (or loads the host's own from wasmUrl).
const external = [/^react($|\/)/, /^react-dom($|\/)/, /^three($|\/)/, /^three-cad-viewer($|\/)/, /^@draftwright\/specify-pmi-wasm($|\/)/];

export default defineConfig({
  plugins: [react()],
  base: './',
  publicDir: false, // public/ holds the dev WASM; hosts serve their own
  worker: { format: 'es', rollupOptions: { external, output: { entryFileNames: 'assets/[name].js' } } },
  build: {
    target: 'es2023',
    // Entries: the components, the DOM-free helpers (no three, no CSS) for a host's Node code, the 3D view's, and the save side (edits).
    lib: {
      entry: { 'specify-ui': 'src/index.ts', helpers: 'src/helpers.ts', view: 'src/view.ts', edits: 'src/specify/edits/index.ts' }, formats: ['es'],
      fileName: (_format, name) => `${name}.js`, cssFileName: 'specify-ui',
    },
    rollupOptions: { external },
  },
});
