import pytest

from specify_core.standards import CLEARANCE, TAP_DRILL, fit_limits, match_size


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
