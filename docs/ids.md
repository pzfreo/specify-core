# Ids: what other tools can build on

These are the names Specify gives a part's requirements and settings: the ids of its questions,
of the feature table's rows, and of the answers a caller sends. They are a contract. An id listed
here keeps its spelling and meaning from one release to the next; renaming or removing one is a
breaking change, made only with a CHANGELOG entry saying why. `tests/ids_contract.rs` reads the
lists below and fails when the code's ids and these lists differ.

<!-- ids:begin -->

## Part-level ids

One per part. An answer to one is stored in the file as the part's setting (or, for "none",
as a decided-none answer), not on any face. Only datums B and C have a control
(`datum.B.control`, `datum.C.control`), and only datum A a flatness (`datum.A.flatness`).

| Id | What it is | Row in the feature table |
| --- | --- | --- |
| `part.material` | the material | yes |
| `part.process` | how the part is made: `machined`, `3D printed: FDM`, `sheet metal`, … ([process.md](process.md)) | yes |
| `part.general_tolerance` | the general tolerance (`ISO 2768-m`, …) | yes |
| `part.locating` | how holes are located: `position tolerances` or `± dimensions` | no: a choice, it writes no PMI |
| `part.surface_finish` | the surface finish where none is given | yes |
| `part.coating` | coating or treatment | yes |
| `part.heat_treatment` | heat treatment | yes |
| `part.edges` | what is done to sharp edges | yes |
| `part.notes` | other notes, an ordered list of text ([part-notes.md](part-notes.md)) | yes |
| `datum.<letter>` | the faces of datum `<letter>`, a list of face numbers; letters `A` to `H` | yes |
| `datum.A.flatness` | flatness of datum A | yes |
| `datum.B.control` | the control on datum B (perpendicularity, or position for a hole) | yes |
| `datum.C.control` | the control on datum C | yes |
| `bom.make_buy` | made or bought: `made`, `bought`, `not known` ([bom.md](bom.md)) | no |
| `bom.description` | the parts-list description | no |
| `bom.part_number` | the part number | no |
| `bom.revision` | the revision | no |
| `bom.optional` | an optional item: `yes` or `no` | no |
| `bom.stock` | `from stock` or `to be made`; not asked, proposed from make/buy | no |
| `bom.name` | the item's name; accepted as an answer, never asked | no |
| `extra.<name>` | a requirement the person adds where no question asked; `<name>` is theirs | no: it is PMI on its own faces |

## Requirement kinds

A feature's requirement has the id `<kind>:<feature>` (below). These are the kinds. There is no
`hole.size`: a hole's size comes from `hole.function`.

| Kind | What it is |
| --- | --- |
| `hole.function` | what a hole is for: a clearance, a fit, `tapped:M6`, … which sets its size |
| `hole.position` | a hole's position tolerance |
| `hole.location` | a hole's location by ± dimensions (when `part.locating` is `± dimensions`) |
| `hole.frame` | the datums a hole's position is measured from, when the part has more than one frame |
| `hole.full_thread` | the minimum full-thread depth of a tapped blind hole |
| `diameter.fit` | a turned diameter's fit or tolerance |
| `diameter.runout` | a turned diameter's runout |

## Added requirement kinds

An `extra.<name>` answer is `{"faces": [...], "kind": ..., "value": ...}` ([extras](../src/extras.rs)).
Its `kind` is one of:

| Kind |
| --- |
| `flatness` |
| `perpendicularity` |
| `parallelism` |
| `fit` |
| `finish` |
| `profile` |
| `runout` |
| `total_runout` |
| `size` |
| `distance` |

## General dimension ids

| Form | Measurement |
|---|---|
| `length:planes:<side>-<side>` | Plane-to-plane length. The two sides are sorted. |
| `location:holes:<reference>-<located>` | Hole location. The second group is the located side. |

<!-- ids:end -->

## A feature id is a kind and the faces it is on

A feature is named by its family and the sorted numbers of its faces: `holes:12`, `holes:15.16`,
`turned_steps:9`, `hole_patterns.grid:6.7.8.10.11.13`. A requirement on it is
`<kind>:<family>:<faces>`.

For example, on the plate (`tests/fixtures/parts/plate.step`) the row
**`hole.position:holes:12`** is the position tolerance of the ⌀4.2 hole whose face is face 12.
On the shaft (`GRM-02_shaft.step`), `hole.full_thread:holes:15.16` is the full-thread depth of
the M2 tapped hole made of faces 15 and 16.

The kinds above are the contract. **The face numbers are not:** they hold for one revision of
the geometry only. Change the model and face 12 may be another face, or gone. What moves a
requirement onto a changed part is carry forward, which matches the old part's faces to the
new one's and gives the requirement its new feature id. A tool that keeps a feature id across
revisions must expect it to change. (Parts whose code names its faces keep their answers on the
same faces when the model is rebuilt: [face-names.md](face-names.md).)

The family names (`holes`, `hole_patterns.<kind>`, `turned_steps`, …) come from quiddity's
feature recognition, and can grow with it.

Each measurement side is a sorted, dot-separated face group. Between two hole axes, the
location id also ends in `-<axis>` (`0` for x, `1` for y, `2` for z). These ids use the same
revision-local face numbers as feature ids.

## An item of the file's other PMI has an id from its STEP entity

The feature table lists the file's requirements on no recognised feature-and-kind as
`file_pmi`. Each that Specify can locate (a dimension, tolerance, datum, thread, knurl or
finish) has an `id`, **`file_pmi:#<entity>`**: the STEP entity its characteristic number is (or
would be) on, for example `file_pmi:#1397`. An edit list removes one by it, numbered or not:
`"items": {"remove": [{"id": "file_pmi:#1397"}]}`. The same form takes a stated row's id, which
removes every requirement stating it that no other feature's row shares: `{"id": "part.material"}`.

The `file_pmi:#` prefix is in the contract. **The entity number is not:** it holds while the file
is saved in place (Specify appends and never renumbers), but a tool that rewrites the file may
renumber its entities. A note that is no thread, knurl or finish, and a setting on no row, have
no id.

## Characteristic numbers are in the contract

Each requirement in the file has a characteristic number, `#N` ([characteristic-numbers.md](characteristic-numbers.md)):

- a change of value or limits keeps its number (`⌀10 ±0.1` becoming `⌀10 ±0.05` is still #3);
- a different measurement (another type, other faces, another reference) gets a new number, and
  the old one is retired;
- removing a requirement retires its number;
- a retired number is never used again.

## Not in the contract

- **Scene item ids** (`planner:dim:…`, the PMI scene's handles) are for presentation: stable
  within a release, not between releases.
- Answers whose id starts with `_` are bookkeeping (`_specify`, `_specify_bom`, `_specify_characteristic_numbers`),
  not requirements.
