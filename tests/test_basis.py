"""Every default specify-core chooses says why, so it can be shown as a choice made."""

from specify_core.api import analyse
from specify_core.rules import questions


def test_the_choices_made_for_a_person_say_why(plate):
    qs = {q.id: q for q in questions(analyse(plate), {})}
    by_prefix = {i.split(":")[0]: q for i, q in qs.items()}
    assert qs["datum.A"].basis.startswith("The largest flat face")
    assert qs["datum.B"].basis == "The largest flat face square to A."
    assert qs["part.general_tolerance"].basis.startswith("ISO 2768-m")
    grid = next(q for i, q in qs.items() if i.startswith("hole.function:hole_patterns"))
    assert grid.basis == "⌀6.6 is an ISO 273 clearance hole for M6."
    assert "H − F" in qs[grid.id.replace("function", "position")].basis
    assert by_prefix["hole.function"].to_dict().get("basis") is not None


def test_a_hole_left_to_the_general_tolerance_has_no_choice_to_explain(plate):
    qs = questions(analyse(plate), {})
    plain = [q for q in qs if q.id.startswith("hole.function") and q.default == "general"]
    assert plain and all(q.basis == "" and "basis" not in q.to_dict() for q in plain)
