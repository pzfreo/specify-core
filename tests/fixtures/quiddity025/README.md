# Independent v0.2.4 boss recovery specimens

These files were written by Specify at `30b2dac`'s source (the pre-upgrade checkout),
with quiddity/haecceity **v0.2.4**, `2f6b3a6c6f6881b4ee91395296877ad6db6cf510`.
Keep them unchanged when upgrading the kernel: they are saved inputs, not current-output
snapshots to regenerate.

- `chamfer.step.gz`: `../revisions/gramil_shaft.new.step.gz`, written with the normal
  defaults, steel, `extra.boss-fit` (h6 on cylinder face 0) and
  `extra.boss-height` (37.6 ±0.05 mm between the end planes). Its boss was recognised as Ø8 × 36.1;
  quiddity-rust#134 restores the 1.5 mm end chamfer, giving the physical 37.6 mm height.
- `round.step.gz`: quiddity-rust v0.2.5 (`6110b4c8`),
  `tests/fixtures/boss_end_round.step.gz`, under that repository's Apache-2.0 licence,
  written through the same **v0.2.4** consumer path, with h6 on cylinder face 0
  and 20 ±0.05 mm between the end planes. The Ø10 boss's cylinder ends at
  Z22.5; its R1.5 convex round ends at Z24. The old 18.5 mm height becomes 20 mm.
- Each `.json` keeps the v0.2.4 boss record, face evidence, settled answers, intent
  **before writing**, and the PMI read back **after writing**. Capture used
  `api::analyse`, `rules::with_all_defaults`, `rules::apply`, `writer::write_bytes`
  with those answers, and `existing::read_existing`. No v0.2.5 outputs were substituted.

The native and browser regressions check stored and external-answer recovery. Native
checks also keep every PMI value, limit, face target and characteristic number, compare
the full intent apart from its input-byte binding, and require a real shortening to
appear in the feature report while the end-plane dimension recomputes its nominal. Specify's saved face fingerprint is distinct
from quiddity's feature fingerprint: the latter's height can change without changing the
former or moving an answer to a different cylinder.
