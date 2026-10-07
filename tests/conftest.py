import os
from pathlib import Path

import pytest
from build123d import (
    Box,
    BuildPart,
    Compound,
    Cylinder,
    GridLocations,
    Locations,
    Mode,
    export_step,
)

#: Quiddity's STEP corpus, which is not redistributed here.
CORPUS = Path(os.environ.get("QUIDDITY_CORPUS", "/app/workspaces/pzfreo/quiddity/tests/corpus"))


def corpus_part(subdir: str, name: str) -> Path:
    path = CORPUS / subdir / name
    if not path.is_file():
        pytest.skip(f"corpus not available at {path}")
    return path


@pytest.fixture(scope="session")
def spool() -> Path:
    """A turned part: bolt circles, a main bore, turned diameters."""
    return corpus_part("cadgenbench", "flanged_spool_132.step")


@pytest.fixture(scope="session")
def ctc03() -> Path:
    """NIST PMI test case 3, geometry only: an inch part stored as an assembly."""
    return corpus_part("nist", "nist_ctc_03_asme1_rc.stp")


@pytest.fixture(scope="session")
def plate(tmp_path_factory) -> Path:
    """A 100x60x10 plate: 2x3 grid of M6 clearance holes, one Ø8 bore, one M5 tap drill."""
    with BuildPart() as bp:
        Box(100, 60, 10)
        with GridLocations(30, 30, 3, 2):
            Cylinder(3.3, 10, mode=Mode.SUBTRACT)
        with Locations((0, 0, 0)):
            Cylinder(4, 10, mode=Mode.SUBTRACT)
        with Locations((45, 0, 0)):
            Cylinder(2.1, 10, mode=Mode.SUBTRACT)
    path = tmp_path_factory.mktemp("parts") / "plate.step"
    export_step(bp.part, str(path))
    return path


@pytest.fixture(scope="session")
def wrapped(tmp_path_factory) -> Path:
    """A plate with one bore, stored as an assembly holding the one solid, as
    the NIST test parts are."""
    with BuildPart() as bp:
        Box(100, 60, 10)
        Cylinder(4, 10, mode=Mode.SUBTRACT)
    bp.part.label = "plate"
    path = tmp_path_factory.mktemp("parts") / "wrapped.step"
    export_step(Compound(children=[bp.part], label="assembly"), str(path))
    return path


@pytest.fixture(scope="session")
def pin(tmp_path_factory) -> Path:
    """A turned pin: Ø6 x 12 (an M6 thread), Ø10 x 20 (a knurl), and a blind
    Ø4.2 x 8 M5 tap drill in its top face. Flat-bottomed: a drill point crashes
    recognition (quiddity#798)."""
    from build123d import Align

    with BuildPart() as bp:
        Cylinder(3, 12, align=(Align.CENTER, Align.CENTER, Align.MIN))
        with Locations((0, 0, 12)):
            Cylinder(5, 20, align=(Align.CENTER, Align.CENTER, Align.MIN))
        with Locations((0, 0, 32)):
            Cylinder(2.1, 8, align=(Align.CENTER, Align.CENTER, Align.MAX), mode=Mode.SUBTRACT)
    path = tmp_path_factory.mktemp("parts") / "pin.step"
    export_step(bp.part, str(path))
    return path


@pytest.fixture(scope="session")
def assembly(tmp_path_factory) -> Path:
    """Two parts: a plate with M6 clearance holes and an M5 tap drill, and a pin
    with a blind M5 tap drill placed twice -- one part, two instances."""
    from build123d import Align, Location

    with BuildPart() as plate:
        Box(100, 60, 10)
        with GridLocations(30, 30, 3, 2):
            Cylinder(3.3, 10, mode=Mode.SUBTRACT)
        with Locations((45, 0, 0)):
            Cylinder(2.1, 10, mode=Mode.SUBTRACT)
    with BuildPart() as pin:
        Cylinder(3, 12, align=(Align.CENTER, Align.CENTER, Align.MIN))
        with Locations((0, 0, 12)):
            Cylinder(5, 20, align=(Align.CENTER, Align.CENTER, Align.MIN))
        with Locations((0, 0, 32)):
            Cylinder(2.1, 8, align=(Align.CENTER, Align.CENTER, Align.MAX), mode=Mode.SUBTRACT)
    plate.part.label = "plate"
    pin.part.label = "pin"
    left = pin.part.moved(Location((-30, 0, 5)))
    right = pin.part.moved(Location((30, 0, 5)))
    path = tmp_path_factory.mktemp("parts") / "assembly.step"
    export_step(Compound(children=[plate.part, left, right], label="assembly"), str(path))
    return path
