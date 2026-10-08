# What specify-core-rust needs from quiddity-rust

quiddity-rust (quiddity and haecceity) is owned by other runs and is not modified from here.
Each need below says what is missing, where specify-core uses it, the stand-in this crate uses
meanwhile (honest: it refuses or reports where it cannot be sure), and how to tell the need is
met. Checked against quiddity-rust branch `ap242` at a16d7af and `prototype/rust-port` at
ef87a63 (2026-10-08).

## U1. A part of an assembly as a `Part` in its own frame

**Missing.** `haecceity::step::read_part_definitions` gives each distinct part's face `#N`s in
specify-core's numbering and its placements, and `read_step` gives one `Part` of every placed
instance. Nothing gives the `Part` of one distinct part, in its own coordinates, with its faces
in `PartDefinition.faces` order: what specify-core's `load(path, part=i)` gives (the solid's own
frame, the frame the analysis, mesh, anchors and mates use).

**Used by.** `part.rs` (`load`, `load_all`), and so features, mesh, anchors, mates for every
assembly and every part stored as an assembly holding one solid (all NIST files).

**Stand-in.** `read_step_placed(bytes, inverse of the part's first placement)`, then the faces
whose `face_source` is in that placement's instances, reordered to `PartDefinition.faces` and
rebuilt with `Part::new` (edges and loops re-indexed). Refused, naming the part, if any face of
the definition is not found exactly once or a placement is not invertible.

**Met when** haecceity offers e.g. `read_part(bytes, &PartDefinition) -> Part` (or the
`PartDefinition` carries it), faces in the definition's order, its face sources kept.

**Also lost by the stand-in.** `Part::new` sets `unresolved_faces`, `unresolved_edges`,
`face_sources` and `edge_sources` empty, and `Part::with_sources` / `Part::with_unresolved` are
`pub(super)`, so a crate outside haecceity cannot restore them on a rebuilt `Part`. `solid_is_valid`
(`brep.rs`) reads `unresolved_faces`, and many quiddity recognisers filter on it, so a rebuilt
part with an unresolved surface would claim valid solids. Meanwhile `LoadedPart` carries the
re-indexed unresolved lists and `read_step`'s own `Part` is used whenever its faces already match
the definition; a rebuilt part with unresolved faces or edges is reported degraded and not
recognised. **Also needed:** a public way to build a `Part` keeping its sources and unresolved
lists (e.g. `pub` `with_sources` / `with_unresolved`), or `read_part` above, which makes it moot.

## U2. Constituent faces and pattern members from quiddity

**Missing.** Python quiddity's evidence view gives a feature's *constituent* faces (the faces
that physically belong to it: a hole's bore and floor) and a pattern's member features.
`quiddity::Features.defining` gives *defining* faces only (`pin.step`: Python `holes:3.5`, Rust
`[3]`), and `HolePattern` holds copies of its member `HoleRecord`s, not their indices. Python's
cross-family reconciliation (which drops `bosses` on turned parts and adds `step_levels`) is not
ported either (`features/mod.rs`: "the Python aggregate's cross-family reconciliation is not
ported").

**Used by.** `features.rs` (feature faces and ids, hole sets), `general.rs` (a hole's floors and
counterbores are excluded from the part's plane levels: with defining faces only, a blind hole's
floor becomes a plane level and gains a general dimension), `rules` (`_bores` filters to the
bore anyway, so hole questions are unaffected).

**Stand-in.** Defining faces; pattern members matched to hole records by equality (the first
not yet taken), as quiddity's own `fingerprint::derived_faces` does. The extra plane levels are
known differences with verdicts.

**Met when** quiddity exposes per record its constituent faces (or its `Occurrence.context`
split into belonging and consulted) and per pattern its member indices.

## U3. Notes and surface finish in `pmi::write`

**Missing.** `pmi::write` refuses notes (`docs/step-ap242.md` decision 5: standard forms only,
none chosen yet). specify-core writes a part's surface-finish default, coating, heat treatment
and edge note, and a finish on faces.

**Used by.** `writer.rs`. The part-note questions have defaults, so almost every intent has
them.

**Stand-in.** Not written; each is listed in the write report's `not_written` (the field Python
has for intent with no place in the file), never dropped silently. See questions.md Q3.

**Met when** `PartPmi.notes` (and a surface texture on faces) are written in a standard form and
read back.

## U4. Face triangulation

**Missing.** haecceity has point clouds on faces (`Part::face_cloud`) but no triangulation.
specify-core's `mesh` (and `serve`'s `analyse`, which returns the mesh) needs triangles per face,
wound outward, at a chordal deflection of 0.001 of the part's diagonal and 0.3 rad.

**Used by.** `mesh.rs`.

