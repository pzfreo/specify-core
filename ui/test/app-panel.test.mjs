import { test } from 'node:test';
import assert from 'node:assert/strict';
import { balloonText, bareAnalysis, changedText, displayText, panelModel, tableOf } from '../app/table.ts';
import { describe, editList, PendingEdits, removalTargets, suggestionEdits } from '../app/edits.ts';
import { compose, emptyBlock, parse } from '../app/general-tolerance.ts';
// The review panel's pieces that are not drawn (specify-panel.md): its reading of the engine's feature
// table, the pending edits as an edit list, and the general tolerance's text (#228).

const table = {
  rows: [
    { id: 'part.material', kind: 'part.material', feature: 'part', faces: [], state: 'stated', value: 'Brass CZ121', open: false },
    { id: 'part.general_tolerance', kind: 'part.general_tolerance', feature: 'part', faces: [], state: 'gap', value: null, suggestion: 'ISO 2768-m', open: true },
    { id: 'part.coating', kind: 'part.coating', feature: 'part', faces: [], state: 'decided_none', value: 'None (as machined)', open: false },
    { id: 'part.heat_treatment', kind: 'part.heat_treatment', feature: 'part', faces: [], state: 'gap', value: null, suggestion: 'None', open: false },
    { id: 'hole.function:holes:14.15', kind: 'hole.function', feature: 'holes:14.15', faces: [14], state: 'gap', value: null, suggestion: 'tapped:M2', open: true },
    { id: 'hole.position:holes:14.15', kind: 'hole.position', feature: 'holes:14.15', faces: [14], state: 'stated', value: 'Position 0.2 | A',
      requirements: [{ number: '#2', kind: 'geometric_tolerance', type: 'Position', faces: [14], text: 'Position 0.2 | A' }], open: false },
    { id: 'datum.A', kind: 'datum.A', feature: 'datum.A', faces: [0], state: 'stated', value: 'datum A',
      requirements: [{ number: '#1', kind: 'datum', type: 'A', faces: [0], text: 'datum A' }], open: false },
  ],
  file_pmi: [
    { number: '#3', id: 'file_pmi:#1401', kind: 'dimension', type: 'Location_LinearDistance', faces: [7, 11], text: 'Location_LinearDistance 37.6' },
    { id: 'file_pmi:#24', kind: 'dimension', type: 'Location_LinearDistance', faces: [41, 37], text: 'Location_LinearDistance' },
    { kind: 'note', type: 'general note', faces: [2], text: 'Deburr' },
  ],
  decided_none: ['part.coating'],
};
const questions = [
  { id: 'part.material', label: 'Material', kind: 'text' },
  { id: 'hole.function:holes:14.15', label: 'Ø1.6 hole', title: 'Ø1.6 hole', kind: 'choice', options: ['general', 'tapped:M2'] },
  { id: 'hole.position:holes:14.15', label: 'Ø1.6 hole position', title: 'Ø1.6 hole', kind: 'choice', options: ['none', '0.1', '0.2'] },
  { id: 'datum.A', label: 'Datum A', kind: 'faces' },
];

test('the panel reads the feature table: features with each kind stated, gap or decided none', () => {
  const model = panelModel(tableOf({ table }), questions);
  assert.deepEqual(model.part.rows.map(r => [r.id, r.state, r.value]), [
    ['part.material', 'stated', 'Brass CZ121'],
    ['part.general_tolerance', 'gap', 'not stated'],
    ['part.coating', 'decided_none', 'none needed'],
    ['part.heat_treatment', 'gap', 'not stated'],
  ]);
  // A gap carries its suggestion as it reads (a choice by its code); a "none" suggestion reads "none".
  const gt = model.part.rows[1];
  assert.equal(gt.suggestion, 'ISO 2768-m');
  assert.equal(gt.suggestionText, 'ISO 2768-m');
  assert.equal(model.part.rows[3].suggestionText, 'none');
  assert.equal(model.part.rows[3].open, false, 'a none suggestion is not open');
  // A feature's rows under its title, each by its own word; a stated row keeps its numbers.
  assert.deepEqual(model.features.map(g => g.title), ['Ø1.6 hole']);
  assert.deepEqual(model.features[0].rows.map(r => [r.label, r.state, r.numbers]), [['Function', 'gap', []], ['Position', 'stated', ['#2']]]);
  assert.equal(model.features[0].rows[0].suggestionText, 'M2 tapped');
  assert.equal(model.features[0].rows[0].name, 'Ø1.6 hole function', 'a row is named with its feature and its kind');
  assert.equal(model.features[0].rows[1].expect, 'Position 0.2 | A', 'a change expects the value the file states');
  assert.equal(model.features[0].rows[1].value, '⌖ 0.2 A', 'and shows it as a drawing writes it');
  assert.deepEqual(model.datums.rows.map(r => r.label), ['Datum A']);
  // "Other PMI in the file": by number when it has one.
  // The engine's ids (`file_pmi:#<entity>`) name each item; one it cannot name has none.
  assert.deepEqual(model.other.map(o => [o.key, o.id, o.number, o.value]), [['#3', 'file_pmi:#1401', '#3', '37.6'], ['file_pmi:#24', 'file_pmi:#24', undefined, 'Linear distance'], ['item:2', undefined, undefined, 'Deburr']]);
  assert.deepEqual(model.other.map(o => removalTargets(o.number ? [o.number] : [], o.id)), [['#3'], [{ id: 'file_pmi:#24' }], []]);
  // Nothing in the model says who wrote a requirement (decision 6).
  assert.doesNotMatch(JSON.stringify(model), /origin|provenance|from the file|written by/);
});

