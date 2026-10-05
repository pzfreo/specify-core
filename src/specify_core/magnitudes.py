"""Recover geometric tolerance magnitudes that OCCT's reader returns as zero.

Ported from step-pmi-viewer's ``magnitudes.py``. ``GetValue`` on a geometric
tolerance returns 0.0 for every tolerance in the NIST PMI reference files. The
values are in the STEP text, held in a complex entity OCCT's GD&T reader does
not unpack::

    #235=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()
          MEASURE_WITH_UNIT(LENGTH_MEASURE(0.04),#5918)...);

So the file is read again as text, and a tolerance's magnitude is looked up by
the name OCCT also reports. Two things differ from the viewer, which shows the
file's own numbers:

* Values are converted to millimetres through the unit the measure names
  (``#5918`` above is a conversion-based inch), because specify-core works in mm.
* A name carried by more than one tolerance is left out rather than guessed.
"""

from __future__ import annotations

import re
from pathlib import Path

_ENTITY = re.compile(r"#(\d+)\s*=\s*(.*?);(?=\s*#\d+\s*=|\s*ENDSEC)", re.S)
#: Each geometric tolerance names its magnitude as the third argument.
_TOLERANCE = re.compile(r"[A-Z_]*TOLERANCE\(\s*'([^']*)'\s*,\s*'[^']*'\s*,\s*#(\d+)")
_MEASURE = re.compile(r"MEASURE_WITH_UNIT\(\s*LENGTH_MEASURE\(\s*([-+0-9.eE]+)\s*\)\s*,\s*#(\d+)")
_SI = re.compile(r"SI_UNIT\(\s*(\.\w+\.|\$)\s*,\s*\.METRE\.\s*\)")
_CONVERSION = re.compile(r"CONVERSION_BASED_UNIT\(\s*'[^']*'\s*,\s*#(\d+)\s*\)")
_PREFIX_MM = {"$": 1000.0, ".MILLI.": 1.0, ".CENTI.": 10.0, ".DECI.": 100.0, ".MICRO.": 0.001}


def tolerance_magnitudes(path: Path | str) -> dict[str, float]:
    """Tolerance name to magnitude in millimetres, read from the STEP text."""
    text = re.sub(r"\s*\n\s*", "", Path(path).read_text(errors="ignore"))
    entities = {eid: body for eid, body in _ENTITY.findall(text)}
    found: dict[str, list[float | None]] = {}
    for body in entities.values():
        for name, ref in _TOLERANCE.findall(body):
            found.setdefault(name, []).append(_millimetres(ref, entities))
    return {
        name: values[0]
        for name, values in found.items()
        if len(values) == 1 and values[0] is not None
    }


def _millimetres(ref: str, entities: dict[str, str], depth: int = 0) -> float | None:
    """The length measure ``#ref`` in mm, or None if its unit cannot be resolved."""
    match = _MEASURE.search(entities.get(ref, ""))
    if match is None:
        return None
    factor = _unit_mm(match.group(2), entities, depth)
    return None if factor is None else float(match.group(1)) * factor


def _unit_mm(ref: str, entities: dict[str, str], depth: int) -> float | None:
    body = entities.get(ref, "")
    si = _SI.search(body)
    if si:
        return _PREFIX_MM.get(si.group(1))
    conversion = _CONVERSION.search(body)
    if conversion and depth < 4:
        return _millimetres(conversion.group(1), entities, depth + 1)
    return None
