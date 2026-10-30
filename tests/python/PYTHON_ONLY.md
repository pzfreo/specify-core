# specify-core's tests that do not run here

`tests/python` holds specify-core's tests (its `tests/` at cc341dd, the revision the oracle was
captured from), run on the Rust-backed package. This file accounts for the rest, so that
"the Rust-backed package passes specify-core's suite" has a precise meaning:

- Each test in `tests/python` passes, or is a strict xfail naming the ledger entry
  (`tests/fixtures/known_differences/`) or the documented decision that explains it
  (`conftest.py`, `KNOWN`).
- Each test of cc341dd that is not there is listed below, with why.

cc341dd has 219 test functions. `tests/python` runs 169 of them (238 cases with their
parameters), 25 of those ported onto the public API (below). The other 50 apply only to the
Python engine.

Where "Rust:" names a file, the Rust repo's test of the same name (ported from specify-core)
checks the same behaviour in the engine.

## Ported onto the public API

The test's intent is kept; what changed is how it reaches the part. Each change is marked
`Ported:` in the test file, except the two regular expressions in `test_conformance.py`.

| Test | Change |
|---|---|
| `test_datums.py`: 8 tests using `rules._bores` | `_bores`, a three-line filter over the analysis's faces, is carried in the test file |
| `test_same_as.py::test_datum_holes_are_never_merged` | the same |
| `test_mesh.py`: 3 tests, `test_writer.py`: 2 tests using `features.describe_faces` | the analysis's `faces`, which are `describe_faces`' output |
| `test_resume.py::test_without_answers_nothing_is_stored`, `test_merge.py::test_a_file_with_pmi_from_elsewhere_stores_no_answers` | `resume.saved(out) is None` read as `parts(out)`' `written` |
| `test_load.py::test_face_indices_are_stable_across_processes` | each face's area and centroid from the analysis, not OpenCascade's |
| `test_load.py::test_binding_identifies_the_file` | `binding.face_count` against the analysis's faces, not `faces.Extent()` |
| `test_load.py::test_an_assembly_holding_one_solid_is_numbered_on_the_solid` | the seven faces counted; OpenCascade's shape type and `index_of` dropped |
| `test_mesh.py::test_every_triangle_winds_outward` | the plate's exact volume, computed, for OpenCascade's; the spool case is dropped (its volume is not known without OpenCascade) |
| `test_conformance.py`: 2 tests | their regular expressions allow Part 21's optional spaces around `=` (OpenCascade writes `#1 = X(`, haecceity `#1=X(`) |
| `test_choices.py::test_a_choice_answer_that_is_not_text_is_refused` | `ValueError` for `choices.SpecError` (its base class): the rules report a refused spec as the answer's `ValueError`, without telling the two apart |
| `test_anchors.py::test_preview_carries_anchors_for_its_labels_when_given_the_file` | `anchors.key`, a one-line join of sorted face ids, is carried in the test file |
| `test_materials.py::test_a_long_material_name_is_written_and_verified` | the longest name from the material question's suggestions, which are `MATERIALS` |

Parts the tests build with build123d are the committed files `tools/make_parts.py` built from
the same code (`tests/fixtures/parts/`).

## Python only (50)

### OpenCascade objects and Python's loaders (4)

| Test | Why |
|---|---|
| `test_load.py::test_index_of_round_trips` | OpenCascade's face objects and their map; a `LoadedPart` here holds no geometry. Rust: `part.rs` |
| `test_features.py::test_xcaf_load_recognises_what_quiddity_load_does` | compares specify-core's two Python loaders (XCAF and Quiddity's). Rust: `features.rs` |
| `test_mesh.py::test_a_face_that_does_not_triangulate_is_reported` | monkeypatches `specify_core.mesh._triangulation`. Rust: `mesh.rs` |
| `test_merge.py::test_datums_on_a_set_of_faces_are_read_from_the_text` | `existing.datum_faces`, Python's reading of the file text around OpenCascade. Rust: `existing.rs` |

### Python's writer, its text route and its fault injection (14)

