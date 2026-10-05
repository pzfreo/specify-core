"""Where labels land on the part, and which way their leaders leave it."""

import pytest

from specify_core.anchors import anchors, key
from specify_core.api import analyse
from specify_core.load import load
from specify_core.serve import reply


@pytest.fixture(scope="module")
def pin_loaded(pin):
    return load(pin)


def _face(analysis, kind, radius=None):
    return next(
        f["id"]
        for f in analysis["faces"]
        if f["kind"] == kind and (radius is None or abs(f.get("radius", 0) - radius) < 1e-6)
    )


def test_a_shaft_is_labelled_from_its_side_and_a_hole_from_its_open_end(pin, pin_loaded):
    analysis = analyse(pin_loaded)
    shaft, hole = _face(analysis, "cylinder", 3.0), _face(analysis, "cylinder", 2.1)
    found = anchors(pin_loaded, [(shaft,), (hole,)], 40.0)
    s, h = found[key((shaft,))], found[key((hole,))]
    # The pin stands along Z: the shaft's leader leaves sideways, square to it.
    assert abs(s["out"][2]) < 1e-6 and s["clear"]
    assert abs((s["at"][0] ** 2 + s["at"][1] ** 2) ** 0.5 - 3.0) < 1e-3  # on its surface
    # The tapped hole opens at the top (Z = 32): its leader leaves up, out of it,
    # from its rim, so the label is not left inside the hole.
    assert h["out"] == [0.0, 0.0, 1.0] and h["clear"]
    assert h["at"][2] == pytest.approx(32.0)
    assert (h["at"][0] ** 2 + h["at"][1] ** 2) ** 0.5 == pytest.approx(2.1, abs=1e-3)


def test_preview_carries_anchors_for_its_labels_when_given_the_file(pin):
    analysis = analyse(pin)
    request = {"id": 1, "op": "preview", "analysis": analysis, "answers": {}}
    assert "anchors" not in reply(request)["result"]["intent"]
    intent = reply({**request, "step": str(pin)})["result"]["intent"]
    labelled = {key(r["faces"]) for r in intent["requirements"] + intent["datums"] if r["faces"]}
    assert labelled and labelled <= set(intent["anchors"])
    assert all(len(a["at"]) == 3 and len(a["out"]) == 3 for a in intent["anchors"].values())


def test_anchors_are_not_given_for_a_different_file(pin, plate):
    analysis = analyse(pin)
    result = reply(
        {"id": 1, "op": "preview", "analysis": analysis, "answers": {}, "step": str(plate)}
    )
    assert result["result"]["intent"]["anchors"] == {}


def test_a_hole_along_x_is_labelled_from_its_rim(tmp_path):
    # As in a turned part lying along X: the bore's centre, as build123d gives
    # it, lies on its wall, so a hole must be told by its normal and axis.
    from build123d import Align, Axis, BuildPart, Cylinder, Mode, export_step

    with BuildPart() as bp:
        Cylinder(3, 20, rotation=(0, 90, 0), align=(Align.CENTER, Align.CENTER, Align.MIN))
        Cylinder(
            0.8,
            3.8,
            rotation=(0, 90, 0),
            align=(Align.CENTER, Align.CENTER, Align.MIN),
            mode=Mode.SUBTRACT,
        )
    path = tmp_path / "rod.step"
    export_step(bp.part, str(path))
    loaded = load(path)
    analysis = analyse(loaded)
    hole = _face(analysis, "cylinder", 0.8)
    found = anchors(loaded, [(hole,)], 21.0)[key((hole,))]
    mouth = min(bp.part.faces().filter_by(Axis.X), key=lambda f: f.center().X).center().X
    assert abs(found["out"][0]) == pytest.approx(1.0) and found["clear"]
    assert found["at"][0] == pytest.approx(mouth)
