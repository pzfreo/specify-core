"""Location dimensions: basic from a position's datum planes, ± from the edges."""
from pathlib import Path
import pytest
from specify_core.api import analyse, apply
from specify_core.load import load
from specify_core.rules import LOCATING, questions


@pytest.fixture(scope="module")
def plate_analysis(plate):
    return analyse(plate)


def _locations(intent):
    return [r for r in intent.requirements if r["kind"] == "location"]


def test_general_writes_no_location(plate_analysis):
    answers = {"part.locating": LOCATING[1], "part.material": "steel"}
    assert _locations(apply(plate_analysis, answers, accept_defaults=True)) == []
