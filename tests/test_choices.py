"""Choosing by purpose or by code, and typed specs checked against the part."""

import pytest

from specify_core.api import analyse, apply, questions
from specify_core.choices import SpecError, callout, code, describe, interpret
from specify_core.serve import reply

HOLE = "hole.function:x"
SHAFT = "diameter.fit:x"
HOLE_OPTIONS = ("general", "clearance:M6", "fit:H7", "fit:H8")


def test_each_option_has_a_purpose_a_code_and_a_callout():
    by_value = {c["value"]: c for c in describe(HOLE, HOLE_OPTIONS, 6.6)}
    assert by_value["clearance:M6"] == {
        "value": "clearance:M6",
        "purpose": "clearance",
        "label": "A bolt passes through",
        "code": "M6 clearance",
        "callout": "⌀6.6 H11 (6.600–6.690) for M6",
    }
    assert by_value["fit:H7"]["purpose"] == by_value["fit:H8"]["purpose"] == "fit"
    assert by_value["general"]["label"] == "Nothing in particular"
    assert callout("fit:H7", 8.0, True) == "⌀8 H7 (8.000–8.015)"
    assert callout("tapped:M2", 1.6, True) == "M2×0.4-6H, ⌀1.6 tap drill"
    assert callout("g6", 10.0, False) == "⌀10 g6 (9.986–9.995)"
    assert describe("hole.position:x", ("0.1",), 6.6) == []


@pytest.mark.parametrize(
    ("qid", "text", "d", "value"),
    [
        (HOLE, "H7", 6.6, "fit:H7"),
        (HOLE, "h6", 6.6, "fit:H6"),
        (HOLE, "JS7", 6.6, "fit:JS7"),
        (HOLE, "M6", 6.6, "clearance:M6"),
        (HOLE, "clearance M6", 6.6, "clearance:M6"),
        (HOLE, "M2", 1.6, "tapped:M2"),
        (HOLE, "tapped M2x0.4", 1.6, "tapped:M2"),
        (HOLE, "none", 6.6, "general"),
        (SHAFT, "G6", 10.0, "g6"),
        (SHAFT, "m6", 10.0, "m6"),
        (SHAFT, "M10", 10.0, "thread:M10"),
        (SHAFT, "diamond knurl", 10.0, "knurl:diamond"),
        ("hole.position:x", "⌀0.150", None, "0.15"),
        ("hole.position:x", "none", None, "none"),
        ("part.general_tolerance", "f", None, "ISO 2768-f"),
    ],
)
def test_typed_specs_are_read_as_answers(qid, text, d, value):
    options = ("ISO 2768-f", "ISO 2768-m") if qid.startswith("part.") else ()
    assert interpret(qid, text, options, d) == value


def test_on_a_shaft_capital_m_is_a_thread_and_lower_case_a_fit():
    options = ("general", "h6", "m6", "thread:M6", "knurl:diamond")
    assert interpret(SHAFT, "M6", options, 6.0) == "thread:M6"
    assert interpret(SHAFT, "m6", options, 6.0) == "m6"
    assert interpret(SHAFT, "M6 thread", options, 6.0) == "thread:M6"
    assert interpret(HOLE, "h7", ("fit:H7",), 8.0) == "fit:H7"


@pytest.mark.parametrize(
    ("qid", "text", "d", "reason"),
    [
        (HOLE, "M8 tapped", 1.6, "M8 tap needs a ⌀6.8 drill"),
        (HOLE, "clearance M8", 6.6, "M8 bolt will not pass"),
        (HOLE, "M6 clearance", 12.0, "too loose"),
        (HOLE, "g6", 6.6, "Holes take an H"),
        (HOLE, "M6x0.75", 5.0, "Only coarse pitch"),
        (HOLE, "M7", 6.0, "not a tabulated metric thread"),
        (SHAFT, "M6", 10.0, "M6 thread is ⌀6"),
        (SHAFT, "knurl", 10.0, "straight or a diamond"),
        ("hole.position:x", "-1", None, "more than 0"),
    ],
)
def test_specs_that_do_not_fit_the_part_are_refused(qid, text, d, reason):
    with pytest.raises(SpecError, match=reason):
        interpret(qid, text, (), d)


