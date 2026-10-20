// Run only via tools/probe_wasm_panic.py, which injects a panic in a disposable copy.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import init, { IntentEngine } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';
await init({ module_or_path: readFileSync(new URL('../crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', import.meta.url)) });
const engine = new IntentEngine('plate.step', readFileSync(new URL('../tests/fixtures/parts/plate.step', import.meta.url)));
const before = engine.review('{}');
const answers = JSON.stringify({ 'part.material': '__panic_probe' });
let failure;
try { engine.review(answers); } catch (error) { failure = error; }
console.log('Hostile answer result:', failure);
assert.ok(failure instanceof WebAssembly.RuntimeError, 'baseline must trap rather than throw a refusal string');
assert.equal(engine.review('{}'), before, 'next shared review still succeeds after the trap');
console.log('Next valid review succeeded with an identical result');
assert.throws(() => engine.free(), /while it was borrowed/, 'panic leaks wasm-bindgen borrow bookkeeping');
console.log('Session free failed: attempted to take ownership of Rust value while it was borrowed');
