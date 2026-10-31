# Shared dimension requirements (#75)

Specify uses `quiddity::dimensioning` from quiddity/haecceity v0.4.0. The dependency's
`docs/dimensioning.md` defines the selection rules and slice boundary. Specify does
not select an independent set of holes, pitches or plane intervals.

A live analysis adds `dimensioning`, the shared `Input::from_recognition` result.
Rules supply the chosen datum face sets and answered/stored dimension contracts to
that input. Hole-position answers supply Basic contracts to the pattern's own
origin and pitches, rather than preserving the generated per-member witnesses.
Hole-only contract passes use the shared planner before the final consolidation;
this prevents a General neighbour swallowing a Basic pattern origin. No additional
basic dimensions are kept beyond the final returned set. Explicit face-picked and
stored dimensions enter `specified`; specified-wins replaces their generated
counterparts. Stored foreign PMI is already present and is not appended twice.

Older JSON analyses have no typed geometry input. Their rule results remain
provisional, and the writer upgrades rule-generated intent using the loaded part.
Hand-authored intent remains authoritative. Invalid planner input is a write
failure, not a silently selected alternative planner.

Each returned `Required` has one row with `planner` lowering information. Sizes and
locations lower through the shared `dimension_kind` helper with a real feature
arena. Actual patterns use `PatternOfFeatures`, counted unpatterned groups use
`MultipleElements`, and synthetic supports use supplemental geometry. AP242 Table
4's `curve length` names axial length quantities (the helper's generic `length`
name is rejected by the kernel's own reader); angular quantities use degrees.
The exact role, count, pitch intervals, covered IDs, supports, frame and optional
reference proof round-trip in a dimension-owned `specify dimension plan` UDA.
Synthetic support coordinates are converted from millimetres into the source
context's length unit using the same step-io reader as the kernel (including inches).
Persistent planner metadata canonicalizes frame noise to nine decimal places;
generated PMI measurements retain Specify's six decimal places in millimetres.
References use the standard `DimensionModifier::Reference` (auxiliary dimension).
Scene IDs and characteristic matching retain the planner's face-derived role IDs,
so different quantities on one support remain distinct. Old numbers are matched
before new numbers are allocated, including reversed undirected linear spans.

The original path remains for slots, recesses/pockets, bosses/pads, turned-step
sizes and turned ladders, round/turned/whole-stock Cartesian envelopes, independent
through-step sections, plate thickness owners, chamfers, fillets and other
explicitly deferred or non-step planes. There is no new dimension-scheme handling.
Every empty-`by` conveyance is a later-slice ownership claim; it is not evidence
that the legacy dimension was conveyed by another dimension. Add-mode writing
retains foreign PMI once and adds the planner's missing slice-one quantities.
Later-slice General rows keep their existing Add-mode omission.

The default plate's all-General planner has 11 quantities. Specify writes 13 when
position frames are accepted: the General centre-hole offsets 50 and 30 cannot
merge with Basic pattern-origin quantities. This is a tolerance-class constraint,
not extra basics or a second pattern location. The drill diameter 4.2 is now also
stated beside its M5 thread. `tests/planner_dimensions.rs` independently checks the
shared set, its complete STEP readback, reference modifiers, stored-PMI authority,
and an actual pre-planner numbered plate resumed twice.

The exact deferred-family predicate is every accepted defining family except
`holes`, `countersinks`, `step_levels`, `risers`, `through_steps` and `plates`, plus
the explicit `through_step section`, `plate thickness`, `turned profile`,
`turned envelope`, `polygonal_stock` and `non-step plane` ownership claims.
Derived pattern families have no defining claims of their own. This also retains
flats, grooves/annular and face grooves, blends, cylindrical profiles, freeform
surfaces, modelled threads, angled/paired/oblique/circular blind steps, oriented
chamfers, ribs, thin walls, interior voids, all slot/pocket/channel/notch/recess
variants, sheet-metal bodies, polygonal bosses/stock and raised pads. Schemes
continue on the existing path; Specify introduces no scheme interpretation here.
