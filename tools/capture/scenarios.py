"""Scenarios: answer sets transcribed from specify-core's tests, and their oracle results.

``make`` writes ``tests/fixtures/oracle/scenarios/<name>.json``, each ``{case, answers, why,
from}`` (plus ``accept_defaults`` when not true and ``write`` when the test writes the file).
Many tests pick faces and question ids from the analysis (the top face, the Ø8 bore, the
question whose prompt names the 6× holes); each function below repeats its test's choice on
the captured analysis of its case, so the committed answers are concrete. Run it again only
when the analyses change.

``run`` captures each scenario's ``questions``, ``open``, ``problems``, ``preview`` and ``apply``
(with ``accept_defaults``) into ``<name>.result.json.gz``, or the refusal in place of each; a
scenario with ``write`` also writes the input with the applied intent and reads it back
(``write``, ``readback``).
"""

from __future__ import annotations

import gzip
import json
import shutil
from collections.abc import Callable
from pathlib import Path
from typing import Any

from .common import attempt, failed, materialise, plain, scratch_dir, scrub, write_json_gz

INCH = 25.4
PLATE, PIN, STACK_BASE = "parts/plate", "parts/pin", "parts/stack#0"
SPOOL = "corpus/cadgenbench/flanged_spool_132"
CTC03 = "corpus/nist/nist_ctc_03_asme1_rc"


def _analysis(oracle: Path, case: str) -> dict[str, Any]:
    return json.loads(gzip.decompress((oracle / case / "analysis.json.gz").read_bytes()))


def _qs(analysis: dict[str, Any], answers: dict[str, Any] | None = None) -> list[Any]:
    from specify_core.rules import questions

    return questions(analysis, answers or {})


def _planes(a: dict[str, Any]) -> list[dict[str, Any]]:
    return [f for f in a["faces"] if f["kind"] == "plane"]


def _top(a: dict[str, Any]) -> int:
    return max(_planes(a), key=lambda f: f["centroid"][2])["id"]


def _bottom(a: dict[str, Any]) -> int:
    return min(_planes(a), key=lambda f: f["centroid"][2])["id"]


def _bore8(a: dict[str, Any]) -> int:
    return next(f for f in a["faces"] if f["kind"] == "cylinder" and abs(f["radius"] - 4) < 1e-6)[
        "id"
    ]


def _plain(a: dict[str, Any]) -> dict[str, str]:
    """Every hole left to the general tolerance (the tests' ``plain``)."""
    return {q.id: "general" for q in _qs(a) if q.id.startswith("hole.function")}


def _feature(a: dict[str, Any], family: str, diameter: float) -> str:
    return next(
        f["id"]
        for f in a["features"]
        if f["family"] == family and f["record"]["diameter"] == diameter
    )


def _function(a: dict[str, Any], text: str) -> str:
    return next(q.id for q in _qs(a) if q.id.startswith("hole.function") and text in q.prompt)


def _ctc03_bore(a: dict[str, Any], hole_id: str) -> list[int]:
    """test_datums.py::_bore."""
    from specify_core.rules import _bores

    faces = {f["id"]: f for f in a["faces"]}
    hole = next(f for f in a["features"] if f["id"] == hole_id)
    return list(_bores(hole["faces"], hole["record"]["diameter"], faces))


def _nist_answers(a: dict[str, Any]) -> dict[str, Any]:
    """test_datums.py fixture ``nist_answers``: CTC-03 as NIST's drawing specifies it."""
    pair, switches = "hole_patterns.rectangle:58.59.60.61", "hole_patterns.linear:72.73.74.75"
    return {
        "part.material": "6061-T6",
        "datum.A": [18, 28],
        "datum.B": _ctc03_bore(a, "holes:58"),
        "datum.C": _ctc03_bore(a, "holes:60"),
        "datum.D": [7],
        "datum.B.control": f"{0.01 * INCH:g}",
        "datum.C.control": f"{0.02 * INCH:g}",
        f"hole.function:{pair}": "fit:H11",
        f"hole.position:{pair}": f"{0.05 * INCH:g}",
        f"hole.function:{switches}": "fit:H11",
        f"hole.frame:{switches}": "D|B|C",
        f"hole.position:{switches}": f"{0.05 * INCH:g}",
        "hole.function:holes:70": "fit:H11",
        "hole.frame:holes:70": "D|B|C",
        "hole.position:holes:70": f"{0.08 * INCH:g}",
        "hole.function:holes:76": "general",
        "hole.function:hole_patterns.rectangle:77.78.79.80": "general",
    }


def _call_out_planes(a: dict[str, Any]) -> dict[str, list[int]]:
    """test_call_out.py::_planes, and its BORE (the first cylinder)."""
    planes = _planes(a)
    z = [f for f in planes if abs(abs(f["direction"][2]) - 1) < 1e-6]
    return {
        "TOP": [max(z, key=lambda f: f["centroid"][2])["id"]],
        "BOTTOM": [min(z, key=lambda f: f["centroid"][2])["id"]],
        "SIDE": [next(f for f in planes if abs(f["direction"][2]) < 1e-6)["id"]],
        "BORE": [next(f["id"] for f in a["faces"] if f["kind"] == "cylinder")],
    }


