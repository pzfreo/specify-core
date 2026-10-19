# Turned part datum defaults

These are geometric defaults to review, not assertions of design intent. Explicit `datum.A`,
`datum.B`, etc. answers and datums already in the file retain precedence.

A part is treated as turned when its recognised `turned_steps` cylinder faces share a single
axis (parallel directions, including reversed directions, and less than 0.0001 mm transverse
axis separation) and their combined area exceeds the area of lateral planar faces. When no
turned steps are recognised, apply the same tests to recognised bosses: an interrupted,
single-diameter shaft need not have any turned steps. Lateral
means the plane normal is not parallel to the axis. Cross holes, milled flats and keyways can
therefore be auxiliary features. Different principal axes and a boss on a mostly planar block
retain the prismatic rule. A part without recognised steps or bosses retains that rule as well.

- **A** is the axis of the plain recognised cylinder with the greatest combined cylindrical
  area. Diameter breaks equal-area ties, then analysis order. This favours a substantial
  locating seat over a short broad collar. Threads and knurls identified by the current
  answers (including grouped answers), recognition or semantic PMI are excluded, including
  every split face of their diameter. Assembly suggestions use the same effective diameter
  default as the questions. All split faces of the selected diameter are included, sorted by
  face id; different diameters are never combined into one datum feature. With no plain
  candidate, use the prismatic fallback rather than a rough cylinder.
- **B** is a face perpendicular to A: the greatest coplanar assembly contact area when known,
  otherwise the largest perpendicular plane by area. Include its coplanar faces. Recess
  defining faces, slot/channel end walls and blind-hole bottoms are excluded; a recess wall
  is used only when it is the sole perpendicular face. Thus a large window end cannot
  displace an external axial stop, even when it carries contact evidence.
- **C** has no automatic default on a turned body. An explicit datum can supply clocking.

Prismatic parts retain the resting/largest plane for A and planes square to it for B and C.
No plane is required to establish a turned A; without a perpendicular plane B stays empty.