test('a table with no rows reads as empty, and the bare analysis sets the file\'s PMI aside', () => {
  assert.deepEqual(tableOf({}), { rows: [], file_pmi: [], decided_none: [] });
  const bare = bareAnalysis({ faces: [1], features: [2], existing: [3], part: { material: 'x' }, table, resume: { answers: {} } });
  assert.deepEqual(bare, { faces: [1], features: [2], existing: [], part: {}, characteristics: [] });
});

test('Add all suggestions: a value suggested is a fill, none is a none; no suggestion or an edit already there, left alone', () => {
  const model = panelModel(tableOf({ table }), questions);
  const rows = [...model.part.rows, ...model.features.flatMap(g => g.rows)];
  const pending = new PendingEdits().with({ kind: 'fill', id: 'hole.function:holes:14.15', name: 'f', value: 'general', text: 'general' });
  const isNone = v => v === null || (Array.isArray(v) ? !v.length : /^none\b/i.test(String(v)));
  assert.deepEqual(suggestionEdits(rows, pending, isNone).map(p => [p.kind, p.id, p.value]), [
    ['fill', 'part.general_tolerance', 'ISO 2768-m'],
    ['none', 'part.heat_treatment', undefined],
  ]);
});

test('balloons read as runs', () => {
  assert.equal(balloonText(['#1', '#2', '#4', '#5', '#6', '#9']), '1–2, 4–6, 9');
  assert.equal(balloonText([]), '');
});

test('the pending edits are one edit list, one edit per row, in the bar\'s words', () => {
  let pending = new PendingEdits();
  pending = pending.with({ kind: 'fill', id: 'part.material', name: 'Material', value: 'Steel', text: 'Steel' });
  pending = pending.with({ kind: 'change', id: 'hole.position:holes:14.15', name: 'Ø1.6 hole position', value: '0.1', text: '⌖ 0.1 A', expect: 'Position 0.2 | A', was: '⌖ 0.2 A' });
  pending = pending.with({ kind: 'remove', id: '#3', name: 'Dimension', targets: removalTargets(['#3'], 'file_pmi:#1401'), was: '37.6' });
  pending = pending.with({ kind: 'remove', id: 'part.edges', name: 'Edges', targets: removalTargets([], 'part.edges'), was: 'Deburr only' });
  pending = pending.with({ kind: 'none', id: 'part.heat_treatment', name: 'Heat treatment' });
  pending = pending.with({ kind: 'clear', id: 'part.coating', name: 'Coating' });
  assert.deepEqual(pending.list(), {
    fill: { 'part.material': 'Steel' },
    change: { 'hole.position:holes:14.15': { to: '0.1', expect: 'Position 0.2 | A' } },
    items: { remove: ['#3', { id: 'part.edges' }] },
    none: ['part.heat_treatment'],
    clear: ['part.coating'],
  });
  assert.deepEqual(pending.values().map(describe), [
    'Add Material: Steel',
    'Change Ø1.6 hole position: ⌖ 0.2 A → ⌖ 0.1 A',
    'Remove Dimension: 37.6 (#3)',
    'Remove Edges: Deburr only',
    'None needed: Heat treatment',
    'Undo none needed: Coating',
  ]);
  // A row's next edit replaces its last; dropping it leaves the rest.
  const again = pending.with({ kind: 'fill', id: 'part.material', name: 'Material', value: 'Brass', text: 'Brass' });
  assert.equal(again.size, 6);
  assert.deepEqual(again.list().fill, { 'part.material': 'Brass' });
  assert.equal(again.without('part.material').list().fill, undefined);
  assert.deepEqual(editList([]), {}, 'no edits: an empty list (Save writes the file as it is)');
  // Two removes of the same number send it once.
  assert.deepEqual(editList([{ kind: 'remove', id: 'a', targets: ['#6'] }, { kind: 'remove', id: 'b', targets: ['#6', '#7'] }]).items.remove, ['#6', '#7']);
});