def _general(a: dict[str, Any]) -> list[dict[str, Any]]:
    """test_call_out.py::_general: the general dimensions with material and defaults."""
    from specify_core.api import apply

    return apply(a, {"part.material": "steel"}, accept_defaults=True).general


def _scenario(
    case: str, answers: dict[str, Any], why: str, source: str, **extra: Any
) -> dict[str, Any]:
    return {"case": case, "answers": answers, "why": why, "from": source, **extra}


# Each maker returns {name: scenario}. ``from`` names the Python test (file::function); a
# parametrised test gives one scenario per parameter.


def _rules(a: Callable[[str], dict[str, Any]]) -> dict[str, dict[str, Any]]:
    plate, spool = a(PLATE), a(SPOOL)
    eights = [q for q in _qs(spool) if q.id.startswith("hole.function") and "Ø8 holes" in q.prompt]
    return {
        "rules_incomplete_answers_are_refused": _scenario(
            PLATE,
            {},
            "No material: apply refuses, naming part.material.",
            "test_rules.py::test_incomplete_answers_are_refused",
        ),
        "rules_intent_from_defaults": _scenario(
            PLATE,
            {"part.material": "6082-T6"},
            "The plate's intent from defaults.",
            "test_rules.py::test_intent_from_defaults",
        ),
        "rules_off_table_holes_can_be_clearance": _scenario(
            SPOOL,
            {eights[0].id: "clearance"},
            "Ø8 holes as clearance for an unstated bolt.",
            "test_rules.py::test_off_table_holes_can_be_clearance_for_an_unstated_bolt",
        ),
        "rules_plane_c_is_held_square_to_a_and_b": _scenario(
            PLATE,
            {"part.material": "steel", "datum.C.control": "0.05"},
            "A plane datum C gets perpendicularity to A|B.",
            "test_datums.py::test_a_plane_c_is_held_square_to_a_and_b",
        ),
        "rules_datums_proposed_only_when_measured_from": _scenario(
            PLATE,
            _plain(plate),
            "Every hole general: no datums proposed.",
            "test_datums.py::test_datums_are_proposed_only_when_something_is_measured_from_them; "
            "test_locating_problems_extras.py::test_an_added_orientation_brings_datums_where_nothing_else_needs_them",
        ),
        "rules_datum_given_is_asked": _scenario(
            PLATE,
            {**_plain(plate), "datum.A": [_top(plate)]},
            "A datum given is asked (empty default) and reported unused.",
            "test_datums.py::test_a_datum_given_or_removed_is_always_asked; "
            "test_locating_problems_extras.py::test_an_unused_and_a_redundant_datum_are_reported",
        ),
        "rules_datum_removed_is_asked": _scenario(
            PLATE,
            {"datum.C": []},
            "C removed: still asked; positions from A|B.",
            "test_datums.py::test_a_datum_given_or_removed_is_always_asked",
        ),
        "rules_unreferenced_datum_is_warned": _scenario(
            PLATE,
            {"part.material": "steel", **_plain(plate), "datum.A": [_top(plate)]},
            "A datum nothing cites, written: one warning.",
            "test_writer.py::test_unreferenced_datums_are_warned",
            write="with answers",
        ),
        "rules_preview_shows_an_answer_as_decided": _scenario(
            PLATE,
            {
                f"hole.position:{_feature_prefix(plate, 'hole_patterns.grid')}": "0.2",
                "part.material": "6082-T6",
            },
            "An answered position is shown with default false.",
            "test_preview.py::test_an_answer_is_shown_as_decided",
        ),
        "rules_preview_refuses_a_bad_datum": _scenario(
            PLATE,
            {"datum.A": [99999]},
            "A face that does not exist is refused in preview too.",
            "test_preview.py::test_a_refused_answer_is_refused_in_preview_too",
        ),
    }


def _feature_prefix(a: dict[str, Any], prefix: str) -> str:
    return next(f["id"] for f in a["features"] if f["id"].startswith(prefix))


