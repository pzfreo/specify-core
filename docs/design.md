# specify-core-rust: design of the port

specify-core (Python, `/Users/paul/repos/specify-core`, AGPL-3.0-or-later) is interview-driven
AP242 PMI authoring: it loads a STEP part, recognises its features, works out what manufacturing
intent is missing, proposes defaults with reasons, and writes the answers back as AP242 semantic
PMI, verified by reading the file back. This crate is its port to Rust, with no OpenCascade and
no Python: geometry, STEP and PMI come from haecceity and features from quiddity, both in
[quiddity-rust](https://github.com/pzfreo/quiddity-rust).

Ground rules, from the maintainer's brief:

- **Accuracy over parity.** Python's outputs are the oracle, not the specification. Every
  difference gets a verdict with evidence (`rust-correct`, `rust-wrong`, `equivalent`,
  `undetermined`, `not-applicable`). Behaviour that exists only to work round OpenCascade is not
  copied where haecceity is more correct.
- **Compatible interfaces.** The command line (`parts`, `analyse`, `mesh`, `questions`, `write`,
  `serve`) and the JSON formats (analysis, answers, intent, mesh, write report, serve protocol)
  stay specify-core's, so Draftwright Specify can run either engine. A deliberate change is
  recorded in the known differences with its reason.
- **Honest failure.** Refuse or report; never silently drop or guess. Deterministic output.
- **Upstream is read-only.** quiddity-rust and draftwright-rust belong to other runs. What this
  crate needs from them and lacks is recorded in [upstream-needs.md](upstream-needs.md), with a
  local, honest stand-in or a refusal meanwhile. Decisions only the maintainer can make are in
  [questions.md](questions.md).

## What specify-core asks of its dependencies

Read from `src/specify_core/*.py` at specify-core `main`:

| Need | Where in Python | OpenCascade / Quiddity call |
|---|---|---|
| Load a STEP once, distinct parts of an assembly, a part placed twice is one part | `load.py` | `STEPCAFControl_Reader`, XCAF labels, `GetFreeShapes`, references, components |
| Face numbering per part, on the part's own solid | `load.py` `_faces` | `TopExp::MapShapes(FACE)` |
| Which `ADVANCED_FACE` each face was read from | `LoadedPart.face_ranks`, `requirements.face_entities_of` | transfer reader `EntityFromShapeResult` |
| Placements of each instance (3x4) and part names | `load.py` `_instances`, `_part_name` | `XCAFDoc_ShapeTool.GetLocation`, `TDataStd_Name` |
| Face kind, area, centroid, plane normal (outward), axis, origin, radius | `features.describe_faces` | `BRepGProp.SurfaceProperties`, `BRepAdaptor_Surface` |
| Recognised features with constituent faces, records, pattern members | `features.recognise` | `quiddity.evidence.build_recognition_evidence` |
| Existing semantic PMI on the part's faces | `existing.py` | XCAF DimTol reader, plus text scans for what it loses (`magnitudes.py`, `datum_faces`, `notes`, `part_settings`) |
| Mesh by face, outward winding, face-boundary polylines without seams | `mesh.py` | `BRepMesh_IncrementalMesh`, `GCPnts_QuasiUniformDeflection`, `Bnd_Box` |
| Mates: cylinders placed per instance, coaxial and overlapping; flat contacts with overlap area | `mates.py` | `BRepAdaptor_Surface`, `BRepTools.UVBounds`, `BRepAlgoAPI_Common`, `Bnd_Box` |
| Label anchors: point on the trimmed face, normal, clear leader direction | `anchors.py`, `sightlines.py` | `quiddity.inspection.inspect_face`, `BRepClass3d_SolidClassifier` |
| Write datums, sizes, locations, geometric tolerances, material | `writer.py` | `STEPCAFControl_Writer` (XCAF DimTol, material tool), then text fixes |
| Write threads, knurls, general tolerance, notes, datum symbols, stored answers | `requirements.py`, `resume.py` | raw Part 21 text appended |
| Add PMI to a file that has some, byte for byte | `merge.py` | OCCT scratch write + text transplant |
| Verify by reading back | `writer.verify`, `_verify_appended` | XCAF reader + text scans |

## The haecceity / quiddity API that replaces it

quiddity-rust is used as a library (Rust to Rust), not through its `quiddity` command line.

| Need | Replacement (branch `ap242`, commit a16d7af) |
|---|---|
| Load, parts, names, placements, face `#N` | `haecceity::step::read_part_definitions(&bytes) -> Vec<PartDefinition>` (`product_definition`, `shape`, `name`, `faces: Vec<u64>` in specify-core's numbering, `edges`, `placements`, `face_index(#N)`) |
| The part's B-rep | `haecceity::read_step(&bytes) -> Part` (every placed instance; faces carry `face_source(i) -> Source { entity, instance }`) |
| Face geometry | `Part.faces[i].surface` (`Surface::{Plane, Cylinder, Cone, Sphere, Torus, ..}` with `Frame`), `Face.reversed`, `Part::face_moments(i) -> FaceMoments { area, centroid, area_vector }`, `face_normal`, `uv_bounds`, `face_centre`, `bounds`, `face_edges`, `edge_faces`, `neighbours` |
| Features | `quiddity::correspondence::recognise(&Part) -> Recognition { features: Features, fingerprints }`; `Features.defining` (defining faces per record); `HolePattern` variants with their member `HoleRecord`s |
| Point classification for sightlines | `haecceity::classify::Classifier::new(&part).classify(p) -> State` |
| Edge polylines | `Part.edges[e].samples`, `Part::edge_faces()` (a seam lists its face once) |
| Existing PMI | `haecceity::p21::Document`, `haecceity::pmi::read(&doc, &parts) -> PmiRead { parts: Vec<PartPmi>, findings, provenance, accounting }` (anchors are face indices of the part) |
| Writing PMI | `haecceity::pmi::write(doc, parts, &[(PartId, PartPmi)], Mode::{Add, Replace, Remove}, PresentationPolicy) -> (Edit, WriteReport)`; every instance validated against the AP242 schema table, rule checks, datum feature symbols for added datums |
| Verification | `haecceity::pmi::read` of the output and `haecceity::pmi::write::differences(&written, &read)` |

Everything in Python that exists only because of OpenCascade is not ported: `p21.py` (escapes:
haecceity writes them), `merge.py` (haecceity's `Mode::Add` keeps every original byte),
`magnitudes.py` and the text fallbacks in `existing.py` (haecceity reads magnitudes and datum
features itself), `locations.mark_basic` (`DimTolerance::Basic`), `writer.py`'s text fixes
(`_drop_runout_zones`, `_type_limits`, `_simple_tolerances`, `_part_datums`, the metre scale
for tolerances), `lettering.py` and the datum symbol drawing (haecceity draws datum symbols,
decision 6 of `docs/step-ap242.md`).

**Face numbering is the same in both engines.** haecceity numbers faces in OpenCascade's
`TopExp::MapShapes` order (`brep.rs`, `PartDefinition`). Checked before this design on
`plate.step`, `pin.step` and the corpus spool `flanged_spool_132.step`: all 113 faces have the
same kind and centroid (to 1e-9) at the same index. The Rust binding therefore keeps
`loader_version` 2, so an analysis made by either engine can be written by the other; the
features stream pins the face-by-face equality on every fixture, and a file where it fails is a
known difference whose analyses must not cross engines (question Q1).

**Features differ by design between the two quiddities.** On `pin.step` Python reports
`holes:3.5` (the bore and its floor: *constituent* faces) and Rust `holes/0 [3]` (*defining*
faces only); Python reports `step_levels` and suppresses `bosses` on a turned part (its
cross-family reconciliation), Rust reports `bosses` and no `step_levels`. Feature ids in this
crate are specify-core's (`family:faces`, from the faces the port uses), so ids for the same
feature can differ between engines. That is safe: an analysis is stored and never re-derived,
and answers are always used with the analysis they were given against.

## Dependency plan

- `quiddity` and `haecceity` by git from `https://github.com/pzfreo/quiddity-rust`, **branch
  `ap242`** (Cargo.lock: a16d7af). `prototype/rust-port` does not yet contain a16d7af (checked
  with `git merge-base --is-ancestor a16d7af origin/prototype/rust-port`), and only `ap242` has
  `read_part_definitions`, `p21`, `pmi`, `express`, `removal` and face provenance. When
  `prototype/rust-port` contains a16d7af, both dependencies switch to it in one commit (`cargo
  update -p quiddity -p haecceity`), and the stream that does so runs the whole suite and the
  oracle comparisons: `prototype/rust-port` also has the framed recognition (`recognise_placed`)
  and the recognition document, which change records (question Q2).
- `serde`, `serde_json` (`preserve_order`, so objects keep Python's key order; `float_roundtrip`),
  `sha2` (the binding's `source_sha256`), `base64` (stored answers), `regex` (typed specs and
  added requirements, as Python reads them). Dev: `flate2` (gzipped
  fixtures). A stream that needs another crate adds one line to `Cargo.toml` and says why in its
  commit; the mates stream is expected to add a polygon-boolean crate for flat contacts
  (upstream need U5).
- Toolchain 1.99 (`rust-toolchain.toml`), edition 2024. No OpenCascade, no Python at run time.
  Python is used only by the oracle capture tool, in specify-core's own venv.

## Module map

| Python (`specify_core/`) | Rust (`src/`) | Notes |
|---|---|---|
| `load.py` | `part.rs` | `LoadedPart { path, bytes, binding, part (haecceity `Part`, the part's own solid in its own frame, faces in specify-core's numbering), definition (`PartDefinition`), part_count, name, placements }`; `load`, `load_all`, `parts`. A part of an assembly is extracted from `read_step` by face source and placed back by the inverse of its first placement (stand-in for U1). `Binding` with `loader_version` 2. |
| `features.py` | `features.rs` | `Face` records (`describe_faces`) from `Surface` and `face_moments`; `recognise` from quiddity: holes, patterns (members matched to hole records by equality, as quiddity's fingerprints do), every other family, and specify-core's own hole sets. |
| `existing.py` | `existing.rs` | `read_existing` maps `PartPmi` to specify-core's `existing` records (kind, OCCT type name, faces, faces2, value, upper, lower, datums: the names are a JSON format, kept); `part_settings` from material, general tolerance class and notes; `notes` from threads, knurls, finish. No text scans. |
| `magnitudes.py`, `p21.py`, `merge.py`, `lettering.py` | none | OpenCascade workarounds; haecceity does it right (above). |
| `standards.py` | `standards.rs` | ISO 286, ISO 273, tap drills, pitches, ISO 2768 classes: tables as `const` arrays in Python's order. |
| `materials.py` | `materials.rs` | Suggestion tables, Python's order. |
| (Python's `:g`, `_fmt`, `repr`) | `pyfmt.rs` | Number formatting shared by choices, rules, problems and the report. |
| `choices.py` | `choices.rs` | `describe`, `split`, `code`, `callout`, `interpret`, `check`, `SpecError`. |
| `rules.py` | `rules/` (`mod.rs` Question, Intent, questions, apply, preview, with_all_defaults, IncompleteError; `datums.rs`; `holes.rs`; `turned.rs`) | Pure functions of the analysis JSON and the answers JSON; no geometry. |
| `extras.py` | `extras.rs` | Added requirements, checked and turned into requirements. |
| `general.py` | `general.rs` | ISO 2768-1 deviations and the general dimensions. |
| `locations.py` | `locations.rs` | `along`, `references`, `distance`, `dimensions`, `parse`. `mark_basic` is not ported (basic is `DimTolerance::Basic`). |
| `problems.py` | `problems.rs` | |
| `mates.py` | `mates.rs` | Cylinders and planes per instance, `mates`, `contacts`, `suggestion`. Overlap area of facing planes by a local polygon intersection of the faces' boundaries (stand-in for U5). |
| `mesh.py` | `mesh.rs`, `triangulate.rs` | Mesh format `pmi-assist-mesh` v2 kept. Triangulation is a local stand-in (U4): each face triangulated in its parameter space from its boundary loops and refined to the deflection; a face it cannot do is listed in `missing`, as the format allows. Edges from the edges' samples, seams left out. |
| `anchors.py`, `sightlines.py` | `anchors.rs`, `sightlines.rs` | Anchor point: `face_centre` checked to lie on the trimmed face (stand-in for quiddity's `inspect_face`, U6); sightlines by `Classifier`. |
| `writer.py`, `requirements.py` | `writer.rs` | Intent → one `PartPmi` per part → `pmi::write` (Add for a bare part or one with foreign PMI, Replace for a file specify-core wrote) → atomic replace of the output → read back and compare. Report format kept. |
| `resume.py` | `resume.rs`, `pyjson.rs` | Stored answers as a CAx-IF user defined attribute on the part (`AttributeSet` 'pmi-assist answers'); Python-written files are read too. The fingerprint hashes Python's exact `json.dumps` text (`pyjson.rs`). |
| `api.py` | `api.rs` | |
| `cli.py` | `main.rs` | Same subcommands, arguments, exit codes (0 ok, 1 write failed, 2 refused/invalid), stdout summaries. |
| `serve.py` | `serve.rs` | Protocol 1, same ops and error codes. Replies on stdout; nothing else writes to stdout. |

## Data types

The JSON formats are the contract with Draftwright Specify, so types serialise to exactly
specify-core's keys, key order and omission rules (`serde_json` with `preserve_order`; fields
`skip_serializing_if` where Python leaves them out). Numbers: integers stay integers, floats are
`f64`; JSON text need not be byte-identical to Python's (`1e-05` against `1e-5` reads the same),
except where a hash is taken (resume fingerprint, `pyjson.rs`).

- `Binding { source_sha256, face_count, loader_version: 2, part }`.
- Analysis (`schema` 1): `{schema, binding, faces: [Face], features: [Feature], mates?,
  existing: [ExistingPmi], part: {material?, general_tolerance?, surface_finish?, coating?,
  heat_treatment?, edges?}, resume?: {answers}}`. `Face { id, kind: plane|cylinder|cone|torus|
  other, area, centroid, direction?, origin?, radius?, contact? }`. `Feature { id, family, record
  (JSON object), faces, members?, notes? }`.
- Answers: `{question id: JSON value}` (strings, face lists, `extra.*` objects).
- `Question` (to_dict order and omissions as Python's), `Intent { schema: 1, rules_version: 1,
  binding, part, datums, requirements, general }` with requirements as JSON objects whose kinds
  are `size`, `location`, `position`, `flatness`, `perpendicularity`, `parallelism`, `profile`,
  `runout`, `total_runout`, `thread`, `knurl`, `finish` (Python's keys). The rules work on
  `serde_json::Value` for the analysis and answers, as Python works on dicts, with typed helpers
  for faces and features: the analysis may come from either engine and from older versions.
- Mesh: `{format: "pmi-assist-mesh", version: 2, binding, bbox, positions, indices, faces,
  missing, edges}`.
- Write report: `{output, written, not_written, warnings}`.
- Errors: `IncompleteError { missing, part }`, invalid answer (`ValueError` in Python) as a typed
  `Invalid(String)`, `SpecError`, verification and write failures; the CLI and `serve` map them to
  Python's exit codes and error codes (`incomplete`, `invalid`, `failed`).

## What is written, and how it differs

| Intent | Python writes | Rust writes (through haecceity) |
|---|---|---|
| Datums A–H | XCAF datums, one per tolerance; datum symbol appended | One `datum` per letter per part, its `datum_feature` on the faces (a hole's cylinders for an axis datum); datum feature symbol (decision 6) |
| Size with fit | limits only (OCCT writes the grade wrongly) | `DimTolerance::Fit` (`limits_and_fits`), the class as stated |
| Size ± / limits, location ±, location basic | XCAF, then `mark_basic` text | `Deviations`, `Basic` |
| Position (Ø, Ⓜ), flatness, perpendicularity, parallelism, profile, runout | XCAF; text fixes | `GeometricTolerance` with zone, modifier, datum system |
| Material | XCAF material tool | `Material` ('material name', decision 2) |
| General tolerance | appended 'default tolerances' text + 'manufacturing requirement' text | `GeneralTolerance::Class` |
| Threads | appended text route + UDA | `Thread` (side, major Ø, form, class, hand, partial area) + `AttributeSet` (pitch, tapping drill, depths, as haecceity's tests map them) |
| Knurls | appended text route + UDA | straight: `Knurl`; diamond: not written (needs a helix angle the intent lacks, Q4), in `not_written` |
| Surface finish on faces, part notes (finish default, coating, heat treatment, edges) | appended text route | not written: haecceity refuses notes (decision 5, no standard form yet); each listed in `not_written` (Q3, U3) |
| Leaders for threads, knurls, finish | appended presentation | none (presentation is out of scope except datum symbols) |
| Stored answers | `PROPERTY_DEFINITION('pmi-assist answers')` text | `AttributeSet` 'pmi-assist answers' on the part, one string item (base64 JSON, payload as Python's) |

Write modes: a bare part is `Mode::Add`; a part with PMI from elsewhere is `Mode::Add` too (the
original's bytes and PMI kept; no answers stored, as in Python, because its PMI cannot be told
apart from what was added); a file specify-core wrote (stored answers that match its faces) is
`Mode::Replace`, every part with stored answers written again together (Python's rules kept). An
assembly with foreign PMI, which Python refuses because of OpenCascade, is written by `Mode::Add`
(a deliberate, recorded difference). An AP214/AP203 file gaining PMI becomes AP242 only if every
instance validates (haecceity decision 1); otherwise the write is refused, naming the instances.

Verification: the output is written beside the destination, read back with `pmi::read`, and
compared with what was written by `pmi::write::differences` (add: the part's PMI before plus the
items written; replace: exactly the items written), the stored answers read back, then renamed
over the destination (symlink written through, permissions kept), as Python does. Any difference
leaves the destination and the source untouched (exit 1).

## Oracle and tests

### Capture tool

`tools/capture.py`, run with `/Users/paul/repos/specify-core/.venv/bin/python` (specify-core's
271 tests pass there). It writes compact checked fixtures under `tests/fixtures/oracle/`, gzipped
JSON, one directory per case, each with `meta.json` (input path relative to its root, the input's
sha256, specify-core's and quiddity's versions and git revision, the command run). For each
input part (each part of an assembly):

- `analysis` (`api.analyse`), `parts` for assemblies;
- `questions` with no answers and after `with_all_defaults`, with defaults, bases, reasons and
  attention; `open` ids; `problems`;
- `preview` intent, `apply(accept_defaults)` intent, `general` dimensions;
- scenarios: answer sets taken from specify-core's tests (extras, datums D–H and frames, typed
  specs, ± locating, runouts, fits, threads, knurls, mates) with their questions, intents,
  problems and refusals (the error kind and message);
- `interpret` cases from `test_choices.py` (typed text → value or refusal);
- the written STEP read back by Python (its `analyse` of the output: existing PMI, part settings,
  resume answers) and the write report; not the STEP bytes, which are not a parity target;
- mesh summary: face count, triangles per face, `missing`, bbox, per-face mesh area, edge count
  (not the triangles: meshes are checked by properties, not parity);
- anchors for preview groups; a `serve` transcript of one session per synthetic part.

Inputs: the synthetic parts of specify-core's `tests/conftest.py` and of individual tests, built by
`tools/make_parts.py` and committed under `tests/fixtures/parts/`; NIST's AP242 CTC-01 to 05
(public-domain US government works, gzipped and committed as haecceity does); the quiddity corpus
(`QUIDDITY_CORPUS`, default `/Users/paul/repos/quiddity/tests/corpus`, never searched for
elsewhere), not committed: its outputs are committed, its inputs are not. A `readback` mode runs
Python's `analyse` on a STEP the Rust port wrote, for the cross-engine check.

### Comparison harness and known differences

`tests/common/` (oracle stream): load a case, compare a Rust JSON value with the oracle's by path
with numeric tolerances (relative 1e-9 by default, per-field overrides such as areas 1e-6),
collect differences as `{case, path, python, rust}`. Each difference must match an entry in
`tests/fixtures/known_differences/<area>.json`, one file per area (`features`, `existing`,
`rules`, `mates`, `mesh`, `anchors`, `writer`, `cli`), each owned by the stream for that area:

```json
{"case": "corpus/cadgenbench/flanged_spool_132", "path": "features[*].faces",
 "pattern": "holes faces exclude floors", "verdict": "rust-correct | rust-wrong | equivalent | undetermined | not-applicable",
 "evidence": "what was checked: the geometry, the standard, the file text, a test"}
```

An unmatched difference fails the test, and so does an entry that matches nothing (stale). A
`rust-wrong` entry is a bug kept visible, not a pass. Tests whose input is a corpus file skip when
`QUIDDITY_CORPUS` does not have it; tests on committed fixtures always run, in CI too.

Python's unit tests are ported where they test behaviour (named after the Python test, citing
it), not where they test an OpenCascade workaround (`test_p21.py`, `test_magnitudes.py`,
`test_conformance.py`'s text fixes, `test_merge.py`'s transplant: their intent is re-expressed
against haecceity, e.g. "the original file is kept byte for byte" for `Mode::Add`). No test is
weakened: a Python assertion that does not hold in Rust becomes a known difference with a verdict.

## Out of scope for now

- **Presentation** other than datum feature symbols (leaders for threads, knurls and finish):
  haecceity decision 6; the semantics are written, and Draftwright draws from semantics.
- **Notes and surface finish in the file** (U3, Q3): no standard form in haecceity yet; reported
  in `not_written`, never dropped silently.
- **Diamond knurls** (Q4): `turned_knurl` needs a helix angle the intent does not have.
- **PMI on assembly occurrences**: haecceity reports it as a finding; specify-core never writes it.
- **The Draftwright round-trip tool** (`tools/draftwright_roundtrip.py`): it runs in
  Draftwright's environment and reads Python's text route, which this port does not write.
- **NIST coverage tooling** (`tools/nist/coverage.py`) and the conformance corpus maker: tools
  around specify-core, not its engine; revisit after stage 4.
- **Inch-specific defaults**: specify-core gives metric defaults only; kept.

## Upstream needs

Recorded precisely, with stand-ins, in [upstream-needs.md](upstream-needs.md): U1 a part of an
assembly as a `Part` in its own frame; U2 constituent faces and pattern members from quiddity;
U3 a standard form for notes and surface finish in `pmi::write`; U4 face triangulation; U5
overlap area of two coplanar faces; U6 a point proved on a face's trimmed domain; U7 the
read-back verification of `quiddity pmi write` as a library function; U8 thread semantics for
pitch and 'number of threads'.

## Stages

The work is planned as four stages of parallel streams (each stream owns disjoint files within
its stage; module stubs exist in `src/` so no stream edits `lib.rs`):

1. Oracle capture tool, fixtures and comparison harness; the part model on haecceity; the
   standards, materials and choices tables.
2. Features and existing PMI; rules (with extras, general, locations, problems); mates; mesh and
   anchors.
3. Writer and resume through haecceity's AP242.
4. API, command line and serve; the cross-engine check and the consolidation of known
   differences and docs.
