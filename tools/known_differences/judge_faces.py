"""Judge each face area and centroid difference of the features sweep against an independent
reference: OpenCascade's adaptive surface integration (BRepGProp.SurfaceProperties with eps
1e-9), which neither engine's analysis uses (specify-core integrates with OpenCascade's
default, haecceity with its own quadrature).

Run with specify-core's environment (it imports specify_core and OCP):

    PY=/Users/paul/repos/specify-core/.venv/bin/python
    SPECIFY_DUMP=DIR SPECIFY_CORPUS=CORPUS cargo test --release --test features -- faces_and_features
    $PY -I tools/known_differences/judge_faces.py DIR/features.json CORPUS TMP DIR/judge.json

The sweep fails while tests/fixtures/known_differences/features.json is stale, but writes its
dump first. Inputs are read as the oracle did (``meta.json.gz``: kind, input, part); a gzipped
input is decompressed into TMP. The output maps case -> face index -> field (``area``,
``centroid[k]``) -> [python, rust, reference], and each face's OpenCascade surface kind; then
run tools/known_differences/features.py.
"""

from __future__ import annotations

import gzip
import json
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepGProp import BRepGProp
from OCP.GProp import GProp_GProps
from specify_core.load import load

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests/fixtures/oracle"
FIELD = re.compile(r"faces\[(\d+)\]\.(area|centroid\[\d\])")


def main(dump: str, corpus: str, tmp: str, out: str) -> None:
    diffs: dict[str, dict[str, dict[str, list[float]]]] = defaultdict(lambda: defaultdict(dict))
    for d in json.load(open(dump)):
        m = FIELD.fullmatch(d["path"])
        if m and isinstance(d["python"], (int, float)) and isinstance(d["rust"], (int, float)):
            diffs[d["case"]][m.group(1)][m.group(2)] = [d["python"], d["rust"]]
    roots = {"corpus": Path(corpus), "nist": ROOT / "tests/fixtures/nist", "parts": ROOT / "tests/fixtures/parts"}
    result = {}
    for case, faces in sorted(diffs.items()):
        meta = json.load(gzip.open(ORACLE / case / "meta.json.gz"))
        path = roots[meta["kind"]] / meta["input"]
        if path.suffix == ".gz":
            plain = Path(tmp) / path.stem
            plain.write_bytes(gzip.open(path).read())
            path = plain
        loaded = load(str(path), part=meta.get("part"))
        judged = {}
        for index, fields in sorted(faces.items(), key=lambda kv: int(kv[0])):
            face = loaded.face(int(index))
            props = GProp_GProps()
            BRepGProp.SurfaceProperties_s(face, props, 1e-9)
            c = props.CentreOfMass()
            ref = {"area": props.Mass(), "centroid[0]": c.X(), "centroid[1]": c.Y(), "centroid[2]": c.Z()}
            kind = str(BRepAdaptor_Surface(face).GetType()).split("_")[-1]
            judged[index] = {"kind": kind, "fields": {k: [py, rs, ref[k]] for k, (py, rs) in sorted(fields.items())}}
        result[case] = judged
        print(case, len(judged), "faces", flush=True)
    with open(out, "w") as fh:
        json.dump(result, fh, indent=1, sort_keys=True)


if __name__ == "__main__":
    if len(sys.argv) != 5:
        sys.exit(__doc__)
    os.makedirs(sys.argv[3], exist_ok=True)
    main(*sys.argv[1:])