def _datums(a: Callable[[str], dict[str, Any]]) -> dict[str, dict[str, Any]]:
    ctc = a(CTC03)
    nist = _nist_answers(ctc)
    without_d = {k: v for k, v in nist.items() if k != "datum.D" and "frame" not in k}
    no_b = {k: v for k, v in nist.items() if "frame" not in k}
    no_b.update({"datum.B": [], "hole.function:holes:60": "fit:H11"})
    hole_d = {
        k: v
        for k, v in {**nist, "datum.D": _ctc03_bore(ctc, "holes:70")}.items()
        if "frame" not in k
    }
    src = "test_datums.py::"
    return {
        "datums_nist_scheme_round_trips": _scenario(
            CTC03,
            nist,
            "CTC-03 datums A-D, hole frames D|B|C, written and read back.",
            src
            + "test_nist_datum_scheme_round_trips; "
            + src
            + "test_a_datum_hole_keeps_its_size_but_leaves_its_set",
            write="without answers",
        ),
        "datums_frames_only_with_extra_datums": _scenario(
            CTC03,
            without_d,
            "Without D no frame questions.",
            src + "test_frames_are_offered_only_with_extra_datums",
        ),
        "datums_frame_not_offered_is_refused": _scenario(
            CTC03,
            {**nist, "hole.frame:holes:70": "C|B|A"},
            "A frame not offered is refused.",
            src + "test_a_frame_that_is_not_offered_is_refused",
        ),
        "datums_frame_answers_lapse_when_d_is_removed": _scenario(
            CTC03,
            {k: v for k, v in nist.items() if k != "datum.D"},
            "D removed: its frames lapse.",
            src + "test_frame_answers_lapse_when_the_extra_datum_goes[pop]",
        ),
        "datums_frame_answers_lapse_when_d_is_emptied": _scenario(
            CTC03,
            {**nist, "datum.D": []},
            "D emptied: its frames lapse.",
            src + "test_frame_answers_lapse_when_the_extra_datum_goes[empty]",
        ),
        "datums_without_a_no_positions_or_controls": _scenario(
            CTC03,
            {**nist, "datum.A": []},
            "No A: no positions, frames or controls.",
            src + "test_without_a_there_are_no_positions_or_controls",
        ),
        "datums_without_b_hole_c_keeps_a_position": _scenario(
            CTC03,
            no_b,
            "No B: C has no control; its position drops its own letter.",
            src + "test_without_b_a_hole_c_has_no_control_but_keeps_a_position",
        ),
        "datums_a_datum_hole_without_a_control_keeps_its_position": _scenario(
            CTC03,
            hole_d,
            "A hole as D keeps a position, frames A|B|C or B|C.",
            src + "test_a_datum_hole_without_a_control_keeps_its_position",
        ),
        "datums_bad_face_is_refused": _scenario(
            CTC03,
            {"datum.A": [99999]},
            "A face that does not exist.",
            src + "test_a_bad_datum_answer_is_a_clear_error",
        ),
        "datums_faces_that_cannot_form_one_datum_are_refused": _scenario(
            CTC03,
            {"datum.A": [18, 7]},
            "Parallel planes are not one datum.",
            src
            + "test_a_bad_datum_answer_is_a_clear_error; "
            + src
            + "test_the_cli_reports_a_bad_datum_answer",
        ),
        "datums_holes_are_never_merged": _scenario(
            CTC03,
            {
                "datum.A": [18, 28],
                "datum.B": _ctc03_bore(ctc, "holes:58"),
                "datum.C": _ctc03_bore(ctc, "holes:60"),
            },
            "Identical datum holes keep their own questions.",
            "test_same_as.py::test_datum_holes_are_never_merged",
        ),
    }


