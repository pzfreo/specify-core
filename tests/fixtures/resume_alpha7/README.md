# Saved before the 0.2 kernel upgrade

These STEP files were written by specify-core 0.1.0-alpha.7 with both kernel crates
pinned to quiddity-rust v0.1.1 (a27b7064), before changing Cargo.toml or Cargo.lock.
They contain real semantic PMI and schema-2 saved answers, including the available
question defaults and an explicit steel material. Their companion JSON records the
answers supplied and the old feature IDs/faces, for checking correspondence without
recomputing the old recogniser's results.

Inputs: quiddity's `tests/fixtures/golden_oriented_slots.step`, corpus
`cadgenbench_inputs/cgb217.step.gz`, and Specify's NIST
`nist_ctc_02_asme1_ap242.stp.gz`. Gzip timestamps are zero. Keep these old files:
regenerating them with the new kernel would erase the upgrade regression.
