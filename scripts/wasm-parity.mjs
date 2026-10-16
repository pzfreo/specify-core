// Exercise the release's web module directly in Node; no second WASM build or npm deps.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { brotliCompressSync, gzipSync, constants } from 'node:zlib';
import init, { IntentEngine, review, interpret, start } from '../crates/specify-core-wasm/pkg/specify_core_wasm.js';

const root = new URL('../', import.meta.url);
const wasm = readFileSync(new URL('crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm', root));
await init({ module_or_path: wasm });
start();
const executable = fileURLToPath(new URL('target/release/examples/parity_reference', root));
const project = Buffer.from(JSON.stringify({ revision: 1, drawing: 'front' })).toString('base64');

// Geometric calculations may differ in their last bits between native and WASM libm.
// Keep all ids, strings, array order, reports and written STEP bytes exact.
function compare(actual, expected, path = '$') {
  if (typeof actual === 'number' && typeof expected === 'number') {
    assert.ok(Math.abs(actual - expected) <= 1e-10 * Math.max(1, Math.abs(expected)), `${path}: ${actual} != ${expected}`);
  } else if (actual && expected && typeof actual === 'object' && typeof expected === 'object') {
    assert.equal(Array.isArray(actual), Array.isArray(expected), `${path}: array type`);
    assert.deepEqual(Object.keys(actual).sort(), Object.keys(expected).sort(), `${path}: keys`);
    for (const key of Object.keys(expected)) compare(actual[key], expected[key], `${path}.${key}`);
  } else assert.deepEqual(actual, expected, path);
}
// Delaunay ties on circular planar caps can choose different diagonals across targets.
// Compare the mesh contract per face, including every vertex and oriented surface area.
function meshGeometry(mesh) {
  const { positions, indices, ...metadata } = mesh;
  assert.equal(positions.length % 3, 0);
  assert.equal(indices.length % 3, 0);
  const surfaces = mesh.faces.map(([start, count]) => {
    const vertices = new Map();
    let area = 0;
    const orientedArea = [0, 0, 0];
    for (let i = start * 3; i < (start + count) * 3; i += 3) {
      const triangle = indices.slice(i, i + 3).map(index => {
        assert.ok(Number.isInteger(index) && index >= 0 && index * 3 + 2 < positions.length);
        const point = positions.slice(index * 3, index * 3 + 3);
        vertices.set(JSON.stringify(point), point);
        return point;
      });
      const [a, b, c] = triangle;
      const u = b.map((x, k) => x - a[k]);
      const v = c.map((x, k) => x - a[k]);
      const cross = [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]];
      area += Math.hypot(...cross) / 2;
      for (let k = 0; k < 3; k++) orientedArea[k] += cross[k] / 2;
    }
    return { vertices: [...vertices.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([, point]) => point), area, orientedArea };
  });
  return { metadata, surfaces };
}
function throwsString(action, pattern) {
  assert.throws(action, error => typeof error === 'string' && pattern.test(error));
}

