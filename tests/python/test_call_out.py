"""Calling out a dimension: a size or a distance a person tolerances themselves,
in place of what the general tolerance would have said of it."""
import pytest
from specify_core.api import analyse
from specify_core.rules import questions
MATERIAL = {"part.material": "steel"}


@pytest.fixture(scope="module")
def plate_analysis(plate):
    return analyse(plate)


def _planes(analysis):
    planes = [f for f in analysis["faces"] if f["kind"] == "plane"]
    z = [f for f in planes if abs(abs(f["direction"][2]) - 1) < 1e-6]
    top = max(z, key=lambda f: f["centroid"][2])
    bottom = min(z, key=lambda f: f["centroid"][2])
    side = next(f for f in planes if abs(f["direction"][2]) < 1e-6)
    return top["id"], bottom["id"], side["id"]


@pytest.mark.parametrize(
    ("extra", "reason"),
    [
        ({"faces": "TOP", "kind": "size", "value": "±0.1"}, "one diameter"),
        ({"faces": "BORE", "kind": "size", "value": "0.1 or so"}, "give ±0.05"),
        ({"faces": "BORE", "kind": "size", "value": "+0/-0"}, "must differ"),
        ({"faces": "TOP", "kind": "distance", "value": "±0.1"}, "from flat faces"),
        ({"faces": "TOP", "reference": "TOP", "kind": "distance", "value": "±0.1"}, "itself"),
        ({"faces": "SIDE", "reference": "BOTTOM", "kind": "distance", "value": "±0.1"}, "parallel"),
        ({"faces": "TOP", "reference": "BOTTOM", "kind": "distance", "value": "H7"}, "give ±0.05"),
        ({"faces": "BORE", "reference": "TOP", "kind": "distance", "value": "±0.1"}, "along"),
        # Limits typed as limits, not deviations, are refused rather than misread.
        ({"faces": "BORE", "kind": "size", "value": "6.58/6.62"}, "deviations from 6.6"),
        (
            {"faces": "TOP", "reference": "BOTTOM", "kind": "distance", "value": "9.9-10.1"},
            "deviations",
        ),
        (
            {"faces": "TOP", "reference": "BOTTOM", "kind": "distance", "value": "0.1-0.05"},
            "deviations",
        ),
        ({"faces": "TOP", "reference": "BOTTOM", "kind": "distance", "value": "+3/-3"}, "too big"),
        (
            {"faces": "TOP", "reference": "12", "kind": "distance", "value": "±0.1"},
            "from flat faces",
        ),
    ],
)
def test_a_call_out_that_does_not_suit_its_faces_is_refused(plate_analysis, extra, reason):
    top, bottom, side = _planes(plate_analysis)
    bore = next(f["id"] for f in plate_analysis["faces"] if f["kind"] == "cylinder")
    named = {"TOP": [top], "BOTTOM": [bottom], "SIDE": [side], "BORE": [bore]}
    extra = {k: named.get(v, v) if k in ("faces", "reference") else v for k, v in extra.items()}
    with pytest.raises(ValueError, match=reason):
        questions(plate_analysis, {"extra.1": extra})
