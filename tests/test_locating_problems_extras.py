"""Locating holes by ± dimensions, the problems list, and requirements a
person adds where nothing asked."""

import pytest

from specify_core.api import analyse, apply, write
from specify_core.existing import read_existing
from specify_core.load import load
from specify_core.problems import problems
from specify_core.rules import LOCATING, questions


@pytest.fixture(scope="module")
def plate_analysis(plate):
    return analyse(plate)


def _top(analysis):
    return max(
        (f for f in analysis["faces"] if f["kind"] == "plane"), key=lambda f: f["centroid"][2]
    )


def _bottom(analysis):
    return min(
        (f for f in analysis["faces"] if f["kind"] == "plane"), key=lambda f: f["centroid"][2]
    )


def _bore8(analysis):
    return next(
        f for f in analysis["faces"] if f["kind"] == "cylinder" and abs(f["radius"] - 4) < 1e-6
    )


def test_holes_located_by_plus_minus_need_no_positions_or_datums(plate_analysis):
    ids = {q.id for q in questions(plate_analysis, {})}
    assert "part.locating" in ids and any(i.startswith("hole.position") for i in ids)
    plus_minus = {"part.locating": LOCATING[1]}
    ids = {q.id for q in questions(plate_analysis, plus_minus)}
    assert not any(i.startswith(("hole.position", "hole.frame", "datum.")) for i in ids)
    intent = apply(plate_analysis, {**plus_minus, "part.material": "steel"}, accept_defaults=True)
    assert intent.datums == [] and not any(r["kind"] == "position" for r in intent.requirements)
    assert any(r["kind"] == "size" for r in intent.requirements)  # fits still apply


def test_a_standard_part_has_no_problems(plate_analysis):
    assert problems(plate_analysis, {}) == []


def test_holes_with_nothing_to_locate_them_are_reported(plate_analysis):
    found = problems(plate_analysis, {"datum.A": []})
    unlocated = [p for p in found if "nothing locates" in p["message"]]
    assert unlocated and all(p["severity"] == "warning" and p["faces"] for p in unlocated)
    assert any("6× Ø6.6 holes are for M6 clearance" in p["message"] for p in unlocated)


def test_an_unused_and_a_redundant_datum_are_reported(plate_analysis):
    plain = {
        q.id: "general" for q in questions(plate_analysis, {}) if q.id.startswith("hole.function")
    }
    found = problems(plate_analysis, {**plain, "datum.A": [_top(plate_analysis)["id"]]})
    assert [p["about"] for p in found] == ["datum.A"] and found[0]["severity"] == "info"
    redundant = problems(plate_analysis, {"datum.B": [_bottom(plate_analysis)["id"]]})
    assert any(p["about"] == "datum.B" and "parallel to A" in p["message"] for p in redundant)


def test_added_requirements_are_written_and_read_back(plate, plate_analysis, tmp_path):
    top, bore = _top(plate_analysis)["id"], _bore8(plate_analysis)["id"]
    answers = {
        "part.material": "steel",
        "extra.1": {"faces": [top], "kind": "flatness", "value": "0.05"},
        "extra.2": {"faces": [bore], "kind": "perpendicularity", "value": "0.02", "datums": ["A"]},
        "extra.3": {"faces": [top], "kind": "parallelism", "value": "0.1", "datums": ["A"]},
        "extra.4": {"faces": [bore], "kind": "fit", "value": "H6"},
    }
    intent = apply(plate_analysis, answers, accept_defaults=True)
    added = {r["question"]: r for r in intent.requirements if r.get("feature") == "added"}
    assert added["extra.1"] == {
        "kind": "flatness",
        "feature": "added",
        "faces": [top],
        "question": "extra.1",
        "tolerance": 0.05,
    }
    assert added["extra.2"]["diametral"] is True and added["extra.2"]["datums"] == ["A"]
    assert added["extra.4"]["fit"] == "H6" and added["extra.4"]["upper"] == pytest.approx(0.009)
    out = tmp_path / "added.step"
    write(plate, intent, out)  # verifies every requirement reads back
    kinds = {e.type for e in read_existing(load(out)) if e.kind == "geometric_tolerance"}
    assert {"Flatness", "Perpendicularity", "Parallelism"} <= kinds


