# Cross-engine fixtures

What each engine reads of the other's files, checked by `tests/cross_engine.rs` (verdicts in
`../known_differences/cross_engine.json`). One directory per synthetic or NIST oracle case
whose every part has Python's accept-defaults intent:

| File | Written by | What it is |
|---|---|---|
| `rust.step.gz` | `cargo run --release --example write_fixtures` | Python's intent (each part's `apply.json.gz`) written by the Rust writer, base answers stored. Byte-checked against today's writer. |
| `rust.refused.txt` | the same | Why the case was not written, in place of `rust.step.gz` (NIST CTC-02 and CTC-04: Python's intents name parts the Rust engine does not have, Q7, so the example's part-index check refuses them before the writer is called). |
| `python_readback.json.gz` | `tools/capture.py cross-engine tests/fixtures/cross_engine` | specify-core's read-back of `rust.step.gz`, in the oracle's `readback.json` shape (per part: `read_existing`, `part_settings`, `resume.answers_for`), or `{error: "crash"}` where OpenCascade died. |
| `python_readback_without_symbols.json.gz` | the same | Only where OpenCascade crashed: a probe reading the file without the datum symbols' `MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP` (U15, Q16). Not a file either engine writes. |
| `python_meta.json.gz` | the same | The read file's sha256 (a stale capture fails the test), specify-core's version, and per part the resume evidence: the payload Python's pattern finds, the face count, the faces it fingerprints and the hash, the schema specify-core writes, and the schema 2 fingerprint of those faces by docs/design.md's canonical form (`fingerprint_v2`). |
| `python_written_evidence.json.gz` | the same | The same resume evidence for the file Python wrote for the oracle (`../oracle/<case>/written.step.gz`). |

Regenerate in this order, in one commit: the example, then the capture (each `.gz` is written
reproducibly; only `captured` dates change on a rerun).
