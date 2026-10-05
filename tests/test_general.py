"""Dimensions under the general tolerance: found and listed, never written."""

from __future__ import annotations

import pytest

from specify_core import api, general

MATERIAL = {"part.material": "Aluminium 6082-T6"}


@pytest.mark.parametrize(
    ("cls", "nominal", "expected"),
    [
        ("ISO 2768-m", 4, 0.1),
        ("ISO 2768-m", 24, 0.2),
        ("ISO 2768-m", 52, 0.3),
        ("ISO 2768-f", 30, 0.1),
        ("ISO 2768-c", 120, 0.8),
        ("ISO 2768-v", 2, None),
        ("ISO 2768-m", 0.3, None),
        ("ISO 2768-f", 3000, None),
    ],
)
def test_iso_2768_linear(cls, nominal, expected):
    assert general.deviation(cls, nominal) == expected


def _general(step):
    analysis = api.analyse(step)
    intent = api.apply(analysis, MATERIAL, accept_defaults=True)
    return analysis, intent


def test_pin_lengths_are_baseline_from_datum_a(pin):
    analysis, intent = _general(pin)
    faces = {f["id"]: f for f in analysis["faces"]}
    a = next(d for d in intent.datums if d["letter"] == "A")
    lengths = [g for g in intent.general if g["kind"] == "location" and not g["feature"]]
    assert lengths, intent.general
    assert all(g["reference"] == a["faces"] for g in lengths)
    # The tap drill's floor is the hole's, not a length of the part.
    assert sorted(g["distance"] for g in lengths) == [20.0, 32.0]
    for g in lengths:
        assert g["general"] and g["upper"] == -g["lower"] == general.deviation(
            "ISO 2768-m", g["distance"]
        )
        assert all(faces[i]["kind"] == "plane" for i in g["faces"])


def test_a_diameter_sized_otherwise_is_not_listed_again(plate):
    _, intent = _general(plate)
    sized = {i for r in intent.requirements if r["kind"] == "size" for i in r["faces"]}
    listed = {i for g in intent.general if g["kind"] == "size" for i in g["faces"]}
    assert sized and not sized & listed
    # Holes positioned from datums have basic locations; none are added.
    positioned = {r["feature"] for r in intent.requirements if r["kind"] == "location"}
    added = {g["feature"] for g in intent.general if g["kind"] == "location" and g["feature"]}
    assert positioned and not positioned & added
    outline = sorted(
        g["distance"] for g in intent.general if g["kind"] == "location" and not g["feature"]
    )
    assert outline == [10.0, 60.0, 100.0]


def test_listed_but_not_written(pin, tmp_path):
    analysis, intent = _general(pin)
    assert intent.general
    assert not any(r.get("general") for r in intent.requirements)
    report = api.write(pin, intent, tmp_path / "pin-pmi.step")
    assert not any(line.startswith("location") for line in report.written)
    assert api.rules.Intent.from_dict(intent.to_dict()).general == intent.general
