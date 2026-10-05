"""The NIST coverage report's face matching and judging, on synthetic faces."""

from __future__ import annotations

import importlib.util
from pathlib import Path

from specify_core.rules import Intent

_spec = importlib.util.spec_from_file_location(
    "coverage", Path(__file__).parents[1] / "tools" / "nist" / "coverage.py"
)
coverage = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(coverage)

Z = [0, 0, 1]


def _cyl(i, centroid, r=10.0, origin=(0, 0, 0)):
    return {
        "id": i, "kind": "cylinder", "radius": r, "centroid": centroid,
        "direction": Z, "origin": list(origin),
    }  # fmt: skip


def _plane(i, centroid, area=100.0):
    return {"id": i, "kind": "plane", "area": area, "centroid": centroid, "direction": Z}


def _intent(requirements=(), datums=(), general=()):
    return Intent(
        binding={}, requirements=list(requirements), datums=list(datums), general=list(general)
    )


def test_a_hole_is_all_of_its_surface_and_a_plane_its_nearest_pieces():
    truth = {
        "faces": [
            _cyl(0, [6.4, 0, 0]),
            _cyl(1, [-6.4, 0, 0]),
            _plane(2, [0, 0, 5], area=200.0),
            _cyl(3, [0, 0, 0], r=4),
        ]
    }
    start = {
        "faces": [
            _cyl(7, [0, 0, 0]),
            _cyl(8, [50, 0, 0], origin=(50, 0, 0)),
            _plane(9, [-5, 0, 5]),
            _plane(10, [5, 0, 5]),
            _plane(11, [90, 0, 5]),
            _plane(12, [0, 0, 6]),
        ]
    }
    assert coverage.mapping(truth, start) == {0: [7], 1: [7], 2: [9, 10], 3: []}


def test_an_inch_files_nominals_are_scaled_and_its_tolerances_not():
    analysis = {
        "existing": [
            {"kind": "dimension", "type": "Size_Diameter", "faces": [3, 1], "value": 0.5,
             "upper": 0.127, "lower": -0.127},
            {"kind": "dimension", "type": "DimensionPresentation", "faces": [3], "value": 0.0},
        ]
    }  # fmt: skip
    [item] = coverage.truth_items(analysis, "inch")
    assert item["key"] == "Size_Diameter@1,3"
    assert (item["value"], item["upper"]) == (12.7, 0.127)


SIZE = {"kind": "dimension", "type": "Size_Diameter", "value": 25.0, "upper": 0.15, "lower": -0.15}


def test_a_size_is_judged_by_its_limits_or_found_under_the_general_tolerance():
    h11 = {"kind": "size", "faces": [7], "fit": "H11", "upper": 0.13, "lower": 0.0}
    assert coverage.judge(SIZE, {7}, _intent([h11])) == (False, "H11 +0.13/+0")
    exact = {**h11, "fit": "", "upper": 0.15, "lower": -0.15}
    assert coverage.judge(SIZE, {7}, _intent([h11, exact])) == (True, "")
    general = _intent(general=[{"kind": "size", "faces": [7], "upper": 0.2, "lower": -0.2}])
    assert coverage.judge(SIZE, {7}, general) == (False, "under the general tolerance, ±0.2")
    assert coverage.judge(SIZE, {7}, _intent()) == (False, "")


def test_a_tolerance_is_matched_by_any_requirement_stating_it():
    item = {"kind": "geometric_tolerance", "type": "Position", "value": 0.75,
            "datums": ["A", "B", "C"]}  # fmt: skip
    loose = {"kind": "position", "faces": [7], "tolerance": 1.0, "datums": ["A", "B", "C"]}
    right = {**loose, "tolerance": 0.75, "mmc": True}
    assert coverage.judge(item, {7}, _intent([loose])) == (False, "1.0 not 0.75")
    assert coverage.judge(item, {7}, _intent([loose, right])) == (
        True,
        "ours has Ⓜ, NIST's not read",
    )
    line = {**item, "type": "ProfileOfLine"}
    assert coverage.judge(line, {7}, _intent()) == (False, "specify-core has no ProfileOfLine")


def test_a_datum_is_judged_by_its_faces():
    item = {"kind": "datum", "type": "B"}
    intent = _intent(datums=[{"letter": "B", "faces": [48]}])
    assert coverage.judge(item, {48}, intent) == (True, "")
    assert coverage.judge(item, {77}, intent) == (False, "datum B on faces [48]")


def test_a_size_with_no_tolerance_read_is_not_judged():
    plain = {**SIZE, "upper": None, "lower": None}
    assert coverage.status(plain, {7}, _intent(), _intent())[0] == "unread"


def test_beyond_lists_what_nist_states_nothing_of_on_those_faces():
    truth = [{**SIZE, "faces": [0]}]
    position = {"kind": "position", "faces": [7], "question": "hole.position:x"}
    size = {"kind": "size", "faces": [7], "question": "hole.function:x"}
    lines = coverage.beyond(truth, {0: [7]}, _intent([position, size]))
    assert lines[0] == "- position on faces 7 (hole.position:x)"
    assert "NIST states 1" in lines[1] and "specify-core states 1" in lines[1]
