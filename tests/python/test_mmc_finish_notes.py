"""Clearance holes positioned at MMC, the part's notes, and surface finish."""
import re
import pytest
from specify_core.api import analyse
from specify_core.rules import questions


@pytest.mark.parametrize("value", ["rough", "0", "Ra 99"])
def test_a_finish_that_is_not_a_roughness_is_refused(plate, value):
    analysis = analyse(plate)
    face = analysis["faces"][0]["id"]
    with pytest.raises(ValueError, match="roughness"):
        questions(analysis, {"extra.1": {"faces": [face], "kind": "finish", "value": value}})
