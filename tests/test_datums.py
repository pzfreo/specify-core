"""Datums as face sets or holes, datum controls, and a frame per tolerance.

CTC-03 is answered as NIST's drawing specifies it (nist_ctc_03_asme1_rc.pdf):
A is two coplanar flange faces, B and C are holes, D is the web, and the
switch-mounting holes are located from D|B|C. Values are NIST's in inches;
sizes use H11 until ± tolerances exist.
"""

import json

import pytest
from build123d import Box, BuildPart, Locations, Mode, export_step

from specify_core.api import analyse, apply, write
from specify_core.cli import main
from specify_core.existing import read_existing
from specify_core.load import load
from specify_core.rules import (
    _bores,
    _datum_control,
    _default_datums,
    _questions,
    datum_kind,
    questions,
)

INCH = 25.4


@pytest.fixture(scope="module")
def ctc03_analysis(ctc03):
    return analyse(ctc03)


def _bore(analysis, hole_id):
    faces = {f["id"]: f for f in analysis["faces"]}
    hole = next(f for f in analysis["features"] if f["id"] == hole_id)
    return list(_bores(hole["faces"], hole["record"]["diameter"], faces))


@pytest.fixture(scope="module")
def nist_answers(ctc03_analysis):
    a = ctc03_analysis
    pair, switches = "hole_patterns.rectangle:58.59.60.61", "hole_patterns.linear:72.73.74.75"
    return {
        "part.material": "6061-T6",
        "datum.A": [18, 28],
        "datum.B": _bore(a, "holes:58"),
        "datum.C": _bore(a, "holes:60"),
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
        # Untoleranced on NIST's drawing (Ø1.065 has its own perpendicularity
        # to E, not modelled here). Metric clearance matching would otherwise
        # take them for M24 and M4 holes: an inch part is .
        "hole.function:holes:76": "general",
        "hole.function:hole_patterns.rectangle:77.78.79.80": "general",
    }


def test_nist_datum_scheme_round_trips(ctc03, ctc03_analysis, nist_answers, tmp_path):
    intent = apply(ctc03_analysis, nist_answers, accept_defaults=True)
    out = tmp_path / "ctc03-pmi.step"
    write(ctc03, intent, out)
    found = {
        (e.type, e.datums, round(e.value / INCH, 4))
        for e in read_existing(load(out))
        if e.kind == "geometric_tolerance"
    }
    assert found == {
        ("Perpendicularity", ("A",), 0.01),  # datum B
        ("Position", ("A", "B"), 0.02),  # datum C
        ("Position", ("A", "B", "C"), 0.05),  # the 2x pair left after B and C
        ("Position", ("D", "B", "C"), 0.05),  # switch mounting holes
        ("Position", ("D", "B", "C"), 0.08),  # Ø2.00
    }


def test_a_datum_hole_keeps_its_size_but_leaves_its_set(ctc03_analysis, nist_answers):
    ids = {q.id: q for q in questions(ctc03_analysis, nist_answers)}
    assert "hole.function:holes:58" in ids and "hole.position:holes:58" not in ids
    pair = ids["hole.function:hole_patterns.rectangle:58.59.60.61"]
    assert pair.prompt == "What are the 2× Ø11.125 holes for?"


def test_frames_are_offered_only_with_extra_datums(ctc03_analysis, nist_answers):
    without_d = {k: v for k, v in nist_answers.items() if k != "datum.D" and "frame" not in k}
    assert not any(q.id.startswith("hole.frame") for q in questions(ctc03_analysis, without_d))
    frame = next(
        q for q in questions(ctc03_analysis, nist_answers) if q.id.startswith("hole.frame")
    )
    assert frame.options == ("A|B|C", "D|B|C")


def test_a_frame_that_is_not_offered_is_refused(ctc03_analysis, nist_answers):
    bad = {**nist_answers, "hole.frame:holes:70": "C|B|A"}
    with pytest.raises(ValueError, match="is not one of"):
        apply(ctc03_analysis, bad, accept_defaults=True)


