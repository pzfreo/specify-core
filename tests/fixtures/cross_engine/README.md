# Cross-engine fixtures

What each engine reads of the other's files, checked by `tests/cross_engine.rs` (verdicts in
`../known_differences/cross_engine.json`). One directory per synthetic or NIST oracle case
whose every part has Python's accept-defaults intent:

| File | Written by | What it is |
|---|---|---|
| `rust.step.gz` | `cargo run --release --example write_fixtures` | Python's intent (each part's `apply.json.gz`) written by the Rust writer, base answers stored. Byte-checked against today's writer. |
| `rust.refused.txt` | the same | Why the case was not written, in place of `rust.step.gz` (NIST CTC-02 and CTC-04: Python's intents name parts the Rust engine does not have, Q7, so the example's part-index check refuses them before the writer is called). |
| `python_readback.json.gz` | `tools/capture.py cross-engine tests/fixtures/cross_engine` | specify-core's read-back of `rust.step.gz`, in the oracle's `readback.json` shape (per part: `read_existing`, `part_settings`, `resume.answers_for`), or `{error: "crash"}` where OpenCascade died. |
| `python_readback_without_symbols.json.gz` | the same | Only where OpenCascade crashed: a probe reading the file without the datum symbols' `MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP` (U15, Q16). Not a file either engine writes. None since haecceity stopped writing that relationship (Q16, quiddity-rust c228f95): every Rust-written file loads. |
| `python_meta.json.gz` | the same | The read file's sha256 (a stale capture fails the test), specify-core's version, and per part the resume evidence: the payload Python's pattern finds, the face count, the faces it fingerprints and specify-core's own hash of them for the schema it writes (`fingerprint`) and for schema 1 (`fingerprint_v1`), the schema specify-core writes, and the schema 2 fingerprint of those faces by docs/design.md's canonical form (`fingerprint_v2`). |
| `python_written_evidence.json.gz` | the same | The same resume evidence for the file Python wrote for the oracle (`../oracle/<case>/written.step.gz`). |

Regenerate in this order, in one commit: the example, then the capture (each `.gz` is written
reproducibly; only `captured` dates change on a rerun). The committed capture is from
specify-core 2cb6499 (cc341dd with its branches `iso286-3150` and `fingerprint-v2` cherry-picked,
loaded with `SPECIFY_CORE=<that checkout>`; docs/questions.md, "Recapture source"), which writes
and reads stored-answer schema 2.

The bytes are the same on every platform CI runs (macOS arm64, Linux x86_64) and were checked
on Linux arm64 and x86_64 (emulated) as well. Before stored-answer schema 2 they were not: schema
1's fingerprint hashed the sign of each zero centroid component, which integration noise gives
differently from one platform's maths library to another's, so the stored payload of 5 cases
differed on Linux (CI run 37920952871). Schema 2 writes every zero unsigned (Q14); it changed
only that line of the committed files (same Cargo.lock), and Linux CI has passed since (run
37962711443).

For #22, regenerate `rust.step.gz` with the example above: every authored semantic
requirement now carries the documented `specify-core:written:v1` token. Recapture the
Python readbacks and SHA-256 metadata with `tools/capture.py cross-engine`; do not edit
compressed bytes or capture hashes. The reference can be reproduced in an isolated checkout
of `pzfreo/specify-core` at `cc341dd` with `origin/iso286-3150` and `origin/fingerprint-v2`
cherry-picked, followed by `uv sync --frozen`. Set `SPECIFY_CORE` to that checkout when
running its `.venv/bin/python tools/capture.py cross-engine tests/fixtures/cross_engine`.
This preserves the original reference source and pinned Python dependencies without needing
another working repository on the host. The capture records the reconstructed commit hash.
The provenance-only readback departures have explicit `rust-correct` verdicts backed by #22;
the original Python oracle is unchanged. See `docs/pmi-provenance.md` for reader matching.

For quiddity-rust#137, regenerated with v0.2.6 (`b9addd9`): dimension/tolerance
callouts, ISO 1302 surface symbols and General notes are now present, multi-face aspects
are composite, locations own their aspects, and datum sizes use separate entities.
The files include #65 characteristic ids and history and are checked byte for byte again.
Python readbacks are recaptured with the documented `cc341dd` + ISO/fingerprint reference;
the Python-written oracle inputs and their evidence remain independent and unchanged.

The pre-numbering Python reader still returns identical readbacks. The newly numbered
fixtures expose 97 number-field differences across 10 parts after existing semantic
ledger entries are applied; `known_differences/cross_engine.json` records this measured
reader limitation. Numbers are compared with their actual standard roots in the native
characteristic tests, rather than removed from this comparison.
