"""Answers suggested by how the parts of an assembly fit together."""

import pytest

from specify_core import api
from specify_core.load import load_all
from specify_core.mates import mates, suggestion

BASE, COVER, PIN = 0, 1, 2


@pytest.fixture(scope="module")
def defaults(stack):
    """Each part's hole and diameter questions: id -> (default, attention, basis)."""
    out = {}
    for k in (BASE, COVER, PIN):
        out[k] = {
            q.id.split(":", 1)[0] + ":" + str(q.diameter): (q.default, q.attention, q.basis)
            for q in api.questions(api.analyse(stack, k), {})
            if q.id.startswith(("hole.function:", "diameter.fit:"))
        }
    return out


def test_a_screw_through_a_clearance_hole_into_a_tap_drill(defaults):
    tapped, attention, basis = defaults[BASE]["hole.function:4.2"]
    assert (tapped, attention) == ("tapped:M5", "check")
    assert basis == (
        "It lines up with the ⌀5.5 M5 clearance hole in cover: "
        "a screw through that threads in here."
    )
    clearance, attention, basis = defaults[COVER]["hole.function:5.5"]
    assert (clearance, attention) == ("clearance:M5", "check")
    assert "M5 tap drill in base" in basis


def test_a_pin_in_its_bores_is_a_fit_on_both(defaults):
    for part in (BASE, COVER):
        fit, attention, basis = defaults[part]["hole.function:8.0"]
        assert (fit, attention) == ("fit:H7", "check")
        assert basis.startswith("The ⌀8 shaft of pin fits in it")
    shaft, attention, basis = defaults[PIN]["diameter.fit:8.0"]
    assert (shaft, attention) == ("h6", "check")
    # The head touches nothing coaxially: no suggestion.
    assert defaults[PIN]["diameter.fit:10.0"][0] == "general"


def test_a_suggested_default_reaches_the_intent(stack):
    analysis = api.analyse(stack, BASE)
    intent = api.apply(analysis, {"part.material": "steel"}, accept_defaults=True)
    fits = {r["fit"] for r in intent.requirements if r["kind"] == "size"}
    assert "H7" in fits


def test_an_answer_given_is_kept_over_a_suggestion(stack):
    analysis = api.analyse(stack, BASE)
    bore = next(q.id for q in api.questions(analysis, {}) if q.diameter == 8.0)
    (q,) = [q for q in api.questions(analysis, {bore: "general"}) if q.id == bore]
    assert q.default == "fit:H7"  # still offered, but the answer stands
    intent = api.apply(analysis, {"part.material": "steel", bore: "general"}, accept_defaults=True)
    assert not any(r.get("question") == bore and r["kind"] == "size" for r in intent.requirements)


def test_a_part_alone_has_no_mates(plate):
    assert "mates" not in api.analyse(plate)


def test_mates_are_by_face_and_disagreement_suggests_nothing(stack):
    parts = load_all(stack, gdt=False)
    found = mates(parts, PIN)
    assert [s["value"] for s in found.values()] == ["h6"]
    agreed = {"1": {"value": "h6", "reason": "a"}, "2": {"value": "h6", "reason": "b"}}
    assert suggestion(agreed, (1, 2))["value"] == "h6"
    assert suggestion(agreed | {"3": {"value": "g6", "reason": "c"}}, (1, 2, 3)) is None
    assert suggestion(agreed, (1, 2, 4)) is None  # one face mates nothing


def test_datum_a_is_the_face_where_the_part_meets_another(stack):
    for part, other in ((COVER, "base"), (BASE, "cover")):
        analysis = api.analyse(stack, part)
        (a,) = [q for q in api.questions(analysis, {}) if q.id == "datum.A"]
        faces = {f["id"]: f for f in analysis["faces"]}
        assert all(faces[i]["contact"]["part"] == other for i in a.default)
        assert a.attention == "check"
        assert a.basis == f"It is where the part meets {other}: what locates it there."
    # The cover's top meets only the pin's head: less contact than its underside.
    cover = api.analyse(stack, COVER)
    (a,) = [q for q in api.questions(cover, {}) if q.id == "datum.A"]
    assert {f["id"]: f for f in cover["faces"]}[a.default[0]]["direction"][2] < 0


def test_a_part_alone_has_no_contacts(plate):
    assert not any("contact" in f for f in api.analyse(plate)["faces"])
