"""Locating holes by ± dimensions, the problems list, and requirements a
person adds where nothing asked."""
import pytest
from specify_core.api import analyse, apply
from specify_core.load import load
from specify_core.rules import LOCATING, questions


@pytest.fixture(scope="module")
def plate_analysis(plate):
    return analyse(plate)


def _top(analysis):
    return max(
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


def test_an_added_orientation_brings_datums_where_nothing_else_needs_them(plate_analysis):
    plain = {
        q.id: "general" for q in questions(plate_analysis, {}) if q.id.startswith("hole.function")
    }
    assert not any(q.id.startswith("datum.") for q in questions(plate_analysis, plain))
    bore = _bore8(plate_analysis)["id"]
    extra = {"faces": [bore], "kind": "perpendicularity", "value": "0.02", "datums": ["A"]}
    assert "datum.A" in {q.id for q in questions(plate_analysis, {**plain, "extra.1": extra})}


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
