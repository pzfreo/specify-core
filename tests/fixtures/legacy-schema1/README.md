# Schema 1 browser export

`legacy_specify.step` is copied unchanged from pzfreo/draftwright-editor
`scripts/fixtures/legacy_specify.step` (commit acd612f7195f8ed4e106fe95be521b57f9768098,
added in 4177066). It is licensed AGPL-3.0-only there, and that licence is compatible with
this crate's AGPL-3.0-or-later: AGPL-3.0-or-later code may be combined with it and the
combination distributed under AGPL-3.0. It is the mounting-plate sample
(`public/samples/plate.step` of Specify's AGPL sample corpus) saved by the pre-alpha
browser writer (Specify `42ff22c0`), with the answers `part.material = 6082-T6` and
`hole.function:holes:9 = fit:H7` stored as schema 1 in a user defined attribute. Opened now,
`part.material` resumes and the hole answer is left open with its value for the person to
confirm (`tests/shared_answers.rs`).

Its stored fingerprint (`39a96723…`) is the one Specify alpha.6 (quiddity alpha.8) computes;
quiddity v0.1.1 gives 14 of its 23 zero centroid components the other sign of zero, so the
schema 1 hash is not reproduced. The schema 2 fingerprint (`594d3e81…`) is the same under
both kernels. This is an immutable compatibility input, not a regenerated expectation.
