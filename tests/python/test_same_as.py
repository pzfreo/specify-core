"""Identical features are asked about once, and each still gets its own spec."""
import pytest
from specify_core.api import analyse
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
