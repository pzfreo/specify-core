import pytest

from specify_core.api import analyse, apply, preview


@pytest.fixture(scope="module")
def plate_analysis(plate):
    return analyse(plate)


def test_nothing_answered_shows_the_defaults(plate_analysis):
    intent = preview(plate_analysis, {})
    assert [d["letter"] for d in intent.datums] == ["A", "B", "C"]
    assert intent.requirements and all(r["default"] for r in intent.requirements)
    assert all(d["default"] for d in intent.datums)
    assert "material" not in intent.part  # no answer and no default


def test_an_answer_is_shown_as_decided(plate_analysis):
    grid = next(
        f["id"] for f in plate_analysis["features"] if f["id"].startswith("hole_patterns.grid")
    )
    answers = {f"hole.position:{grid}": "0.2", "part.material": "6082-T6"}
    intent = preview(plate_analysis, answers)
    (position,) = [
        r for r in intent.requirements if r.get("feature") == grid and r["kind"] == "position"
    ]
    assert (position["tolerance"], position["default"]) == (0.2, False)
    assert position["question"] == f"hole.position:{grid}"
    assert intent.part["material"] == "6082-T6"


def test_every_item_names_its_question(plate_analysis):
    intent = apply(plate_analysis, {"part.material": "steel"} | _defaults(plate_analysis))
    assert all("question" in item for item in intent.datums + intent.requirements)
    assert {d["question"] for d in intent.datums} == {"datum.A", "datum.B", "datum.C"}


def test_a_refused_answer_is_refused_in_preview_too(plate_analysis):
    with pytest.raises(ValueError, match="datum A"):
        preview(plate_analysis, {"datum.A": [99999]})


def _defaults(analysis):
    from specify_core.rules import with_all_defaults

    return with_all_defaults(analysis, {})
