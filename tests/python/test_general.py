"""Dimensions under the general tolerance: found and listed, never written."""
from __future__ import annotations
import pytest
from specify_core import api
MATERIAL = {"part.material": "Aluminium 6082-T6"}


def _general(step):
    analysis = api.analyse(step)
    intent = api.apply(analysis, MATERIAL, accept_defaults=True)
    return analysis, intent


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
