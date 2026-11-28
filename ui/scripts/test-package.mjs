// Packs the real tarball and uses it in a minimal Vite host app (test/package-host), so the
// post-build steps run on every PR: scripts/worker-url.mjs (the `new Worker(new URL(...))` form
// host bundlers build as a worker), the worker's external import of @draftwright/specify-pmi-wasm,
// and scripts/rewrite-dts-imports.mjs (the shipped .d.ts must typecheck with skipLibCheck: false).
// The host is a copy in .cache/package-host with a package.json written here. It must typecheck,
// build (worker and .wasm emitted), and pass test/step-diff-browser.mjs served by `vite preview`.
// The tarball is packed as release.yml packs it (scripts/pack.mjs: the engine dependency pinned to the
// package's own version), and the host installs that engine from the in-repo WASM build.
// Run from ui/ after `npm run build` and `npm run fetch:wasm`. CI supplies one host
// version (or "pinned") and the tarball already packed by the UI build.
import { execFileSync, spawn } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { engineBuild } from './fetch-wasm.mjs';
import { pack } from './pack.mjs';

const root = process.cwd();
const packDir = resolve(root, '.cache/package-host-pack');
const host = resolve(root, '.cache/package-host');
const port = 4292;
const url = `http://127.0.0.1:${port}`;
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';

function fail(message) {
  console.error(`test:package FAILED: ${message}`);
  process.exit(1);
}
const run = (cmd, args, cwd, env) => execFileSync(cmd, args, { cwd, stdio: 'inherit', env: { ...process.env, ...env } });

const rootPkg = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));

const dev = rootPkg.devDependencies;
const version = name => dev[name] ?? fail(`${name} is not in the root devDependencies`);
const hostVersions = [version('three'), '0.186.1', '0.187.0'];
const [variant, prepacked, ...extra] = process.argv.slice(2);
const onlyBare = variant === 'no-viewer';
const selectedVersion = onlyBare ? undefined : variant === 'pinned' ? version('three') : variant;
if (extra.length || (selectedVersion && !hostVersions.includes(selectedVersion))) {
  fail(`usage: npm run test:package -- [no-viewer|pinned|${hostVersions.join('|')} [tarball]]`);
}

function packLocalBuild() {
  if (!existsSync(resolve(root, 'dist'))) fail('dist/ is missing; run `npm run build` first');
  rmSync(packDir, { recursive: true, force: true });
  mkdirSync(packDir, { recursive: true });
  return pack(packDir, root);
}
const tarball = prepacked ? resolve(root, prepacked) : packLocalBuild();
if (!existsSync(tarball)) fail(`tarball is missing: ${tarball}`);
// Reads ui/dist, which a prepacked tarball's CI job does not have (the ui job runs it after the build).
if (!prepacked) run(process.execPath, ['--test', 'test/helpers-node.test.mjs'], root, { HELPERS_DIST: 'required' });

