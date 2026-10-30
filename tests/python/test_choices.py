"""Choosing by purpose or by code, and typed specs checked against the part."""
import pytest
from specify_core.api import analyse, apply, questions


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


@pytest.mark.parametrize("value", [0.1, 3, True, ["general"], {"value": "general"}, None])
def test_a_choice_answer_that_is_not_text_is_refused(plate, value):
    analysis = analyse(plate)
    qid = next(q.id for q in questions(analysis, {}) if q.id.startswith("hole.position:"))
    # Ported: the engine's refusal is a ValueError, of which specify-core's SpecError is one.
    with pytest.raises(ValueError, match=qid):
        questions(analysis, {qid: value})
