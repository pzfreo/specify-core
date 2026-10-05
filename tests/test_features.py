import json

from quiddity import import_step_geometry
from quiddity.evidence import build_recognition_evidence

from specify_core.api import analyse
from specify_core.load import load


def test_xcaf_load_recognises_what_quiddity_load_does(spool):
    """specify-core recognises the XCAF-loaded shape, not Quiddity's own import."""
    ours = build_recognition_evidence(load(spool).part)
    theirs = build_recognition_evidence(import_step_geometry(str(spool)))
    records = lambda v: sorted(json.dumps(v.record(f).to_dict(), default=str) for f in v.features)  # noqa: E731
    assert records(ours) == records(theirs)


def test_analysis_is_json_and_ids_are_unique(spool):
    analysis = analyse(spool)
    json.dumps(analysis)
    ids = [f["id"] for f in analysis["features"]]
    assert len(ids) == len(set(ids))


def test_hole_patterns_link_every_member(spool):
    analysis = analyse(spool)
    ids = {f["id"] for f in analysis["features"]}
    patterns = [f for f in analysis["features"] if f["family"] == "hole_patterns"]
    assert len(patterns) == 3
    for p in patterns:
        assert not p.get("notes")
        assert len(p["members"]) == len(p["record"]["holes"])
        assert set(p["members"]) <= ids


def test_grid_is_linked(plate):
    analysis = analyse(plate)
    grids = [f for f in analysis["features"] if f["family"] == "hole_patterns"]
    assert [g["record"]["kind"] for g in grids] == ["grid"]
    assert len(grids[0]["members"]) == 6


def _plate_with_holes(tmp_path, locations):
    from build123d import Box, BuildPart, Cylinder, Locations, Mode, export_step

    with BuildPart() as bp:
        Box(100, 60, 10)
        with Locations(*locations):
            Cylinder(2.75, 10, mode=Mode.SUBTRACT)
    path = tmp_path / "plate.step"
    export_step(bp.part, str(path))
    return [f for f in analyse(path)["features"] if f["family"] == "hole_patterns"]


def test_four_corner_holes_are_quiddity_rectangle_with_members(tmp_path):
    """quiddity#790 and #791: the pattern and its member holes come from Quiddity."""
    (rect,) = _plate_with_holes(tmp_path, [(x, y) for x in (-40, 40) for y in (-20, 20)])
    assert rect["record"]["kind"] == "rectangle"
    assert rect["id"].startswith("hole_patterns.rectangle:")
    assert len(rect["members"]) == 4 and not rect.get("notes")


def test_identical_holes_no_pattern_claims_form_a_set(tmp_path):
    """A pair is no layout Quiddity recognises, but the two are specified together."""
    (pair,) = _plate_with_holes(tmp_path, [(-30, 10), (25, -15)])
    assert (pair["record"]["kind"], len(pair["members"])) == ("set", 2)