// The peer range spans three 0.184 to 0.187. 0.184.0 is what three-cad-viewer pins and the repo
// tests against; 0.186.1 is what the draftwright editor runs; 0.187.0 is the newest.
async function testHost(threeVersion) {
  console.log(`\n== host on three ${threeVersion} ==`);
  // Fresh host.
  rmSync(host, { recursive: true, force: true });
  cpSync(resolve(root, 'test/package-host'), host, { recursive: true });
  let harness = readFileSync(resolve(root, 'test/harness/main.tsx'), 'utf8');
  for (const from of ["'../../src/index'", "'../../src/model/diff'"]) {
    if (!harness.includes(from)) fail(`test/harness/main.tsx no longer imports ${from}`);
    harness = harness.replace(from, "'@draftwright/specify-ui'");
  }
  if (harness.includes('../../src')) fail('test/harness/main.tsx still imports from ../../src');
  writeFileSync(resolve(host, 'src/harness.tsx'), harness);

  const pick = names => Object.fromEntries(names.map(name => [name, version(name)]));
  writeFileSync(resolve(host, 'package.json'), JSON.stringify({
    name: 'package-host',
    private: true,
    type: 'module',
    dependencies: {
      '@draftwright/specify-ui': `file:${tarball}`,
      ...pick(['react', 'react-dom', 'three-cad-viewer']),
      three: threeVersion,
    },
    devDependencies: pick(['vite', '@vitejs/plugin-react', 'typescript', '@types/react', '@types/react-dom', 'esbuild']),
    // The engine at this version is published by the same release, after this test: the in-repo build.
    overrides: { '@draftwright/specify-pmi-wasm': `file:${engineBuild}` },
  }, null, 2));

  run(npm, ['install', '--no-audit', '--no-fund'], host);

  const installed = JSON.parse(readFileSync(resolve(host, 'node_modules/three/package.json'), 'utf8')).version;
  if (installed !== threeVersion) fail(`host three is ${installed}, expected ${threeVersion}`);
  const nested = resolve(host, 'node_modules/three-cad-viewer/node_modules/three');
  if (threeVersion === version('three')) {
    // The version three-cad-viewer pins: one three only.
    if (existsSync(nested)) fail('a second copy of three is installed under three-cad-viewer');
    console.log(`three ${installed} (single copy)`);
  } else {
    // A newer three in the peer range (the draftwright editor's): three-cad-viewer depends on its
    // own exact three, so npm nests that one under it. Record what is installed.
    const own = existsSync(nested) ? JSON.parse(readFileSync(resolve(nested, 'package.json'), 'utf8')).version : 'none';
    console.log(`three ${installed} for the host, three-cad-viewer's own three: ${own}`);
  }

  run(npx, ['tsc', '-p', '.'], host);
  run(npx, ['vite', 'build'], host);
  // The main entry under a consumer bundler that tree-shakes as the package declares (sideEffects: css only):
  // a host that imports one component still gets the package's own view bound to it, as the bundle holds
  // the viewer's code, not only the component's.
  writeFileSync(resolve(host, 'main-only.ts'), "import { StepDiff } from '@draftwright/specify-ui';\nconsole.log(StepDiff);\n");
  run(npx, ['esbuild', 'main-only.ts', '--bundle', '--format=esm', '--platform=browser', '--outdir=dist-main', '--loader:.css=empty', '--loader:.wasm=empty', '--log-level=error'], host);
  const main = readFileSync(resolve(host, 'dist-main/main-only.js'), 'utf8');
  if (!main.includes('node_modules/three-cad-viewer/')) fail('the main entry, tree-shaken to one component, lost its three-cad-viewer view');
  const assets = readdirSync(resolve(host, 'dist/assets'));
  if (!assets.some(name => /^intent\.worker.*\.js$/.test(name))) fail(`no intent.worker JS in the build: ${assets.join(', ')}`);
  if (!assets.some(name => name.endsWith('.wasm'))) fail(`no .wasm in the build: ${assets.join(', ')}`);
  console.log(`built assets: ${assets.filter(name => /intent\.worker|\.wasm$/.test(name)).join(', ')}`);

  // Serve and run the browser test against it.
  const vite = resolve(host, 'node_modules/vite/bin/vite.js');
  const server = spawn(process.execPath, [vite, 'preview', '--host', '127.0.0.1', '--port', String(port), '--strictPort'], { cwd: host, stdio: 'inherit' });
  let serverFailed = false;
  let stopping = false;
  server.on('exit', code => {
    if (!stopping) { serverFailed = true; console.error(`vite preview exited early (${code})`); }
  });
  const stop = () => { stopping = true; server.kill(); };
  process.on('exit', stop);
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { stop(); process.exit(1); });

  try {
    let up = false;
    for (let i = 0; i < 60 && !up && !serverFailed; i++) {
      up = await fetch(url).then(response => response.ok, () => false);
      if (!up) await new Promise(done => setTimeout(done, 500));
    }
    if (!up) fail('vite preview did not respond');
    run(process.execPath, ['test/step-diff-browser.mjs'], root, { BASE_URL: url });
    if (serverFailed) fail('vite preview died during the browser test');
  } catch (error) {
    stop();
    fail(error.message);
  }
  stop();
}