An axial hole receives **diametral position to A alone**, with its nominal axis coincident
with A. This uses the existing ISO 1101 position characteristic: its median line is constrained
by a cylindrical zone in the nominal position. A sets that zone's axis; a perpendicular B
adds no radial or angular constraint to it. B remains available for axial distances and for
non-coaxial hole positions. This is an axis location control, rather than runout (which would
also constrain the surface form). The fastener allowance is unchanged: M2 still defaults to
Ø0.2, `(2.4 - 2) / 2`. The source for the zone interpretation is the
[ZEISS ISO 1101 GPS poster](https://qualityforum.zeiss.com/migration/images/2500_56edcb4b5a2d29f667a6473ad461aeb8.pdf).

The missing-location problem recognises a coaxial hole as located by an actual primary axis
of a turned part, independently of its diameter and the presence of B. Removing A does not
silently reinstate the proposed axis. Parallel offset holes and transverse holes receive no
exemption. Runout likewise references A, and is not proposed on A's own diameter.

## Corpus verification

Using the pinned Rust kernel on the real STEP files under `QUIDDITY_CORPUS`:

| Part | Before default scheme | After default scheme | Default review problems, before → after |
| --- | --- | --- | --- |
| `gramel/string_post.step` | A plane 11; B empty | A cylinder 0 (Ø6); B shoulder 8 | none → none |
| `gramel/GRM-03_thumbwheel_drive_screw.step` with M3 thread and Ø10 knurl answers | A plane 0; B cylinder 6 (Ø3) | A cylinder 2 (Ø5); B Ø10-to-Ø5 shoulder 0 | none → B unused (info) |
| `tests/fixtures/parts/GRM-02_shaft.step` | A D-flat 2/6; B window side 12; C window end 5 | A cylinder 0 (Ø8); B end faces 7/11; C empty | no unlocated holes |
| `gramel/GRM-05_depth_lock_bolt.step` | Proposed A plane 2; B cylinder 0 | Proposed A cylinder 0 (Ø10); B shoulder 2 | none → none |

Reviewer follow-up against the initial axis-first branch: GRM-03 previously selected knurled
face 3 (Ø10), with B on shoulder 0. With
`{"hole.function:holes:14":"tapped:M2","diameter.fit:turned_steps:6":"thread:M3","diameter.fit:turned_steps:3":"knurl:straight"}`
it now selects A on face 2 (Ø5) and retains B on shoulder 0, matching the designer's AP242
intent. The Ø5 × 3 seat has about 47.12 mm² of cylindrical area; the remaining Ø6 × 0.5 collar
has only 9.42 mm². This is a general seat-area rule, with no part names or fixed diameters in
the selection code. Before thread/knurl answers are supplied, the bare STEP has no such
semantic information: its longest cylinder, Ø3 face 6, has the largest area and is proposed.
The string post likewise changes from the initial branch's short Ø7.5 face 9 to Ø6 face 0.
GRM-02 still had the old planar scheme on that branch because recognition reports its main
cylinder as a boss. Its fixture now runs unconditionally, with tests for a very large window
contact and for the sole-perpendicular-face fallback. GRM-03 runs against both the committed
analysis capture and the actual corpus STEP when `QUIDDITY_CORPUS` is set.

The depth lock bolt has no hole requiring location, so no datums are emitted until a runout
or another datum-referenced control is requested. Its runout now uses A. The thumbwheel's B
is retained as an available axial stop; the default coaxial position only uses A, so the
existing unused-datum information is accurate.

The live kernel names the axial holes `holes:17` and `holes:12`, respectively (the captured
Python analyses include their floor faces in the ids). The string post's axial M2 hole changes
from position to plane A to position to axis A; its cross hole changes from A to A|B. The
thumbwheel's axial M2 hole changes from plane A|axis B to axis A. Neither original default
review happened to reproduce the prototype's missing-location warning; regression tests
exercise missing A, missing B, and offset/cross-hole cases explicitly.

Both M2 parts were also written as AP242 with accepted defaults and read-back verification.
The string post writes without warnings. The thumbwheel reports the existing writer warning
that an unreferenced B is omitted by OCCT-based readers, consistent with its unused-B info;
B is still present in the written file and the verified intent.

## Blind tapped hole help and summaries

For the string post's Ø1.6 × 4 mm blind tap drill the question's `why` includes:

> For a blind tapped hole, set the minimum full-thread depth by typing e.g.
> "M2 tapped, 2.5 mm full thread". The default is drill depth minus 3 pitches,
> rounded down to 0.5 mm (at least one pitch).

The choice's code and a typed `M2` answer summary now read **M2 tapped, 2.5 mm full thread**;
its callout includes **2.5 mm min full thread**. This is `(4 - 3 × 0.4) = 2.8 mm`, rounded
down to 2.5 mm. The thumbwheel's 3.8 mm tap drill also defaults to 2.5 mm. An explicit
`M2 tapped, 2.3 mm full thread` keeps 2.3 mm; `M2 tapped, 5 mm full thread` uses the existing
syntax and is accepted when the drill has room (at least 6.2 mm for M2). Through holes and
unknown drill depths receive no blind-depth claim. Stored values remain `tapped:M2` unless a
depth is explicitly specified (`tapped:M2@2.3`); display resolution does not stamp an implicit
depth into an answer. Both native serve and WASM interpretation use the same display rule.

## Oracle regression guard

The Python captures intentionally retain the old defaults. `tests/rules_oracle.rs` still
compares every field, question order, and requirement against them. The complete list of
expected deviations (both old and new values) is additionally pinned in
`tests/fixtures/rule_defaults_oracle.json.gz`, before the documented known-difference
verdicts are applied. This prevents the changed turned-part scopes from accepting unrelated
regressions, including any change to a prismatic part. Inspect with `gzip -dc`; after a reviewed
rule change regenerate with `UPDATE_RULE_DEFAULTS=1 cargo test --release --test rules_oracle`.
The CLI sweep inherits these exact values with its artifact prefixes normalised; explicit
old-default answer variants are also pinned. Its independent library/executable composition
comparison remains unfiltered. Axial length tests now verify their unchanged distances from
shoulder B, and runout tests verify the primary-axis frame A.