| Test | Why |
|---|---|
| `test_writer.py::test_a_failed_write_leaves_the_destination_and_the_source` | monkeypatches Python's `writer.verify`. Rust: `writer.rs` |
| `test_writer.py::test_a_failed_merge_leaves_a_part_with_pmi_as_it_was` | monkeypatches Python's `writer.verify` and calls `merge.carries_pmi`. Rust: `writer.rs` |
| `test_writer.py::test_limits_are_stated_correctly_in_the_file` | `writer._file_limits`, Python's parse of OpenCascade's output. Rust: `writer.rs` |
| `test_assembly.py::test_each_part_has_its_own_pmi` | `requirements.face_entities`/`entities_of`/`_anchors`, Python's STEP entity mapping. Rust: `assembly_write.rs` |
| `test_assembly.py::test_each_part_is_verified_against_its_own_pmi` | the same, and `writer._part_items`/`_part_tolerance_values`. Rust: `assembly_write.rs` |
| `test_requirements.py::test_the_file_still_reads_with_the_same_faces` | `requirements.face_entities`. Rust: `writer.rs` |
| `test_requirements.py::test_faces_resolve_inside_an_assembly` | `requirements.face_entities`. Rust: `writer.rs` |
| `test_merge.py::test_only_a_part_with_pmi_is_merged` | `merge.carries_pmi`, Python's OpenCascade transplant path. Rust: `existing.rs`, `writer.rs` |
| `test_requirements.py::test_sentences_are_in_the_form_draftwright_reads` | writer.json, "an assertion on Python's text route or drawing, not ported" |
| `test_requirements.py::test_sentences_claim_only_what_is_known` | the same |
| `test_requirements.py::test_the_default_finish_is_in_the_form_draftwright_draws` | the same |
| `test_requirements.py::test_verification_notices_a_missing_requirement` | the same (`writer._verify_appended`) |
| `test_mmc_finish_notes.py::test_a_face_finish_is_written_on_its_face` | the same (`existing.notes`) |
| `test_drawn.py::test_letters_are_strokes_scaled_to_their_height` | the same (`lettering.line`) |

### Private rules (5)

| Test | Why |
|---|---|
| `test_datums.py::test_datum_faces_must_be_coplanar_or_one_hole` | `rules.datum_kind`. Rust: `datums.rs` |
| `test_datums.py::test_default_datum_a_takes_every_coplanar_face` | `rules._questions` with a part built inline. Rust: `datums.rs` |
| `test_datums.py::test_a_bore_parallel_to_datum_a_gets_no_control` | `rules._datum_control`. Rust: `datums.rs` |
| `test_datums.py::test_turned_datum_b_takes_every_face_of_a_split_bore` | `rules._default_datums`, whose turned datums the engine replaces (rules.json, "axis-first turned datum defaults"). Rust: `datums.rs` |
| `test_profile_runout.py::test_turned_diameters_offer_runout_about_the_part_axis_but_not_the_axis_itself` | `rules._default_datums` (the same). Rust: `profile_runout.rs` |

### Python modules the package does not expose (20)

Their tables and functions are the engine's own (named), checked in Rust against
specify-core's (the oracle's sweeps, `docs/design.md`, "Status"). The package exposes
`choices` and `problems`, whose tests run here.

| Tests | Module | Rust |
|---|---|---|
| `test_standards.py`: all 6 | `standards` | `standards.rs` |
| `test_threads.py::test_thread_standards` | `standards` | `threads.rs` |
| `test_call_out.py::test_a_distance_leaves_the_general_list_by_place_not_by_the_faces_picked` | `general._between` | none by name |
| `test_general.py::test_iso_2768_linear`, `test_pin_lengths_are_baseline_from_datum_a` | `general.deviation` | `general.rs` |
| `test_locations.py::test_a_typed_location_tolerance`, `test_only_untoleranced_locations_are_marked_basic` | `locations.parse`, `mark_basic` | `locations.rs` |
| `test_materials.py::test_each_material_is_named_once`, `test_printed_materials_name_their_process_and_metals_carry_uns_numbers` | `materials.MATERIALS` | `materials.rs` |
| `test_mates.py::test_mates_are_by_face_and_disagreement_suggests_nothing` | `mates`, `load.load_all` | `mates.rs` |
| `test_anchors.py::test_a_shaft_is_labelled_from_its_side_and_a_hole_from_its_open_end`, `test_a_hole_along_x_is_labelled_from_its_rim` | `anchors` | `anchors.rs` |
| `test_magnitudes.py::test_magnitudes_are_read_from_the_text_in_millimetres` | `magnitudes`, Python's recovery of the tolerance values OpenCascade reads as 0 | none by name (haecceity reads them; existing.json's NIST sweep) |
| `test_p21.py::test_non_ascii_is_escaped_and_read_back` | `p21.escape`/`unescape` | none by name (haecceity's writer) |
| `test_resume.py::test_answers_for_other_faces_are_not_resumed` | forges specify-core's stored-answer payload (schema 1, `'pmi-assist answers'`) | `resume.rs` |

### A tool of the Python repository (7)

`test_nist_coverage.py`, all 7: they test `tools/nist/coverage.py`, specify-core's NIST
coverage report, not the package.