// A host with its own view uses the /host entry and does not need three-cad-viewer (an optional peer):
// installed without it, it typechecks and builds, with Vite and with webpack, and no built file mentions it.
const WEBPACK = '5.113.0';
const ESBUILD_LOADER = '4.5.0';
const webpackConfig = `import { resolve } from 'node:path';
export default {
  mode: 'production',
  entry: './src/main.tsx',
  output: { path: resolve(import.meta.dirname, 'dist-webpack'), filename: '[name].js', clean: true },
  resolve: { extensions: ['.tsx', '.ts', '.js'] },
  module: { rules: [
    { test: /\\.tsx?$/, loader: 'esbuild-loader', options: { loader: 'tsx', jsx: 'automatic', target: 'es2022' } },
    { test: /\\.css$/, type: 'asset/resource' },
  ] },
  performance: { hints: false },
};
`;

function listFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap(e => e.isDirectory() ? listFiles(resolve(dir, e.name)) : [resolve(dir, e.name)]);
}
function assertNoViewer(dir, label) {
  const files = listFiles(dir);
  const js = files.filter(f => /\.(m?js)$/.test(f));
  // Vite names the worker; webpack numbers its chunks, so for it the .wasm and the three chunks stand for the worker.
  if (label === 'vite' && !js.some(f => /intent\.worker/.test(f))) fail(`${label}: no intent worker in the build`);
  if (label === 'webpack' && js.length < 3) fail(`${label}: the worker chunk is missing (${js.length} JS files)`);
  if (!files.some(f => f.endsWith('.wasm'))) fail(`${label}: no .wasm in the build`);
  for (const file of files) {
    if (file.includes('three-cad-viewer')) fail(`${label}: the build holds ${file}`);
    if (js.includes(file) && readFileSync(file, 'utf8').includes('three-cad-viewer')) fail(`${label}: ${file} mentions three-cad-viewer`);
  }
  console.log(`${label}: built without three-cad-viewer (${js.length} JS files)`);
}