test('requirements read as a drawing writes them; a removal names its number, else its id', () => {
  assert.equal(displayText('Position 0.2 | A | B'), '⌖ 0.2 A B');
  assert.equal(displayText('Perpendicularity 0.05 | A'), '⟂ 0.05 A');
  assert.equal(displayText('Size_Diameter 3.3'), 'Ø3.3');
  assert.equal(displayText('Location_LinearDistance 1.5; Location_LinearDistance 2.375'), '1.5; 2.375');
  assert.equal(displayText('Brass CZ121'), 'Brass CZ121');
  assert.equal(changedText('⌖ 0.2 A', '0.1'), '⌖ 0.1 A');
  assert.equal(changedText('Brass', 'Steel'), 'Steel');
  assert.deepEqual(removalTargets(['#4', '#5'], 'hole.position:holes:13'), ['#4', '#5']);
  assert.deepEqual(removalTargets([], 'part.material'), [{ id: 'part.material' }]);
});

// #228: the four worked examples of specify-0.3.0.md, as the editor composes and reads them.
test('the general tolerance: a class, or a block row by row with angles and a note, as one line of text', () => {
  // 1. The #228 half-shell, an inch block.
  const inch = { form: 'block', units: 'in', rows: [{ places: 2, tolerance: '±.01' }, { places: 3, tolerance: '±.005' }], angles: '±1°', notes: ['fractions ±1/64'] };
  assert.equal(compose(inch), '.XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°');
  assert.deepEqual(parse(compose(inch)), inch);
  // 2. ISO 2768-m, a class.
  assert.equal(compose({ form: 'class', value: 'ISO 2768-m' }), 'ISO 2768-m');
  assert.deepEqual(parse('ISO 2768-m'), { form: 'class', value: 'ISO 2768-m' });
  // 4. A metric block.
  const metric = { form: 'block', units: 'mm', rows: [{ places: 0, tolerance: '±0.5' }, { places: 1, tolerance: '±0.2' }, { places: 2, tolerance: '±0.05' }], angles: '±0°30′', notes: [] };
  assert.equal(compose(metric), 'X ±0.5 mm, X.X ±0.2, X.XX ±0.05, angles ±0°30′');
  assert.deepEqual(parse(compose(metric)), metric);
  assert.deepEqual(parse(compose(metric), 'in'), metric, 'a line of no places states its unit, whatever the file\'s');
  // Rows left blank are not written; a new block starts with two rows in the file's unit.
  assert.equal(compose({ ...metric, rows: [...metric.rows, { places: 3, tolerance: ' ' }], angles: '' }), 'X ±0.5 mm, X.X ±0.2, X.XX ±0.05');
  assert.deepEqual(emptyBlock('in').rows.map(r => r.places), [2, 3]);
  assert.equal(emptyBlock('mm').units, 'mm');
});

test('the general tolerance: no places is "X" with its unit, read back as 0 places in that unit', () => {
  const inch = { form: 'block', units: 'in', rows: [{ places: 0, tolerance: '±.5' }, { places: 2, tolerance: '±.01' }], angles: '', notes: [] };
  assert.equal(compose(inch), 'X ±.5 in, .XX ±.01');
  assert.deepEqual(parse(compose(inch)), inch);
  assert.deepEqual(parse(compose(inch), 'mm'), inch);
  const onlyInchX = { form: 'block', units: 'in', rows: [{ places: 0, tolerance: '±.5' }], angles: '', notes: [] };
  assert.deepEqual(parse(compose(onlyInchX), 'mm'), onlyInchX, 'an inch block of no places alone in a mm file');
  const mm = { form: 'block', units: 'mm', rows: [{ places: 0, tolerance: '±0.5' }], angles: '', notes: [] };
  assert.equal(compose(mm, 'in'), 'X ±0.5 mm');
  assert.deepEqual(parse(compose(mm, 'in'), 'in'), mm);
  // A mm block with places in an inch file states its unit on its first row.
  const mmPlaces = { form: 'block', units: 'mm', rows: [{ places: 1, tolerance: '±0.2' }], angles: '', notes: [] };
  assert.equal(compose(mmPlaces, 'in'), 'X.X ±0.2 mm');
  assert.deepEqual(parse(compose(mmPlaces, 'in'), 'in'), mmPlaces);
  // Mixed rows keep the block's unit: no ".XX" turns into "X.XX".
  assert.equal(compose(parse('X ±1/32 in, .XX ±.01')), 'X ±1/32 in, .XX ±.01');
  // The engine's own spelling (" · " between lines) reads back too.
  assert.deepEqual(parse('.XX ±.01 · .XXX ±.005 · ±1/64 · angles ±1°').rows.length, 2);
  // Angles alone: a block with no length to give a unit to, read in the file's unit.
  const angles = { form: 'block', units: 'in', rows: [], angles: '±1°', notes: [] };
  assert.equal(compose(angles, 'in'), 'angles ±1°');
  assert.deepEqual(parse(compose(angles, 'in'), 'in'), angles);
  assert.deepEqual(parse(compose({ ...angles, units: 'mm' }), 'mm'), { ...angles, units: 'mm' });
});
