"""Runout on turned diameters, and profile and runout added on any faces."""
import pytest
from specify_core.api import analyse
from specify_core.rules import questions


def test_a_runout_needs_a_datum_but_a_profile_does_not(plate):
    analysis = analyse(plate)
    face = analysis["faces"][0]["id"]
    with pytest.raises(ValueError, match="which datum"):
        questions(analysis, {"extra.1": {"faces": [face], "kind": "runout", "value": "0.05"}})
    questions(analysis, {"extra.1": {"faces": [face], "kind": "profile", "value": "0.05"}})