async function testHostWithoutViewer(threeVersion) {
  console.log(`\n== host without three-cad-viewer, on three ${threeVersion} ==`);
  // Outside the repo: resolution walks up from the host, and would find this repo's own three-cad-viewer.
  const bare = resolve(mkdtempSync(resolve(tmpdir(), 'package-host-no-viewer-')), 'host');
  cpSync(resolve(root, 'test/package-host'), bare, { recursive: true });
  rmSync(resolve(bare, 'src'), { recursive: true });
  mkdirSync(resolve(bare, 'src'));
  writeFileSync(resolve(bare, 'src/main.tsx'), `import '@draftwright/specify-ui/style.css';
import { createRoot } from 'react-dom/client';
import { AssemblyDiffView, SpecifyReview, StepDiff, useSpecifyHost, type HostView } from '@draftwright/specify-ui/host';

// A view of the host's own: the components draw through it and load no viewer of ours.
const view = {} as HostView;
const none = new Uint8Array();
const file = { name: 'a.step', bytes: none };
createRoot(document.getElementById('root')!).render(<>
  <StepDiff before={file} after={file} view={view} />
  {/* @ts-expect-error the host entry has no view of its own: \`view\` is required */}
  <StepDiff before={file} after={file} />
  {typeof AssemblyDiffView === 'function' && typeof SpecifyReview === 'function' && typeof useSpecifyHost === 'function' && 'imported'}
</>);
`);
  const pick = names => Object.fromEntries(names.map(name => [name, version(name)]));
  writeFileSync(resolve(bare, 'package.json'), JSON.stringify({
    name: 'package-host-no-viewer',
    private: true,
    type: 'module',
    dependencies: { '@draftwright/specify-ui': `file:${tarball}`, ...pick(['react', 'react-dom']), three: threeVersion },
    devDependencies: {
      ...pick(['vite', '@vitejs/plugin-react', 'typescript', '@types/react', '@types/react-dom']),
      webpack: WEBPACK, 'esbuild-loader': ESBUILD_LOADER, ...pick(['esbuild']),
    },
    overrides: { '@draftwright/specify-pmi-wasm': `file:${engineBuild}` },
  }, null, 2));
  writeFileSync(resolve(bare, 'webpack.config.mjs'), webpackConfig);
  // /view's helpers that make no viewer of ours (here the picker): esbuild must bundle them without three-cad-viewer.
  writeFileSync(resolve(bare, 'picker.ts'), `import { createPicker, createLook, createDiffDrawn } from '@draftwright/specify-ui/view';
console.log(typeof createPicker, typeof createLook, typeof createDiffDrawn);
`);
  run(npm, ['install', '--no-audit', '--no-fund'], bare);
  if (existsSync(resolve(bare, 'node_modules/three-cad-viewer'))) fail('three-cad-viewer was installed in the host without it');
  run(npx, ['tsc', '-p', '.'], bare);
  run(npx, ['vite', 'build'], bare);
  assertNoViewer(resolve(bare, 'dist'), 'vite');
  writeFileSync(resolve(bare, 'webpack-build.mjs'), `import webpack from 'webpack';
import config from './webpack.config.mjs';
webpack(config, (error, stats) => {
  if (error || stats.hasErrors()) { console.error(error ?? stats.toString({ colors: false, errors: true })); process.exit(1); }
  console.log(stats.toString({ colors: false, modules: false, assets: true }));
});
`);
  run(process.execPath, ['webpack-build.mjs'], bare);
  assertNoViewer(resolve(bare, 'dist-webpack'), 'webpack');
  // three-cad-viewer is left external (the host does not have it); no stylesheet of it may be imported, which a
  // bundler with no CSS loader (esbuild, in the editor's build) would fail on.
  run(npx, ['esbuild', 'picker.ts', '--bundle', '--format=esm', '--platform=browser', '--outdir=dist-esbuild', '--external:three-cad-viewer', '--log-level=warning'], bare);
  const picked = readFileSync(resolve(bare, 'dist-esbuild/picker.js'), 'utf8');
  // (esbuild keeps an unused import of an external module; a host that has three-cad-viewer resolves it.)
  if (/three-cad-viewer\/css/.test(picked)) fail("esbuild: /view's picker bundle imports three-cad-viewer's stylesheet");
  // Which entries' module graphs reach three-cad-viewer's stylesheet: only the main entry's.
  const dist = resolve(bare, 'node_modules/@draftwright/specify-ui/dist');
  const graph = entry => {
    const seen = new Set(); const todo = [entry];
    while (todo.length) {
      const file = todo.pop();
      if (seen.has(file)) continue;
      seen.add(file);
      for (const m of readFileSync(resolve(dist, file), 'utf8').matchAll(/(?:from|import)\s*"\.\/([^"]+\.js)"/g)) todo.push(m[1]);
    }
    return [...seen].filter(file => /three-cad-viewer\/css/.test(readFileSync(resolve(dist, file), 'utf8')));
  };
  for (const entry of ['view.js', 'host.js', 'helpers.js']) {
    const css = graph(entry);
    if (css.length) fail(`${entry} reaches three-cad-viewer's stylesheet through ${css.join(', ')}`);
  }
  if (!graph('specify-ui.js').length) fail("the main entry no longer loads three-cad-viewer's stylesheet");
  console.log("esbuild: /view's picker bundled; no entry but the main one reaches three-cad-viewer's stylesheet");
}

if (!onlyBare) for (const threeVersion of selectedVersion ? [selectedVersion] : hostVersions) await testHost(threeVersion);
await testHostWithoutViewer(selectedVersion ?? version('three'));
console.log('test:package passed');
