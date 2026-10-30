import pytest
from specify_core.api import analyse
from specify_core.rules import questions


@pytest.fixture(scope="module")
def plate_analysis(plate):
    return analyse(plate)


def _by_prompt(qs, text):
    return next(q for q in qs if text in q.prompt)


def test_defaults_come_from_standard_sizes(plate_analysis):
    qs = questions(plate_analysis, {})
    assert _by_prompt(qs, "the 6× Ø6.6").default == "clearance:M6"
    assert _by_prompt(qs, "Ø4.2 hole").default == "tapped:M5"
    assert _by_prompt(qs, "Ø8 hole").default == "general"


def test_datums_default_to_three_square_planes(plate_analysis):
    qs = {q.id: q for q in questions(plate_analysis, {})}
    faces = {f["id"]: f for f in plate_analysis["faces"]}
    a, b, c = (faces[qs[f"datum.{x}"].default[0]] for x in "ABC")
    assert a["area"] == max(f["area"] for f in faces.values() if f["kind"] == "plane")
    dot = lambda u, v: sum(x * y for x, y in zip(u, v, strict=True))  # noqa: E731
    assert dot(a["direction"], b["direction"]) == pytest.approx(0)
    assert dot(b["direction"], c["direction"]) == pytest.approx(0)


def test_position_follows_the_fastener_formula(plate_analysis):
    qs = questions(plate_analysis, {})
    grid = _by_prompt(qs, "Position tolerance (Ø) for the 6× Ø6.6")
    tapped = _by_prompt(qs, "Position tolerance (Ø) for the Ø4.2")
    assert grid.default == "0.6"  # floating: 6.6 - 6
    assert tapped.default == "0.25"  # fixed: (5.5 - 5) / 2


def test_general_holes_get_no_position_question(plate_analysis):
    qs = questions(plate_analysis, {})
    assert not any("Position" in q.prompt and "Ø8 hole" in q.prompt for q in qs)


def test_off_table_holes_can_be_clearance_for_an_unstated_bolt(spool):
    """the spool's Ø8 holes match no ISO 273 size."""
    qs = {q.id: q for q in questions(analyse(spool), {})}
    eights = [q for q in qs.values() if q.id.startswith("hole.function") and "Ø8 holes" in q.prompt]
    assert eights and all("clearance" in q.options for q in eights)
    answered = {eights[0].id: "clearance"}
    position = next(
        q
        for q in questions(analyse(spool), answered)
        if q.id == eights[0].id.replace("function", "position")
    )
    assert position.default == "2"  # Ø8 - M6, the largest bolt it clears


def test_a_coarse_series_hole_defaults_to_clearance(spool):
    qs = questions(analyse(spool), {})
    tens = next(q for q in qs if q.id.startswith("hole.function") and "Ø10 holes" in q.prompt)
    assert tens.default == "clearance:M8" and tens.attention == "routine"


def test_a_bore_is_not_offered_as_clearance(spool):
    bore = next(q for q in questions(analyse(spool), {}) if q.id == "hole.function:holes:34.55")
    assert not any(o.startswith("clearance") for o in bore.options)