for (const part of ['plate', 'pin', 'a_placed_part_is_meshed_in_the_frame_the_analysis_uses']) {
  const path = fileURLToPath(new URL(`tests/fixtures/parts/${part}.step`, root));
  const name = `${part}.step`;
  const bytes = readFileSync(path);
  const engine = new IntentEngine(name, bytes);
  try {
    const snapshot = JSON.parse(engine.snapshot());
    const initial = JSON.parse(engine.review('{}'));
    const question = initial.questions.find(q => /^(hole.function:|diameter.fit:)/.test(q.id));
    assert.ok(question, `${part}: a diameter choice to interpret`);
    assert.ok(question.diameter > 0, 'presented question preserves diameter');
    assert.ok(initial.open.includes(question.id), 'unanswered question remains open despite preview defaults');
    const text = question.id.startsWith('hole.') ? 'H7' : 'h6';
    const chosen = JSON.parse(interpret(JSON.stringify(snapshot.analysis), '{}', question.id, text));
    assert.ok(chosen.callout, 'diameter interpretation includes callout');
    const answers = { 'part.material': 'steel', [question.id]: chosen.value };
    const answersJson = JSON.stringify(answers);
    assert.ok(!JSON.parse(engine.review(answersJson)).open.includes(question.id), 'explicit answer closes question');
    const native = spawnSync(executable, [], {
      cwd: fileURLToPath(root), encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
      input: JSON.stringify({ name, path, answers, project, question_id: question.id, text }),
    });
    assert.equal(native.status, 0, native.error?.message ?? native.stderr);
    const expected = JSON.parse(native.stdout);
    const { mesh: actualMesh, ...actualSnapshot } = snapshot;
    const { mesh: expectedMesh, ...expectedSnapshot } = expected.snapshot;
    compare(actualSnapshot, expectedSnapshot);
    compare(meshGeometry(actualMesh), meshGeometry(expectedMesh), '$.mesh');
    compare(JSON.parse(engine.review(answersJson)), expected.review);
    compare(JSON.parse(review(JSON.stringify(snapshot.analysis), answersJson)), expected.free_review);
    assert.deepEqual(JSON.parse(interpret(JSON.stringify(snapshot.analysis), answersJson, question.id, text)), expected.interpret);
    const bare = JSON.parse(engine.writePmi(answersJson));
    const stored = JSON.parse(engine.writePmi(answersJson, project));
    assert.deepEqual(bare, expected.bare, 'bare STEP bytes and report');
    assert.deepEqual(stored, expected.project, 'project STEP bytes and report');
    assert.ok(stored.report.written.length > 0, 'PMI written');
    assert.ok(Buffer.from(stored.bytes).includes(Buffer.from(`DESCRIPTIVE_REPRESENTATION_ITEM('draftwright editor project','${project}')`)));
    const reopened = new IntentEngine(name, Uint8Array.from(stored.bytes));
    try { assert.deepEqual(JSON.parse(reopened.snapshot()).analysis.resume.answers, answers); }
    finally { reopened.free(); }
    throwsString(() => engine.review('[]'), /Invalid answers/);
    throwsString(() => engine.writePmi('{}', 'not base64!'), /not base64/);
    throwsString(() => engine.writePmi('{}', ''), /not base64/);
    throwsString(() => review('{', '{}'), /Invalid analysis/);
    throwsString(() => interpret(JSON.stringify(snapshot.analysis), '{}', 'missing', 'H7'), /No choice question/);
    console.log(`${part}: snapshot, review, interpretation, exact PMI bytes/report, project and resume parity passed`);
  } finally { engine.free(); }
  throwsString(() => new IntentEngine(name, gzipSync(bytes)), /Decompress/);
}
for (const [part, pattern] of [['assembly', /one distinct part/]]) {
  throwsString(() => new IntentEngine(`${part}.step`, readFileSync(new URL(`tests/fixtures/parts/${part}.step`, root))), pattern);
}
console.log(`WASM sizes: raw ${wasm.length} bytes; gzip ${gzipSync(wasm, { level: 9 }).length} bytes; Brotli ${brotliCompressSync(wasm, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length} bytes`);
// Duplicate the wrapped plate's occurrence to exercise the one-placement refusal.
const wrapped = readFileSync(new URL('tests/fixtures/parts/wrapped.step', root), 'utf8');
const twice = wrapped.replace('ENDSEC;\nEND-ISO', `#500 = CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#501,#503);
#501 = ( REPRESENTATION_RELATIONSHIP('','',#32,#10)
REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#454)
SHAPE_REPRESENTATION_RELATIONSHIP() );
#503 = PRODUCT_DEFINITION_SHAPE('Placement','Placement of an item',#504);
#504 = NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','plate','',#5,#27,$);
ENDSEC;\nEND-ISO`);
throwsString(() => new IntentEngine('twice.step', Buffer.from(twice)), /one instance/);
