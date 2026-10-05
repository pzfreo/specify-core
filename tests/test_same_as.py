"""Identical features are asked about once, and each still gets its own spec."""

import pytest

from specify_core.api import analyse, apply
from specify_core.rules import questions


@pytest.fixture(scope="module")
def spool_analysis(spool):
    return analyse(spool)


def _merged(analysis, text):
    return next(q for q in questions(analysis, {}) if text in q.prompt)


def test_identical_bolt_circles_are_one_question(spool_analysis):
    q = _merged(spool_analysis, "Ø8 holes")
    assert q.prompt == "What are the 24× Ø8 holes in 2 identical groups for?"
    assert len(q.parts) == 2 and len(q.faces) == 24
    assert q.to_dict()["features"] == [fid for fid, _ in q.parts]
    assert not [x for x in questions(spool_analysis, {}) if "12× Ø8" in x.prompt]


def test_each_group_keeps_its_own_size_and_position(spool_analysis):
    q = _merged(spool_analysis, "Ø8 holes")
    answers = {"part.material": "steel", q.id: "clearance"}
    intent = apply(spool_analysis, answers | _defaults(spool_analysis, answers))
    for fid, bores in q.parts:
        mine = [r for r in intent.requirements if r["feature"] == fid]
        assert sorted(r["kind"] for r in mine) == ["position", "size"]
        assert all(r["faces"] == list(bores) for r in mine)
        assert all(r["question"] in (q.id, q.id.replace("function", "position")) for r in mine)


def test_identical_turned_diameters_are_one_question(spool_analysis):
    q = _merged(spool_analysis, "Ø130")
    assert q.prompt.startswith("Is the turned 2× Ø130")
    answers = {"part.material": "steel", q.id: "h7"}
    intent = apply(spool_analysis, answers | _defaults(spool_analysis, answers))
    fits = [r for r in intent.requirements if r.get("fit") == "h7"]
    assert sorted(r["feature"] for r in fits) == sorted(fid for fid, _ in q.parts)


def test_datum_holes_are_never_merged(ctc03):
    """CTC-03's datums B and C are identical holes, each its own datum."""
    from specify_core.rules import _bores

    analysis = analyse(ctc03)
    faces = {f["id"]: f for f in analysis["faces"]}
    feats = {f["id"]: f for f in analysis["features"]}

    def bore(h):
        return list(_bores(feats[h]["faces"], feats[h]["record"]["diameter"], faces))

    answers = {"datum.A": [18, 28], "datum.B": bore("holes:58"), "datum.C": bore("holes:60")}
    ids = {q.id for q in questions(analysis, answers)}
    assert {"hole.function:holes:58", "hole.function:holes:60"} <= ids


def _defaults(analysis, answers):
    from specify_core.rules import with_all_defaults

    return with_all_defaults(analysis, answers)
