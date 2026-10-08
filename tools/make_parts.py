"""Write the synthetic STEP parts specify-core's tests build, as committed fixtures.

specify-core's tests (tests/conftest.py) build their parts with build123d at run time. The Rust
port cannot, so they are built once here, in specify-core's own environment, and committed under
tests/fixtures/parts/. The STEP header carries a timestamp, so running this again changes every
file's bytes (and so every oracle binding). So a file that already exists is never written
again: the committed parts keep their bytes, and only parts added here are written. To rebuild
one deliberately, delete it and re-run the oracle capture for it.

    /Users/paul/repos/specify-core/.venv/bin/python tools/make_parts.py tests/fixtures/parts

The first five parts are exactly the conftest fixtures of the same name; the rest are the parts
individual tests build inline, each named after its test (without ``test_``) and citing it.
Keep them in step.
"""

from __future__ import annotations

import sys
from pathlib import Path

from build123d import (
    Align,
    Box,
    BuildPart,
    Compound,
    Cylinder,
    GridLocations,
    Location,
    Locations,
    Mode,
    export_step,
)


def plate() -> object:
    """A 100x60x10 plate: 2x3 grid of M6 clearance holes, one Ø8 bore, one M5 tap drill."""
    with BuildPart() as bp:
        Box(100, 60, 10)
        with GridLocations(30, 30, 3, 2):
            Cylinder(3.3, 10, mode=Mode.SUBTRACT)
        with Locations((0, 0, 0)):
            Cylinder(4, 10, mode=Mode.SUBTRACT)
        with Locations((45, 0, 0)):
            Cylinder(2.1, 10, mode=Mode.SUBTRACT)
    return bp.part


def wrapped() -> object:
    """A plate with one bore, stored as an assembly holding the one solid (as NIST's are)."""
    with BuildPart() as bp:
        Box(100, 60, 10)
        Cylinder(4, 10, mode=Mode.SUBTRACT)
    bp.part.label = "plate"
    return Compound(children=[bp.part], label="assembly")


def _pin() -> object:
    with BuildPart() as bp:
        Cylinder(3, 12, align=(Align.CENTER, Align.CENTER, Align.MIN))
        with Locations((0, 0, 12)):
            Cylinder(5, 20, align=(Align.CENTER, Align.CENTER, Align.MIN))
        with Locations((0, 0, 32)):
            Cylinder(2.1, 8, align=(Align.CENTER, Align.CENTER, Align.MAX), mode=Mode.SUBTRACT)
    return bp.part


def pin() -> object:
    """A turned pin: Ø6 x 12 (an M6 thread), Ø10 x 20 (a knurl), a blind Ø4.2 x 8 M5 tap drill."""
    return _pin()


def assembly() -> object:
    """A plate with M6 clearance holes and an M5 tap drill, and a pin placed twice."""
    with BuildPart() as bp:
        Box(100, 60, 10)
        with GridLocations(30, 30, 3, 2):
            Cylinder(3.3, 10, mode=Mode.SUBTRACT)
        with Locations((45, 0, 0)):
            Cylinder(2.1, 10, mode=Mode.SUBTRACT)
    plate_part = bp.part
    pin_part = _pin()
    plate_part.label = "plate"
    pin_part.label = "pin"
    left = pin_part.moved(Location((-30, 0, 5)))
    right = pin_part.moved(Location((30, 0, 5)))
    return Compound(children=[plate_part, left, right], label="assembly")


def stack() -> object:
    """A cover screwed to a base and pinned (four M5 tap drills under four M5 clearance holes,
    an Ø8 bore in both, a headed Ø8 pin through them)."""
    with BuildPart() as base:
        Box(60, 40, 10)
        with GridLocations(40, 20, 2, 2):
            Cylinder(2.1, 10, mode=Mode.SUBTRACT)
        Cylinder(4, 10, mode=Mode.SUBTRACT)
    with BuildPart() as cover:
        Box(60, 40, 6)
        with GridLocations(40, 20, 2, 2):
            Cylinder(2.75, 6, mode=Mode.SUBTRACT)
        Cylinder(4, 6, mode=Mode.SUBTRACT)
    with BuildPart() as headed:
        Cylinder(4, 16, align=(Align.CENTER, Align.CENTER, Align.MIN))
        with Locations((0, 0, 16)):
            Cylinder(5, 3, align=(Align.CENTER, Align.CENTER, Align.MIN))
    base.part.label = "base"
    cover.part.label = "cover"
    headed.part.label = "pin"
    parts = [
        base.part,
        cover.part.moved(Location((0, 0, 8))),
        headed.part.moved(Location((0, 0, -5))),
    ]
    return Compound(children=parts, label="stack")


