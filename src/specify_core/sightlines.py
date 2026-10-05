"""Choose the direction a label's leader should leave a face in.

Copied from quid2pmi (pzfreo/quid2pmi, ``src/quid2pmi/sightlines.py`` at
14a811a, Apache-2.0) until it has a shared home; keep the two in step. The
only changes are the vector helpers, inlined below, and fewer samples.

Choose the direction a feature's label should be read from.

A leader is only intuitive if you can see the point it lands on. This module
picks, for each anchor, a direction whose leader leaves the surface through free
space rather than tunnelling through the solid on its way to the label.

The test is whether the leader passes through *material*, not whether it touches
a face. A leader running straight up the wall of a bore lies in that bore's own
cylindrical face the whole way, which a ray/face intersection reports as a hit at
every step but which is perfectly readable. Sampling the solid's point
classification distinguishes the two: points in free space and points lying on a
face are both fine, points strictly inside the material are not.
"""

from __future__ import annotations

from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.gp import gp_Pnt
from OCP.TopAbs import TopAbs_IN, TopAbs_ON
from OCP.TopoDS import TopoDS_Shape

Vec = tuple[float, float, float]


def add(a: Vec, b: Vec) -> Vec:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def scale(a: Vec, k: float) -> Vec:
    return (a[0] * k, a[1] * k, a[2] * k)


def normalise(a: Vec) -> Vec | None:
    length = (a[0] ** 2 + a[1] ** 2 + a[2] ** 2) ** 0.5
    return None if length < 1e-12 else (a[0] / length, a[1] / length, a[2] / length)


#: The six principal directions, used as fallbacks in a fixed order.
PRINCIPAL: tuple[Vec, ...] = (
    (0.0, 0.0, 1.0),
    (0.0, 0.0, -1.0),
    (1.0, 0.0, 0.0),
    (-1.0, 0.0, 0.0),
    (0.0, 1.0, 0.0),
    (0.0, -1.0, 0.0),
)

#: Samples taken along a candidate leader.
SAMPLES = 24


class SightTester:
    """Tests whether a leader from a point escapes the part without entering it."""

    def __init__(self, shape: TopoDS_Shape, reach: float, tolerance: float = 1e-6) -> None:
        self._classifier = BRepClass3d_SolidClassifier()
        self._classifier.Load(shape)
        self._reach = reach
        self._tolerance = tolerance

    def clear_run(self, origin: Vec, direction: Vec) -> float:
        """How far a leader can travel from ``origin`` before entering material."""
        unit = normalise(direction)
        if unit is None:
            return 0.0
        step = self._reach / SAMPLES
        for index in range(1, SAMPLES + 1):
            point = add(origin, scale(unit, index * step))
            self._classifier.Perform(gp_Pnt(*point), self._tolerance)
            if self._classifier.State() == TopAbs_IN:
                return (index - 1) * step
        return self._reach

    def on_surface(self, point: Vec, tolerance: float) -> bool:
        """Whether ``point`` lies on the solid's boundary rather than in thin air."""
        self._classifier.Perform(gp_Pnt(*point), tolerance)
        return self._classifier.State() == TopAbs_ON

    def is_clear(self, origin: Vec, direction: Vec) -> bool:
        return self.clear_run(origin, direction) >= self._reach

    def choose(self, origin: Vec, preferred: list[Vec]) -> tuple[Vec, bool]:
        """The first preferred direction with a clear run, else the least obstructed.

        Returns the direction and whether the run is genuinely clear, so the caller
        can report how many leaders it could not route cleanly.
        """
        candidates: list[Vec] = []
        for direction in (*preferred, *PRINCIPAL):
            unit = normalise(direction)
            if unit is not None and unit not in candidates:
                candidates.append(unit)
        best: Vec = candidates[0]
        best_run = -1.0
        for candidate in candidates:
            run = self.clear_run(origin, candidate)
            if run >= self._reach:
                return candidate, True
            if run > best_run:
                best, best_run = candidate, run
        return best, False
