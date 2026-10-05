"""Tapped holes, external threads and knurls as intent."""

import pytest

from specify_core import standards
from specify_core.api import analyse, apply, questions


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


def test_thread_standards():
    assert standards.full_thread(8, "M5") == 5.5  # 8 - 3 x 0.8, down to 0.5
    assert standards.thread_size(6.0) == "M6"
    assert standards.thread_size(62.0) is None


def test_turned_diameters_offer_a_thread_at_metric_sizes_and_knurls(pin_analysis):
    six = _ids(pin_analysis)[f"diameter.fit:{_feature(pin_analysis, 'turned_steps', 6.0)}"]
    assert "thread:M6" in six.options
    assert {"knurl:straight", "knurl:diamond"} <= set(six.options)
    assert six.prompt == "Is the turned Ø6 diameter a fit, a thread or knurled?"


def test_a_tapped_hole_carries_its_thread_and_drill(pin_analysis):
    hole = _feature(pin_analysis, "holes", 4.2)
    assert _ids(pin_analysis)[f"hole.function:{hole}"].default == "tapped:M5"
    intent = apply(pin_analysis, {"part.material": "brass"}, accept_defaults=True)
    (thread,) = [r for r in intent.requirements if r["kind"] == "thread"]
    assert {k: thread[k] for k in ("side", "designation", "pitch", "class", "hand")} == {
        "side": "internal",
        "designation": "M5",
        "pitch": 0.8,
        "class": "6H",
        "hand": "RH",
    }
    assert (thread["drill_diameter"], thread["drill_depth"], thread["full_thread"]) == (
        4.2,
        8.0,
        5.5,
    )
    assert thread["through"] is False


def test_external_thread_and_knurl_from_turned_answers(pin_analysis):
    six = _feature(pin_analysis, "turned_steps", 6.0)
    ten = _feature(pin_analysis, "turned_steps", 10.0)
    answers = {
        "part.material": "brass",
        f"diameter.fit:{six}": "thread:M6",
        f"diameter.fit:{ten}": "knurl:diamond",
    }
    reqs = {
        r["feature"]: r for r in apply(pin_analysis, answers, accept_defaults=True).requirements
    }
    assert {k: reqs[six][k] for k in ("kind", "side", "spec", "class", "length", "nominal")} == {
        "kind": "thread",
        "side": "external",
        "spec": "M6x1",
        "class": "6g",
        "length": 12.0,
        "nominal": 6.0,
    }
    assert {k: reqs[ten][k] for k in ("kind", "pattern", "pitch", "diameter")} == {
        "kind": "knurl",
        "pattern": "diamond",
        "pitch": 1.0,
        "diameter": 10.0,
    }
