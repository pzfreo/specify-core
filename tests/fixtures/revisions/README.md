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

`bearing_flange` is build123d's `examples/cast_bearing_unit.py` (Apache-2.0, by Gumyr), an
oval flanged bearing housing with drafted sides, exported with build123d 0.11.0. The new
revision changes two parameters: the flange `A1` 11 → 13 thick and the bolt holes
`Hole(5)` → `Hole(4.5)`, Ø10 → Ø9.
