"""What NIST's STEP File Analyzer found wrong in written files, kept fixed."""

import re

from specify_core.api import analyse, apply, write
from specify_core.rules import questions


def test_no_malformed_runout_zone_is_written(pin, tmp_path):
    # OCCT writes RUNOUT_ZONE_DEFINITION with two attributes of three for every
    # diameter-shaped zone, and an orphan orientation in metres.
    analysis = analyse(pin)
    runout = next(q for q in questions(analysis, {}) if q.id.startswith("diameter.runout"))
    intent = apply(analysis, {"part.material": "brass", runout.id: "0.02"}, accept_defaults=True)
    assert any(r.get("diametral") for r in intent.requirements) and any(
        r["kind"] == "runout" for r in intent.requirements
    )
    out = tmp_path / "zones.step"
    write(pin, intent, out)  # verifies it reads back
    text = out.read_text()
    assert "RUNOUT_ZONE" not in text
    assert "PLANE_ANGLE_UNIT() SI_UNIT($,.METRE.)" not in text
    assert "TOLERANCE_ZONE_FORM('cylindrical or circular')" in text
    refs = {int(r) for r in re.findall(r"#(\d+)", text.split("DATA;", 1)[1])}
    ids = {int(i) for i in re.findall(r"^#(\d+)\s*=", text, re.M)}
    assert refs <= ids, "a reference to a removed entity"


def test_callout_presentation_is_named(pin, tmp_path):
    analysis = analyse(pin)
    shaft = next(
        f["id"]
        for f in analysis["features"]
        if f["family"] == "turned_steps" and f["record"]["diameter"] == 6.0
    )
    answers = {"part.material": "brass", f"diameter.fit:{shaft}": "thread:M6"}
    intent = apply(analysis, answers, accept_defaults=True)
    out = tmp_path / "named.step"
    write(pin, intent, out)
    text = out.read_text()
    assert "TESSELLATED_GEOMETRIC_SET(''" not in text
    assert "TESSELLATED_ANNOTATION_OCCURRENCE(''" not in text
    assert "TESSELLATED_GEOMETRIC_SET('note'" in text


def test_limits_are_typed_lengths_and_runout_is_simple(pin, tmp_path):
    # A select value must say its type; a circular runout is a simple entity.
    analysis = analyse(pin)
    runout = next(q for q in questions(analysis, {}) if q.id.startswith("diameter.runout"))
    fit = next(q for q in questions(analysis, {}) if q.id.startswith("diameter.fit"))
    answers = {"part.material": "brass", runout.id: "0.02", fit.id: "g6"}
    intent = apply(analysis, answers, accept_defaults=True)
    assert any(r["kind"] == "size" for r in intent.requirements)
    out = tmp_path / "typed.step"
    write(pin, intent, out)  # verifies limits and runout read back
    text = re.sub(r"\s*\n\s*", " ", out.read_text())
    assert not re.search(r"= MEASURE_WITH_UNIT\(\s*[-+0-9.]", text)
    assert "LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(" in text
    assert not re.search(r"(RUNOUT|PARALLELISM|PERPENDICULARITY)_TOLERANCE\s*\(\s*\)", text)
    assert re.search(r"= CIRCULAR_RUNOUT_TOLERANCE\('", text)


def test_every_datum_referenced_tolerance_is_written_simply(plate, tmp_path):
    analysis = analyse(plate)
    planes = [f for f in analysis["faces"] if f["kind"] == "plane"]
    top = max(planes, key=lambda f: f["centroid"][2])
    side = next(f for f in planes if abs(f["direction"][2]) < 0.1)
    bore = next(f for f in analysis["faces"] if f["kind"] == "cylinder")
    kinds = [
        (top, "parallelism"),
        (side, "perpendicularity"),
        (bore, "runout"),
        (bore, "total_runout"),
    ]
    answers = {"part.material": "steel"} | {
        f"extra.{n}": {"faces": [face["id"]], "kind": kind, "value": "0.05", "datums": ["A"]}
        for n, (face, kind) in enumerate(kinds)
    }
    out = tmp_path / "kinds.step"
    write(plate, apply(analysis, answers, accept_defaults=True), out)  # verifies
    text = re.sub(r"\s*\n\s*", " ", out.read_text())
    simple = r"(PARALLELISM|PERPENDICULARITY|RUNOUT)_TOLERANCE\s*\(\s*\)"
    assert not re.search(simple, text), "a tolerance written complex"
    for kind in ("PARALLELISM", "PERPENDICULARITY", "CIRCULAR_RUNOUT", "TOTAL_RUNOUT"):
        assert re.search(rf"= {kind}_TOLERANCE\('", text)
