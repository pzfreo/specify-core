"""Python's ISO 286 fit limits over a grid of sizes to 3150 mm, for tests/standards.rs.

Run with specify-core's venv, from the repository root, naming the specify-core source tree
to load (a checkout of the branch to compare with; default the main checkout):

    /Users/paul/repos/specify-core/.venv/bin/python -I tests/fixtures/iso286_oracle.py \
        [/path/to/specify-core/src] | gzip -n -9 > tests/fixtures/iso286_oracle.json.gz

The grid: every class the rules offer (and the other grades and letters fit_limits takes) at
every range edge to 3150 mm, just inside and outside each edge, the middle of each range, and
beyond 3150 mm. Each result is {"ok": [upper, lower]} or {"err": message, "type": name}.
The first key, "source", records the specify-core commit the values came from. Deterministic.
"""

import json
import subprocess
import sys

SRC = sys.argv[1] if len(sys.argv) > 1 else "/Users/paul/repos/specify-core/src"
sys.path.insert(0, SRC)

from specify_core import standards  # noqa: E402

EDGES = [3, 6, 10, 18, 30, 50, 80, 120, 180, 250, 315, 400, 500,
         630, 800, 1000, 1250, 1600, 2000, 2500, 3150]
LETTERS = ["H", "JS", "js", "f", "g", "h", "k", "m", "n", "p"]
CLASSES = [f"{letter}{grade}" for letter in LETTERS for grade in range(5, 12)]

nominals = {0.5, 3150.01, 3200.0, 5000.0}
lower = 0
for edge in EDGES:
    nominals.update({float(edge), edge - 0.01, edge + 0.01, (lower + edge) / 2})
    lower = edge
nominals.update({600.0, 1300.0})
NOMINALS = sorted(nominals)


def run(fit, nominal):
    try:
        return {"ok": list(standards.fit_limits(fit, nominal))}
    except Exception as exc:  # noqa: BLE001 - the oracle records whatever Python raises
        return {"err": str(exc), "type": type(exc).__name__}


source = subprocess.run(
    ["git", "-C", SRC, "rev-parse", "HEAD"], capture_output=True, text=True, check=False
).stdout.strip() or "unknown"

json.dump(
    {"source": source, "fits": [[fit, n, run(fit, n)] for fit in CLASSES for n in NOMINALS]},
    sys.stdout,
    separators=(",", ":"),
)
sys.stdout.write("\n")
