import { test } from 'node:test';
import assert from 'node:assert/strict';
import { balloonText, bareAnalysis, changedText, displayText, isNoneValue, tableOf } from '../src/specify/feature-table.ts';
import { describe, withDrafts } from '../app/edits.ts';
import { editList, PendingEdits, rejectedNoneByPart, removalTargets, suggestionEdits } from '../src/specify/edit-list.ts';
import { fileRows, otherRows, queueValue, requirementLabel, requirementName, rowSuggestion, rowSuggestionText, rowText } from '../src/specify/file-review.ts';
import { groupQuestions } from '../src/specify/review.ts';
import { editForCopy } from '../src/specify/review-assembly.ts';
import { compose, emptyBlock, parse } from '../src/specify/general-tolerance.ts';
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
const labels = [
  { id: 'part.material', label: 'Material', kind: 'text', faces: [] },
  { id: 'hole.function:holes:14.15', label: 'Ø1.6 hole', title: 'Ø1.6 hole', feature: 'holes:14.15', faces: [14], kind: 'choice', options: ['general', 'tapped:M2'] },
  { id: 'hole.position:holes:14.15', label: 'Ø1.6 hole position', title: 'Ø1.6 hole', feature: 'holes:14.15', faces: [14], kind: 'choice', options: ['none', '0.1', '0.2'] },
  { id: 'datum.A', label: 'Datum A', kind: 'faces', faces: [0] },
];

// Gaps are returned as questions by review. Labels from the bare part also name stored rows.
function view(table, features = []) {
  const asked = table.rows.filter(r => r.state === 'gap').map(r => ({
    id: r.id, faces: r.faces, feature: r.feature, default: r.suggestion,
    ...labels.find(q => q.id === r.id),
  }));
  const analysis = { table, features };
  const rows = fileRows(analysis, asked, labels);
  const groups = groupQuestions(rows.map(row => row.question), true);
  return { rows, groups, other: otherRows(analysis) };
}

test('the review reads the feature table: features with each kind stated, gap or decided none', () => {
  const model = view(table);
  const part = model.groups.find(g => g.key === 'part').questions.map(q => model.rows.find(r => r.id === q.id));
  assert.deepEqual(['part.material', 'part.general_tolerance', 'part.coating', 'part.heat_treatment'].map(id => {
    const r = part.find(one => one.id === id);
    return [r.id, r.state, r.state === 'gap' ? 'not stated' : r.state === 'decided_none' ? 'none needed' : rowText(r)];
  }), [
    ['part.material', 'stated', 'Brass CZ121'],
    ['part.general_tolerance', 'gap', 'not stated'],
    ['part.coating', 'decided_none', 'none needed'],
    ['part.heat_treatment', 'gap', 'not stated'],
  ]);
  const gt = part.find(r => r.id === 'part.general_tolerance');
  assert.equal(rowSuggestion(gt), 'ISO 2768-m');
  assert.equal(rowSuggestionText(gt), 'ISO 2768-m');
  const heat = part.find(r => r.id === 'part.heat_treatment');
  assert.equal(rowSuggestionText(heat), 'none');
  assert.equal(heat.open, false, 'a none suggestion is not open');
  const features = model.groups.filter(g => g.key !== 'part' && g.key !== 'datums');
  assert.deepEqual(features.map(g => g.title), ['Ø1.6 hole']);
  const hole = features[0].questions.map(q => model.rows.find(r => r.id === q.id));
  assert.deepEqual(hole.map(r => [requirementLabel(r.question), r.state, r.requirements?.flatMap(p => p.number ?? []) ?? []]),
    [['Function', 'gap', []], ['Position', 'stated', ['#2']]]);
  assert.equal(rowSuggestionText(hole[0]), 'M2 tapped');
  assert.equal(requirementName(hole[0].question), 'Ø1.6 hole function', 'a row is named with its feature and its kind');
  assert.equal(hole[1].value, 'Position 0.2 | A', 'a change expects the value the file states');
  assert.equal(rowText(hole[1]), '⌖ 0.2 A', 'and shows it as a drawing writes it');
  assert.deepEqual(model.groups.find(g => g.key === 'datums').questions.map(requirementLabel), ['Datum A']);
  assert.deepEqual(model.other.map(r => [r.id, r.other.id, r.other.number, rowText(r)]), [
    ['#3', 'file_pmi:#1401', '#3', '37.6'], ['file_pmi:#24', 'file_pmi:#24', undefined, 'Linear distance'], ['item:2', undefined, undefined, 'Deburr'],
  ]);
  assert.deepEqual(model.other.map(({ other }) => removalTargets(other.number ? [other.number] : [], other.id)), [['#3'], [{ id: 'file_pmi:#24' }], []]);
  assert.doesNotMatch(JSON.stringify(model), /origin|provenance|from the file|written by/);
});

