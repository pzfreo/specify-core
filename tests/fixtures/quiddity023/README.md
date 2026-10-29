# quiddity v0.2.3 consumer specimens

- `turned_body_disc.step`: copied unchanged from quiddity-rust v0.2.3 (`ced75a5`),
  `tests/fixtures/turned_body_disc.step`, under that repository's Apache-2.0 licence.
  Ø22 × 7, Ø8 through bore, two 0.25 mm end chamfers. Its former full-length OD
  boss is suppressed by #123; its defining cylinder remains face 1.
- `face_grooves.step`: generated with OpenCascade by revolving a closed polygon in
  the radius/Z plane through 360° about Z. Vertices in mm:
  `(4,0), (6.5,0), (6.75,.25), (9,.25), (9.25,0), (11,0), (11,7),
  (9.25,7), (9,6.75), (6.75,6.75), (6.5,7), (4,7)`.
  The two annular face recesses have Ø18.5/Ø13 rims, depth 0.25 and 45° walls.
  This is an original synthetic specimen, not a captured recognition output.
- `turned_body_disc.v022.json`: Specify's feature list on that unchanged disc with
  quiddity/haecceity v0.2.2 (`e58d174`), captured before applying the 0.2.3 rules.
  It pins the removed `bosses:1` and the previous OD datum behaviour.
