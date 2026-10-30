"""Datums as face sets or holes, datum controls, and a frame per tolerance.

CTC-03 is answered as NIST's drawing specifies it (nist_ctc_03_asme1_rc.pdf):
A is two coplanar flange faces, B and C are holes, D is the web, and the
switch-mounting holes are located from D|B|C. Values are NIST's in inches;
sizes use H11 until ± tolerances exist.
"""
import json
import pytest
from specify_core.api import analyse
from specify_core.rules import questions
INCH = 25.4


@pytest.fixture(scope="module")
def ctc03_analysis(ctc03):
    return analyse(ctc03)


def test_a_bad_datum_answer_is_a_clear_error(ctc03_analysis):
    with pytest.raises(ValueError, match="datum A: face 99999 does not exist"):
        questions(ctc03_analysis, {"datum.A": [99999]})
    with pytest.raises(ValueError, match="datum A: .* cannot form one datum"):
        questions(ctc03_analysis, {"datum.A": [18, 7]})


def test_a_datum_bore_on_a_turned_part_keeps_its_fit(spool):
    """The spool's default datum B is its bore; a bore that is a datum is often a
    bearing seat, so its fit is still asked for, only its position is not."""
    analysis = analyse(spool)
    ids = {q.id for q in questions(analysis, {})}
    assert "hole.function:holes:34.55" in ids
    assert "hole.position:holes:34.55" not in ids
    assert "datum.B.control" in ids


def test_datums_are_proposed_only_when_something_is_measured_from_them(plate):
    analysis = analyse(plate)
    qs = questions(analysis, {})
    assert {"datum.A", "datum.B", "datum.C"} <= {q.id for q in qs}
    # Asked after the features they serve.
    ids = [q.id for q in qs]
    assert ids.index("datum.A") > max(i for i, x in enumerate(ids) if x.startswith("hole."))
    plain = {q.id: "general" for q in qs if q.id.startswith("hole.function")}
    assert not any(q.id.startswith("datum.") for q in questions(analysis, plain))


def test_a_datum_given_or_removed_is_always_asked(plate):
    analysis = analyse(plate)
    plain = {q.id: "general" for q in questions(analysis, {}) if q.id.startswith("hole.function")}
    top = max(
        (f for f in analysis["faces"] if f["kind"] == "plane"), key=lambda f: f["centroid"][2]
    )
    added = {q.id: q for q in questions(analysis, {**plain, "datum.A": [top["id"]]})}
    assert added["datum.A"].default == () and "datum.B" not in added
    removed = {q.id for q in questions(analysis, {"datum.C": []})}
    assert "datum.C" in removed
    positions = [
        q for q in questions(analysis, {"datum.C": []}) if q.id.startswith("hole.position")
    ]
    assert positions and all(q.frame == "A|B" for q in positions)
