"""#126: an answer whose question is not asked is listed in not_written; unknown IDs are refused."""
import pytest
from specify_pmi import api

from written import write_answers

UNKNOWN = "Invalid answers: bogus.question: unknown answer ID"


def answers(locating):
    return {
        "part.material": "6082-T6",
        "part.locating": locating,
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": "0.05",
        "hole.position:holes:12": "0.1",
        "hole.location:holes:12": "±0.1",
    }


def written(plate, tmp_path, given):
    return write_answers(plate, given, tmp_path / "out.step", accept_defaults=True)


def test_flatness_under_both_ways_of_locating(plate, tmp_path):
    position = answers("position tolerances")
    with pytest.raises(ValueError, match="hole.location:holes:12"):
        written(plate, tmp_path, position)
    position.pop("hole.location:holes:12")
    assert "flatness 0.05 on [2]" in written(plate, tmp_path, position).written

    dimensions = answers("± dimensions")
    with pytest.raises(ValueError, match="datum.A.flatness") as refused:
        written(plate, tmp_path, dimensions)
    assert "hole.position:holes:12" in str(refused.value)
    dimensions.pop("datum.A.flatness")
    dimensions.pop("hole.position:holes:12")
    assert not [line for line in written(plate, tmp_path, dimensions).written if "flatness" in line]


def test_an_unknown_id_is_refused(plate, tmp_path):
    analysis = api.analyse(plate)
    given = {**answers("position tolerances"), "bogus.question": "x"}
    for call in (
        lambda: api.questions(analysis, given),
        lambda: api.apply(analysis, given, accept_defaults=True),
        lambda: api.preview(analysis, given),
    ):
        with pytest.raises(ValueError, match=UNKNOWN):
            call()
