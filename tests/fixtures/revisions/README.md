# Revision pairs

Pairs of revisions of one small part (`<case>.old.step.gz`, `<case>.new.step.gz`) for
`tests/revision.rs`. They are quiddity-rust's correspondence fixtures
(`tests/fixtures/revisions`), copied unchanged:

| Case | What changed |
| --- | --- |
| `hole_moved` | the Ø6 hole moved from x = 40 to x = 25 |
| `hole_added` | a Ø7 hole added at (30, 30) |
| `hole_removed` | the Ø7 hole at (30, 30) removed; the holes after it renumber |
| `boss_resized` | the boss Ø20 → Ø26 |
| `plate_thickened` | the plate 10 → 12 thick; its pattern holes are symmetric repeats |
| `whole_part_moved` | the whole part turned and moved, nothing else |

The `complex_*` pairs are made by `complex.py` (build123d; specify-core-rust #56): a 100 × 80 × 10
plate with 4 × Ø6 holes on a Ø60 circle, a pocket, two chamfers and a boss underneath (each
`.old`), changed in ways that read badly before, each `.new`:

| Case | What changed |
| --- | --- |
| `complex_many` | holes 4 × Ø6 → 6 × Ø8, the pocket deeper and wider, a slot added, the chamfers and the boss removed |
| `complex_fillet_all` | R5 fillets on the four corners (the chamfers carry round them) |
| `complex_moved` | the geometry turned 90° and moved, and the holes Ø6 → Ø7 |
| `complex_remodelled` | the same shape, built as two halves |
| `complex_counterbore` | the through holes counterbored |
| `complex_pattern` | 4 holes on Ø60 → 5 on Ø48 |

`bearing_flange` is build123d's `examples/cast_bearing_unit.py` (Apache-2.0, by Gumyr), an
oval flanged bearing housing with drafted sides, exported with build123d 0.11.0. The new
revision changes two parameters: the flange `A1` 11 → 13 thick and the bolt holes
`Hole(5)` → `Hole(4.5)`, Ø10 → Ø9.
