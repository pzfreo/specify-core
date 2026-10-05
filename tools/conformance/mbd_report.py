"""What FreeCAD's MBD workbench makes of each STEP file's semantic PMI.

Its AP242 import preview is plain Python, so no FreeCAD is needed:

    python tools/conformance/mbd_report.py MBD_WORKBENCH_DIR CORPUS_DIR

prints, per file, how many datums, datum systems, dimensions and feature
control frames it would import natively, then what it reports it cannot.
"""

import sys
from pathlib import Path

sys.path.insert(0, sys.argv[1])
from freecad.mbd_workbench import MBDImporter  # noqa: E402

KINDS = ("datums", "datum_systems", "dimensions", "fcfs")

print("| file | " + " | ".join(KINDS) + " |")
print("|---|" + "---|" * len(KINDS))
notes = {}
for path in sorted(Path(sys.argv[2]).glob("*.step")):
    preview = MBDImporter.semantic_import_preview(str(path))
    cells = []
    for kind in KINDS:
        records = preview["candidates"][kind]
        ready = sum(1 for r in records if r.get("can_create_native"))
        cells.append(f"{ready}/{len(records)}")
    print(f"| {path.name} | " + " | ".join(cells) + " |")
    notes[path.name] = preview.get("limitations", [])
for name, limits in notes.items():
    print(f"\n**{name}**\n")
    for line in limits or ["none"]:
        print(f"- {line}")