def _extras(a: Callable[[str], dict[str, Any]]) -> dict[str, dict[str, Any]]:
    plate, pin = a(PLATE), a(PIN)
    top, bottom, bore = _top(plate), _bottom(plate), _bore8(plate)
    src = "test_locating_problems_extras.py::"
    out = {
        "locating_plus_minus_needs_no_positions_or_datums": _scenario(
            PLATE,
            {"part.locating": "± dimensions", "part.material": "steel"},
            "Holes located by ± dimensions: no positions, no datums, no locations.",
            src + "test_holes_located_by_plus_minus_need_no_positions_or_datums; "
            "test_locations.py::test_general_writes_no_location",
        ),
        "locating_plus_minus_tolerance_on_the_grid": _scenario(
            PLATE,
            {
                "part.locating": "± dimensions",
                "part.material": "steel",
                next(
                    q.id
                    for q in _qs(plate, {"part.locating": "± dimensions"})
                    if q.id.startswith("hole.location") and "grid" in q.id
                ): "±0.05",
            },
            "A ± location tolerance on the grid: 12 locations.",
            "test_locations.py::test_holes_located_by_plus_minus_can_be_given_a_tolerance",
            write="with answers",
        ),
        "locating_only_a_tapped_hole_positioned": _scenario(
            PLATE,
            {
                "part.material": "steel",
                **{
                    q.id: "general"
                    for q in _qs(plate)
                    if q.id.startswith("hole.function") and not str(q.default).startswith("tapped")
                },
            },
            "Only the tapped hole positioned: its basic dimensions are the only dimensions.",
            "test_locations.py::test_a_part_whose_only_dimensions_are_locations_keeps_its_tolerance_values",
            write="with answers",
        ),
        "problems_holes_with_nothing_to_locate_them": _scenario(
            PLATE,
            {"datum.A": []},
            "No A: holes are reported unlocated.",
            src + "test_holes_with_nothing_to_locate_them_are_reported",
        ),
        "problems_a_redundant_datum": _scenario(
            PLATE,
            {"datum.B": [bottom]},
            "B parallel to A is reported.",
            src + "test_an_unused_and_a_redundant_datum_are_reported",
        ),
        "extras_are_written_and_read_back": _scenario(
            PLATE,
            {
                "part.material": "steel",
                "extra.1": {"faces": [top], "kind": "flatness", "value": "0.05"},
                "extra.2": {
                    "faces": [bore],
                    "kind": "perpendicularity",
                    "value": "0.02",
                    "datums": ["A"],
                },
                "extra.3": {"faces": [top], "kind": "parallelism", "value": "0.1", "datums": ["A"]},
                "extra.4": {"faces": [bore], "kind": "fit", "value": "H6"},
            },
            "Flatness, perpendicularity, parallelism and a fit added.",
            src + "test_added_requirements_are_written_and_read_back",
            write="with answers",
        ),
        "extras_an_orientation_brings_datums": _scenario(
            PLATE,
            {
                **_plain(plate),
                "extra.1": {
                    "faces": [bore],
                    "kind": "perpendicularity",
                    "value": "0.02",
                    "datums": ["A"],
                },
            },
            "An added perpendicularity brings datum A.",
            src + "test_an_added_orientation_brings_datums_where_nothing_else_needs_them",
        ),
        "extras_citing_a_missing_datum": _scenario(
            PLATE,
            {
                "extra.1": {
                    "faces": [bore],
                    "kind": "parallelism",
                    "value": "0.1",
                    "datums": ["E"],
                },
                "part.material": "x",
            },
            "Citing datum E, which is not given: reported, not written; E asked.",
            src + "test_an_added_requirement_citing_a_missing_datum_is_reported_not_written",
        ),
        "extras_only_cited_datums_are_proposed_b": _scenario(
            PLATE,
            {
                "part.locating": "± dimensions",
                "extra.1": {
                    "faces": [bore],
                    "kind": "perpendicularity",
                    "value": "0.02",
                    "datums": ["B"],
                },
            },
            "Only B is proposed.",
            src + "test_only_the_datums_added_orientations_cite_are_proposed",
        ),
        "extras_only_cited_datums_are_proposed_a_b": _scenario(
            PLATE,
            {
                "part.locating": "± dimensions",
                "extra.1": {
                    "faces": [bore],
                    "kind": "perpendicularity",
                    "value": "0.02",
                    "datums": ["A", "B"],
                },
            },
            "A and B proposed, not C.",
            src + "test_only_the_datums_added_orientations_cite_are_proposed",
        ),
        "extras_an_orientation_on_its_own_datum": _scenario(
            PLATE,
            {
                "datum.A": [top],
                "extra.1": {
                    "faces": [top],
                    "kind": "parallelism",
                    "value": "0.05",
                    "datums": ["A"],
                },
            },
            "Measured from itself: reported.",
            src + "test_an_orientation_on_its_own_datum_is_reported",
        ),
        "extras_profile_and_total_runout": _scenario(
            PLATE,
            {
                "part.material": "steel",
                "extra.1": {"faces": [top], "kind": "profile", "value": "0.1"},
                "extra.2": {"faces": [top], "kind": "profile", "value": "0.2", "datums": ["A"]},
                "extra.3": {
                    "faces": [bore],
                    "kind": "total_runout",
                    "value": "0.03",
                    "datums": ["A"],
                },
            },
            "Profile with and without datums, total runout.",
            "test_profile_runout.py::test_profile_and_total_runout_can_be_added_on_any_faces",
            write="with answers",
        ),
        "extras_a_runout_needs_a_datum": _scenario(
            PLATE,
            {"extra.1": {"faces": [plate["faces"][0]["id"]], "kind": "runout", "value": "0.05"}},
            "A runout without a datum is refused.",
            "test_profile_runout.py::test_a_runout_needs_a_datum_but_a_profile_does_not",
        ),
        "extras_a_profile_needs_no_datum": _scenario(
            PLATE,
            {"extra.1": {"faces": [plate["faces"][0]["id"]], "kind": "profile", "value": "0.05"}},
            "A profile without a datum is accepted.",
            "test_profile_runout.py::test_a_runout_needs_a_datum_but_a_profile_does_not",
        ),
        "extras_every_datum_referenced_tolerance": _scenario(
            PLATE,
            {
                "part.material": "steel",
                **{
                    f"extra.{n}": {"faces": [face], "kind": kind, "value": "0.05", "datums": ["A"]}
                    for n, (face, kind) in enumerate(
                        [
                            (top, "parallelism"),
                            (
                                next(
                                    f["id"] for f in _planes(plate) if abs(f["direction"][2]) < 0.1
                                ),
                                "perpendicularity",
                            ),
                            (
                                next(f["id"] for f in plate["faces"] if f["kind"] == "cylinder"),
                                "runout",
                            ),
                            (
                                next(f["id"] for f in plate["faces"] if f["kind"] == "cylinder"),
                                "total_runout",
                            ),
                        ]
                    )
                },
            },
            "Parallelism, perpendicularity, runout and total runout to A.",
            "test_conformance.py::test_every_datum_referenced_tolerance_is_written_simply",
            write="with answers",
        ),
        "extras_finish_on_a_face_of_the_pin": _scenario(
            PIN,
            {
                "part.material": "brass",
                "extra.0": {"faces": [_top(pin)], "kind": "finish", "value": "Ra 0.8"},
            },
            "A finish on the pin's top face.",
            "test_requirements.py::test_a_finish_on_faces_is_not_a_second_default",
            write="with answers",
        ),
    }
    refused = [
        ({"faces": [], "kind": "flatness", "value": "0.1"}, "pick the faces"),
        ({"faces": [99999], "kind": "flatness", "value": "0.1"}, "does not exist"),
        ({"faces": "BORE", "kind": "flatness", "value": "0.1"}, "flat faces"),
        ({"faces": "TOP", "kind": "fit", "value": "H7"}, "cylinder"),
        ({"faces": "BORE", "kind": "fit", "value": "Q7"}, "not a fit"),
        ({"faces": "TOP", "kind": "perpendicularity", "value": "0.1"}, "which datum"),
        ({"faces": "TOP", "kind": "flatness", "value": "none"}, "give a tolerance"),
        ({"faces": "TOP", "kind": "roundness", "value": "0.1"}, "not one of"),
    ]
    named = {"TOP": [top], "BORE": [bore]}
    for n, (extra, reason) in enumerate(refused):
        if isinstance(extra["faces"], str):
            extra = {**extra, "faces": named[extra["faces"]]}
        out[f"extras_refused_{n}"] = _scenario(
            PLATE,
            {"extra.1": extra},
            f"Refused: {reason}.",
            src + f"test_an_added_requirement_that_does_not_suit_its_faces_is_refused[{n}]",
        )
    for value in ["rough", "0", "Ra 99"]:
        out[f"extras_finish_not_a_roughness_{value.replace(' ', '_')}"] = _scenario(
            PLATE,
            {"extra.1": {"faces": [plate["faces"][0]["id"]], "kind": "finish", "value": value}},
            "A finish that is not a roughness is refused.",
            f"test_mmc_finish_notes.py::test_a_finish_that_is_not_a_roughness_is_refused[{value}]",
        )
    return out


