"""Write the STEP files specify-core's PMI is checked against other tools with.

Each is a part enriched by specify-core, beside what a CAD system wrote for the
same part where there is one, so a checker's verdict on ours can be read
against its verdict on theirs:

    pin.step           turned pin: external thread, knurl, tapped hole, A|B
    plate.step         plate: clearance pattern, H7 bore, positions on A|B|C
    ctc03-pmi.step     NIST CTC-03 geometry, given NIST's datums and positions
    ctc03-nist.step    NIST's own AP242 CTC-03, for comparison
    ctc01-merged.step  NIST CTC-01 with its PMI kept and material added
    ctc01-nist.step    NIST's own AP242 CTC-01, for comparison

    python tools/conformance/make_corpus.py NIST_DIR OUT_DIR

NIST_DIR holds NIST's PMI test files, at any depth (Release/NIST-PMI-STEP-Files.zip in
usnistgov/SFA), which are not redistributed here.
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

from build123d import Align, Box, BuildPart, Cylinder, GridLocations, Locations, Mode, export_step

from specify_core.api import analyse, apply, write
from specify_core.rules import _bores, questions

INCH = 25.4


def pin(path: Path) -> Path:
    with BuildPart() as bp:
        Cylinder(3, 12, align=(Align.CENTER, Align.CENTER, Align.MIN))
        with Locations((0, 0, 12)):
            Cylinder(5, 20, align=(Align.CENTER, Align.CENTER, Align.MIN))
        with Locations((0, 0, 32)):
            Cylinder(2.1, 8, align=(Align.CENTER, Align.CENTER, Align.MAX), mode=Mode.SUBTRACT)
    export_step(bp.part, str(path))
    return path


def plate(path: Path) -> Path:
    with BuildPart() as bp:
        Box(100, 60, 10)
        with GridLocations(30, 30, 3, 2):
            Cylinder(3.3, 10, mode=Mode.SUBTRACT)
        with Locations((0, 0, 0)):
            Cylinder(4, 10, mode=Mode.SUBTRACT)
        with Locations((45, 0, 0)):
            Cylinder(2.1, 10, mode=Mode.SUBTRACT)
    export_step(bp.part, str(path))
    return path


def _asked(analysis, text: str) -> str:
    """The id of the first hole or turned-diameter question whose prompt has ``text``."""
    return next(
        q.id
        for q in questions(analysis, {})
        if q.id.startswith(("hole.function", "diameter.fit")) and text in q.prompt
    )


def _bore(analysis, hole_id: str) -> list[int]:
    faces = {f["id"]: f for f in analysis["faces"]}
    hole = next(f for f in analysis["features"] if f["id"] == hole_id)
    return list(_bores(hole["faces"], hole["record"]["diameter"], faces))


def find(nist: Path, name: str) -> Path:
    """A NIST file by name, wherever the download put it (the geometry-only
    files are in a subfolder of the zip)."""
    found = next(nist.rglob(name), None)
    if found is None:
        raise FileNotFoundError(f"{name} not under {nist}")
    return found


def enrich(source: Path, out: Path, answers) -> None:
    analysis = analyse(source)
    if callable(answers):
        answers = answers(analysis)
    report = write(source, apply(analysis, answers, accept_defaults=True), out)
    print(f"{out.name}: {len(report.written)} written, {len(report.warnings)} warnings")
    for line in report.not_written + report.warnings:
        print(f"  {line}")


def main(nist: Path, out: Path) -> None:
    out.mkdir(parents=True, exist_ok=True)
    work = out / "geometry"
    work.mkdir(exist_ok=True)

    enrich(
        pin(work / "pin.step"),
        out / "pin.step",
        lambda a: {
            "part.material": "CZ121",
            "part.general_tolerance": "ISO 2768-f",
            _asked(a, "Ø6"): "thread:M6",
            _asked(a, "Ø10"): "knurl:diamond",
        },
    )
    enrich(
        plate(work / "plate.step"),
        out / "plate.step",
        lambda a: {"part.material": "Aluminium 6082-T6", _asked(a, "Ø8"): "fit:H7"},
    )

    pair, switches = "hole_patterns.rectangle:58.59.60.61", "hole_patterns.linear:72.73.74.75"
    enrich(
        find(nist, "nist_ctc_03_asme1_rc.stp"),
        out / "ctc03-pmi.step",
        lambda a: {
            "part.material": "6061-T6",
            "datum.A": [18, 28],
            "datum.B": _bore(a, "holes:58"),
            "datum.C": _bore(a, "holes:60"),
            "datum.D": [7],
            "datum.B.control": f"{0.01 * INCH:g}",
            "datum.C.control": f"{0.02 * INCH:g}",
            f"hole.function:{pair}": "fit:H11",
            f"hole.position:{pair}": f"{0.05 * INCH:g}",
            f"hole.function:{switches}": "fit:H11",
            f"hole.frame:{switches}": "D|B|C",
            f"hole.position:{switches}": f"{0.05 * INCH:g}",
            "hole.function:holes:70": "fit:H11",
            "hole.frame:holes:70": "D|B|C",
            "hole.position:holes:70": f"{0.08 * INCH:g}",
            "hole.function:holes:76": "general",
            "hole.function:hole_patterns.rectangle:77.78.79.80": "general",
        },
    )
    shutil.copy(find(nist, "nist_ctc_03_asme1_ap242-e2.stp"), out / "ctc03-nist.step")

    enrich(
        find(nist, "nist_ctc_01_asme1_ap242-e1.stp"),
        out / "ctc01-merged.step",
        {"part.material": "CZ121"},
    )
    shutil.copy(find(nist, "nist_ctc_01_asme1_ap242-e1.stp"), out / "ctc01-nist.step")
    shutil.rmtree(work)


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]))