**Stand-in.** `triangulate.rs`: each face triangulated in its parameter space from its boundary
loops (`uv_loops`), refined until the deflections hold, mapped to 3-D on the exact surface,
wound by the face's outward normal. A face it cannot triangulate is listed in the mesh's
`missing` (the format's own way of saying so). Checked by properties: every triangle's centroid
maps into the face's domain, the triangles' area is within 1% of `face_moments` area, normals
outward.

**Met when** haecceity offers `Part::triangulate(face, deflection, angular) -> (points,
triangles)`.

## U5. Overlap area of two coplanar faces

**Missing.** `mates.contacts` needs the area two facing coplanar faces of different parts share
(Python: `BRepAlgoAPI_Common` and its surface area). haecceity has no boolean of faces.

**Used by.** `mates.rs` (contacts, and so datum A's default in an assembly).

**Stand-in.** The two faces' boundary loops (edge samples, outer and inner) projected into their
common plane and intersected as polygons with holes (a polygon-boolean crate); the area of the
result. Curved boundaries are sampled at the edge samples' density, so the area is approximate
(reported to 3 decimals as in Python; differences are known differences with verdicts).

**Met when** haecceity offers the common area of two planar faces.

## U6. A point proved on a face's trimmed domain

**Missing.** specify-core's anchors use Python quiddity's `inspect_face(face).anchor`, a point
proved to lie on the face's real trim. haecceity's `Part::face_centre` is build123d's
`Face.center()` (the middle of the parameter range on a curved face), which can fall outside a
trimmed face.

**Used by.** `anchors.rs`.

**Stand-in.** `face_centre`, accepted only if the face's `FaceDomain::contains` its parameters;
otherwise the centroid of the face's largest triangle from the U4 stand-in; otherwise no anchor
for that face (the page falls back to its own guess, as specify-core documents).

**Met when** haecceity offers a point on the trimmed face with its parameters.

## U7. Read-back verification as a library function

**Missing.** `quiddity pmi write`'s check (add: the part's PMI before plus exactly the items
written, a feature or datum equal to an existing one being that one; replace: exactly the items
written; other parts and findings unchanged) is in `src/bin/quiddity.rs` (`verify`,
`item_keys`), not in the library.

**Used by.** `writer.rs`.

**Stand-in.** The same comparison rebuilt here from `pmi::read` and
`pmi::write::differences`, with the add-mode merge rule restated and tested.

**Met when** quiddity or haecceity exports it.

## U8. Thread pitch and 'number of threads'

**Missing.** `pmi::Thread` has no pitch, and what 'number of threads' (a ratio) counts is open
in haecceity (`docs/step-ap242.md`, stage 2: "Open: what a thread's 'number of threads'
counts"). haecceity's own test of specify-core intents writes `number_of_threads` 1, `form`
"M", the designation as `qualifier`, and the pitch in an attribute set.

**Used by.** `writer.rs` (internal and external threads).

**Stand-in.** haecceity's test mapping, exactly; the pitch, tapping drill and depths in an
`AttributeSet` on the thread's feature; `through` as the string "true" (Python's UDA form; the
writer refuses booleans).

**Met when** haecceity settles the meaning of 'number of threads' and where the pitch goes.

## U9. Part names over the assembly structure, and one read for parts and geometry

**Missing.** `PartDefinition.name` is the product's `name` as written (escapes unresolved), and
haecceity exposes no assembly structure: which product definitions are assemblies and which
components each holds. specify-core names a part after an assembly that holds it alone (an
exporter's wrapper: `tests/fixtures/parts/wrapped.step` is "assembly", not "plate"), and
OpenCascade names a product by its `id` when its `name` is empty (NIST FTC-08 and CTC-04 RD:
"NIST PMI FTC 08 ASME1"). Separately, `read_part_definitions` reads the whole file and drops the
`Part` it built, so loading a part reads the file twice (`read_part_definitions`, then
`read_step` or `read_step_placed`).

**Used by.** `part.rs` (`parts`, `load`, `load_all`: every part's name; every load's time).

**Stand-in.** Names from a `haecceity::p21::Document` of the same bytes: the
`NEXT_ASSEMBLY_USAGE_OCCURRENCE`s give each assembly's components (depth first from the roots,
components in file order), `PRODUCT_DEFINITION` → `PRODUCT_DEFINITION_FORMATION` → `PRODUCT`
gives the name, the id when the name is empty, escapes resolved by `p21::decode`. A component
that XCAF makes of a product's several shape representations (NIST files, named `SOLID` by
OpenCascade) is not an assembly here: such a part is named by its product. The file is read
twice, plus the `p21` parse.

**Met when** `PartDefinition` carries its display name (or its product's id and the assembly
structure with names), and one call returns the `Part` with the definitions (or `read_part`
of U1, reading once).

## U5, as built (mates stream)