def test_an_added_orientation_brings_datums_where_nothing_else_needs_them(plate_analysis):
    plain = {
        q.id: "general" for q in questions(plate_analysis, {}) if q.id.startswith("hole.function")
    }
    assert not any(q.id.startswith("datum.") for q in questions(plate_analysis, plain))
    bore = _bore8(plate_analysis)["id"]
    extra = {"faces": [bore], "kind": "perpendicularity", "value": "0.02", "datums": ["A"]}
    assert "datum.A" in {q.id for q in questions(plate_analysis, {**plain, "extra.1": extra})}


def test_an_added_requirement_citing_a_missing_datum_is_reported_not_written(plate_analysis):
    bore = _bore8(plate_analysis)["id"]
    answers = {"extra.1": {"faces": [bore], "kind": "parallelism", "value": "0.1", "datums": ["E"]}}
    intent = apply(plate_analysis, {**answers, "part.material": "x"}, accept_defaults=True)
    assert not any(r.get("question") == "extra.1" for r in intent.requirements)
    found = problems(plate_analysis, answers)
    assert any(p["about"] == "datum.E" and "datum E" in p["message"] for p in found)
    # E is asked, empty, so it can be picked.
    assert "datum.E" in {q.id for q in questions(plate_analysis, answers)}


@pytest.mark.parametrize(
    ("extra", "reason"),
    [
        ({"faces": [], "kind": "flatness", "value": "0.1"}, "pick the faces"),
        ({"faces": [99999], "kind": "flatness", "value": "0.1"}, "does not exist"),
        ({"faces": "BORE", "kind": "flatness", "value": "0.1"}, "flat faces"),
        ({"faces": "TOP", "kind": "fit", "value": "H7"}, "cylinder"),
        ({"faces": "BORE", "kind": "fit", "value": "Q7"}, "not a fit"),
        ({"faces": "TOP", "kind": "perpendicularity", "value": "0.1"}, "which datum"),
        ({"faces": "TOP", "kind": "flatness", "value": "none"}, "give a tolerance"),
        ({"faces": "TOP", "kind": "roundness", "value": "0.1"}, "not one of"),
    ],
)
def test_an_added_requirement_that_does_not_suit_its_faces_is_refused(
    plate_analysis, extra, reason
):
    named = {"TOP": [_top(plate_analysis)["id"]], "BORE": [_bore8(plate_analysis)["id"]]}
    extra = (
        {**extra, "faces": named.get(extra["faces"], extra["faces"])}
        if isinstance(extra["faces"], str)
        else extra
    )
    with pytest.raises(ValueError, match=reason):
        questions(plate_analysis, {"extra.1": extra})


def test_only_the_datums_added_orientations_cite_are_proposed(plate_analysis):
    plus_minus = {"part.locating": LOCATING[1]}
    bore = _bore8(plate_analysis)["id"]
    extra = {"faces": [bore], "kind": "perpendicularity", "value": "0.02", "datums": ["B"]}
    asked = {q.id: q for q in questions(plate_analysis, {**plus_minus, "extra.1": extra})}
    assert [i for i in asked if i.count(".") == 1 and i.startswith("datum.")] == ["datum.B"]
    assert asked["datum.B"].default  # proposed from the part
    both = {**extra, "datums": ["A", "B"]}
    asked_both = {q.id for q in questions(plate_analysis, {**plus_minus, "extra.1": both})}
    assert {"datum.A", "datum.B"} <= asked_both and "datum.C" not in asked_both


def test_an_orientation_on_its_own_datum_is_reported(plate_analysis):
    top = _top(plate_analysis)["id"]
    extra = {"faces": [top], "kind": "parallelism", "value": "0.05", "datums": ["A"]}
    found = problems(plate_analysis, {"datum.A": [top], "extra.1": extra})
    assert any(p["about"] == "extra.1" and "measured from itself" in p["message"] for p in found)
