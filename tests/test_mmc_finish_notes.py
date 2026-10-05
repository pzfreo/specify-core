"""Clearance holes positioned at MMC, the part's notes, and surface finish."""

import re

import pytest

from specify_core.api import analyse, apply, write
from specify_core.existing import notes, part_settings
from specify_core.load import load
from specify_core.rules import questions


@pytest.fixture(scope="module")
def written(plate, tmp_path_factory):
    analysis = analyse(plate)
    bore = next(
        q.id
        for q in questions(analysis, {})
        if q.id.startswith("hole.function") and "Ø8" in q.prompt
    )
    top = max(
        (f for f in analysis["faces"] if f["kind"] == "plane"), key=lambda f: f["centroid"][2]
    )
    answers = {
        "part.material": "Aluminium 6082-T6",
        bore: "fit:H7",
        "part.coating": "Anodise black (MIL-A-8625 Type II)",
        "part.heat_treatment": "Solution treat and age to T6",
        "part.surface_finish": "Ra 1.6",
        "extra.1": {"faces": [top["id"]], "kind": "finish", "value": "0.8"},
    }
    intent = apply(analysis, answers, accept_defaults=True)
    out = tmp_path_factory.mktemp("notes") / "plate.step"
    report = write(plate, intent, out)
    return intent, report, out, top["id"]


def test_clearance_holes_are_positioned_at_mmc_and_fitted_ones_are_not(written):
    intent, report, out, _ = written
    positions = [r for r in intent.requirements if r["kind"] == "position"]
    clearance = [r for r in positions if "grid" in r["feature"]]
    fitted = [r for r in positions if r["feature"].startswith("holes:")]
    assert clearance and all(r.get("mmc") for r in clearance)
    assert fitted and not any(r.get("mmc") for r in fitted)
    assert any(w.startswith("position") and " MMC on " in w for w in report.written)
    assert "MAXIMUM_MATERIAL_REQUIREMENT" in out.read_text()


def test_the_parts_notes_are_written_and_read_back(written):
    intent, report, out, _ = written
    assert intent.part["coating"].startswith("Anodise black")
    assert "edges Break sharp edges 0.2 max" in report.written
    assert part_settings(load(out)) == {
        "material": "Aluminium 6082-T6",
        "general_tolerance": "ISO 2768-m",
        "surface_finish": "Ra 1.6",
        "coating": "Anodise black (MIL-A-8625 Type II)",
        "heat_treatment": "Solution treat and age to T6",
        "edges": "Break sharp edges 0.2 max",
    }


def test_nothing_is_written_for_no_coating_or_heat_treatment(plate, tmp_path):
    analysis = analyse(plate)
    intent = apply(analysis, {"part.material": "steel"}, accept_defaults=True)
    assert "coating" not in intent.part and "heat_treatment" not in intent.part
    text = write(plate, intent, tmp_path / "p.step") and (tmp_path / "p.step").read_text()
    assert not re.search(r"'(surface treatment|heat treatment)'", text)


def test_a_face_finish_is_written_on_its_face(written):
    intent, report, out, top = written
    (finish,) = [r for r in intent.requirements if r["kind"] == "finish"]
    assert finish["value"] == "Ra 0.8"
    assert f"surface finish Ra 0.8 on faces [{top}]" in report.written
    assert "'Ra 0.8 \\X2\\00B5\\X0\\m'" in out.read_text()  # µ, escaped as Part 21 asks
    assert any(n.type == "surface finish" and top in n.faces for n in notes(out))


@pytest.mark.parametrize("value", ["rough", "0", "Ra 99"])
def test_a_finish_that_is_not_a_roughness_is_refused(plate, value):
    analysis = analyse(plate)
    face = analysis["faces"][0]["id"]
    with pytest.raises(ValueError, match="roughness"):
        questions(analysis, {"extra.1": {"faces": [face], "kind": "finish", "value": value}})