@pytest.mark.parametrize(
    ("text", "value"),
    [
        ("M2 x 0.4, usable thread depth 2.3 mm", "tapped:M2@2.3"),
        ("M2 tapped, 2 deep", "tapped:M2@2"),
        ("M2x0.4-6H 1.5mm min full thread", "tapped:M2@1.5"),
        ("M2-6H", "tapped:M2"),
        ("tapped:M2@2.3", "tapped:M2@2.3"),
    ],
)
def test_a_tapped_hole_keeps_a_typed_thread_depth(text, value):
    assert interpret(HOLE, text, (), 1.6, 3.5) == value


@pytest.mark.parametrize(
    ("text", "depth", "reason"),
    [
        # Issue 7: 3.15 mm of a 3.5 mm hole was read as plain M2, the depth lost.
        ("M2 x 0.4, usable thread depth 3.15 mm", 3.5, "at most 2.3 mm"),
        ("M2 tapped, 2 deep", None, "blind hole of known depth"),
        ("M2 3.15", 3.5, "3.15 mm is not read"),
        ("M2 tapped 2 deep 3 long", 3.5, "one thread depth"),
        ("M2 2 in deep", 3.5, "Lengths are in mm"),
        ("M2-6G", 3.5, "Only the 6H class"),
    ],
)
def test_a_number_that_cannot_be_kept_is_refused_not_dropped(text, depth, reason):
    with pytest.raises(SpecError, match=reason):
        interpret(HOLE, text, (), 1.6, depth)
    with pytest.raises(SpecError, match="Only a tapped hole takes a depth"):
        interpret(HOLE, "M6 clearance, 10 deep", (), 6.6, 10.0)


def test_a_thread_depth_is_named_in_the_code_and_callout():
    assert code("tapped:M2@2.3") == "M2 tapped, 2.3 mm full thread"
    assert (
        callout("tapped:M2@2.3", 1.6, True) == "M2×0.4-6H, 2.3 mm min full thread, ⌀1.6 tap drill"
    )


def test_a_typed_answer_reaches_the_intent_and_a_wrong_one_is_refused(plate):
    analysis = analyse(plate)
    qs = questions(analysis, {})
    bore = next(q for q in qs if q.id.startswith("hole.function") and "Ø8" in q.prompt)
    assert {c["purpose"] for c in bore.to_dict()["choices"]} >= {"fit", "general"}
    answers = {"part.material": "steel", bore.id: "fit:H6"}
    (size,) = [
        r
        for r in apply(analysis, answers, accept_defaults=True).requirements
        if r.get("question") == bore.id and r["kind"] == "size"
    ]
    assert size["fit"] == "H6"
    with pytest.raises(ValueError, match="M5 tap needs"):
        questions(analysis, {bore.id: "tapped:M5"})


def test_the_serve_child_interprets_what_was_typed(plate):
    analysis = analyse(plate)
    bore = next(
        q for q in questions(analysis, {}) if q.id.startswith("hole.function") and "Ø8" in q.prompt
    )
    ok = reply(
        {
            "id": 1,
            "op": "interpret",
            "analysis": analysis,
            "answers": {},
            "question": bore.id,
            "text": "H7",
        }
    )
    assert ok["result"] == {"value": "fit:H7", "code": "H7", "callout": "⌀8 H7 (8.000–8.015)"}
    bad = reply(
        {
            "id": 2,
            "op": "interpret",
            "analysis": analysis,
            "answers": {},
            "question": bore.id,
            "text": "M12",
        }
    )
    assert bad["error"]["code"] == "invalid" and "M12" in bad["error"]["message"]


@pytest.mark.parametrize("value", [0.1, 3, True, ["general"], {"value": "general"}, None])
def test_a_choice_answer_that_is_not_text_is_refused(plate, value):
    analysis = analyse(plate)
    qid = next(q.id for q in questions(analysis, {}) if q.id.startswith("hole.position:"))
    with pytest.raises(SpecError, match=qid):
        questions(analysis, {qid: value})