def _turned(a: Callable[[str], dict[str, Any]]) -> dict[str, dict[str, Any]]:
    pin, plate = a(PIN), a(PLATE)
    runouts = [q for q in _qs(pin) if q.id.startswith("diameter.runout")]
    fit = next(q for q in _qs(pin) if q.id.startswith("diameter.fit"))
    hole = f"hole.function:{_feature(pin, 'holes', 4.2)}"
    six, ten = _feature(pin, "turned_steps", 6.0), _feature(pin, "turned_steps", 10.0)
    bore = _function(plate, "Ø8")
    return {
        "turned_a_runout_is_written_about_the_frame": _scenario(
            PIN,
            {"part.material": "brass", runouts[0].id: "0.02"},
            "A runout on the first turned diameter.",
            "test_profile_runout.py::test_a_runout_is_written_about_the_frame_and_read_back; "
            "test_conformance.py::test_no_malformed_runout_zone_is_written",
            write="with answers",
        ),
        "turned_runout_and_a_g6_fit": _scenario(
            PIN,
            {"part.material": "brass", runouts[0].id: "0.02", fit.id: "g6"},
            "A runout and a g6 shaft.",
            "test_conformance.py::test_limits_are_typed_lengths_and_runout_is_simple",
            write="with answers",
        ),
        "turned_a_tapped_hole_carries_its_thread": _scenario(
            PIN,
            {"part.material": "brass"},
            "The blind M5 tap drill's thread and drill.",
            "test_threads.py::test_a_tapped_hole_carries_its_thread_and_drill",
        ),
        "turned_a_typed_thread_depth_too_deep_is_refused": _scenario(
            PIN,
            {hole: "tapped:M5@6"},
            "6 mm of full thread in an 8 mm drill: at most 5.6.",
            "test_threads.py::test_a_typed_thread_depth_is_written_and_read_back",
        ),
        "turned_a_typed_thread_depth_is_written": _scenario(
            PIN,
            {"part.material": "brass", hole: "tapped:M5@5.6"},
            "5.6 mm of full thread, written and resumed.",
            "test_threads.py::test_a_typed_thread_depth_is_written_and_read_back",
            write="with answers",
        ),
        "turned_external_thread_and_diamond_knurl": _scenario(
            PIN,
            {
                "part.material": "brass",
                f"diameter.fit:{six}": "thread:M6",
                f"diameter.fit:{ten}": "knurl:diamond",
            },
            "An M6 thread on Ø6 and a diamond knurl on Ø10.",
            "test_threads.py::test_external_thread_and_knurl_from_turned_answers",
        ),
        "turned_external_thread_and_straight_knurl": _scenario(
            PIN,
            {
                "part.material": "CW614N leaded brass",
                f"diameter.fit:{six}": "thread:M6",
                f"diameter.fit:{ten}": "knurl:straight",
            },
            "An M6 thread and a straight knurl, written.",
            "test_requirements.py::pin_written; "
            "test_conformance.py::test_callout_presentation_is_named",
            write="without answers",
        ),
        "turned_a_straight_knurl_resumes": _scenario(
            PIN,
            {"part.material": "brass", f"diameter.fit:{ten}": "knurl:straight"},
            "A straight knurl, written with its answers.",
            "test_resume.py::"
            "test_a_written_file_resumes_its_answers_and_is_rewritten_not_added_to; "
            "test_drawn.py::pin_drawn",
            write="with answers",
        ),
        "choices_a_typed_fit_reaches_the_intent": _scenario(
            PLATE,
            {"part.material": "steel", bore: "fit:H6"},
            "The Ø8 bore as H6.",
            "test_choices.py::test_a_typed_answer_reaches_the_intent_and_a_wrong_one_is_refused",
        ),
        "choices_a_tap_on_the_bore_is_refused": _scenario(
            PLATE,
            {bore: "tapped:M5"},
            "M5 tapped in an Ø8 hole: refused.",
            "test_choices.py::test_a_typed_answer_reaches_the_intent_and_a_wrong_one_is_refused",
        ),
    }