@pytest.mark.parametrize(
    "remove", [lambda a: a.pop("datum.D"), lambda a: a.update({"datum.D": []})]
)
def test_frame_answers_lapse_when_the_extra_datum_goes(ctc03_analysis, nist_answers, remove):
    """Removing D must not leave its D|B|C answers breaking apply."""
    answers = dict(nist_answers)
    remove(answers)
    intent = apply(ctc03_analysis, answers, accept_defaults=True)
    assert {tuple(r["datums"]) for r in intent.requirements if r["kind"] == "position"} == {
        ("A", "B"),
        ("A", "B", "C"),
    }


def test_without_a_there_are_no_positions_or_controls(ctc03_analysis, nist_answers):
    ids = {q.id for q in questions(ctc03_analysis, {**nist_answers, "datum.A": []})}
    assert not any(i.startswith(("hole.position", "hole.frame")) or "control" in i for i in ids)


def test_without_b_a_hole_c_has_no_control_but_keeps_a_position(ctc03_analysis, nist_answers):
    answers = {k: v for k, v in nist_answers.items() if "frame" not in k}
    answers.update({"datum.B": [], "hole.function:holes:60": "fit:H11"})
    qs = {q.id: q for q in questions(ctc03_analysis, answers)}
    assert "datum.C.control" not in qs
    assert qs["hole.position:holes:60"].frame == "A"  # its own letter dropped from A|C


def test_a_datum_hole_without_a_control_keeps_its_position(ctc03_analysis, nist_answers):
    """D gets no control in v1, so a hole chosen as D must still be located."""
    answers = {**nist_answers, "datum.D": _bore(ctc03_analysis, "holes:70")}
    answers = {k: v for k, v in answers.items() if "frame" not in k}
    qs = {q.id: q for q in questions(ctc03_analysis, answers)}
    assert qs["hole.position:holes:70"].frame == "A|B|C"
    assert qs["hole.frame:holes:70"].options == ("A|B|C", "B|C")


def test_a_bad_datum_answer_is_a_clear_error(ctc03_analysis):
    with pytest.raises(ValueError, match="datum A: face 99999 does not exist"):
        questions(ctc03_analysis, {"datum.A": [99999]})
    with pytest.raises(ValueError, match="datum A: .* cannot form one datum"):
        questions(ctc03_analysis, {"datum.A": [18, 7]})


def test_datum_faces_must_be_coplanar_or_one_hole(ctc03_analysis):
    faces = {f["id"]: f for f in ctc03_analysis["faces"]}
    assert datum_kind([18, 28], faces) == "plane"
    assert datum_kind(_bore(ctc03_analysis, "holes:58"), faces) == "axis"
    with pytest.raises(ValueError, match="cannot form one datum"):
        datum_kind([18, 7], faces)  # parallel planes, not coplanar
    with pytest.raises(ValueError, match="cannot form one datum"):
        datum_kind([18, *_bore(ctc03_analysis, "holes:58")], faces)


@pytest.fixture(scope="module")
def slotted(tmp_path_factory):
    """A plate whose largest face, the bottom, is cut in two by a slot across it;
    pockets keep the top smaller than either half."""
    with BuildPart() as bp:
        Box(100, 60, 10)
        with Locations((0, 0, -3.5)):
            Box(4, 60, 3, mode=Mode.SUBTRACT)
        with Locations((-25, 0, 4), (25, 0, 4)):
            Box(40, 45, 2, mode=Mode.SUBTRACT)
    path = tmp_path_factory.mktemp("parts") / "slotted.step"
    export_step(bp.part, str(path))
    return analyse(path)


def test_default_datum_a_takes_every_coplanar_face(slotted):
    # The slotted block has nothing to position, so A is proposed only on request.
    (a,) = [q for q in _questions(slotted, {}, propose="ABC") if q.id == "datum.A"]
    bottoms = [f["id"] for f in slotted["faces"] if f["kind"] == "plane" and f["centroid"][2] == -5]
    assert len(bottoms) == 2
    assert sorted(a.default) == sorted(bottoms)


