// Run only via tools/probe_wasm_panic.py, which injects a panic in a disposable copy.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import init, { IntentEngine } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';
await init({ module_or_path: readFileSync(new URL('../crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', import.meta.url)) });
const engine = new IntentEngine('plate.step', readFileSync(new URL('../tests/fixtures/parts/plate.step', import.meta.url)));
const answers = JSON.stringify({ __panic_probe: true });
let failure;
try { engine.review(answers); } catch (error) { failure = error; }
console.log('Hostile answer result:', failure);
assert.ok(failure instanceof WebAssembly.RuntimeError, 'baseline must trap rather than throw a refusal string');
try { console.log('Next valid review:', JSON.parse(engine.review('{}')).questions.length, 'questions'); }
catch (error) { console.log('Next valid review failed:', error); }
try { engine.free(); console.log('Session free succeeded'); }
catch (error) { console.log('Session free failed:', error); }
