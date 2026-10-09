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

| Need | Replacement (branch `prototype/rust-port`, commit 5798c59) |
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
  `prototype/rust-port`**, which contains the former `ap242` head a16d7af (the stage 1-5 pin;
  checked with `git merge-base --is-ancestor a16d7af origin/prototype/rust-port`). Cargo.lock
  pins the commit (5798c59 since the move, question Q2), so fresh clones and CI resolve the
  same one; moving to a newer head is `cargo update -p quiddity -p haecceity` in a commit of
  its own, followed by the whole suite and the oracle comparisons with the area files
  rejudged (the features sweep's per-face and per-feature entries name exact cases and
  indices; `tools/known_differences/features.py` says how). Recognition stays caller-space
  (`correspondence::recognise`), as Python's entry points, not the framed `recognise_placed`
  (Q2).
- **Cargo.toml and Cargo.lock across parallel streams.** A stream that adds a crate adds its
  line in alphabetical order within its table (`[dependencies]` or `[dev-dependencies]`) and
  regenerates the lock with `cargo update -p <crate>` only (never a bare `cargo update`, which
  would move other pins). The integrator resolves a Cargo.lock conflict by taking either side
  and rerunning `cargo update -p <crate>` for each crate the merged streams added (or `cargo
  check`, which adds missing entries without moving existing ones), never by editing the lock
  by hand.
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
| `load.py` | `part.rs` | `LoadedPart { path, bytes, binding, part (haecceity `Part`, the part's own solid in its own frame, faces in specify-core's numbering), definition (`PartDefinition`), part_count, name, placements }`; `load`, `load_all`, `parts`. A part of an assembly is extracted from `read_step` by face source and placed back by the inverse of its first placement (stand-in for U1); `read_step`'s own `Part` is used directly whenever its faces already equal the definition's. A rebuilt `Part` loses haecceity's unresolved face and edge lists (`Part::new` empties them and `with_unresolved` is `pub(super)`), so `LoadedPart` carries them re-indexed (`unresolved_faces`, `unresolved_edges`), and `describe_faces`, `recognise`, `mesh` and `anchors` consult them: a part with any is reported degraded and recognition refuses on it rather than run on a falsely valid solid. `LoadedPart` is not `Clone` (`haecceity::Part` is only `Debug`); APIs take it by reference. `Binding` with `loader_version` 2. |
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
| `anchors.py`, `sightlines.py` | `anchors.rs`, `sightlines.rs` | Anchor point: `face_centre` checked to lie on the trimmed face (stand-in for quiddity's `inspect_face`, U6); sightlines by `Classifier`. The reach (0.12 of the diagonal) uses the exact bbox diagonal, where Python's `serve._diagonal` uses OCCT's loose `BRepBndLib` box: anchor points differ systematically by that ratio, recorded once as a `rust-correct` pattern. |
| `writer.py`, `requirements.py` | `writer.rs` | Intent → one `PartPmi` per part → `pmi::write` (Add for a bare part or one with foreign PMI, Replace for a file specify-core wrote) → atomic replace of the output → read back and compare. Report format kept. |
| `resume.py` | `resume.rs`, `pyjson.rs` | Stored answers as a CAx-IF user defined attribute on the part (`AttributeSet` 'pmi-assist answers'); Python-written files are read too. The fingerprint hashes Python's exact `json.dumps` text (`pyjson.rs`); schema 2 written, zeros unsigned (Q14), schema 1 read. |
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
- Stored answers: `{schema, answers, part, face_count, faces}`, compact JSON
  (`separators=(",", ":")`), base64, in a `DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist
  answers','<base64>')`. Schema 2 is written; schemas 1 and 2 are read (compared as Python's
  `==` compares: `1.0` and `true` are schema 1), any other is passed over. A payload resumes
  when its `face_count` is the part's and its `faces` is the part's fingerprint *of the
  payload's own schema* (Q14).

### The fingerprint (both engines, byte for byte)

For each face in face order (`describe_faces`), the pair `[kind, [x, y, z]]`: `kind` the face's
kind string, `x, y, z` its centroid's components each rounded to 3 decimals by Python's
`round(c, 3)` (correctly rounded, half to even; `pyfmt::round_half_even`). Then:

- **Schema 2** (written): every component that is zero after rounding is written unsigned:
  `0.0`, never `-0.0` (Python `0.0 if r == 0 else r`; Rust `if r == 0.0 { 0.0 } else { r }`).
  Rounding comes first, so `-0.0004` becomes `0.0`.
- **Schema 1** (read only): the components as rounded, signs of zero kept.

The list is written as Python's `json.dumps(faces)` writes it with its defaults: `[` `]`
brackets, `", "` between items and `": "` (unused here), strings as JSON strings with
`ensure_ascii`, each float as `float.__repr__` (shortest round-trip digits, always a `.0` or an
exponent: `5.0`, `-0.109`, `1e+16`); Rust writes the same text with `pyjson::dumps`. The
fingerprint is the lowercase hex SHA-256 of that text's UTF-8 bytes (Python
`hashlib.sha256(text.encode()).hexdigest()`). Example, the plate's first two faces:
`[["plane", [-50.0, 0.0, 0.0]], ["plane", [0.0, -30.0, 0.0]], ...]` where schema 1 had
`[-50.0, -0.0, -0.0]` from OpenCascade and `[-50.0, -0.0, 0.0]` from haecceity.
`resume::fingerprint_of` implements both schemas over such a list; the cross-engine check
(`tests/cross_engine.rs`, `schema_two_fingerprints_are_the_same_in_both_engines`) compares it
with the hash computed in Python from Python's own faces (`tools/capture/crossengine.py`,
`_fingerprint_v2`) on every part of every file in the check.

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
| Knurls | appended text route + UDA | not written, straight or diamond: `turned_knurl` needs parameters the intent lacks (WR2; a diamond's helix angle, WR9), so each is in `not_written` (Q4, Q15) |
| Surface finish on faces, part notes (finish default, coating, heat treatment, edges) | appended text route | not written: haecceity refuses notes (decision 5, no standard form yet); each listed in `not_written` (Q3, U3) |
| Leaders for threads, knurls, finish | appended presentation | none (presentation is out of scope except datum symbols) |
| Stored answers | `PROPERTY_DEFINITION('pmi-assist answers')` text | `AttributeSet` 'pmi-assist answers' on the part, one string item (base64 JSON, payload as Python's) |

Write modes: a bare part is `Mode::Replace` (the same file as `Mode::Add` for a part with no
PMI; it also drops Python-form stored answers that do not resume, as Python does; Q18); a part
with PMI from elsewhere is `Mode::Add` (the original's bytes and PMI kept; no answers stored, as in Python, because its PMI cannot be told
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
- the write report and the written STEP read back by Python. **The read-back calls Python's
  functions directly, not `api.analyse`**: for a file with stored answers it can resume,
  `api.analyse` returns `existing: []` and `part: {}` (`api.py`, the `answers_for` branch), and
  Python stores answers on every write of a bare part, so every synthetic part's read-back
  would be empty. The capture records, for every written part, `readback.json` = `{existing:
  [e.to_dict() for e in read_existing(load(out, part=k))], part: part_settings(that loaded
  part), resume: resume.answers_for(that loaded part)}` (a call that raises gives `{error,
  message}` in place of its value). The same artifact shape comes from `capture.py readback`
  (stage 4's cross-engine check). For synthetic parts and NIST files only, the written STEP is
  committed gzipped as `written.step.gz` (stages 3 and 4 resume and read Python-written files);
  its bytes are not a parity target;
- mesh summary: face count, triangles per face, `missing`, bbox, per-face mesh area, edge count
  (not the triangles: meshes are checked by properties, not parity);
- anchors for preview groups; a `serve` transcript of one session per synthetic part.

Inputs: the synthetic parts of specify-core's `tests/conftest.py` and of individual tests, built by
`tools/make_parts.py` and committed under `tests/fixtures/parts/`; NIST's AP242 CTC-01 to 05
(public-domain US government works, gzipped and committed as haecceity does); the quiddity corpus
(`QUIDDITY_CORPUS`, default `/Users/paul/repos/quiddity/tests/corpus`, never searched for
elsewhere), not committed: its outputs are committed, its inputs are not. A `readback` mode runs
the same direct read-back (above) on a STEP the Rust port wrote, for the cross-engine check.

**Gzipped corpus files.** Six corpus inputs are gzipped (`cadgenbench_inputs/cgb{202,217,242,
243}.step.gz`, `inventory_refusal/{13975,14052}.step.gz`); neither engine reads gzip, and
haecceity's `corpus_upgrade.json` pins the `cadgenbench_inputs` ones as upgradable, so keeping
them as refusals would lose real cases. Decision: both engines see the decompressed file. The
capture tool decompresses to a temporary `.step` and runs Python on it; `meta.json` records the
sha256 of both the `.gz` and the decompressed bytes (the binding's `source_sha256` is the
decompressed one); the Rust harness's `case.input()` decompresses to a temporary file and
returns its path. The CLI itself still refuses gzip, as specify-core does.

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
`rust-wrong` entry is a bug kept visible, not a pass. `check_known(area, diffs)` is the
asserting form, used once per area by the area's own sweep. The harness also has a
non-asserting `explained(areas, diffs) -> Vec<Difference>` returning only the differences no
entry of the named areas' files matches, with no stale check and no tally: a sweep that crosses
areas filters through earlier areas' files with it, because running their `check_known` there
would report every entry not met in that sweep as stale.

**Feature ids cascade.** Feature ids differ between engines by design (defining versus
constituent faces, U2), and question ids, answer keys, intent requirements and report lines
embed them. So comparisons with Python are made where the inputs are Python's (Python's
analysis and answers into the Rust rules, writer, CLI and serve); Rust-produced analyses are
checked by composition (the CLI equals the library) and by smoke tests (the rules run on them
without panic or refusal), not entry by entry against Python. The cascade is recorded once as a
documented rule in the `cli` area, not per difference. Tests whose input is a corpus file skip when
`QUIDDITY_CORPUS` does not have it; tests on committed fixtures always run, in CI too.

Python's unit tests are ported where they test behaviour (named after the Python test, citing
it), not where they test an OpenCascade workaround (`test_p21.py`, `test_magnitudes.py`,
`test_conformance.py`'s text fixes, `test_merge.py`'s transplant: their intent is re-expressed
against haecceity, e.g. "the original file is kept byte for byte" for `Mode::Add`). No test is
weakened: a Python assertion that does not hold in Rust becomes a known difference with a verdict.

Each of specify-core's 32 test files appears once below (the stage porting it in brackets).

Ported: `test_load.py` [1 part; its reading parts 2 features], `test_standards.py`,
`test_materials.py`, `test_choices.py` [1 tables], `test_features.py` [2 features],
`test_rules.py`, `test_datums.py`, `test_general.py`, `test_locations.py` (minus `mark_basic`),
`test_locating_problems_extras.py`, `test_attention.py`, `test_basis.py`, `test_call_out.py`,
`test_same_as.py`, `test_profile_runout.py` [2 rules], `test_threads.py` [2 rules; writing 3],
`test_mmc_finish_notes.py` [2 rules; reading 2 features; writing 3], `test_preview.py` [2 rules;
anchors 4],
`test_mates.py` [2 mates], `test_mesh.py`, `test_anchors.py` [2 mesh-anchors; preview and serve
parts 4], `test_writer.py`, `test_requirements.py` (semantic content), `test_resume.py`,
`test_merge.py` (re-expressed for `Mode::Add`), `test_drawn.py` (datum symbols),
`test_conformance.py` (re-expressed, below) [3 writer], `test_assembly.py` [1 parts listing;
2 mates; 3 writes; 4 API and CLI], `test_serve.py` [4].

Not ported: `test_p21.py` (escapes: haecceity writes them), `test_magnitudes.py` (OCCT magnitude
text scans), `test_nist_coverage.py` (tests `tools/nist/coverage.py`, tooling: revisit with
that tool, see "later").

`test_conformance.py`: kept are "every written file passes the schema check" (haecceity
validates every written instance; the test asserts the write report and a clean express check)
and "what was asked is what is read back". Dropped: its assertions on OpenCascade text fixes
(runout zones dropped, type limits, simple tolerances, part datums, the metre scale) and on the
text routes for notes, threads and knurls, which this port does not write (each a
`not-applicable` entry in `writer.json` naming the assertion), and its use of
`tools/conformance`'s generated corpus (that tool is in "later").

**JSON text.** Documents are compared parsed, not as text. Two text-level differences are
expected and recorded by the CLI stream up front: Python writes the analysis file with
`json.dumps(indent=1)` and `ensure_ascii` on, escaping µ, × and ⌀, where `serde_json` writes
UTF-8 (`equivalent`: same parsed value); Python can write `NaN`/`Infinity` tokens where a value
escapes `features._plain`, and `serde_json` writes `null` (`rust-correct`: RFC 8259 has no such
tokens and a JavaScript client's `JSON.parse` rejects them).

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
pitch and 'number of threads'; U9 part names over the assembly structure and one read; U10
specify-core's own notes on faces as anchored items; U11 `face_moments` on every face to 1e-6;
U12 the feature families quiddity-rust does not recognise (ten at a16d7af, three since the move); U13 simple parameter-space loops
at degenerate corners.

## Status

All five planned stages are delivered: every module of specify-core is ported, composed by the
API, the command line and `serve`, and checked against outputs captured from specify-core
(Python). What follows states what the tests verify, as of stage 5 (consolidation). The known
differences are generated into [known-differences.md](known-differences.md) by
`tests/known_differences_doc.rs`, which fails when the document is not the rendering of
`tests/fixtures/known_differences/*.json`.

Stage 5 was integrated into `main` on 2026-10-09. It delivered the generated known-differences
document, this Status section and the README's usage and differences. On the merged tree
`cargo fmt --all --check` and `cargo clippy --all-targets --release -- -D warnings` are clean,
and `SPECIFY_CORPUS=... cargo test --release` passes all 302 tests. The dependencies have since
moved from rev a16d7af to `prototype/rust-port` (5798c59, Q2), with every oracle comparison
rerun and the changed differences rejudged; that move was integrated into `main` on 2026-10-09,
where fmt and clippy are clean and all 302 tests pass with `SPECIFY_CORPUS` set. Three of the maintainer's
decisions were then integrated into `main` on 2026-10-09: stored answers schema 2 (Q14, Rust
engine), ISO 286 to 3150 mm (Q8, Rust engine; the comparison with Python above 500 mm waits for
specify-core's extension) and writing the rest of an intent whose items name no faces (Q17).
On that tree fmt and clippy are clean and all 312 tests pass with `SPECIFY_CORPUS` set. The
knurl derivation (Q4 and Q15) is not yet implemented (questions.md, Q15 status).

### Implemented and verified, per module

Counts of differences are those each area's sweep prints with `SPECIFY_CORPUS` set (corpus
cases skip without it); each difference matches an entry with a verdict, and an unmatched
difference or a stale entry fails the sweep.

| Module | Verified by | Against Python |
|---|---|---|
| Oracle and harness (`tools/capture.py`, `tools/capture/`, `tests/common`) | `tests/oracle_harness.rs` (the harness on hand-made values; every case's input is the file it was captured from) | Fixtures for the 11 synthetic parts, the quiddity corpus cases and the NIST AP242 files, plus scenarios transcribed from specify-core's tests |
| `part.rs` (load, parts, binding at `loader_version` 2) | `tests/part.rs` | Face numbering equals Python's on every synthetic, corpus and NIST part checked except corpus `inventory_refusal/13975` (Q10, Q13); NIST products with surface models are one part (Q7) |
| `standards.rs`, `materials.rs`, `choices.rs`, `pyfmt.rs`, `pyjson.rs` | `tests/standards.rs`, `tests/materials.rs`, `tests/choices.rs`, `tests/pyfmt.rs` | Equal to Python-generated tables and sweeps (`choices_oracle.json.gz`, `pyfmt.json`) |
| `features.rs` | `tests/features.rs` | 120 corpus and NIST parts, none refused as degraded: 19945 differences since the move to 5798c59 (14661 undetermined, most of them the vertices and triangles of three sheet-metal flat patterns; 3900 rust-wrong, 1161 rust-correct, 211 equivalent, 12 not-applicable; 8652 on a16d7af), each face value judged against OpenCascade's adaptive integral |
| `existing.rs` | `tests/existing.rs` | 120 inputs and 17 written parts: 230 differences (208 rust-correct, 22 not-applicable) |
| `rules/`, `extras.rs`, `general.rs`, `locations.rs`, `problems.rs` | `tests/rules_oracle.rs`, `tests/rules.rs`, `tests/datums.rs`, `tests/general.rs`, `tests/locations.rs`, `tests/extras.rs`, `tests/problems.rs`, `tests/attention.rs`, `tests/basis.rs`, `tests/call_out.rs`, `tests/same_as.rs`, `tests/profile_runout.rs`, `tests/threads.rs`, `tests/mmc_finish_notes.rs`, `tests/preview.rs`, `tests/rust_analyses.rs` | Equal to Python on all 994 captured artifacts (from Python's inputs: 248 question sets, 116 file writes, 98 scenarios); `rules.json` is empty. The rules also run on the Rust engine's own analyses of every synthetic part (`tests/rust_analyses.rs`) |
| `mates.rs` | `tests/mates.rs` | 18 multi-part parts: 7 differences (6 not-applicable, Q7; 1 rust-correct) |
| `mesh.rs`, `triangulate.rs` | `tests/mesh.rs` | 120 cases, 1 312 080 triangles: 1048 differences (553 equivalent, 419 rust-correct, 37 undetermined, 27 not-applicable, 12 rust-wrong); meshes judged by properties (areas, boxes, edges, missing faces), not triangle by triangle |
| `anchors.rs`, `sightlines.rs` | `tests/anchors.rs` | 124 cases, 708 anchors: 259 differences (129 equivalent, 123 rust-correct, 6 not-applicable, 1 undetermined) |
| `writer.rs`, `resume.rs` | `tests/writer.rs`, `tests/resume.rs`, `tests/assembly_write.rs`, `tests/write_oracle.rs` | 136 writes, from Python's intents, read back by the Rust reader: 713 differences (632 not-applicable, 60 rust-correct, 21 undetermined, none rust-wrong); every file `tests/writer.rs` writes passes haecceity's EXPRESS schema and rule checks |
| `api.rs`, `main.rs`, `serve.rs` | `tests/api.rs`, `tests/cli.rs`, `tests/serve.rs` | The binary's `analyse`, `mesh`, `questions` and `write` against the library's results (exactly) and against Python's command line and serve transcripts (`cli_oracle.json.gz`): 55 differences (24 rust-correct, 15 equivalent, 16 not-applicable); `test_serve.py` and `test_assembly.py`'s API and command-line tests ported |
| Cross-engine (`examples/write_fixtures.rs`, `tools/capture/crossengine.py`) | `tests/cross_engine.rs` | Python reading every Rust-written synthetic and NIST file, and the Rust engine reading the 11 Python-written synthetic files: 70 differences (25 rust-correct, 43 undetermined, 2 not-applicable) |

Known-difference entries by verdict (entries, not differences): 4164 in all, 951 rust-correct,
1588 rust-wrong, 76 equivalent, 1492 undetermined, 57 not-applicable, after the Q8, Q14 and Q17
integration (4152 after the move to `prototype/rust-port` 5798c59; 3634 at stage 5, on a16d7af). The whole suite
(`SPECIFY_CORPUS=/Users/paul/repos/quiddity/tests/corpus cargo test --release`, 312 tests
since the Q8, Q14 and Q17 integration) passed at stage 5, again after the move to
`prototype/rust-port` 5798c59, and after that integration. The command line was also run by hand at stage 5 on `tests/fixtures/parts/plate.step` and `assembly.step`
(every README command; exit codes as documented).

### Stand-ins in use

What quiddity-rust does not yet provide, with the local stand-in (details and the "met when"
in [upstream-needs.md](upstream-needs.md)) and the parts each affects in the oracle:

| Need | Stand-in | Affects in the oracle |
|---|---|---|
| U1 part of an assembly in its own frame | Faces taken from `read_step_placed` by face source, rebuilt with `Part::new`; a rebuilt part with unresolved faces or edges is reported degraded and not recognised | Rebuilt: the 5 parts of `parts/assembly` and `parts/stack` (`tests/part.rs`, `rebuilt_parts_carry_their_instances_unresolved_lists`). Degraded: none; all 100 corpus files load and none has a rebuilt or unresolved part (`corpus_parts_and_their_unresolved_faces`), and none of the 120 parts the features sweep compares is refused as degraded |
| U2 constituent faces, pattern members | Defining faces; members matched to hole records by equality | Every analysis: feature ids differ from Python's (the cascade, `cli.json`); seven `rust-wrong` patterns in `features.json` (floors, drill points, boss ends, an angled step's plane, pattern members) |
| U3 notes and surface finish | Not written; each listed in `not_written` | Every write with a finish default, coating, heat treatment, edge note or face finish (`writer.json`, not-applicable) |
| U4 face triangulation | `triangulate.rs` (constrained Delaunay in parameter space, `spade`) | Every mesh; faces listed in `missing`: cgb202 face 399 (U13), the zero-area thread end faces of `threaded_connector_109` and cgb242 face 483, and `inventory_refusal/14052` face 1 (a hole crossing its outer loop) (`mesh.json`) |
| U5 overlap area of coplanar faces | Polygon intersection of the faces' loops (`i_overlay`), arcs resampled exactly | Contacts of every assembly part: the 18 multi-part parts the mates sweep compares; equal to Python's 3 decimals on `parts/stack` |
| U6 a point on the trimmed face | `face_centre` if inside the face's domain, else the largest triangle's centroid, else no anchor | Every anchor (708 on 124 cases); NIST FTC-07 faces 185-187, where Python's point is off the trim (`anchors.json`, rust-correct) |
| U7 read-back verification as a library function | The comparison rebuilt from `pmi::read` and `pmi::write::differences` | Every write (136 oracle writes, `tests/write_oracle.rs`) |
| U8 thread pitch and 'number of threads' | haecceity's own test mapping; pitch, drill and depths in an `AttributeSet` | Every written thread |
| U9 names over the assembly structure, one read | Names from a `p21::Document` of the same bytes; the file is read twice | Every load; `parts/wrapped` named "assembly"; NIST FTC-08 and CTC-04 RD named by the product id |
| U10 specify-core's notes on faces | Its `GEOMETRIC_ITEM_SPECIFIC_USAGE`s read from the `p21::Document` | Reading the 11 Python-written synthetic files (`tests/existing.rs`, `tests/cross_engine.rs`) |
| U11 `face_moments` on every face | None: a face without moments refuses the part; accuracy reported value by value | None refused since 5798c59 (cgb217 face 29, refused at a16d7af, now has moments); 1407 face values `rust-wrong`, 1167 `undetermined` (`features.json`) |
| U12 families not recognised | None: absent from the analysis | One `rust-wrong` entry per remaining family (`section_recesses`, `risers`, `step_levels`: 853 features) in `features.json`; 2740 features at a16d7af, ten families |
| U13 simple parameter-space loops | Closed freeform directions unwrapped; a loop still crossing or jumping refuses the face | cgb202 face 399, listed in `missing` (`mesh.json`, rust-wrong); cgb242 faces 715 and 726 triangulate since 5798c59 |
| U14 replace of a thread haecceity wrote | `writer::clear` removes the part's PMI in a scratch copy first; the bare 'thread runout' aspect is allowed as a new finding | Every rewrite of a resumed part with a thread (`tests/resume.rs`) |
| U15 OpenCascade crashes on datum symbols | None: written as haecceity writes it (valid AP242) | All 7 Rust-written synthetic files with datums crash specify-core (`cross_engine.json`, undetermined; Q16) |

### Open rust-wrong and undetermined work

Listed entry by entry in [known-differences.md](known-differences.md), "Open work"; in short:

- **features** (1576 rust-wrong, 1480 undetermined entries): face areas and centroids off the
  adaptive integral (U11: 1407 rust-wrong, 1167 where neither engine is within 1e-6); the three
  missing families (U12); 158 blend chains Python supersedes by its fillets (U2's
  reconciliation); constituent faces (U2); families or features one engine reports and the
  other does not (among them the recess source families Python's evidence view leaves out),
  and small differences in feature records (locations up to 1.85e-3 mm, body keys up to 2e-5
  relative, 3e-4 on cgb217, blend points up to 0.023 mm, sheet-metal flat patterns tessellated
  more coarsely), undetermined; corpus `inventory_refusal/13975`'s face order (Q10).
- **mesh** (12 rust-wrong, 4 undetermined): a freeform face not triangulated (cgb202 face 399,
  U13); single inward triangles of at most 7.3e-6 mm2 on eight faces at degenerate corners
  (cgb202, cgb207, cgb242); refinement stopping at its round cap on cgb243 face 537; NIST CTC-05 AP242
  edges, the folded surface and the face order of `inventory_refusal/13975` (Q13), undetermined.
- **anchors** (1 undetermined): a leader from `inventory_refusal/14052`'s invalid face.
- **writer** (3 undetermined): knurls not written (Q4, Q15).
- **cross_engine** (2 undetermined): OpenCascade crashes on Rust-written datums (Q16, U15);
  Python does not read AP242 threads (Q20). Resume (Q14) is rust-correct both ways: schema 1
  files fail on signs of zero as the rule says, and the captured specify-core does not yet read
  the schema 2 the Rust writer stores, whose fingerprint its own faces give.
- **rules, existing, mates, cli**: none.

### Open questions

Every question in [questions.md](questions.md) is open, each with what is done meanwhile:

- **Interchange between engines:** Q1 (mixing engines on one analysis), Q10 and Q13 (a file whose
  face order OpenCascade changes: corpus `inventory_refusal/13975`), Q14 (resume across engines:
  done in the Rust engine as schema 2; specify-core's side pending), Q16 (Rust-written files with datums crash OpenCascade 7.9,
  U15), Q20 (what Python reads of a Rust-written file, and which side should change).
- **What is written:** Q3 (part notes and surface finish, listed in `not_written`), Q4 and Q15
  (knurls, none written), Q6 (an assembly with foreign PMI written by add mode), Q17
  (requirements on no faces listed in `not_written`, the rest written), Q18 (bare parts by `Mode::Replace`; a second material in
  add mode refused).
- **Engine behaviour:** Q2 (moved to `prototype/rust-port`; recognition kept caller-space),
  Q7 (a product with surface models: one part), Q8 (ISO 286 extended to 3150 mm), Q9
  (malformed input refused as invalid), Q11 (an undecided sightline sample blocks), Q12 (mesh
  density, about 1.8 times Python's triangles), Q19 (unmeasurable contacts as warnings).
- **Repository:** Q5 (whether CI checks out the corpus).

### What remains

- **Out of scope, as above:** presentation other than datum feature symbols; notes and surface
  finish in the file (U3, Q3); knurls (Q4, Q15); PMI on assembly occurrences; the Draftwright
  round-trip tool; the NIST coverage tooling and `test_nist_coverage.py`; the conformance corpus
  maker; inch-specific defaults.
- **Waiting on quiddity-rust:** each need in [upstream-needs.md](upstream-needs.md); when one
  is met, its stand-in is replaced and the affected area files' entries go stale, which the
  sweeps report.
- **Not captured by the oracle** (`tests/fixtures/oracle/README.md`): scenarios whose input is a
  file Python wrote (built instead from the committed `written.step.gz` files by
  `tests/resume.rs`, `tests/writer.rs` and `tests/cross_engine.rs`), an assembly write with
  different answers per part through Python, and tests of internals on hand-made face dicts.
- **CI and the corpus:** without `SPECIFY_CORPUS` (or `QUIDDITY_CORPUS`) every corpus case and
  the 15 corpus-backed scenarios skip (Q5).
