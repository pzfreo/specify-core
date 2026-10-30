"""Tapped holes, external threads and knurls as intent."""
import re
import pytest
from specify_core.api import analyse, questions


@pytest.fixture(scope="module")
def pin_analysis(pin):
    return analyse(pin)


def _ids(analysis):
    return {q.id: q for q in questions(analysis, {})}


def _feature(analysis, family, diameter):
    return next(
        f["id"]
        for f in analysis["features"]
        if f["family"] == family and f["record"]["diameter"] == diameter
    )


def test_turned_diameters_offer_a_thread_at_metric_sizes_and_knurls(pin_analysis):
    six = _ids(pin_analysis)[f"diameter.fit:{_feature(pin_analysis, 'turned_steps', 6.0)}"]
    assert "thread:M6" in six.options
    assert {"knurl:straight", "knurl:diamond"} <= set(six.options)
    assert six.prompt == "Is the turned Ø6 diameter a fit, a thread or knurled?"
