import pytest

from specify_core.api import analyse, questions
from specify_core.choices import describe
from specify_core.standards import (
    CLEARANCE,
    HOLE_FITS,
    SHAFT_FITS,
    TAP_DRILL,
    fit_limits,
    match_size,
)


@pytest.mark.parametrize(
    ("fit", "nominal", "upper", "lower"),
    [
        ("H7", 8, 0.015, 0.0),
        ("H7", 40, 0.025, 0.0),
        ("H7", 47, 0.025, 0.0),
        ("H8", 25, 0.033, 0.0),
        ("g6", 25, -0.007, -0.020),
        ("h6", 20, 0.0, -0.013),
        ("f7", 30, -0.020, -0.041),
        ("k6", 50, 0.018, 0.002),
        ("p6", 20, 0.035, 0.022),
        ("js6", 20, 0.0065, -0.0065),
        # Range boundaries: 10 is in "over 6 up to 10", 10.01 in the next.
        ("H7", 10, 0.015, 0.0),
        ("H7", 10.01, 0.018, 0.0),
    ],
)
def test_fit_limits_match_iso_286(fit, nominal, upper, lower):
    assert fit_limits(fit, nominal) == pytest.approx((upper, lower))


# Above 500 mm: limits from ISO 286-2:2010 Table 17 (shafts) and IT7 of ISO 286-1:2010
# Table 1 (H7), one size in each range to 3150 mm, and a range edge on each side.
@pytest.mark.parametrize(
    ("fit", "nominal", "upper", "lower"),
    [
        ("g6", 600, -22, -66),
        ("h6", 700, 0, -50),
        ("k6", 900, 56, 0),
        ("m6", 1100, 106, 40),
        ("f7", 1300, -110, -235),
        ("g6", 1800, -32, -124),
        ("m6", 2200, 178, 68),
        ("f7", 3000, -145, -355),
        ("k6", 3150, 135, 0),
        ("H7", 500, 63, 0),
        ("H7", 500.01, 70, 0),
        ("H7", 1250, 105, 0),
        ("H7", 1250.01, 125, 0),
        ("p6", 2600, 375, 240),
        ("n6", 600, 88, 44),
        ("js6", 1100, 33, -33),
        ("H11", 2000, 920, 0),
        ("h5", 2600, 0, -96),
    ],
)
def test_fit_limits_match_iso_286_to_3150_mm(fit, nominal, upper, lower):
    assert fit_limits(fit, nominal) == pytest.approx((upper / 1000, lower / 1000))


def test_sizes_beyond_3150_mm_and_k_above_grade_7_are_refused():
    with pytest.raises(ValueError, match="outside 0-3150 mm"):
        fit_limits("H7", 3150.01)
    # k above 500 mm is 0 for every grade, but grades above 7 stay refused as below.
    with pytest.raises(ValueError):
        fit_limits("k8", 600)


def test_a_600_mm_diameter_is_described_with_its_limits():
    # ISO 286-1:2010 Table 1, over 500 up to 630 mm: IT7 70, IT8 110, IT9 175, IT11 440 µm.
    holes = {
        c["value"]: c["callout"]
        for c in describe(
            "hole.function:x", ["general", "fit:H7", "fit:H8", "fit:H9", "fit:H11"], 600.0
        )
    }
    assert holes["fit:H7"] == "⌀600 H7 (600.000–600.070)"
    assert holes["fit:H8"] == "⌀600 H8 (600.000–600.110)"
    assert holes["fit:H9"] == "⌀600 H9 (600.000–600.175)"
    assert holes["fit:H11"] == "⌀600 H11 (600.000–600.440)"
    shafts = describe("diameter.fit:x", [f"fit:{f}" for f in SHAFT_FITS], 600.0)
    assert "⌀600 g6 (599.934–599.978)" in {c["callout"] for c in shafts}


def test_a_600_mm_hole_is_offered_fits(plate):
    analysis = analyse(plate)
    hole = next(
        f
        for f in analysis["features"]
        if f["family"] == "holes" and f["record"].get("diameter") == 8.0
    )
    hole["record"]["diameter"] = 600.0
    question = next(
        q for q in questions(analysis, {}) if q.feature == hole["id"] and "Ø600" in q.prompt
    )
    assert {f"fit:{fit}" for fit in HOLE_FITS} <= set(question.options)
    callouts = {c["value"]: c["callout"] for c in question.to_dict()["choices"]}
    assert callouts["fit:H7"] == "⌀600 H7 (600.000–600.070)"


@pytest.mark.parametrize("fit", ["Z7", "H3", "k8"])
def test_unsupported_fits_are_refused(fit):
    with pytest.raises(ValueError):
        fit_limits(fit, 10)


def test_size_matching():
    assert match_size(6.6, CLEARANCE) == "M6"
    assert match_size(5.0, TAP_DRILL) == "M6"
    assert match_size(7.3, CLEARANCE) is None


def test_clearance_holes_match_any_iso_273_series():
    from specify_core.standards import clearance_bolts

    assert clearance_bolts(6.4) == ["M6"]  # fine
    assert clearance_bolts(6.6) == ["M6"]  # medium
    assert clearance_bolts(10.0) == ["M8"]  # coarse
    assert clearance_bolts(8.0) == []  # between M6 coarse (7.0) and M8 fine (8.4)


def test_an_unstated_bolt_is_the_largest_a_hole_plausibly_clears():
    from specify_core.standards import largest_bolt_below

    assert largest_bolt_below(8.0) == "M6"
    assert largest_bolt_below(62.0) is None  # a bore, not a clearance hole


def test_small_metric_threads_are_known():
    """An M2 tapped hole is drilled Ø1.6; below M3 nothing was offered."""
    from specify_core.standards import clearance_bolts, full_thread, thread_size

    assert match_size(1.6, TAP_DRILL) == "M2"
    assert match_size(2.05, TAP_DRILL) == "M2.5"
    assert match_size(1.25, TAP_DRILL) == "M1.6"
    assert clearance_bolts(2.4) == ["M2"]
    assert thread_size(2.5) == "M2.5"
    assert full_thread(4.0, "M2") == 2.5
