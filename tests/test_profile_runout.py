"""Runout on turned diameters, and profile and runout added on any faces."""

import pytest

from specify_core.api import analyse, apply, write
from specify_core.existing import read_existing
from specify_core.load import load
from specify_core.rules import _default_datums, questions


@pytest.fixture(scope="module")
def pin_analysis(pin):
    return analyse(pin)


def _runouts(analysis, answers=None):
    return [q for q in questions(analysis, answers or {}) if q.id.startswith("diameter.runout")]


def test_turned_diameters_offer_runout_about_the_part_axis_but_not_the_axis_itself(pin_analysis):
    faces = {f["id"]: f for f in pin_analysis["faces"]}
    axis = set(_default_datums(faces, pin_analysis["features"])[1])
    asked = _runouts(pin_analysis)
    assert asked and all(q.frame == "A|B" and q.default == "none" for q in asked)
    assert all(q.attention == "refine" and "0.02" in q.options for q in asked)
    assert not any(set(q.faces) & axis for q in asked)


def test_a_runout_is_written_about_the_frame_and_read_back(pin, pin_analysis, tmp_path):
    (q, *_) = _runouts(pin_analysis)
    intent = apply(pin_analysis, {"part.material": "brass", q.id: "0.02"}, accept_defaults=True)
    (runout,) = [r for r in intent.requirements if r["kind"] == "runout"]
    assert runout["tolerance"] == 0.02 and runout["datums"] == ["A", "B"]
    out = tmp_path / "runout.step"
    write(pin, intent, out)  # verifies it reads back
    assert any(e.type == "CircularRunout" for e in read_existing(load(out)))


def test_profile_and_total_runout_can_be_added_on_any_faces(plate, tmp_path):
    analysis = analyse(plate)
    top = max(
        (f for f in analysis["faces"] if f["kind"] == "plane"), key=lambda f: f["centroid"][2]
    )
    bore = next(
        f for f in analysis["faces"] if f["kind"] == "cylinder" and abs(f["radius"] - 4) < 1e-6
    )
    answers = {
        "part.material": "steel",
        "extra.1": {"faces": [top["id"]], "kind": "profile", "value": "0.1"},
        "extra.2": {"faces": [top["id"]], "kind": "profile", "value": "0.2", "datums": ["A"]},
        "extra.3": {
            "faces": [bore["id"]],
            "kind": "total_runout",
            "value": "0.03",
            "datums": ["A"],
        },
    }
    intent = apply(analysis, answers, accept_defaults=True)
    added = {r["question"]: r for r in intent.requirements if r.get("feature") == "added"}
    assert added["extra.1"]["datums"] == [] and not added["extra.1"]["diametral"]
    assert added["extra.3"]["kind"] == "total_runout" and not added["extra.3"]["diametral"]
    out = tmp_path / "profile.step"
    write(plate, intent, out)
    kinds = {e.type for e in read_existing(load(out)) if e.kind == "geometric_tolerance"}
    assert {"ProfileOfSurface", "TotalRunout"} <= kinds


def test_a_runout_needs_a_datum_but_a_profile_does_not(plate):
    analysis = analyse(plate)
    face = analysis["faces"][0]["id"]
    with pytest.raises(ValueError, match="which datum"):
        questions(analysis, {"extra.1": {"faces": [face], "kind": "runout", "value": "0.05"}})
    questions(analysis, {"extra.1": {"faces": [face], "kind": "profile", "value": "0.05"}})
