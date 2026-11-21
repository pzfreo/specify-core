// For #79: browser BOM API and native sourced JSON agree, including answered UDAs.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import init, { IntentEngine, acceptDefaults } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';
const wasm = readFileSync(new URL('../crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', import.meta.url));
await init({ module_or_path: wasm });
const executable = join(process.env.CARGO_TARGET_DIR ?? 'target', 'release', 'specify-core');
for (const path of process.argv.slice(2).length ? process.argv.slice(2) : ['tests/fixtures/parts/assembly.step']) {
  let bytes = readFileSync(path);
  if (path.endsWith('.gz')) bytes = gunzipSync(bytes);
  const rows = JSON.parse(IntentEngine.bom(bytes));
  const native = JSON.parse(execFileSync(executable, ['bom', path], { encoding: 'utf8' }));
  assert.deepEqual(rows, native);
  assert.ok(rows.length > 0);
  for (const row of rows) {
    for (const [key, field] of Object.entries(row)) {
      if (key !== 'part') assert.ok(['file', 'answered', 'default'].includes(field.source));
    }
  }
}
assert.throws(() => IntentEngine.bom(new Uint8Array(64 * 1024 * 1024 + 1)), /exceeds/);
console.log('WASM BOM parity and input limit passed');

// BOM suggestions are never answers: the defaults taken for a part of an assembly have none.
const assembly = readFileSync('tests/fixtures/parts/assembly.step');
const engine = IntentEngine.openPart('assembly', assembly, 0);
const analysis = JSON.stringify(JSON.parse(engine.snapshot()).analysis);
const defaults = JSON.parse(acceptDefaults(analysis, '{}'));
assert.ok(Object.keys(defaults).every(key => !key.startsWith('bom.')));
console.log('WASM BOM suggestions are not answers passed');
