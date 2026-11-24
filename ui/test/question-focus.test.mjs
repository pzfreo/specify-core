// Moved from draftwright-editor scripts/question-focus.test.mjs (5df98e3) with the code it tests.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { build } from 'esbuild';
// Execute the production focus effect with a DOM/timer boundary; no copied focus logic.
const source = readFileSync('src/specify/SpecifyReview.tsx', 'utf8');
const start = source.indexOf('  // Only a banner or blank-cell request');
const end = source.indexOf('  useEffect(() => {\n    const id = focusRequest', start);
assert.ok(start >= 0 && end > start);
const built = await build({ stdin: { contents: `export function focus(focusRequest, slotQuestion) {
  let focused = 0;
  const readOnly = false; // the package's read-only mode (4cdb65ea); the editor's review is never read-only
  const slots = { question: { querySelector: () => ({ focus: () => focused++ }) } };
  const useEffect = effect => effect();
  const window = { setTimeout: action => { action(); return 1; }, clearTimeout: () => {} };
  ${source.slice(start, end)}
  return focused;
}`, loader: 'ts' }, write: false, format: 'esm' });
const effect = await import(`data:text/javascript;base64,${Buffer.from(built.outputFiles[0].text).toString('base64')}`);
test('only explicitly opening a banner or blank-cell question focuses its input', () => {
  assert.equal(effect.focus({ questionId: 'part.material', focusInput: true }, 'part.material'), 1);
  assert.equal(effect.focus({ questionId: 'part.material', focusInput: true }, 'hole.function:h'), 0);
});
test('callout selection, queue selection and /specify keep their existing keyboard focus', () => {
  assert.equal(effect.focus({ questionId: 'hole.function:h' }, 'hole.function:h'), 0);
  assert.equal(effect.focus({ questionId: 'part.material', scroll: false }, 'part.material'), 0);
  assert.equal(effect.focus(null, 'part.material'), 0);
});