test('a table with no rows reads as empty, and the bare analysis sets the file\'s PMI aside', () => {
  assert.deepEqual(tableOf({}), { rows: [], file_pmi: [], decided_none: [] });
  const bare = bareAnalysis({ faces: [1], features: [2], existing: [3], part: { material: 'x' }, table, resume: { answers: {} } });
  assert.deepEqual(bare, { faces: [1], features: [2], existing: [], part: {}, characteristics: [] });
});

test('Add all suggestions: a value suggested is a fill, none is a none; no suggestion or an edit already there, left alone', () => {
  const rows = view(table).rows.map(row => ({ ...row, suggestion: rowSuggestion(row), suggestionText: rowSuggestionText(row) }));
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
  const id = 'hole.function:holes:14.15';
  const approval = { ...table.rows.find(row => row.id === id), state: 'decided_none', value: 'general' };
  const [approved] = view({ ...table, rows: [approval] }).rows;
  const replacement = { clear: [id], fill: { [id]: 'none' } };
  const none = queueValue(approved, approved.question, new PendingEdits(), 'none');
  assert.deepEqual(none.list(), replacement, 'None replaces a saved approval in one edit');
  assert.equal(none.size, 1);
  assert.deepEqual(queueValue({ ...approved, value: 'none' }, approved.question, new PendingEdits(), 'None').list(), {});

  const part = { table: { ...table, rows: [approval] }, features: [{ id: approval.feature, family: 'holes', faces: approval.faces }] };
  const forCopy = (analysis, id) => {
    const { edit } = editForCopy({ kind: 'none', id, name: id }, analysis, analysis, face => face);
    return editList(edit ? [edit] : []);
  };
  assert.deepEqual(forCopy(part, id), replacement, 'Each copy replaces its own approval');
  const locating = 'part.locating';
  assert.deepEqual(forCopy({ ...part, resume: { answers: { [locating]: 'position tolerances' } } }, locating),
    { clear: [locating], fill: { [locating]: 'none' } }, 'A stored choice has no PMI to remove');

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
  // The engine's notation (feature-table): the zone and modifiers stay, as a change keeps them.
  assert.equal(changedText('⌖ ⌀0.3Ⓜ A', '0.2'), '⌖ ⌀0.2Ⓜ A');
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

test('plane lengths appear under The part and hole locations under their feature', () => {
  const length = 'length:planes:2-3';
  const location = 'location:holes:2-14';
  const generalRows = [
    { id: length, kind: 'length', feature: 'planes:2-3', faces: [3], state: 'gap', suggestion: '14 ±0.2 from B', open: false },
    { id: location, kind: 'location', feature: 'holes:2-14', faces: [14], state: 'gap', suggestion: '15 ±0.2 from B', open: false },
  ];
  const model = view({ ...table, rows: [...table.rows, ...generalRows] }, [
    { id: 'holes:14.15', family: 'holes', faces: [14, 15], record: {} },
  ]);
  const part = model.groups.find(g => g.key === 'part');
  assert.equal(requirementLabel(part.questions.find(q => q.id === length)), 'Length');
  const hole = model.groups.find(g => g.key === 'holes:14.15');
  assert.equal(requirementLabel(hole.questions.find(q => q.id === location)), 'Location');
  const rows = model.rows.map(row => ({ ...row, suggestion: rowSuggestion(row), suggestionText: rowSuggestionText(row) }));
  const pending = new PendingEdits().with({ kind: 'none', id: length, name: 'Length' });
  const all = suggestionEdits(rows, pending, isNoneValue);
  assert.equal(all.find(p => p.id === location)?.value, '15 ±0.2 from B');
  assert.ok(!all.some(p => p.id === length));
});

test('a BOM field uses Parts list, outside the part requirements', () => {
  const bom = { id: 'bom.make_buy', feature: 'part', kind: 'bom.make_buy', state: 'stated', value: 'Buy', faces: [] };
  const model = view({ ...table, rows: [...table.rows, bom] });
  assert.equal(model.groups.find(g => g.key === 'part').questions.some(q => q.id.startsWith('bom.')), false);
  assert.deepEqual(model.groups.find(g => g.key === 'bom').questions.map(q => q.id), [bom.id]);
});

test('save matches a None needed refusal only to the affected part and queued row', () => {
  const coating = new PendingEdits([{ kind: 'none', id: 'part.coating', name: 'Coating' }]);
  const pending = new Map([[0, coating], [1, coating]]);
  const refusal = 'A coating is required';
  const failure = entry => [{ entry, reason: refusal }];
  const scoped = { entry: 'displayed diagnostic may use any layout', part: 1, editEntry: 'none["part.coating"]', reason: refusal };
  assert.deepEqual([...rejectedNoneByPart(pending, [scoped], true)],
    [[1, new Map([['part.coating', refusal]])]]);
  assert.deepEqual([...rejectedNoneByPart(new Map([[0, coating]]), failure('none["part.coating"]'), false)],
    [[0, new Map([['part.coating', refusal]])]]);
  assert.equal(rejectedNoneByPart(pending, [{ ...scoped, editEntry: 'fill["part.coating"]' }], true).size, 0);

  const edit = { kind: 'none', id: 'hole.position:holes:14.15', name: 'Position' };
  const copyOrigin = { part: 0, edit };
  const copyId = 'hole.position:holes:1.2';
  const copies = new Map([
    [0, new PendingEdits([{ ...edit, copyOrigin }])],
    [1, new PendingEdits([{ ...edit, id: copyId, copyOrigin, targets: ['#4'] }])],
  ]);
  assert.deepEqual([...rejectedNoneByPart(copies, [{ ...scoped, editEntry: 'items.remove["#4"]' }], true, () => 'Pin copy')], [
    [1, new Map([[copyId, refusal]])],
    [0, new Map([[edit.id, `Pin copy: ${refusal}`]])],
  ]);
});

test('the model keeps individual drawing notes in order', () => {
  const notes = { id: 'part.notes', feature: 'part', kind: 'part.notes', state: 'stated', value: ['Deburr; check edges', 'Mark lot'], faces: [] };
  const model = view({ ...table, rows: [...table.rows, notes] });
  assert.deepEqual(model.rows.find(r => r.id === notes.id).value, notes.value);
});

test('working answers share the pending bar and edit lists across parts, with local edits taking precedence', () => {
  const drafts = {
    '0': { fill: { 'part.material': 'Steel' }, none: ['part.coating'] },
    '1': { fill: { 'part.material': 'Brass' }, clear: ['part.heat_treatment'] },
  };
  const local = new Map([[1, new PendingEdits([{ kind: 'fill', id: 'part.material', name: 'Material', value: 'Bronze', text: 'Bronze' }])]]);
  const pending = withDrafts(local, drafts, (_part, id, value) => ({ name: id.slice(5), text: String(value) }));
  assert.equal(describe(pending.get(0).get('part.material')), 'Add material: Steel');
  assert.equal(pending.get(0).get('part.material').draft, true);
  assert.deepEqual(pending.get(0).list(), { fill: { 'part.material': 'Steel' }, none: ['part.coating'] });
  assert.deepEqual(pending.get(1).list(), { fill: { 'part.material': 'Bronze' }, clear: ['part.heat_treatment'] });
  assert.equal(pending.get(1).get('part.material').draft, undefined);
  assert.match(describe(pending.get(1).get('part.material'), drafts[1]), /replaces the agent’s answer/);
  assert.equal(withDrafts(local, {}, () => assert.fail('no draft')).size, 1);
});

test('an explicit guarded change preserves its expectation and a refused answer sends no edit', () => {
  const change = { to: 'Bronze', expect: 'Brass' };
  const reason = 'the file states "Brass" for part.material. To change it, use change with "to": "Bronze", "expect": "Brass".';
  const describeRow = (_part, _id, value) => ({ name: 'Material', text: String(value) });
  const refused = withDrafts(new Map(), { '1': { refused: { 'part.material': reason } } }, describeRow).get(1);
  assert.equal(refused.get('part.material')?.kind, 'refused');
  assert.ok(describe(refused.get('part.material')).includes(reason));
  assert.deepEqual(refused.list(), {});
  const pending = withDrafts(new Map(), { '1': { change: { 'part.material': change } } }, describeRow).get(1);
  assert.deepEqual(pending.list(), { change: { 'part.material': change } });
  assert.equal(describe(pending.get('part.material')), 'Change Material: Brass → Bronze');
  assert.equal(pending.get('part.material').draft, true);
});
