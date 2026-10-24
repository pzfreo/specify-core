# Changelog

All notable changes to specify-core-rust. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- For #3, retain the existing turned-diameter ISO 261 thread choices (coarse and fine)
  alongside fits and knurls, and include both 0.05 mm match boundaries despite floating
  point subtraction noise. Otherwise no thread choice is offered. Verify typed/selected
  STEP equivalence and native/WASM exposure.
- For #21, expose blind tapped holes' minimum full-thread depth as an editable text
  question in mm, with the computed default visible. An answered tap retains its inline
  or computed manufacturing depth; optional edits override it, remain unrounded, reach STEP PMI and
  resume from saved answers. Through holes and unknown drill depths have no depth question.
- For #35, `resume::rematch_answers(loaded, features, answers, evidence, automatic)` matches
  answers held outside the STEP (a saved project's) to the part as now recognised, with the
  same face-support rule as answers stored in a STEP. Callers keep `resume::evidence(loaded)`
  (`{schema: 2, face_count, faces}`) with their answers. Legacy projects without saved face
  evidence (and schema 1 exports whose fingerprint can't be reproduced) resume with their
  answers matched by feature ID (a datum kept when its faces exist), the rest `unresolved` with their values, and `unverified` says so. Evidence that
  contradicts the part still leaves every answer on a feature or a datum (including
  `datum.X.flatness`/`.control`) `unresolved` with its value; only `part.*` answers are kept
  then. `automatic` marks answers the default path gave and returns their IDs renamed.
  New public
  fields `UnresolvedAnswer.automatic`, `Resumed.unverified` and `Resumed.automatic` break Rust
  struct-literal callers. See `docs/design.md`, "Saved answers across kernel upgrades".
- For #35, `carried::attributes(source, name, value)` stores an application's named JSON
  payload in the STEP for `writer::write_bytes_with`, with a `specify-core original` record.
  `carried::payload(bytes, name)` reads it back, and `carried::original(bytes)` gives back the
  part first written from, verified by its sha256. The payload's encoding is the editor
  project's, so files written either way are read either way. Only an item in a user defined
  attribute is read as a payload. The core sets no payload size limit; callers set their own.

### Changed

- Pin quiddity and haecceity, including WASM, to tagged **v0.2.0-alpha.1**. Specify's
  version remains alpha.7. Draftwright's older v0.1.1 pin still produces the expected
  advisory warning.
- Adopt recognition schema 6 (#92): publish file-coordinate records, accept measuring
  axes as letters or vectors, and derive slot/channel end planes from the record's own
  origin and distances. Remove `_local`/`_frame`; recognition provenance is never geometry.
  All three golden 30° slots and their six end planes retain questions, defaults and
  written PMI through a 30° Z rotation/translation and a generic rigid rotation.
- Kernel frame-vote corrections (#81) recover fillets: cgb217 **6 → 14**, CTC-02 AP242
  **3 → 20**. With the concavity correction (#89), cgb202 is **272 → 279** (16 recovered
  convex fillets, nine previously reported concave fillets removed). CTC-02 AP242 gains
  **4 pockets** and step levels **2 → 13**; its four former risers disappear. Ambiguous 49.5-mm hole rows
  are no longer patterns (#82). Symmetric countersink seats stay linked (#83), and plate
  boundaries retain their original planes (#84/#85). Fingerprints remain version 3.
- Preserve old-kernel saved-answer fixtures with **13 / 37 / 34 answers** for oriented
  slots, cgb217 and CTC-02 AP242. Resume checks require each value to reattach by geometric
  support or remain open with its original ID and reason, never disappear.

### Fixed

- For #35, schema 1 answers saved by the pre-alpha browser writer resume again under
  quiddity v0.1.1. That kernel gives some zero centroid components the other sign, so their
  schema 1 hash cannot be reproduced. Such payloads in this port's attribute form now resume
  without face evidence, with their answers matched by feature ID, an
  `analysis.resume.unverified` reason, a warning and a `saved_answers_unverified` review
  problem. Rewriting stores schema 2.
  Python-written schema 1 files still open as foreign files (Q14).

### WASM API

- For the kernel upgrade (#92), `snapshot().analysis.features[*].record` uses schema-6
  file-coordinate measurements. Axis fields accept letters or unit vectors. Converted
  records carry a shared `origin`, including when their axes mix letters and vectors;
  `_local` and `_frame` are removed. Method signatures do not change.

- For #3 and #21, editors use the existing question/choice/default structures and unchanged
  JS/TypeScript signatures. Render `hole.full_thread:<feature>` as a text question; its
  string default is drill depth minus three pitches, rounded down to 0.5 mm (at least one
  pitch), or the inline depth. Submit a positive mm value that leaves three pitches, or
  use `acceptDefaults`. The depth is an optional `refine` question, excluded from `open`:
  an answered tap keeps its inline or computed depth in preview and written PMI even
  without a depth answer. Merely showing the depth question adds no accepted answer.
- For #35, add `IntentEngine.rematchAnswers(answers: string, evidence?: string | null,
  automatic?: string | null): string` and `IntentEngine.evidence(): string`. Editors keep
  `evidence()` with saved answers and use `rematchAnswers` instead of setting aside answers
  by a list of answer kinds. Without evidence answers are matched by feature ID; with
  evidence that does not match every answer on a feature or a datum is returned in
  `unresolved` with its value, not in `answers`. See docs/wasm.md, "Answers
  saved outside the STEP".
- For #35, `IntentEngine.writePmi(answers, project?, payload?)` takes an optional third
  argument, `{name, value}` JSON, carried in the STEP as base64 JSON under `name`. Give the
  project or the payload, not both. Either now also records the original part. Add
  `IntentEngine.payload(name: string): string | undefined` and
  `IntentEngine.original(): Uint8Array | undefined`. Existing calls are unchanged; a
  `writePmi` without a project or payload writes none, dropping any the opened file carried,
  and no original record. Written bytes with a project gain the `specify-core original` attribute. Analysis can report
  `resume.unverified`; review can report `code: "saved_answers_unverified"`. The Rust
  `session::Session::write_pmi` gains the `payload` argument (pass `None`).

## 0.1.0-alpha.7 (2026-10-23)

### Added

- For #22, STEP-only provenance on every Specify-written semantic PMI requirement, using
  the stable `specify-core:written:v1` token in AP242 fields and representation descriptions.
  Imported PMI remains unmarked; existing-PMI results expose the token independently of
  stored answers. `existing::part_settings_provenance` also exposes settings authorship.
  The new public `ExistingPmi.provenance` field breaks Rust struct-literal callers; add
  `provenance: None` when constructing an unmarked row. See `docs/pmi-provenance.md`.

### Changed

- Pin quiddity/haecceity to **v0.1.1** (a27b7064), including the WASM crate. Specify's
  version is unchanged. Draftwright's released kernel pin temporarily differs; the
  advisory pin checker continues to warn.
- Adopt the kernel's file-grid/near-axis publication and support-plane plate boundaries
  (#79), and remove CTC-04's false quarter-cone countersink features (#80). Hidden-line
  drawing corrections are upstream; Specify does not consume that drawing API.
- Read framed recognition correctly: file-space vectors/positions stay in file space,
  local slot/channel end planes are projected through the inferred frame, and remaining
  local record coordinates carry explicit frame metadata. Equal-area datum proposals
  keep face order across rigid motion within max(1e-6 mm², 1e-6 times the larger area).
  Aligned and framed slot/channel end-plane checks use the same published precision.
- Migrate saved feature answers by unique same-family geometric face support after
  validating the legacy ordered face fingerprint. Unmatched, ambiguous or colliding
  answers retain their old ID/value/reason in `analysis.resume.unresolved` and appear in
  the existing review warnings; affected questions stay open. **Editor upgrading note:**
  review these warnings and answer the open questions before saving; recover old values
  from the resume report: unresolved old values are not persisted when saving again.
  Files whose geometry no longer matches open as foreign files, retaining existing PMI
  and part settings. Alpha.6 STEP files and unchanged schema-2 payload bytes remain
  supported. See `docs/design.md`, "Saved answers across kernel upgrades".
- **Fewer features on four parts.** Under quiddity v0.1.x, 44 features that alpha.6 recognised are not
  recognised and their faces are unexplained in quiddity's face accounting (no named refusal): 24 fillets
  on cgb202, 18 on the NIST CTC-02 AP242 file, one on cgb207 and an oriented chamfer on mfcadpp/10252.
  Their questions no longer appear. They are recorded as `undetermined` in the known differences
  pending a quiddity fix.
- Adapt PMI to the v0.1.x model and step-io 0.3 decoded strings; keep stored answers,
  literal note text and the STEP-only provenance token round-tripping.

- CI: run the full workspace tests on Ubuntu and macOS with Quiddity's pinned STEP corpus;
  require the corpus and its captured inputs instead of silently skipping them.
- **Native Rust helper:** `pyfmt::fmt_rules` now returns `Result<String, FormatError>`;
  callers must handle non-finite dimension refusals instead of relying on a panic (#26).

### Fixed

- Refuse invalid browser answers, state, non-finite dimensions and oversized inputs with
  thrown string errors; preserve the session after refusals. Audit the WASM-reachable panic
  and indexing sites and exercise hostile inputs in the WASM CI job (#26).
- Preserve Python-compatible text answers (including `null`, empty and non-string values)
  and saved state with stale or repeated automatic-answer IDs at the WASM boundary (#26).
  Reuse core choice validation and the settled review to avoid redundant rule passes.

### WASM API

- No JS/TypeScript signatures change for the kernel pin. Session analysis can additionally
  report `resume.migrated` and `resume.unresolved`; review problems expose unresolved
  saved-answer warnings using `code: "saved_answer_unresolved"`.

- No JavaScript/TypeScript signature changes (#26). Invalid answer IDs, contradictory choices,
  invalid choice/note types and malformed state now refuse explicitly. Inputs are
  bounded to 64 MiB STEP, 8 MiB per JSON/project argument and 64 KiB names/interpretation/mode.

## 0.1.0-alpha.6 (2026-10-20)

### WASM API

- Behaviour change for the editor (issue #20, #28): without a call to `acceptDefaults`, unanswered
  default datums, positions and the general tolerance are no longer drawn or written. Review
  intent and `writePmi` include only answered specifications and the file's own datums. The
  defaults stay on their questions as suggestions with their reasons, and their ids stay in
  `open`; an unanswered general tolerance is absent from `intent.part`. Intent items keep
  their `default` field, which is now always `false`. No signatures change.
- Add `IntentEngine.acceptDefaults(answers: string): string` and
  `acceptDefaults(analysis: string, answers: string): string` (#28). Both return ordinary
  editable answers with every unanswered default accepted to the same fixed point as CLI/serve
  `accept_defaults`. Supplied answers are preserved, questions without defaults stay open, and
  the session is not changed. The existing `review`, `settle` and `writePmi` take the returned
  answers. Existing signatures are unchanged.

### Added

- Free-text part notes as semantic PMI (#25). A new `part.notes` question (kind `text_list`,
  default `[]`) takes an ordered list of literal strings for notes about making the part;
  drawing-only notes stay in the drawing Spec and no existing text is migrated. Each note is
  written as a part-owned `semantic text` attribute set with a `general note` text item and a
  one-based integer `note index`. Answers that are not a list of strings are refused by the
  rules and by the writer. `existing::part_settings` reads the notes back as `notes` in index
  order, also from files without stored answers and for each part of an assembly. Writing an
  identical list over notes another tool wrote adds no duplicates; writing a different one is
  refused. Rules version 3. See `docs/part-notes.md`.

### Changed

- Datums A/B/C are now proposed as measurement origins on every part, not only when a hole
  position or runout needs a frame: also in ± locating mode, when every hole uses the general
  tolerance, and on parts without holes (#24; rules version 2). The same A/B/C defaults apply, including a
  turned part's axis A and shoulder B, and the ± datum prompts ask where the part is measured
  from. Explicit ± hole locations are dimensioned from the selected datum planes instead of
  the default edge faces, so changing or removing a datum changes or removes those
  dimensions. ± mode asks for and adds no position tolerances, datum controls or runout, and does not
  report unused datums. Accepted datums are written as AP242 datum features without
  geometric tolerances. Like other defaults, these datums are written only once answered or
  accepted (#28).
- Rewriting a resumed file whose PMI and stored answers are unchanged returns the original
  bytes, after the replacement has been written and verified (#25).
- Pin quiddity and haecceity (native and WASM) to quiddity-rust `v0.1.0-alpha.8`,
  matching draftwright-rust alpha.9 (#27). This brings derived PMI geometry, measured
  modelled threads and graphic-only PMI presentation accounting. Modelled thread
  faces are excluded from plain datum seats; existing thread interviews and verified
  writing remain available. New departures from the frozen Python captures are
  recorded with independent OpenCascade evidence in the known differences.

### Fixed

- Answering a question no longer accepts every unanswered default (issue #20, #28). Review,
  preview and WASM PMI writing include only answered specifications and existing datums;
  datum, hole-position and general-tolerance defaults remain open suggestions with their
  reasons. CLI/serve writing with explicit `accept_defaults` is unchanged. The editor's
  automatic ±/plain-hole path does not accept the remaining suggestions. Answered controls
  waiting for unaccepted datum frames now report `awaits_datum_frame` in review/serve
  problems instead of disappearing silently; strict apply still refuses an unavailable
  datum control.
- Turned parts with a common principal axis now default to A on the plain recognised cylinder
  with the greatest total cylindrical area (diameter breaks ties) (#23). Threads and knurls from
  answers, recognition or PMI are excluded. Interrupted single-diameter shafts recognised as
  dominant bosses also use their axis. B is a perpendicular locating shoulder (assembly
  contact first, otherwise largest area), excluding recess walls unless the only perpendicular
  face. Explicit datum answers and file datums retain precedence; prismatic parts keep plane
  datums. Coaxial holes use diametral position to A and are recognised as located by that axis;
  offset and cross holes still need location. Runout follows the primary axis. See
  `docs/turned-defaults.md` for detection, ISO 1101 zone interpretation and before/after
  corpus verification.
- Blind tapped-hole help now gives a valid full-thread-depth example (such as
  `M2 tapped, 2.5 mm full thread`) and explains the computed default (#23). Choice and
  typed-answer summaries show that depth, including native serve and WASM interpretation;
  canonical answers, explicit depths, through holes and unknown depths retain their behaviour.

## 0.1.0-alpha.5 (2026-10-18)

### WASM API

- `IntentEngine.settle(state)` returns `{review, state}`: the review settled to the default path
  with label anchors. `answerEdit`, `tolerancingEdit` and `openedTolerancing` change a project's
  `{answers, auto, gdt}` as the editor's tolerancing controls, All specs answers and project
  opening do.
- Add a reviewed TypeScript API snapshot and CI acknowledgement gate: declaration or snapshot
  changes require a new Unreleased WASM API bullet; refresh with `python3 tools/check_wasm_api.py --update`.

### Added

- `tolerancing`: a drawing by default, GD&T on request. The editor's rules for the default path
  (± locating, plain holes except proposed taps, plain diameters except threads and knurls,
  recorded in `auto`) and for GD&T mode changes, moved from draftwright-editor so the editor and
  the MCP server make the same changes.
- Enforce a reviewed optimized WASM size ceiling, with raw, gzip and Brotli measurements and
  headroom on every build. Compare both quiddity kernel pins with Draftwright's latest release
  on main and before uploading releases; mismatches warn and add a Kernel note to release notes.

## 0.1.0-alpha.4 (2026-10-16)

### Added

- Browser package `specify-core-wasm` in a Cargo workspace (issue #11, PR #13), preserving the
  editor's `IntentEngine`, JSON review/interpretation and verified PMI writing API, including
  face-order checks, geometry anchors and the optional `draftwright editor project` attribute
  via the upstream writer. Shared `api::review` composes presented questions, open ids, preview
  intent and problems for any front end. Filesystem APIs and the line service are cfg-gated
  out of WASM; native entry points and default Cargo commands retain their behaviour.
- `wasm-release` profile (LTO, one codegen unit, optimization level 3), wasm-pack builds and
  Node/native parity on PRs and main; published `v*` releases attach a web `pkg/` tarball and
  SHA-256 checksum after parity verification. Actions are SHA-pinned, release write access is
  job-scoped, and nothing is published to npm. See `docs/wasm.md` for use and editor migration.

- `writer::{write_bytes_with, write_parts_bytes_with}` accepts caller-provided
  `&[haecceity::pmi::AttributeSet]`, written and verified in the same edit as the PMI and stored
  answers (issue #10, PR #12). `writer::{write_with, write_parts_with}` also supports atomic file
  output, including `.gz` destinations. Existing APIs delegate with an empty slice and keep
  their signatures and bytes. Nonempty extras require a single-part file; both set and item
  names reserve `pmi-assist answers` to protect resume. Extras follow the existing replace/add
  behaviour: replace removes old attributes, so callers resupply those wanted; add preserves
  existing attributes. Tests mirror the editor's base64 `draftwright editor project`, including
  readback through haecceity, resumed answers, name clashes, empty-slice parity, rewrites,
  foreign PMI and gzip output.

## 0.1.0-alpha.3 (2026-10-16)

Pinned by rev to quiddity-rust's 0.1.0-alpha.4 release commit `f5ddb72` (from `v0.1.0-alpha.3`,
by way of rev `0d601d2`); no tag exists yet.

### Added

- Gzipped STEP input (maintainer decision 2026-10-15): every command, serve request and library
  load (`part::File::read`/`of_bytes`, `part::load_bytes`, `api`) reads a gzipped STEP file or
  bytes, told by its magic bytes, not its name, and decompressed in memory. The binding's
  `source_sha256` and the stored-answers fingerprint are of the decompressed bytes, so a `.step`
  and its `.step.gz` bind and resume identically. `write` to a destination named `.gz` writes it
  gzipped (a write in place to a `.gz` input stays gzipped); other destinations are STEP text. A
  gzip that does not decompress is refused, saying so. `part::gunzip`.

- A turned diameter's question ("a fit, a thread or knurled?") offers every ISO 261 metric
  thread at its size as a choice beside the fits and knurls: the coarse thread as before
  (`thread:M10`, called up M10×1.5-6g), then the fine pitches (`thread:M10x1.25`, code
  `M10×1.25 thread`, callout M10×1.25-6g), from M1.6 to M64 (`standards::ISO_261`,
  `external_threads`, `external_thread`). A size ISO 261 gives in fine pitches only, such as
  Ø15, offers those (M15×1.5, M15×1); a diameter at no ISO 261 size offers no thread. Typing
  the same thread in the free-text box reads as the same choice and writes the same AP242
  thread; a fine pitch typed for a turned diameter is no longer refused. Existing answers
  (`thread:M6`) are unchanged. The oracle sweeps set aside exactly these threads and still compare everything else with Python.
- `part::File`: one haecceity `StepFile::read` per file, from which every part, its
  definitions and the file's hash are taken (`read`, `of_bytes`, `of_shared`, `load`,
  `load_all`, `parts`, `definitions`, `sha256`, `path`, `bytes`).
- `api::write_bytes(bytes, jobs)`, `api::IN_MEMORY` and `writer::write_parts_bytes`: write
  several parts of a file held in memory; the other parts are loaded from the same bytes.
- `PartSummary.product_id`, listed after `name` in the library, `api::parts`, the CLI and serve
  (Python's listing has no such key: recorded as `rust-correct` in `docs/known-differences.md`).

### Changed

- quiddity-rust 0.1.0-alpha.4 takes Python's local-degradation retry, so on corpus parts 13975
  and 14052 the features match Python's: the four entries for features only Rust reported there
  (`holes:0` and `pockets:*` on 13975, `holes:8` and `plates:5.6` on 14052) are removed as stale,
  and the section-recess entry no longer cites 14052 (`docs/known-differences.md`). No question
  changes.

- A gzipped STEP file is no longer refused ("the file is gzip-compressed; decompress it first");
  see Added.

- Writing an assembly mixed in kind, one part resuming specify-core's stored answers and another
  carrying PMI from elsewhere, writes each part by its own mode: replace for the resuming part
  (its PMI is the intent's, once, and its answers are stored), add for the other (nothing it had
  is lost). It was one add for both, which wrote the resumed part's PMI a second time and was
  refused by haecceity (`item_identified_representation_usage.UR1`).
- The analysis of a part held in a `part::File` (`api::analyse_reporting_in`: `analyse --part
  N`, serve) takes the file's part definitions from it and parses the file as Part 21 once, for
  the stored answers and the PMI together, instead of reading the definitions again and parsing
  up to three more times. Output is unchanged; on `nist_ctc_01_asme1_rd.stp` the analysis runs
  in 6% fewer instructions (9.10e9 to 8.55e9), 7% on `parts/plate.step`. New:
  `existing::read_parsed`, `resume::stored_in`, `resume::answers_for_parsed`.
- `docs/upstream-needs.md`: U1 and U9 cite quiddity-rust PR #10 (rev 0d601d20).
- Every part is haecceity's own `Part` (`StepFile::part`); specify-core's stand-ins for part
  extraction, part names, the placed-back read and face matching are gone (upstream needs U1
  and U9 met). `LoadedPart` keeps all its fields; `rebuilt` is now always false.
- Analysis of an assembly part (`api::analyse_file`, CLI and serve `analyse`) loads the part
  and the other parts its mates need from one read; `api::analyse` loads them from
  `loaded.bytes`, never from the path.
- A part of an assembly can be written alone: the refusal when other parts had stored answers
  is gone, and the write mode is decided over the parts being written. An assembly with no
  part chosen still refuses, naming its parts.
- The plate read from the assembly lists 6 of its 26 mesh edge polylines in a different order
  (haecceity numbers edges its own way); the edge set is unchanged.

## 0.1.0-alpha.2 (2026-10-14)

Pinned to quiddity-rust `v0.1.0-alpha.3` (from `v0.1.0-alpha.1`).

### Added

- `part::load_bytes` and `writer::write_bytes`: one part loaded from bytes and written to
  verified AP242 bytes, with no filesystem (for the editor's browser build); byte for byte
  the file API's output (`tests/bytes_api.rs`).
- `Question::to_presented_dict()`: an opt-in presentation record with a feature row's `title`
  and a typed `characteristic` (flatness, perpendicularity, parallelism, position, runout) for
  datum, hole and turned questions; `to_dict()` is unchanged.

### Changed

- quiddity-rust `v0.1.0-alpha.3` publishes `section_recesses` in `Features`, so the analysis
  lists section recesses. Against Python, 154 of them differ only by Python's added plane faces
  (constituent, not defining: upstream need U2, as for holes and bosses), and the regions Python
  lists but quiddity-rust refuses or does not recover are still listed as missing
  (`docs/known-differences.md`). No question changes: the rules read only holes, hole patterns
  and turned steps.
- Writing PMI into the NIST AP203 files (CTC-01, CTC-02, CTC-03, CTC-05, FTC-06, FTC-09) now
  succeeds: haecceity's upgrade repairs their two classes of instance that are invalid in AP242
  (and in AP203) and reports each repair, where the write was refused before. These writes now
  match the Python oracle.

## 0.1.0-alpha.1 (2026-10-10)

The first alpha: a pure-Rust port of [specify-core](https://github.com/pzfreo/specify-core),
interview-driven AP242 PMI authoring for STEP parts, with no OpenCascade and no Python. Features
come from quiddity-rust's recognisers; STEP reading, meshing and PMI reading and writing from its
haecceity kernel (pinned to quiddity-rust `v0.1.0-alpha.1`).

### Added

- Every module of specify-core ported: part loading, features, existing PMI, the rules and
  their defaults with reasons, mates in assemblies, meshing for a face picker, anchors, the
  writer with resume and assemblies, the API, the command line (`analyse`, `mesh`, `questions`,
  `write`, `parts`, `serve`) and the JSON-lines service, with specify-core's JSON formats and
  exit codes, so Draftwright Specify can drive either engine.
- Every output checked against captures from the Python implementation; every difference from
  Python carries a verdict and its evidence (`docs/known-differences.md`).
- Written files open in OpenCascade, and so in specify-core's Python: datums, sizes and
  tolerances on one face or several (one usage per face), notes and surface texture in AP242's
  standard forms.
- Stored answers in schema 2 (fingerprints with unsigned zeros), which resume across engines
  with specify-core's `fingerprint-v2` branch.
- ISO 286 fits up to 3150 mm, agreeing with specify-core's `iso286-3150` branch on every class
  and size.
- Knurls written as AP242 turned knurls, their parameters derived from the knurling standard
  (whole tooth count at the nominal pitch; 30° helix and no hand for a diamond knurl).
- Built on quiddity-rust v0.1.0-alpha.1's kernel API (the maintainer's PR #1: AP242
  attribute sets' description and general property); risers and step levels from its
  recognition, with the differences from specify-core's quiddity 0.3.9 oracle verdicted.
- A write whose requirement names no faces writes the rest and lists that item as not written.
- About three times faster than the Python engine on the one part timed (cadgenbench
  flanged_spool_132: analyse 0.8 s against 2.4 s, write 0.5 s against 1.5 s).

### Known limitations

- Answers the older specify-core stored (schema 1) do not resume in Rust; specify-core's
  `fingerprint-v2` and `iso286-3150` branches are not yet merged into its main.
- specify-core's Python reads a Rust-written fit without its limits, and no thread or surface
  finish (it reads only its own forms of those).
- A knurl's tooth depth (half the pitch) is assumed, as no table available gives it
  (docs/questions.md).
- Open maintainer questions are in docs/questions.md; the needs from quiddity-rust in
  docs/upstream-needs.md.
