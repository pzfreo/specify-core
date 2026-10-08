"""Write the synthetic STEP parts specify-core's tests build, as committed fixtures.

specify-core's tests (tests/conftest.py) build their parts with build123d at run time. The Rust
port cannot, so they are built once here, in specify-core's own environment, and committed under
tests/fixtures/parts/. The STEP header carries a timestamp, so running this again changes every
file's bytes (and so every oracle binding): re-run it only together with the oracle capture.

    /Users/paul/repos/specify-core/.venv/bin/python tools/make_parts.py tests/fixtures/parts

Each part is exactly the conftest fixture of the same name; keep them in step.
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


PARTS = {"plate": plate, "wrapped": wrapped, "pin": pin, "assembly": assembly, "stack": stack}


def main(out: str) -> None:
    folder = Path(out)
    folder.mkdir(parents=True, exist_ok=True)
    for name, build in PARTS.items():
        export_step(build(), str(folder / f"{name}.step"))
        print(folder / f"{name}.step")


if __name__ == "__main__":
    main(sys.argv[1])
