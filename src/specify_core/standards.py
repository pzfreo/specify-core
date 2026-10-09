"""Standards knowledge: ISO 286 fits and metric bolt sizes.

Only what the v1 rules use. Values are ISO 286-1/-2 tabulated values in
micrometres for nominal sizes up to 3150 mm; each size range is "over the lower
bound, up to and including the upper".
"""

from __future__ import annotations

import bisect
import math
import re

#: Upper bounds of the ISO 286 nominal size ranges, in mm: ISO 286-1:2010
#: Table 1's main ranges. Above 500 mm Tables 2 and 3 split each range in two
#: (500-560 and 560-630, ...), but every deviation tabulated here is the same in
#: both halves, so the main ranges suffice.
# fmt: off
_RANGES = (3, 6, 10, 18, 30, 50, 80, 120, 180, 250, 315, 400, 500,
           630, 800, 1000, 1250, 1600, 2000, 2500, 3150)
# fmt: on

#: Standard tolerance grades, micrometres, per size range: ISO 286-1:2010 Table 1
#: (500 to 3150 mm cross-checked with the h6, k6, m6, g6 and f7 limits of
#: ISO 286-2:2010 Table 17).
# fmt: off
_IT = {
    5: (4, 5, 6, 8, 9, 11, 13, 15, 18, 20, 23, 25, 27,  # up to 500 mm
        32, 36, 40, 47, 55, 65, 78, 96),  # 500 to 3150 mm
    6: (6, 8, 9, 11, 13, 16, 19, 22, 25, 29, 32, 36, 40,  # up to 500 mm
        44, 50, 56, 66, 78, 92, 110, 135),  # 500 to 3150 mm
    7: (10, 12, 15, 18, 21, 25, 30, 35, 40, 46, 52, 57, 63,  # up to 500 mm
        70, 80, 90, 105, 125, 150, 175, 210),  # 500 to 3150 mm
    8: (14, 18, 22, 27, 33, 39, 46, 54, 63, 72, 81, 89, 97,  # up to 500 mm
        110, 125, 140, 165, 195, 230, 280, 330),  # 500 to 3150 mm
    9: (25, 30, 36, 43, 52, 62, 74, 87, 100, 115, 130, 140, 155,  # up to 500 mm
        175, 200, 230, 260, 310, 370, 440, 540),  # 500 to 3150 mm
    10: (40, 48, 58, 70, 84, 100, 120, 140, 160, 185, 210, 230, 250,  # up to 500 mm
         280, 320, 360, 420, 500, 600, 700, 860),  # 500 to 3150 mm
    11: (60, 75, 90, 110, 130, 160, 190, 220, 250, 290, 320, 360, 400,  # up to 500 mm
         440, 500, 560, 660, 780, 920, 1100, 1350),  # 500 to 3150 mm
}
# fmt: on

#: Shaft fundamental deviations, micrometres. f, g, h give the upper deviation
#: (es): ISO 286-1:2010 Table 2; k, m, n, p give the lower (ei): Table 3. k is the
#: value for grades 4 to 7 (above 500 mm it is 0 for every grade).
# fmt: off
_SHAFT = {
    "f": (-6, -10, -13, -16, -20, -25, -30, -36, -43, -50, -56, -62, -68,  # up to 500 mm
          -76, -80, -86, -98, -110, -120, -130, -145),  # 500 to 3150 mm
    "g": (-2, -4, -5, -6, -7, -9, -10, -12, -14, -15, -17, -18, -20,  # up to 500 mm
          -22, -24, -26, -28, -30, -32, -34, -38),  # 500 to 3150 mm
    "h": (0,) * 21,
    "k": (0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4, 5,  # up to 500 mm
          0, 0, 0, 0, 0, 0, 0, 0),  # 500 to 3150 mm
    "m": (2, 4, 6, 7, 8, 9, 11, 13, 15, 17, 20, 21, 23,  # up to 500 mm
          26, 30, 34, 40, 48, 58, 68, 76),  # 500 to 3150 mm
    "n": (4, 8, 10, 12, 15, 17, 20, 23, 27, 31, 34, 37, 40,  # up to 500 mm
          44, 50, 56, 66, 78, 92, 110, 135),  # 500 to 3150 mm
    "p": (6, 12, 15, 18, 22, 26, 32, 37, 43, 50, 56, 62, 68,  # up to 500 mm
          78, 88, 100, 120, 140, 170, 195, 240),  # 500 to 3150 mm
}
# fmt: on
_UPPER_DEVIATION = {"f", "g", "h"}

_FIT = re.compile(r"^(H|JS|js|[fghkmnp])(\d{1,2})$")

#: Hole fits offered in questions, and shaft fits, most common first.
HOLE_FITS = ("H7", "H8", "H9", "H11")
SHAFT_FITS = ("h6", "g6", "f7", "k6", "m6", "n6", "p6", "h7", "js6")

#: ISO 965 tolerance classes for general-purpose metric threads.
INTERNAL_THREAD_CLASS = "6H"
EXTERNAL_THREAD_CLASS = "6g"

#: Knurl patterns as AP242's turned_knurl names them, and a common pitch, mm.
KNURL_PATTERNS = ("straight", "diamond")
KNURL_PITCH = 1.0