def test_a_plane_c_is_held_square_to_a_and_b(plate):
    analysis = analyse(plate)
    qs = {q.id: q for q in questions(analysis, {})}
    assert qs["datum.C.control"].prompt == (
        "Perpendicularity required on datum C, relative to A|B?"
    )
    answers = {"part.material": "steel", "datum.C.control": "0.05"}
    intent = apply(analysis, answers, accept_defaults=True)
    (c,) = [r for r in intent.requirements if r["feature"] == "datum C"]
    assert (c["kind"], c["datums"], c["diametral"]) == ("perpendicularity", ["A", "B"], False)


def _faces(*specs):
    return {i: {"id": i, **spec} for i, spec in enumerate(specs)}


def test_a_bore_parallel_to_datum_a_gets_no_control():
    """A cross-bore runs along A: it would need parallelism, which v1 cannot write."""
    faces = _faces(
        {"kind": "plane", "direction": (0, 0, 1)},
        {"kind": "cylinder", "direction": (1, 0, 0)},
        {"kind": "cylinder", "direction": (0, 0, 1)},
    )
    plane = {"faces": [0], "kind": "plane"}
    assert _datum_control("B", {"A": plane, "B": {"faces": [1], "kind": "axis"}}, faces) is None
    assert _datum_control("B", {"A": plane, "B": {"faces": [2], "kind": "axis"}}, faces) == {
        "kind": "perpendicularity",
        "datums": ["A"],
        "diametral": True,
    }


def test_turned_datum_b_takes_every_face_of_a_split_bore():
    common = {"kind": "cylinder", "direction": (0, 0, 1), "origin": (0, 0, 0), "radius": 5.0}
    faces = _faces(
        {"kind": "plane", "direction": (0, 0, 1), "centroid": (0, 0, 0), "area": 100.0},
        {**common, "area": 20.0},
        {**common, "area": 20.0},
        {**common, "radius": 9.0, "area": 10.0},
    )
    _, b, _ = _default_datums(faces, [{"family": "turned_steps"}])
    assert b == (1, 2)


def test_a_datum_bore_on_a_turned_part_keeps_its_fit(spool):
    """The spool's default datum B is its bore; a bore that is a datum is often a
    bearing seat, so its fit is still asked for, only its position is not."""
    analysis = analyse(spool)
    ids = {q.id for q in questions(analysis, {})}
    assert "hole.function:holes:34.55" in ids
    assert "hole.position:holes:34.55" not in ids
    assert "datum.B.control" in ids


def test_the_cli_reports_a_bad_datum_answer(ctc03_analysis, tmp_path, capsys):
    analysis, answers = tmp_path / "analysis.json", tmp_path / "answers.json"
    analysis.write_text(json.dumps(ctc03_analysis))
    answers.write_text(json.dumps({"datum.A": [18, 7]}))
    assert main(["questions", str(analysis), "--answers", str(answers)]) == 2
    assert "invalid answer: datum A" in capsys.readouterr().err


def test_datums_are_proposed_only_when_something_is_measured_from_them(plate):
    analysis = analyse(plate)
    qs = questions(analysis, {})
    assert {"datum.A", "datum.B", "datum.C"} <= {q.id for q in qs}
    # Asked after the features they serve.
    ids = [q.id for q in qs]
    assert ids.index("datum.A") > max(i for i, x in enumerate(ids) if x.startswith("hole."))
    plain = {q.id: "general" for q in qs if q.id.startswith("hole.function")}
    assert not any(q.id.startswith("datum.") for q in questions(analysis, plain))


def test_a_datum_given_or_removed_is_always_asked(plate):
    analysis = analyse(plate)
    plain = {q.id: "general" for q in questions(analysis, {}) if q.id.startswith("hole.function")}
    top = max(
        (f for f in analysis["faces"] if f["kind"] == "plane"), key=lambda f: f["centroid"][2]
    )
    added = {q.id: q for q in questions(analysis, {**plain, "datum.A": [top["id"]]})}
    assert added["datum.A"].default == () and "datum.B" not in added
    removed = {q.id for q in questions(analysis, {"datum.C": []})}
    assert "datum.C" in removed
    positions = [
        q for q in questions(analysis, {"datum.C": []}) if q.id.startswith("hole.position")
    ]
    assert positions and all(q.frame == "A|B" for q in positions)