The stand-in in `src/mates.rs` (`overlap`, `edge_points`) intersects the two faces' loops with
the `i_overlay` crate (even-odd fill, outer and inner loops alike), in the first face's plane.
Circular arcs are resampled from their exact curves to a sagitta of 1e-9 of their radius (made
only for faces whose boxes already meet), so straight and circular boundaries give the exact
area to Python's 3 decimals (the stack's 2254.701 and 28.274, checked against hand-computed
areas in `tests/mates.rs`); ellipses and splines use haecceity's edge samples (2e-4 mm chord
tolerance) and may differ in the last digit. A loop that does not close (consecutive edges or its
ends more than 0.01 mm apart) or runs along an unresolved edge is refused: no contact for that
pair, the reason from `mates::contacts_with_refusals`. Nothing more is needed from haecceity
than U5 as stated above.

## U10. specify-core's notes on faces, as anchored items

**Missing.** specify-core (Python) writes a thread, knurl or surface finish on faces by its own
text route: a part-level `PROPERTY_DEFINITION('manufacturing requirement', <kind>, ...)` (which
`pmi::read` reads as a `Note` with no owner) and a `SHAPE_ASPECT(<kind>)` whose
`GEOMETRIC_ITEM_SPECIFIC_USAGE`s, named `<kind>` ('internal thread', 'external thread', 'knurl',
'surface finish', 'surface texture'), name the faces. Nothing semantic refers to that aspect (only
a presentation link and, for threads and knurls, a user defined attribute), so `pmi::read`
reports no note on those faces.

**Used by.** `existing.rs` (`read_existing`'s notes): the rules treat a face with a thread or knurl
note as covered and do not ask for its size.

**Stand-in.** The `GEOMETRIC_ITEM_SPECIFIC_USAGE`s of those names are read from the
`haecceity::p21::Document` (name and identified item), each face mapped through
`PartDefinition::face_index`, exactly as Python's `existing.notes` reads its own text; threads
and knurls `pmi::read` holds semantically (`Thread`, `Knurl`) are added. Checked against
Python's read-back of every file it wrote (`tests/existing.rs`) and a live write of the plate
with a face finish.

**Met when** `pmi::read` reports these notes with their faces (for instance a `Note` on the
aspect's feature), or specify-core-rust no longer needs to read Python-written files.

## U11. `Part::face_moments` on every face, to 1e-6

**Missing.** `describe_faces` gives each face's area and centroid from `Part::face_moments`.
(a) It returns `None` for corpus `cadgenbench_inputs/cgb217.step` face 29 (`#2586`, a B-spline
surface; OpenCascade: area 49.9176), so `describe_faces` refuses that part rather than invent a
value; it is the only part of the oracle's 120 compared (every captured corpus file and the NIST
files) affected. (b) Its accuracy: against OpenCascade's adaptive integration
(`BRepGProp.SurfaceProperties` with eps 1e-9), on 1307 face areas or centroids (B-spline, torus,
cylinder and sphere faces of cgb202, cgb203, cgb207, cgb242, cgb243, threaded_connector_109 and
NIST CTC/FTC files) haecceity is off by 1e-6 to 1e-3 relative where OpenCascade's default
integral is within 1e-6; on 860 others haecceity is within and OpenCascade not; on 1097 neither
is (`tests/fixtures/known_differences/features.json`, each with both numbers and the reference;
regenerated by `tools/known_differences/judge_faces.py` and `features.py`).

**Used by.** `features.rs` (`describe_faces`: the analysis's faces, which rules use to pick datum
faces and top faces), and so every analysis.

**Stand-in.** None for (a): the part's faces are refused, naming the face. (b) is reported value
by value in the known differences; no correction is applied.

**Met when** `face_moments` gives every face of the corpus a value within 1e-6 relative of an
adaptive reference.

## U12. The feature families quiddity-rust does not recognise

**Missing.** Python quiddity's evidence view reports ten families quiddity-rust a16d7af does not
return from `correspondence::recognise`: `slots`, `oriented_slots`, `polygonal_bosses`, `pads`,
`sheet_metal_bodies`, `freeform_surfaces` and `blends` (no recogniser), `section_recesses` (not
in `Features`; being ported on prototype/rust-port), `risers` (`recognise_risers` exists but is
outside `Features`) and `step_levels` (made by Python's cross-family reconciliation, U2). On the
oracle's cases that is 2740 features Python reports and Rust does not
(`tests/fixtures/known_differences/features.json`, one `rust-wrong` entry per family).

**Used by.** `features.rs` (`recognise`): the analysis's features, which Draftwright's picker
lists. specify-core's rules read only `holes`, `hole_patterns` and `turned_steps`, so no question
or default changes.

**Stand-in.** None: the families are absent from the analysis (an absence, not a guess); each
missing feature is a known difference.

**Met when** `correspondence::recognise` returns these families with their defining (or, with
U2, constituent) faces.