def _plate_answers(a: Callable[[str], dict[str, Any]]) -> dict[str, dict[str, Any]]:
    plate = a(PLATE)
    top = _top(plate)
    qid = next(q.id for q in _qs(plate) if q.id.startswith("hole.position:"))
    out = {
        "notes_mmc_finish_and_part_notes": _scenario(
            PLATE,
            {
                "part.material": "Aluminium 6082-T6",
                _function(plate, "Ø8"): "fit:H7",
                "part.coating": "Anodise black (MIL-A-8625 Type II)",
                "part.heat_treatment": "Solution treat and age to T6",
                "part.surface_finish": "Ra 1.6",
                "extra.1": {"faces": [top], "kind": "finish", "value": "0.8"},
            },
            "Clearance positions at MMC, every part note, a face finish.",
            "test_mmc_finish_notes.py::written",
            write="without answers",
        ),
        "merge_enriched_plate": _scenario(
            PLATE,
            {
                "part.material": "steel",
                _function(plate, "Ø8"): "fit:H7",
                _function(plate, "6×"): "general",
            },
            "The plate with a fitted, positioned Ø8 bore (the file test_merge.py merges into).",
            "test_merge.py::enriched",
            write="without answers",
        ),
    }
    for n, value in enumerate([0.1, 3, True, ["general"], {"value": "general"}, None]):
        out[f"choices_an_answer_that_is_not_text_is_refused_{n}"] = _scenario(
            PLATE,
            {qid: value},
            "A choice answer that is not text.",
            f"test_choices.py::test_a_choice_answer_that_is_not_text_is_refused[{n}]",
        )
    return out