#: ISO 273 clearance holes, mm: fine, medium and coarse series. A hole between
#: a bolt's fine and coarse sizes is a plausible clearance hole for it.
CLEARANCE_FINE = {
    "M1.6": 1.7,
    "M2": 2.2,
    "M2.5": 2.7,
    "M3": 3.2,
    "M4": 4.3,
    "M5": 5.3,
    "M6": 6.4,
    "M8": 8.4,
    "M10": 10.5,
    "M12": 13.0,
    "M16": 17.0,
    "M20": 21.0,
    "M24": 25.0,
}
CLEARANCE_COARSE = {
    "M1.6": 2.0,
    "M2": 2.6,
    "M2.5": 3.1,
    "M3": 3.6,
    "M4": 4.8,
    "M5": 5.8,
    "M6": 7.0,
    "M8": 10.0,
    "M10": 12.0,
    "M12": 14.5,
    "M16": 18.5,
    "M20": 24.0,
    "M24": 28.0,
}
#: ISO 273 medium clearance holes and ISO 261 coarse tap drills, mm.
CLEARANCE = {
    "M1.6": 1.8,
    "M2": 2.4,
    "M2.5": 2.9,
    "M3": 3.4,
    "M4": 4.5,
    "M5": 5.5,
    "M6": 6.6,
    "M8": 9.0,
    "M10": 11.0,
    "M12": 13.5,
    "M16": 17.5,
    "M20": 22.0,
    "M24": 26.0,
}
TAP_DRILL = {
    "M1.6": 1.25,
    "M2": 1.6,
    "M2.5": 2.05,
    "M3": 2.5,
    "M4": 3.3,
    "M5": 4.2,
    "M6": 5.0,
    "M8": 6.8,
    "M10": 8.5,
    "M12": 10.2,
    "M16": 14.0,
    "M20": 17.5,
    "M24": 21.0,
}
PITCH = {
    "M1.6": 0.35,
    "M2": 0.4,
    "M2.5": 0.45,
    "M3": 0.5,
    "M4": 0.7,
    "M5": 0.8,
    "M6": 1.0,
    "M8": 1.25,
    "M10": 1.5,
    "M12": 1.75,
    "M16": 2.0,
    "M20": 2.5,
    "M24": 3.0,
}

GENERAL_TOLERANCES = ("ISO 2768-f", "ISO 2768-m", "ISO 2768-c", "ISO 2768-v")


def fit_limits(fit: str, nominal: float) -> tuple[float, float]:
    """Upper and lower deviation in mm for ``fit`` (e.g. ``H7``, ``g6``) at ``nominal``."""
    match = _FIT.match(fit)
    if match is None:
        raise ValueError(f"unsupported fit {fit!r}")
    letter, grade = match.group(1), int(match.group(2))
    if grade not in _IT:
        raise ValueError(f"unsupported grade IT{grade}")
    if not 0 < nominal <= _RANGES[-1]:
        raise ValueError(f"nominal size {nominal} outside 0-3150 mm")
    index = bisect.bisect_left(_RANGES, nominal)
    it = _IT[grade][index]
    if letter == "H":
        upper, lower = it, 0
    elif letter in ("JS", "js"):
        upper, lower = it / 2, -it / 2
    elif letter in _UPPER_DEVIATION:
        upper = _SHAFT[letter][index]
        lower = upper - it
    else:
        if letter == "k" and grade > 7:
            raise ValueError("k deviations are tabulated here for grades 4 to 7 only")
        lower = _SHAFT[letter][index]
        upper = lower + it
    return upper / 1000.0, lower / 1000.0


def match_size(diameter: float, table: dict[str, float], tolerance: float = 0.05) -> str | None:
    """The thread size whose table entry is within ``tolerance`` of ``diameter``."""
    for size, value in table.items():
        if abs(value - diameter) <= tolerance:
            return size
    return None


def clearance_bolts(diameter: float, tolerance: float = 0.05) -> list[str]:
    """Bolts ``diameter`` is a plausible ISO 273 clearance hole for: within the
    fine-to-coarse range of each, nearest its medium size first."""
    fits = [
        size
        for size in CLEARANCE
        if CLEARANCE_FINE[size] - tolerance <= diameter <= CLEARANCE_COARSE[size] + tolerance
    ]
    return sorted(fits, key=lambda size: abs(CLEARANCE[size] - diameter))


def largest_bolt_below(diameter: float) -> str | None:
    """The largest tabulated bolt a hole of ``diameter`` could plausibly clear:
    one it passes, with no more than twice that bolt's coarse-series play. A
    Ø62 bore clears an M24 by 38 mm, which is not a clearance hole."""
    plausible = [
        size
        for size in CLEARANCE
        if 0 < diameter - bolt_diameter(size) <= 2 * (CLEARANCE_COARSE[size] - bolt_diameter(size))
    ]
    return max(plausible, key=bolt_diameter, default=None)


def bolt_diameter(size: str) -> float:
    return float(size.removeprefix("M"))


def full_thread(drill_depth: float, size: str) -> float:
    """Minimum full thread in a blind tapped hole: the drill depth less three
    pitches for the tap's lead and the drill point, rounded down to 0.5 mm."""
    usable = drill_depth - 3 * PITCH[size]
    return max(PITCH[size], math.floor(usable * 2) / 2)


def thread_size(diameter: float, tolerance: float = 0.05) -> str | None:
    """The coarse metric thread whose nominal is ``diameter``: an external thread."""
    return next((size for size in PITCH if abs(bolt_diameter(size) - diameter) <= tolerance), None)
