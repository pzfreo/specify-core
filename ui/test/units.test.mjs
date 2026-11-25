import { test } from 'node:test';
import assert from 'node:assert/strict';
import { labelName, labelRow, labelStated, statedUnit, unitLine, withUnits } from '../src/specify/units.ts';

test('each length says its unit, once', () => {
  for (const [text, shown] of [
    ['⌀0.438 ±0.005', '⌀0.438 ±0.005 in'],
    ['⌀0.438 ±0.005 · ⌀0.438 ±0.005', '⌀0.438 ±0.005 in · ⌀0.438 ±0.005 in'],
    ['⌀0.2375 +0.005/-0.001', '⌀0.2375 +0.005/-0.001 in'],
    ['⌀1.5 ±0.005; Diameter, no value stated', '⌀1.5 ±0.005 in; Diameter, no value stated'],
    ['(0.75) · 0.82 ±0.06', '(0.75) in · 0.82 ±0.06 in'],
    ['⌓ 0.01', '⌓ 0.01 in'],
    ['∠ 0.04 A', '∠ 0.04 in A'],
    ['⏥ 0.005/0.25×0.25', '⏥ 0.005/0.25×0.25 in'],
    ['⌖ ⌀0.2 A', '⌖ ⌀0.2 in A'],
    ['⌖ ⌀0.2 Ⓜ A B C', '⌖ ⌀0.2 in Ⓜ A B C'],
    ['⌖ ⌀0.05Ⓜ A BⓂ CⓂ', '⌖ ⌀0.05 in Ⓜ A BⓂ CⓂ'],
    ['9.8–10.2', '9.8–10.2 in'],
    ['±0.004', '±0.004 in'],
  ]) assert.equal(withUnits(text, 'in'), shown);
  for (const [text, shown] of [
    ['10.4 from right face', '10.4 mm from right face'],
    ['1.90754 ±0.1 from A', '1.90754 ±0.1 mm from A'],
    ['⌀3.3 H11', '⌀3.3 mm H11'],
    ['M4 clearance · ⌖0.76 A|B|C', 'M4 clearance · ⌖0.76 mm A|B|C'],
    ['M2×0.4-6H ↧ 2', 'M2×0.4-6H ↧ 2 mm'],
    ['⌀4.763 is an ISO 273 clearance hole for M4.', '⌀4.763 mm is an ISO 273 clearance hole for M4.'],
    ['⌀3.05 = H − F for a floating M24 bolt: this ⌀27.051 hole less the bolt.', '⌀3.05 mm = H − F for a floating M24 bolt: this ⌀27.051 mm hole less the bolt.'],
    ['A 2 bottom faces · C ⌀11.125 hole', 'A 2 bottom faces · C ⌀11.125 mm hole'],
  ]) assert.equal(withUnits(text, 'mm'), shown);
});

test('what is not a length, or says its unit already, is left alone', () => {
  for (const text of ['M4 clearance', 'M4×0.7-6H THRU', 'H7', 'Ra 3.2', 'ISO 2768-m', '2768-m', '60° ±1°', 'E|B|C', 'none', 'general',
    'none needed', 'datum B', '3 mm ±0.05', '±0.05 mm', '⌀3.3 mm H11', '1/4-20 UNC-2B', '—', '']) {
    for (const unit of ['mm', 'in']) assert.equal(withUnits(text, unit), text, `${text} (${unit})`);
  }
  // An inch file's thread depth: which unit it reads in is not known here, so nothing is claimed.
  assert.equal(withUnits('M2×0.4-6H ↧ 2', 'in'), 'M2×0.4-6H ↧ 2');
});

test('the panel says the file\'s unit, and that Specify works in mm', () => {
  assert.equal(unitLine('mm'), 'Units: mm');
  assert.equal(unitLine('in'), 'File in inches · Specify works in mm');
});

test('what a file states is in the unit it was written in, told from its value in mm', () => {
  assert.equal(statedUnit('⌀0.438 ±0.005', 11.1252), 'in');
  assert.equal(statedUnit('⌀4.7625 ±0.004', 4.7625), 'mm');
  assert.equal(statedUnit('⌀34.8 - ⌀35.2', 35), 'mm');
  assert.equal(statedUnit('R0.012 - R0.022', 0.4318), 'in');
  assert.equal(statedUnit('60° ±1°', 60), null);
  assert.equal(statedUnit('⌀5', 7), null);
  // Specify writes mm into an inch file: each says its own.
  const existing = [{ text: '⌀0.438 ±0.005', value: 11.1252 }, { text: '⌀4.7625 ±0.004', value: 4.7625 }];
  assert.equal(labelStated('⌀0.438 ±0.005 · ⌀4.7625 ±0.004 · Diameter, no value stated', existing),
    '⌀0.438 ±0.005 in · ⌀4.7625 ±0.004 mm · Diameter, no value stated');
  assert.equal(labelStated('⌀9 ±0.1', existing), '⌀9 ±0.1', 'not told: none said');
  assert.equal(labelRow({ source: 'existing:3', text: '0.75 from A', nominal: 19.05, upper: null }), '0.75 in from A');
  assert.equal(labelRow({ source: 'general:2', text: '57.2795 from A', nominal: 57.2795, upper: null }), '57.2795 mm from A');
});

test('a control the file states is labelled from its tolerance', () => {
  assert.equal(labelRow({ source: 'existing:1', text: '⌖ ⌀0.2', nominal: null, upper: 0.2 }), '⌖ ⌀0.2 mm');
  assert.equal(labelRow({ source: 'existing:1', text: '⌖ ⌀0.03', nominal: null, upper: 0.762 }), '⌖ ⌀0.03 in');
});

test('names are in mm, and say so in an inch file', () => {
  for (const [name, inInch] of [['Ø50.8 hole', 'Ø50.8 mm hole'], ['4× Ø11.125 holes', '4× Ø11.125 mm holes'],
    ['What is the Ø2.5 hole for?', 'What is the Ø2.5 mm hole for?'], ['2× Ø10 × 11 through holes', '2× Ø10 × 11 mm through holes'], ['top face', 'top face']]) {
    assert.equal(labelName(name, 'in'), inInch);
    assert.equal(labelName(name, 'mm'), name);
  }
});
