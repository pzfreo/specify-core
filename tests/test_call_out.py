"""Calling out a dimension: a size or a distance a person tolerances themselves,
in place of what the general tolerance would have said of it."""

import pytest

from specify_core import general
from specify_core.api import analyse, apply, write
from specify_core.existing import read_existing
from specify_core.load import load
from specify_core.problems import problems
from specify_core.rules import questions

MATERIAL = {"part.material": "steel"}


@pytest.fixture(scope="module")
def plate_analysis(plate):
    return analyse(plate)


def _general(analysis, answers=None):
    return apply(analysis, {**MATERIAL, **(answers or {})}, accept_defaults=True).general


def _added(intent, key):
    return next(r for r in intent.requirements if r.get("question") == key)


def test_a_called_out_size_is_written_and_leaves_the_general_list(plate, plate_analysis, tmp_path):
    size = next(g for g in _general(plate_analysis) if g["kind"] == "size")
    extra = {"faces": size["faces"], "kind": "size", "value": "+0.1/-0"}
    intent = apply(plate_analysis, {**MATERIAL, "extra.1": extra}, accept_defaults=True)
    req = _added(intent, "extra.1")
    assert (req["kind"], req["nominal"], req["upper"], req["lower"], req["fit"]) == (
        "size",
        size["nominal"],
        0.1,
        0.0,
        "",
    )
    assert not any(g["kind"] == "size" and g["faces"] == size["faces"] for g in intent.general)
    write(plate, intent, tmp_path / "size.step")  # verifies the limits read back


def test_a_called_out_size_may_be_plus_minus_or_a_fit(plate_analysis):
    size = next(g for g in _general(plate_analysis) if g["kind"] == "size")
    for value, (fit, upper, lower) in {"±0.05": ("", 0.05, -0.05), "H7": ("H7", None, 0.0)}.items():
        extra = {"faces": size["faces"], "kind": "size", "value": value}
        req = _added(
            apply(plate_analysis, {**MATERIAL, "extra.1": extra}, accept_defaults=True), "extra.1"
        )
        assert req["fit"] == fit and req["lower"] == lower
        assert upper is None or req["upper"] == upper


def test_a_called_out_length_is_written_and_leaves_the_general_list(
    plate, plate_analysis, tmp_path
):
    length = next(g for g in _general(plate_analysis) if g["kind"] == "location")
    extra = {
        "faces": length["faces"],
        "reference": length["reference"],
        "kind": "distance",
        "value": "±0.05",
    }
    intent = apply(plate_analysis, {**MATERIAL, "extra.1": extra}, accept_defaults=True)
    req = _added(intent, "extra.1")
    assert req["kind"] == "location" and req["distance"] == pytest.approx(length["distance"])
    assert (req["upper"], req["lower"]) == (0.05, -0.05)
    assert not any(
        g["kind"] == "location" and {*g["faces"]} == {*length["faces"]} for g in intent.general
    )
    out = tmp_path / "length.step"
    write(plate, intent, out)  # verifies the location reads back
    found = [e for e in read_existing(load(out)) if e.kind == "dimension"]
    assert any(e.type.startswith("Location") for e in found)


def test_a_distance_to_a_holes_axis_is_measured_from_a_plane_along_it(plate_analysis):
    faces = {f["id"]: f for f in plate_analysis["faces"]}
    bore = next(f for f in faces.values() if f["kind"] == "cylinder")
    side = next(
        f
        for f in faces.values()
        if f["kind"] == "plane"
        and abs(sum(a * b for a, b in zip(f["direction"], bore["direction"], strict=False))) < 1e-6
    )
    extra = {"faces": [bore["id"]], "reference": [side["id"]], "kind": "distance", "value": "±0.1"}
    req = _added(
        apply(plate_analysis, {**MATERIAL, "extra.1": extra}, accept_defaults=True), "extra.1"
    )
    offset = [o - c for o, c in zip(bore["origin"], side["centroid"], strict=False)]
    expected = abs(sum(o * n for o, n in zip(offset, side["direction"], strict=False)))
    assert req["distance"] == pytest.approx(expected, abs=1e-5) and req["reference"] == [side["id"]]


def _planes(analysis):
    planes = [f for f in analysis["faces"] if f["kind"] == "plane"]
    z = [f for f in planes if abs(abs(f["direction"][2]) - 1) < 1e-6]
    top = max(z, key=lambda f: f["centroid"][2])
    bottom = min(z, key=lambda f: f["centroid"][2])
    side = next(f for f in planes if abs(f["direction"][2]) < 1e-6)
    return top["id"], bottom["id"], side["id"]


