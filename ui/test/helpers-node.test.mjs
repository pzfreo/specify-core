// The built ./helpers entry loads under plain Node (no CSS, no DOM) and has what a host's Node code and tests use.
// Needs dist/ (npm run build); test:package runs it with HELPERS_DIST=required so a missing build fails there.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const entry = resolve(import.meta.dirname, '../dist/helpers.js');
const built = existsSync(entry);
if (!built && process.env.HELPERS_DIST === 'required') throw new Error('dist/helpers.js is missing; run `npm run build`');

test('dist/helpers.js imports under plain Node and exports the host surface', { skip: !built && 'dist/ not built' }, () => {
  const names = ['as9102', 'xlsx', 'zip', 'workbookFiles', 'diffQuery', 'WorkerClient', 'createIntentEngine',
    'tolerancingMode', 'intentParts', 'intentPart', 'editorDefault', 'readCarried', 'characteristics'];
  const script = `import(${JSON.stringify(pathToFileURL(entry).href)}).then(m => console.log(JSON.stringify(Object.keys(m))))`;
  // A fresh Node, no loader flags: what a host's own process does.
  const exported = JSON.parse(execFileSync(process.execPath, ['-e', script], { encoding: 'utf8' }));
  const missing = names.filter(name => !exported.includes(name));
  assert.deepEqual(missing, []);
});
