"""specify-core's test fixtures (its tests/conftest.py at cc341dd), for the Rust-backed package.

specify-core builds its synthetic parts with build123d at run time; the Rust-backed package
has no build123d, so they are the parts tools/make_parts.py built from the same code and
committed under tests/fixtures/parts/, the files the oracle was captured from. The corpus
parts are Quiddity's, as in specify-core.
"""

import os
from pathlib import Path

import pytest

#: The committed synthetic parts (tools/make_parts.py).
PARTS = Path(__file__).resolve().parents[1] / "fixtures" / "parts"

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
def plate() -> Path:
    """A 100x60x10 plate: 2x3 grid of M6 clearance holes, one Ø8 bore, one M5 tap drill."""
    return PARTS / "plate.step"


@pytest.fixture(scope="session")
def wrapped() -> Path:
    """A plate with one bore, stored as an assembly holding the one solid, as
    the NIST test parts are."""
    return PARTS / "wrapped.step"


@pytest.fixture(scope="session")
def pin() -> Path:
    """A turned pin: Ø6 x 12 (an M6 thread), Ø10 x 20 (a knurl), and a blind
    Ø4.2 x 8 M5 tap drill in its top face."""
    return PARTS / "pin.step"


@pytest.fixture(scope="session")
def assembly() -> Path:
    """Two parts: a plate with M6 clearance holes and an M5 tap drill, and a pin
    with a blind M5 tap drill placed twice -- one part, two instances."""
    return PARTS / "assembly.step"


@pytest.fixture(scope="session")
def stack() -> Path:
    """A cover screwed to a base and pinned: the base has four M5 tap drills and
    an Ø8 bore, the cover four M5 clearance holes over them and an Ø8 hole, and
    a headed pin, Ø8 through both, has its Ø10 head on the cover."""
    return PARTS / "stack.step"


#: Tests that fail on the Rust engine because of a recorded difference: test -> the ledger
#: entry (tests/fixtures/known_differences/) that explains it. Each is a strict xfail, so a
#: test that starts passing is reported.
KNOWN = {
    "test_datums.py::test_datums_are_proposed_only_when_something_is_measured_from_them": "rules.json, "
    "'measurement datums are proposed before dimensions, including in ± mode' (draftwright-rust "
    "#122): datums are proposed when no feature needs a geometric tolerance",
    "test_datums.py::test_a_datum_given_or_removed_is_always_asked": "rules.json, 'measurement "
    "datums are proposed before dimensions, including in ± mode' (draftwright-rust #122): A, B "
    "and C keep their defaults when no feature needs a geometric tolerance",
    "test_locating_problems_extras.py::"
    "test_an_added_orientation_brings_datums_where_nothing_else_needs_them": "rules.json, "
    "'measurement datums are proposed before dimensions, including in ± mode' (draftwright-rust "
    "#122): datums are proposed before any orientation needs them",
    "test_locating_problems_extras.py::test_only_the_datums_added_orientations_cite_are_proposed": (
        "rules.json, 'measurement datums are proposed before dimensions, including in ± mode' "
        "(draftwright-rust #122): in ± mode A, B and C are proposed, not only the cited B"
    ),
}


def pytest_collection_modifyitems(items):
    for item in items:
        name = f"{item.path.name}::{item.originalname}"
        if name in KNOWN:
            item.add_marker(pytest.mark.xfail(reason=KNOWN[name], strict=True))
