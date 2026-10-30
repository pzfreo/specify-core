"""Location dimensions: basic from a position's datum planes, ± from the edges."""
import pytest
from specify_core.api import analyse, apply, write
from specify_core.existing import read_existing
from specify_core.load import load
from specify_core.rules import LOCATING, questions


@pytest.fixture(scope="module")
def plate_analysis(plate):
    return analyse(plate)


def _locations(intent):
    return [r for r in intent.requirements if r["kind"] == "location"]


def test_positioned_holes_get_basic_dimensions_from_their_datum_planes(
    plate, plate_analysis, tmp_path
):
    intent = apply(plate_analysis, {"part.material": "steel"}, accept_defaults=True)
    basic = _locations(intent)
    assert basic and all(r["basic"] and "upper" not in r for r in basic)
    datum_faces = {tuple(d["faces"]) for d in intent.datums if d["letter"] in "BC"}
    assert {tuple(r["reference"]) for r in basic} <= datum_faces
    # The M5 hole at x=45, y=0 on a 100 x 60 plate: 95 and 30 from its edges.
    m5 = sorted(r["distance"] for r in basic if r["feature"] == "holes:12")
    assert m5 == [30.0, 95.0]
    out = tmp_path / "basic.step"
    write(plate, intent, out)  # verifies each location reads back
    text = out.read_text()
    assert text.count("DIMENSIONAL_LOCATION(") == len(basic)
    assert text.count("'dimensional note','theoretical'") == 1  # one item, shared
    back = [e for e in read_existing(load(out)) if e.type.startswith("Location")]
    assert len(back) == len(basic)


def test_holes_located_by_plus_minus_can_be_given_a_tolerance(plate, plate_analysis, tmp_path):
    asked = [
        q
        for q in questions(plate_analysis, {"part.locating": LOCATING[1]})
        if q.id.startswith("hole.location")
    ]
    assert asked and all(q.default == "general" and "±0.05" in q.options for q in asked)
    grid = next(q.id for q in asked if "grid" in q.id)
    answers = {"part.locating": LOCATING[1], "part.material": "steel", grid: "±0.05"}
    intent = apply(plate_analysis, answers, accept_defaults=True)
    located = _locations(intent)
    assert len(located) == 12 and all(r["upper"] == 0.05 and r["lower"] == -0.05 for r in located)
    assert not any(r.get("basic") for r in located)
    out = tmp_path / "pm.step"
    write(plate, intent, out)
    assert "'theoretical'" not in out.read_text()


def test_general_writes_no_location(plate_analysis):
    answers = {"part.locating": LOCATING[1], "part.material": "steel"}
    assert _locations(apply(plate_analysis, answers, accept_defaults=True)) == []


def test_a_part_whose_only_dimensions_are_locations_keeps_its_tolerance_values(plate, tmp_path):
    """OCCT writes tolerance values in other units when a file has no dimension:
    with only a tapped hole positioned, its basic dimensions are the only ones."""
    analysis = analyse(plate)
    answers = {"part.material": "steel"}
    for q in questions(analysis, {}):
        if q.id.startswith("hole.function") and not str(q.default).startswith("tapped"):
            answers[q.id] = "general"
    intent = apply(analysis, answers, accept_defaults=True)
    assert not any(r["kind"] == "size" for r in intent.requirements)
    assert any(r["kind"] == "location" for r in intent.requirements)
    (position,) = [r for r in intent.requirements if r["kind"] == "position"]
    write(plate, intent, tmp_path / "tapped.step")  # verifies the value reads back
    back = [e for e in read_existing(load(tmp_path / "tapped.step")) if e.type == "Position"]
    assert [e.value for e in back] == [position["tolerance"]]
