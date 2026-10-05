"""Review, not interrogation: which questions need a person at all."""

from collections import Counter

from specify_core.api import analyse
from specify_core.rules import ATTENTION, questions


def _by_attention(analysis):
    qs = questions(analysis, {})
    assert {q.attention for q in qs} <= set(ATTENTION)
    return qs, Counter(q.attention for q in qs)


def test_only_material_must_be_answered_on_a_standard_part(plate):
    """Clearance sizes match standards, so those holes need no check. The
    plate's top and bottom are the same size, though, and which one it is
    mounted on cannot be read from the geometry."""
    qs, _ = _by_attention(analyse(plate))
    assert [q.id for q in qs if q.attention == "required"] == ["part.material"]
    checks = [q.id for q in qs if q.attention == "check"]
    assert "datum.A" in checks and all(
        c == "datum.A" or c.startswith("hole.function") for c in checks
    )


def test_a_hole_assumed_tapped_is_flagged_for_a_person(plate):
    """A thread is claimed from a diameter alone, so it is proposed, not assumed."""
    qs, _ = _by_attention(analyse(plate))
    (tapped,) = [q for q in qs if str(q.default).startswith("tapped:")]
    assert tapped.default == "tapped:M5" and tapped.attention == "check"
    assert tapped.reason.startswith("⌀4.2 is the tap-drill size for M5")


def test_groups_of_unmatched_holes_are_flagged_with_a_reason(spool):
    qs, _ = _by_attention(analyse(spool))
    checks = [q for q in qs if q.attention == "check"]
    # The 6x Ø10 match M8 in ISO 273's coarse series and default to clearance;
    # the two identical 12x Ø8 circles match no bolt series: one check.
    (check,) = checks
    assert "bolt_circle" in check.id and "24× Ø8 holes in 2 identical groups" in check.prompt
    assert "position tolerance" in check.reason
    # A single odd hole -- the spool's bore -- is just a hole.
    assert next(q for q in qs if q.id == "hole.function:holes:34.55").attention == "routine"


def test_tightenings_are_refinements(spool):
    qs, _ = _by_attention(analyse(spool))
    for q in qs:
        if q.id.startswith("diameter.fit:") or q.id.endswith((".control", ".flatness")):
            assert q.attention == "refine", q.id


def test_a_doubtful_primary_datum_is_flagged(ctc03):
    """CTC-03's largest face is not NIST's datum A; another is nearly as large."""
    qs, _ = _by_attention(analyse(ctc03))
    a = next(q for q in qs if q.id == "datum.A")
    assert a.attention == "check" and "nearly as large" in a.reason


def test_attention_is_in_the_json(plate):
    q = questions(analyse(plate), {})[0].to_dict()
    assert q["attention"] == "required"