# Parts built inline by individual tests (grep BuildPart / export_step in specify-core/tests).


def a_hole_along_x_is_labelled_from_its_rim() -> object:
    """tests/test_anchors.py::test_a_hole_along_x_is_labelled_from_its_rim: a Ø6 x 20 rod along
    X with a blind Ø1.6 x 3.8 hole in its end (the test's ``rod.step``)."""
    with BuildPart() as bp:
        Cylinder(3, 20, rotation=(0, 90, 0), align=(Align.CENTER, Align.CENTER, Align.MIN))
        Cylinder(
            0.8,
            3.8,
            rotation=(0, 90, 0),
            align=(Align.CENTER, Align.CENTER, Align.MIN),
            mode=Mode.SUBTRACT,
        )
    return bp.part


def _plate_with_holes(locations: list[tuple[float, float]]) -> object:
    """tests/test_features.py::_plate_with_holes: a 100x60x10 plate with Ø5.5 holes."""
    with BuildPart() as bp:
        Box(100, 60, 10)
        with Locations(*locations):
            Cylinder(2.75, 10, mode=Mode.SUBTRACT)
    return bp.part


def four_corner_holes_are_quiddity_rectangle_with_members() -> object:
    """tests/test_features.py::test_four_corner_holes_are_quiddity_rectangle_with_members: four
    Ø5.5 holes at (±40, ±20)."""
    return _plate_with_holes([(x, y) for x in (-40, 40) for y in (-20, 20)])


def identical_holes_no_pattern_claims_form_a_set() -> object:
    """tests/test_features.py::test_identical_holes_no_pattern_claims_form_a_set: two Ø5.5 holes
    at (-30, 10) and (25, -15)."""
    return _plate_with_holes([(-30, 10), (25, -15)])


def default_datum_a_takes_every_coplanar_face() -> object:
    """tests/test_datums.py fixture ``slotted``, used by
    test_default_datum_a_takes_every_coplanar_face: a plate whose bottom is cut in two by a slot
    across it, with pockets that keep the top smaller than either half."""
    with BuildPart() as bp:
        Box(100, 60, 10)
        with Locations((0, 0, -3.5)):
            Box(4, 60, 3, mode=Mode.SUBTRACT)
        with Locations((-25, 0, 4), (25, 0, 4)):
            Box(40, 45, 2, mode=Mode.SUBTRACT)
    return bp.part


def a_placed_part_is_meshed_in_the_frame_the_analysis_uses() -> object:
    """tests/test_mesh.py::test_a_placed_part_is_meshed_in_the_frame_the_analysis_uses: the
    wrapped plate moved to (100, 200, 300) inside its assembly (the test's ``placed.step``)."""
    with BuildPart() as bp:
        Box(100, 60, 10)
        Cylinder(4, 10, mode=Mode.SUBTRACT)
    part = bp.part.moved(Location((100, 200, 300)))
    part.label = "plate"
    return Compound(children=[part], label="assembly")


def a_file_with_two_parts_must_say_which() -> object:
    """tests/test_load.py::test_a_file_with_two_parts_must_say_which: two unit boxes, unnamed,
    5 apart (the test's ``two.step``)."""
    a, b = Box(1, 1, 1), Box(1, 1, 1).moved(Location((5, 0, 0)))
    return Compound(children=[a, b])


PARTS = {
    "plate": plate,
    "wrapped": wrapped,
    "pin": pin,
    "assembly": assembly,
    "stack": stack,
    "a_hole_along_x_is_labelled_from_its_rim": a_hole_along_x_is_labelled_from_its_rim,
    "four_corner_holes_are_quiddity_rectangle_with_members": (
        four_corner_holes_are_quiddity_rectangle_with_members
    ),
    "identical_holes_no_pattern_claims_form_a_set": identical_holes_no_pattern_claims_form_a_set,
    "default_datum_a_takes_every_coplanar_face": default_datum_a_takes_every_coplanar_face,
    "a_placed_part_is_meshed_in_the_frame_the_analysis_uses": (
        a_placed_part_is_meshed_in_the_frame_the_analysis_uses
    ),
    "a_file_with_two_parts_must_say_which": a_file_with_two_parts_must_say_which,
}


def main(out: str) -> None:
    folder = Path(out)
    folder.mkdir(parents=True, exist_ok=True)
    for name, build in PARTS.items():
        path = folder / f"{name}.step"
        if path.exists():
            # Never rewritten: its bytes are bound into the committed oracle.
            print(f"{path} exists, kept")
            continue
        export_step(build(), str(path))
        print(path)


if __name__ == "__main__":
    main(sys.argv[1])
