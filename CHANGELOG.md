# Changelog

All notable changes to specify-core-rust. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

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