def _call_out(a: Callable[[str], dict[str, Any]]) -> dict[str, dict[str, Any]]:
    plate = a(PLATE)
    material = {"part.material": "steel"}
    general = _general(plate)
    size = next(g for g in general if g["kind"] == "size")
    length = next(g for g in general if g["kind"] == "location")
    named = _call_out_planes(plate)
    faces = {f["id"]: f for f in plate["faces"]}
    cyl = next(f for f in faces.values() if f["kind"] == "cylinder")
    side = next(
        f
        for f in faces.values()
        if f["kind"] == "plane"
        and abs(sum(x * y for x, y in zip(f["direction"], cyl["direction"], strict=False))) < 1e-6
    )
    src = "test_call_out.py::"
    out = {
        "callout_a_size_is_written": _scenario(
            PLATE,
            {**material, "extra.1": {"faces": size["faces"], "kind": "size", "value": "+0.1/-0"}},
            "A called-out size leaves the general list.",
            src + "test_a_called_out_size_is_written_and_leaves_the_general_list",
            write="with answers",
        ),
        "callout_a_size_plus_minus": _scenario(
            PLATE,
            {**material, "extra.1": {"faces": size["faces"], "kind": "size", "value": "±0.05"}},
            "A size ±0.05.",
            src + "test_a_called_out_size_may_be_plus_minus_or_a_fit",
        ),
        "callout_a_size_as_a_fit": _scenario(
            PLATE,
            {**material, "extra.1": {"faces": size["faces"], "kind": "size", "value": "H7"}},
            "A size as H7.",
            src + "test_a_called_out_size_may_be_plus_minus_or_a_fit",
        ),
        "callout_a_length_is_written": _scenario(
            PLATE,
            {
                **material,
                "extra.1": {
                    "faces": length["faces"],
                    "reference": length["reference"],
                    "kind": "distance",
                    "value": "±0.05",
                },
            },
            "A called-out length leaves the general list.",
            src + "test_a_called_out_length_is_written_and_leaves_the_general_list",
            write="with answers",
        ),
        "callout_a_distance_to_a_holes_axis": _scenario(
            PLATE,
            {
                **material,
                "extra.1": {
                    "faces": [cyl["id"]],
                    "reference": [side["id"]],
                    "kind": "distance",
                    "value": "±0.1",
                },
            },
            "A distance from a side to a hole's axis.",
            src + "test_a_distance_to_a_holes_axis_is_measured_from_a_plane_along_it",
        ),
        "callout_a_diameter_sized_twice": _scenario(
            PLATE,
            {
                "extra.1": {"faces": size["faces"], "kind": "fit", "value": "H7"},
                "extra.2": {"faces": size["faces"], "kind": "size", "value": "±0.05"},
            },
            "Sized twice: reported.",
            src + "test_a_diameter_sized_twice_is_reported",
        ),
        "callout_a_datum_a_call_out_is_measured_from_is_used": _scenario(
            PLATE,
            {
                **_plain(plate),
                "datum.A": named["BOTTOM"],
                "extra.1": {
                    "faces": named["TOP"],
                    "reference": named["BOTTOM"],
                    "kind": "distance",
                    "value": "±0.1",
                },
            },
            "A used by a call-out is not unused.",
            src + "test_a_datum_a_call_out_is_measured_from_is_used",
        ),
    }
    refused = [
        ({"faces": "TOP", "kind": "size", "value": "±0.1"}, "one diameter"),
        ({"faces": "BORE", "kind": "size", "value": "0.1 or so"}, "give ±0.05"),
        ({"faces": "BORE", "kind": "size", "value": "+0/-0"}, "must differ"),
        ({"faces": "TOP", "kind": "distance", "value": "±0.1"}, "from flat faces"),
        ({"faces": "TOP", "reference": "TOP", "kind": "distance", "value": "±0.1"}, "itself"),
        ({"faces": "SIDE", "reference": "BOTTOM", "kind": "distance", "value": "±0.1"}, "parallel"),
        ({"faces": "TOP", "reference": "BOTTOM", "kind": "distance", "value": "H7"}, "give ±0.05"),
        ({"faces": "BORE", "reference": "TOP", "kind": "distance", "value": "±0.1"}, "along"),
        ({"faces": "BORE", "kind": "size", "value": "6.58/6.62"}, "deviations from 6.6"),
        (
            {"faces": "TOP", "reference": "BOTTOM", "kind": "distance", "value": "9.9-10.1"},
            "deviations",
        ),
        (
            {"faces": "TOP", "reference": "BOTTOM", "kind": "distance", "value": "0.1-0.05"},
            "deviations",
        ),
        ({"faces": "TOP", "reference": "BOTTOM", "kind": "distance", "value": "+3/-3"}, "too big"),
        (
            {"faces": "TOP", "reference": "12", "kind": "distance", "value": "±0.1"},
            "from flat faces",
        ),
    ]
    for n, (extra, reason) in enumerate(refused):
        extra = {k: named.get(v, v) if k in ("faces", "reference") else v for k, v in extra.items()}
        out[f"callout_refused_{n}"] = _scenario(
            PLATE,
            {"extra.1": extra},
            f"Refused: {reason}.",
            src + f"test_a_call_out_that_does_not_suit_its_faces_is_refused[{n}]",
        )
    for n, value in enumerate(["-0.05/-0.1", "+0.1/+0.05", "+0.2/-0.1", "+0.1/0"]):
        out[f"callout_a_distance_with_limits_{n}"] = _scenario(
            PLATE,
            {
                **material,
                "extra.1": {
                    "faces": named["TOP"],
                    "reference": named["BOTTOM"],
                    "kind": "distance",
                    "value": value,
                },
            },
            f"A distance {value}, written.",
            src + f"test_a_distance_with_any_limits_is_written_and_verified[{value}]",
            write="with answers",
        )
    for n, value in enumerate(["Ra 0.8", ["finish"], None, 3]):
        out[f"callout_an_added_requirement_that_is_not_an_object_{n}"] = _scenario(
            PLATE,
            {**material, "extra.finish1": value},
            "An added requirement that is not an object.",
            src + f"test_an_added_requirement_that_is_not_an_object_is_refused[{n}]",
        )
    return out