@pytest.mark.parametrize(
    ("extra", "reason"),
    [
        ({"faces": "TOP", "kind": "size", "value": "±0.1"}, "one diameter"),
        ({"faces": "BORE", "kind": "size", "value": "0.1 or so"}, "give ±0.05"),
        ({"faces": "BORE", "kind": "size", "value": "+0/-0"}, "must differ"),
        ({"faces": "TOP", "kind": "distance", "value": "±0.1"}, "from flat faces"),
        ({"faces": "TOP", "reference": "TOP", "kind": "distance", "value": "±0.1"}, "itself"),
        ({"faces": "SIDE", "reference": "BOTTOM", "kind": "distance", "value": "±0.1"}, "parallel"),
        ({"faces": "TOP", "reference": "BOTTOM", "kind": "distance", "value": "H7"}, "give ±0.05"),
        ({"faces": "BORE", "reference": "TOP", "kind": "distance", "value": "±0.1"}, "along"),
        # Limits typed as limits, not deviations, are refused rather than misread.
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
    ],
)
def test_a_call_out_that_does_not_suit_its_faces_is_refused(plate_analysis, extra, reason):
    top, bottom, side = _planes(plate_analysis)
    bore = next(f["id"] for f in plate_analysis["faces"] if f["kind"] == "cylinder")
    named = {"TOP": [top], "BOTTOM": [bottom], "SIDE": [side], "BORE": [bore]}
    extra = {k: named.get(v, v) if k in ("faces", "reference") else v for k, v in extra.items()}
    with pytest.raises(ValueError, match=reason):
        questions(plate_analysis, {"extra.1": extra})


@pytest.mark.parametrize("value", ["-0.05/-0.1", "+0.1/+0.05", "+0.2/-0.1", "+0.1/0"])
def test_a_distance_with_any_limits_is_written_and_verified(plate, plate_analysis, tmp_path, value):
    top, bottom, _ = _planes(plate_analysis)
    extra = {"faces": [top], "reference": [bottom], "kind": "distance", "value": value}
    intent = apply(plate_analysis, {**MATERIAL, "extra.1": extra}, accept_defaults=True)
    write(plate, intent, tmp_path / "limits.step")  # verifies the limits are in the file


def test_a_diameter_sized_twice_is_reported(plate_analysis):
    size = next(g for g in _general(plate_analysis) if g["kind"] == "size")
    answers = {
        "extra.1": {"faces": size["faces"], "kind": "fit", "value": "H7"},
        "extra.2": {"faces": size["faces"], "kind": "size", "value": "±0.05"},
    }
    found = [p for p in problems(plate_analysis, answers) if "sized twice" in p["message"]]
    assert [p["about"] for p in found] == ["extra.2"] and "H7 and ±0.05" in found[0]["message"]


def test_a_distance_leaves_the_general_list_by_place_not_by_the_faces_picked():
    z = [0.0, 0.0, 1.0]
    faces = {
        1: {"id": 1, "kind": "plane", "centroid": [0, 0, 0], "direction": z},
        2: {"id": 2, "kind": "plane", "centroid": [50, 0, 0], "direction": [0, 0, -1.0]},
        3: {"id": 3, "kind": "plane", "centroid": [0, 0, 10], "direction": z},
        4: {"id": 4, "kind": "plane", "centroid": [40, 5, 10], "direction": z},
        5: {"id": 5, "kind": "plane", "centroid": [0, 0, 12], "direction": z},
    }
    # Two faces at one level, picked one at a time, from either face of the base.
    assert general._between([3, 4], [1, 2], faces) == general._between([4], [2], faces)
    assert general._between([3], [1], faces) == general._between([1], [3], faces)
    assert general._between([5], [1], faces) != general._between([3], [1], faces)


def test_a_datum_a_call_out_is_measured_from_is_used(plate_analysis):
    top, bottom, _ = _planes(plate_analysis)
    answers = {
        "datum.A": [bottom],
        "extra.1": {"faces": [top], "reference": [bottom], "kind": "distance", "value": "±0.1"},
    }
    plain = {
        q.id: "general" for q in questions(plate_analysis, {}) if q.id.startswith("hole.function")
    }
    unused = [
        p for p in problems(plate_analysis, {**plain, **answers}) if "not used" in p["message"]
    ]
    assert not any(p["about"] == "datum.A" for p in unused)
