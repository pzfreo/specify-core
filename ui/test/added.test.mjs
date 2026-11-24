// "Added by you" keys (extra.N): one supply, so a call-out held in the host's bar and a requirement
// added in the review never share a key (independent review of PR #256, finding 2).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nextKey } from '../src/specify/added.ts';

const flatness = { faces: [2], kind: 'flatness', value: '0.05' };

test('a new requirement is one past the highest added', () => {
  assert.equal(nextKey({}), 'extra.1');
  assert.equal(nextKey({ 'extra.1': flatness, 'extra.3': flatness }), 'extra.4');
});

test('keys the host holds are skipped: a call-out in the bar, then + Add a requirement', () => {
  assert.equal(nextKey({}, ['extra.1']), 'extra.2');
  assert.equal(nextKey({ 'extra.2': flatness }, ['extra.1', 'extra.5']), 'extra.6');
});

test('only extra.N keys count, and answers that are not requirements do not', () => {
  assert.equal(nextKey({ 'part.material': 'Steel', 'extra.7': 'not a requirement' }, ['part.coating']), 'extra.1');
});