def _assemblies(a: Callable[[str], dict[str, Any]]) -> dict[str, dict[str, Any]]:
    spool, base = a(SPOOL), a(STACK_BASE)
    eights = next(q for q in _qs(spool) if "Ø8 holes" in q.prompt)
    turned = next(q for q in _qs(spool) if "Ø130" in q.prompt)
    ids = [q.id for q in _qs(spool)]
    pattern = next(i for i in ids if i.startswith("hole.function:hole_patterns"))
    hole = next(i for i in ids if i.startswith("hole.function:holes"))
    diameters = [i for i in ids if i.startswith("diameter.fit")]
    bore = next(q.id for q in _qs(base) if q.diameter == 8.0)
    return {
        "same_as_each_group_keeps_its_own_size_and_position": _scenario(
            SPOOL,
            {"part.material": "steel", eights.id: "clearance"},
            "Two identical bolt circles, one answer.",
            "test_same_as.py::test_each_group_keeps_its_own_size_and_position",
        ),
        "same_as_identical_turned_diameters": _scenario(
            SPOOL,
            {"part.material": "steel", turned.id: "h7"},
            "Two identical turned diameters, one answer.",
            "test_same_as.py::test_identical_turned_diameters_are_one_question",
        ),
        "writer_every_kind_v1_writes": _scenario(
            SPOOL,
            {
                "part.material": "6082-T6",
                pattern: "fit:H7",
                hole: "fit:H7",
                diameters[0]: "p6",
                diameters[1]: "g6",
                "datum.A.flatness": "0.02",
            },
            "Every kind of requirement v1 writes on the spool.",
            "test_writer.py::spool_written",
            write="without answers",
        ),
        "mates_an_answer_given_is_kept_over_a_suggestion": _scenario(
            STACK_BASE,
            {"part.material": "steel", bore: "general"},
            "The base's Ø8 bore answered general over H7.",
            "test_mates.py::test_an_answer_given_is_kept_over_a_suggestion",
        ),
    }


MAKERS = [_rules, _datums, _extras, _turned, _plate_answers, _call_out, _assemblies]


def make(oracle: Path) -> list[str]:
    """Write every scenario's JSON from the captured analyses; returns the names."""
    cache: dict[str, dict[str, Any]] = {}

    def analysis(case: str) -> dict[str, Any]:
        if case not in cache:
            cache[case] = _analysis(oracle, case)
        return cache[case]

    folder = oracle / "scenarios"
    folder.mkdir(parents=True, exist_ok=True)
    names: list[str] = []
    for maker in MAKERS:
        for name, scenario in maker(analysis).items():
            if name in names:
                raise ValueError(f"scenario {name} made twice")
            names.append(name)
            text = json.dumps(plain(scenario), ensure_ascii=False, indent=1)
            (folder / f"{name}.json").write_text(text + "\n")
    return names


def run(oracle: Path, only: set[str] | None = None) -> list[str]:
    """Capture every scenario's result (or those named in ``only``); returns the scenarios
    not run (their case is not captured), each with why."""
    folder = oracle / "scenarios"
    skipped = []
    for path in sorted(folder.glob("*.json")):
        name = path.stem
        if only is not None and name not in only:
            continue
        spec = json.loads(path.read_text())
        if not (oracle / spec["case"] / "analysis.json.gz").exists():
            skipped.append(f"{name}: case {spec['case']} is not captured")
            continue
        result, subs = _result(oracle, spec)
        write_json_gz(folder / f"{name}.result.json.gz", scrub(result, subs))
    return skipped


def _result(oracle: Path, spec: dict[str, Any]) -> tuple[dict[str, Any], dict[str, str]]:
    """One scenario's result, and the paths to scrub from it."""
    from specify_core import api, rules
    from specify_core.problems import problems

    from .cases import readback, root_of

    case_dir = oracle / spec["case"]
    analysis = _analysis(oracle, spec["case"])
    answers = spec["answers"]

    def questions() -> dict[str, Any]:
        qs = rules.questions(analysis, answers)
        return plain(
            {
                "questions": [q.to_dict() for q in qs],
                "open": [q.id for q in rules.open_questions(qs, answers)],
            }
        )

    asked = attempt(questions)
    result: dict[str, Any] = {
        "questions": asked if failed(asked) else asked["questions"],
        "open": asked if failed(asked) else asked["open"],
        "problems": attempt(lambda: plain(problems(analysis, answers))),
        "preview": attempt(lambda: plain(api.preview(analysis, answers).to_dict())),
    }
    defaults = spec.get("accept_defaults", True)
    intent = attempt(lambda: api.apply(analysis, answers, accept_defaults=defaults))
    result["apply"] = intent if failed(intent) else plain(intent.to_dict())
    if not spec.get("write") or failed(intent):
        return result, {}
    meta = json.loads(gzip.decompress((case_dir / "meta.json.gz").read_bytes()))
    scratch = scratch_dir()
    try:
        source = root_of(meta["kind"]) / meta["input"]
        step = materialise(source, scratch)
        out = scratch / "written.step"
        stored = answers if spec["write"] == "with answers" else None
        report = attempt(lambda: plain(api.write(step, intent, out, answers=stored).to_dict()))
        result["write"] = report
        if not failed(report):
            result["readback"] = readback(out)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    return result, {str(step): "{input}", str(source): "{input}", str(scratch): "{output}"}
