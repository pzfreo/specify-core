"""Fixtures for the Python package's smoke tests: committed parts (tests/fixtures/parts)."""

from pathlib import Path

import pytest

PARTS = Path(__file__).resolve().parents[1] / "fixtures" / "parts"


@pytest.fixture(scope="session")
def plate() -> Path:
    """A 100x60x10 plate: 2x3 grid of M6 clearance holes, one Ø8 bore, one M5 tap drill."""
    return PARTS / "plate.step"


@pytest.fixture(scope="session")
def assembly() -> Path:
    """Two parts: a plate, and a pin placed twice."""
    return PARTS / "assembly.step"
