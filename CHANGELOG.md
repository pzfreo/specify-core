# Changelog

All notable changes to specify-core-rust. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- A turned diameter's question ("a fit, a thread or knurled?") offers every ISO 261 metric
  thread at its size as a choice beside the fits and knurls: the coarse thread as before
  (`thread:M10`, called up M10×1.5-6g), then the fine pitches (`thread:M10x1.25`, code
  `M10×1.25 thread`, callout M10×1.25-6g), from M1.6 to M64 (`standards::ISO_261`,
  `external_threads`, `external_thread`). A size ISO 261 gives in fine pitches only, such as
  Ø15, offers those (M15×1.5, M15×1); a diameter at no ISO 261 size offers no thread. Typing
  the same thread in the free-text box reads as the same choice and writes the same AP242
  thread; a fine pitch typed for a turned diameter is no longer refused. Existing answers
  (`thread:M6`) are unchanged. The oracle sweeps set aside exactly these threads and still compare everything else with Python.

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
