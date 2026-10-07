"""Write intent into the loaded XCAF document as AP242 semantic PMI, and verify it.

A bare part's intent is added to the document the file was loaded into, so
names and colours are kept. In an assembly, each part's intent is added to
its own part, all in one document and one write; the other parts are kept. A
part that already has PMI is written without it and the new PMI transplanted
into the original file (``merge``). Nothing
graphical is written through OCCT; a drawing is made from the semantics. Each
requirement appended has a leader, and each new datum a datum feature symbol
(``requirements``), for viewers that show only presentation.

OCCT behaviours this module depends on, each found by measurement:

* ``STEPControl_Controller.Init_s()`` must run before the schema is set, or the
  setting is ignored and AP214 without PMI is written (done in ``load``).
* A datum needs ``SetPosition`` or it is not linked into the tolerance's
  datum system.
* The position (primary, secondary, tertiary) is held on the datum label, not
  on the tolerance, so one label per letter cannot be secondary in A|B and in
  D|B|C: D|B|C reads back as B|C|D. Each tolerance gets its own datum labels,
  as OCCT's reader builds them; the file still has one DATUM per letter
  (spikes/datum_frames.py).
* The writer negates the lower tolerance: to write a lower deviation L, pass -L.
* The reader cannot return a tolerance whose deviations are both below nominal
  (g6, f7): it drops the limits and garbles the value (Ø70 g6 reads back as
  35.0145). The file is correct; limits are therefore verified in its text.
* ``SetClassOfTolerance`` writes the grade wrongly (IT7 is written as grade 9),
  so fits are written as limits only; the fit name stays in intent.
* ``CommonLabel`` dimensions segfault the writer after any STEP read in the same
  process, and a presentation with no edges crashes it. Neither is ever written.
* The writer takes the length unit for geometric tolerances from the last
  dimension it wrote. With no dimension in the document it falls back to a
  bare metre while writing the millimetre value unchanged, so every tolerance
  is 1000x too large. Values are then given in metres, which is what the file
  will say (OCCT 7.9, ``STEPCAFControl_Writer::writeDGTsAP242``).
* OCCT cannot write back PMI it has read (magnitudes become 0, angles are
  converted twice, unresolved references become null). PMI is therefore added
  to a part that already has some by ``merge``, never by rewriting the file.
* Every diameter-shaped zone (a position ⌀, a runout) gets a
  ``RUNOUT_ZONE_DEFINITION`` with its orientation missing -- two attributes of
  three, a syntax error -- and an orphan orientation whose angle unit is the
  metre. A runout needs no zone definition, so they are removed
  (``_drop_runout_zones``).
* A size's limits are written as ``MEASURE_WITH_UNIT(0.075,#u)``, a select
  value with no type; and a runout, parallelism or perpendicularity as a
  complex entity, where a checker expects its simple form. Both are rewritten
  (``_type_limits``, ``_simple_tolerances``).
* Angles are radians. (No angular PMI is written in v1.)
* The reader imports a datum only when some tolerance references it, although
  the writer writes it either way.
* The writer keeps one datum per name in the document: in an assembly, a second
  part's datum A would become the first part's. Each part's datums after the
  first are named for their part and renamed in the text (``_part_datums``).
"""

from __future__ import annotations

import os
import re
import shutil
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import OCP.XCAFDimTolObjects as X
from OCP.IFSelect import IFSelect_RetDone
from OCP.Interface import Interface_Static
from OCP.STEPCAFControl import STEPCAFControl_Writer
from OCP.TCollection import TCollection_HAsciiString
from OCP.TDF import TDF_Label, TDF_LabelSequence
from OCP.XCAFDoc import (
    XCAFDoc_Datum,
    XCAFDoc_Dimension,
    XCAFDoc_DocumentTool,
    XCAFDoc_GeomTolerance,
)

from . import locations, merge, p21, requirements, resume
from .existing import read_existing
from .load import LoadedPart, init_step, load, load_all
from .rules import Intent

_GEOM_TYPES = {
    "position": X.XCAFDimTolObjects_GeomToleranceType_Position,
    "flatness": X.XCAFDimTolObjects_GeomToleranceType_Flatness,
    "perpendicularity": X.XCAFDimTolObjects_GeomToleranceType_Perpendicularity,
    "parallelism": X.XCAFDimTolObjects_GeomToleranceType_Parallelism,
    "profile": X.XCAFDimTolObjects_GeomToleranceType_ProfileOfSurface,
    "runout": X.XCAFDimTolObjects_GeomToleranceType_CircularRunout,
    "total_runout": X.XCAFDimTolObjects_GeomToleranceType_TotalRunout,
}
_GEOM_NAMES = {
    "position": "Position",
    "flatness": "Flatness",
    "perpendicularity": "Perpendicularity",
    "parallelism": "Parallelism",
    "profile": "ProfileOfSurface",
    "runout": "CircularRunout",
    "total_runout": "TotalRunout",
}


@dataclass
class WriteReport:
    output: Path
    written: list[str] = field(default_factory=list)
    #: Intent that has no place in the file yet; it must reach the drawing another way.
    not_written: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "output": str(self.output),
            "written": self.written,
            "not_written": self.not_written,
            "warnings": self.warnings,
        }


class VerificationError(RuntimeError):
    pass


#: A part to write: the part, its intent, and the answers it came from (or None).
Job = tuple[LoadedPart, Intent, "dict[str, Any] | None"]


def write(
    loaded: LoadedPart,
    intent: Intent,
    output: str | Path,
    *,
    answers: dict[str, Any] | None = None,
) -> WriteReport:
    """Write ``loaded`` with ``intent`` added as AP242 to ``output``, and verify it.

    A file specify-core wrote itself (``resume``) is written again from its geometry,
    so its PMI is replaced. Given ``answers``, a file written from a bare part
    stores them, to be resumed; one written by ``merge`` does not."""
    return write_parts([(loaded, intent, answers)], output)


def write_parts(jobs: list[Job], output: str | Path) -> WriteReport:
    """Write each part of one file with its intent added, as ``write`` does one.

    An assembly is written from its geometry, every part at once: a bare one, or
    one specify-core wrote (whose parts with answers stored must all be written
    again). One that already has PMI of its own is refused."""
    final = Path(output)
    if not jobs:
        raise ValueError("nothing to write")
    if len({loaded.path for loaded, _, _ in jobs}) != 1:
        raise ValueError("the parts written together must be of one file")
    if len({loaded.binding.part for loaded, _, _ in jobs}) != len(jobs):
        raise ValueError("a part is written once")
    for loaded, intent, _ in jobs:
        _check_binding(loaded, intent)
    # Written and verified beside the destination, which is replaced only once
    # all is well: a failed write leaves it, and the source, as they were. A
    # symlinked destination is written through, to its target, and an existing
    # file keeps its permissions.
    target = final.resolve() if final.is_symlink() else final
    output = target.with_name(f".{target.name}.{os.getpid()}.{uuid.uuid4().hex[:8]}.part")
    try:
        report = _write_verified(jobs, output)
        if target.exists():
            shutil.copymode(target, output)
        os.replace(output, target)
    finally:
        output.unlink(missing_ok=True)
    report.output = final
    return report


def _check_binding(loaded: LoadedPart, intent: Intent) -> None:
    if intent.binding.get("source_sha256") != loaded.binding.source_sha256:
        raise ValueError("intent was made for a different file")
    # An analysis made before parts were told apart is of a file of one part.
    if intent.binding.get("part", 0) != loaded.binding.part:
        raise ValueError(
            f"intent was made for part {intent.binding.get('part', 0)}, "
            f"not part {loaded.binding.part}"
        )
    if intent.binding.get("face_count") != loaded.binding.face_count:
        raise ValueError("intent face count does not match the file")
    # A loader change may number the faces differently for the same bytes.
    if intent.binding.get("loader_version") != loaded.binding.loader_version:
        raise ValueError(
            f"intent was made by loader version {intent.binding.get('loader_version')!r}, "
            f"not {loaded.binding.loader_version}: analyse the file again"
        )


def _write_verified(jobs: list[Job], output: Path) -> WriteReport:
    first = jobs[0][0]
    path, assembly = first.path, first.part_count > 1
    bare = not merge.carries_pmi(path)
    if assembly:
        stored = resume.stored(path)
        resumed = bool(stored)
        if not bare and not resumed:
            raise ValueError(
                "this assembly already has PMI not written by specify-core; "
                "adding to it is not supported yet"
            )
        unwritten = sorted(set(stored) - {loaded.binding.part for loaded, _, _ in jobs})
        if unwritten:
            raise ValueError(
                f"part(s) {', '.join(map(str, unwritten))} have answers stored in this file; "
                "write them again with these, or their PMI would be lost"
            )
        # One document holding every part, so all are written at once.
        everything = load_all(path, gdt=False)
        geometry = [everything[loaded.binding.part] for loaded, _, _ in jobs]
    else:
        resumed = resume.answers_for(first) is not None
        geometry = [first if bare else load(path, gdt=False)]
    for (loaded, _, _), part in zip(jobs, geometry, strict=True):
        if part.binding != loaded.binding:
            raise RuntimeError("the part loaded without its PMI has different faces")
    pairs = [(part, intent) for part, (_, intent, _) in zip(geometry, jobs, strict=True)]
    if bare or resumed:
        report, appended = _write(pairs, output, set())
    else:
        scratch = output.with_name(output.name + ".scratch")
        try:
            report, appended = _write(pairs, scratch, merge.datum_letters(path))
            merge.transplant(path, scratch, output, appended[0].count)
        finally:
            scratch.unlink(missing_ok=True)
    for (loaded, intent, answers), done in zip(jobs, appended, strict=True):
        verify(output, intent, loaded.binding.face_count, loaded.binding.part)
        _verify_appended(output, intent, done, loaded.binding.part)
        if answers is not None and (bare or resumed):
            resume.embed(output, loaded, answers)
    return report


def _write(
    pairs: list[tuple[LoadedPart, Intent]], output: Path, existing: set[str]
) -> tuple[WriteReport, list[requirements.Appended]]:
    """Add each intent to its part, all of one document, and write it to ``output``.
    Datums lettered in ``existing`` are the file's: written only where a tolerance
    cites them, for ``merge`` to replace with the file's own."""
    report = WriteReport(output)
    doc = pairs[0][0].doc
    dimtol = XCAFDoc_DocumentTool.DimTolTool_s(doc.Main())
    dimensions = TDF_LabelSequence()
    dimtol.GetDimensionLabels(dimensions)
    # Location dimensions are dimensions too: a part with only tapped holes has
    # no size dimension but has its positions' basic dimensions.
    has_dimension = dimensions.Length() > 0 or any(
        r["kind"] in ("size", "location") for _, intent in pairs for r in intent.requirements
    )
    tolerance_scale = 1.0 if has_dimension else 0.001
    for loaded, intent in pairs:
        part = WriteReport(output)
        _add(loaded, intent, part, existing, tolerance_scale)
        # In an assembly, each line says which part it is of.
        name = f"{loaded.name or f'part {loaded.binding.part}'}: " if len(pairs) > 1 else ""
        report.written += [name + line for line in part.written]
        report.warnings += [name + line for line in part.warnings]

    init_step()
    Interface_Static.SetCVal_s("write.step.schema", "AP242DIS")
    writer = STEPCAFControl_Writer()
    writer.SetDimTolMode(True)
    writer.SetNameMode(True)
    writer.SetColorMode(True)
    if not writer.Transfer(doc):
        raise RuntimeError("OCCT could not transfer the part for writing")
    if writer.Write(str(output)) != IFSelect_RetDone:
        raise RuntimeError(f"OCCT could not write the STEP ({_write_failures(writer)})")
    _drop_runout_zones(output)
    _type_limits(output)
    _simple_tolerances(output)
    _part_datums(output)
    # OCCT cannot mark a dimension basic; it is marked before anything is appended.
    locations.mark_basic(output)
    appended = [
        requirements.append(output, loaded, intent, frozenset(existing)) for loaded, intent in pairs
    ]
    # OCCT and the text appended write raw UTF-8 where the file's edition asks
    # for escapes; this file is new, so all of it is escaped.
    p21.escape_file(output)
    return report, appended


def _add(
    loaded: LoadedPart,
    intent: Intent,
    report: WriteReport,
    existing: set[str],
    tolerance_scale: float,
) -> None:
    """Add ``intent`` to ``loaded``'s part of its document."""
    shape_tool = XCAFDoc_DocumentTool.ShapeTool_s(loaded.doc.Main())
    dimtol = XCAFDoc_DocumentTool.DimTolTool_s(loaded.doc.Main())

    def labels(face_ids) -> TDF_LabelSequence:
        seq = TDF_LabelSequence()
        for i in face_ids:
            face = loaded.face(int(i))
            found = TDF_Label()
            if not shape_tool.FindSubShape(loaded.label, face, found):
                found = shape_tool.AddSubShape(loaded.label, face)
            if found.IsNull():
                raise RuntimeError(f"cannot address face {i}")
            seq.Append(found)
        return seq

    faces_of = {d["letter"]: d["faces"] for d in intent.datums}

    def datum(letter: str, position: int) -> TDF_Label:
        label = dimtol.AddDatum()
        obj = X.XCAFDimTolObjects_DatumObject()
        obj.SetName(TCollection_HAsciiString(_datum_name(letter, loaded.binding.part)))
        obj.SetPosition(position)
        XCAFDoc_Datum.Set_s(label).SetObject(obj)
        dimtol.SetDatum(labels(faces_of[letter]), label)
        return label

    referenced = {d["letter"] for d in _referenced_datums(intent)}
    new_datums = [d for d in intent.datums if d["letter"] not in existing]
    for entry in new_datums:
        if entry["letter"] not in referenced:
            datum(entry["letter"], 1)
        report.written.append(f"datum {entry['letter']}")

    for req in intent.requirements:
        kind = req["kind"]
        if kind == "size":
            obj = X.XCAFDimTolObjects_DimensionObject()
            obj.SetType(X.XCAFDimTolObjects_DimensionType_Size_Diameter)
            obj.SetValue(float(req["nominal"]))
            obj.SetUpperTolValue(float(req["upper"]))
            obj.SetLowerTolValue(-float(req["lower"]))
            label = dimtol.AddDimension()
            XCAFDoc_Dimension.Set_s(label).SetObject(obj)
            dimtol.SetDimension(labels(req["faces"]), TDF_LabelSequence(), label)
            report.written.append(f"Ø{req['nominal']:g} {req.get('fit', '')} on {req['feature']}")
        elif kind in _GEOM_TYPES:
            obj = X.XCAFDimTolObjects_GeomToleranceObject()
            obj.SetType(_GEOM_TYPES[kind])
            obj.SetValue(float(req["tolerance"]) * tolerance_scale)
            if req.get("diametral"):
                obj.SetTypeOfValue(X.XCAFDimTolObjects_GeomToleranceTypeValue_Diameter)
            if req.get("mmc"):
                obj.SetMaterialRequirementModifier(X.XCAFDimTolObjects_GeomToleranceMatReqModif_M)
            label = dimtol.AddGeomTolerance()
            XCAFDoc_GeomTolerance.Set_s(label).SetObject(obj)
            dimtol.SetGeomTolerance(labels(req["faces"]), label)
            for position, letter in enumerate(req.get("datums", ()), start=1):
                dimtol.SetDatumToGeomTol(datum(letter, position), label)
            report.written.append(
                f"{kind} {req['tolerance']:g}{' MMC' if req.get('mmc') else ''} "
                f"on {req.get('feature') or req['faces']}"
            )
        elif kind == "thread":
            report.written.append(
                f"{req['side']} thread {req['spec']}-{req['class']} on {req['feature']}"
            )
        elif kind == "knurl":
            report.written.append(f"{req['pattern']} knurl on {req['feature']}")
        elif kind == "finish":
            report.written.append(f"surface finish {req['value']} on faces {req['faces']}")
        elif kind == "location":
            obj = X.XCAFDimTolObjects_DimensionObject()
            obj.SetType(X.XCAFDimTolObjects_DimensionType_Location_LinearDistance)
            obj.SetValue(float(req["distance"]))
            if not req.get("basic"):
                obj.SetUpperTolValue(float(req["upper"]))
                obj.SetLowerTolValue(-float(req["lower"]))
            label = dimtol.AddDimension()
            XCAFDoc_Dimension.Set_s(label).SetObject(obj)
            dimtol.SetDimension(labels(req["reference"]), labels(req["faces"]), label)
            how = (
                "basic"
                if req.get("basic")
                else f"±{req['upper']:g}"
                if req["upper"] == -req["lower"]
                else f"+{req['upper']:g}/{req['lower']:+g}"
            )
            report.written.append(
                f"location {req['distance']:g} {how} to {req['feature']} "
                f"from faces {req['reference']}"
            )
        else:
            raise ValueError(f"unknown requirement kind {kind!r}")
    for entry in new_datums:
        if entry["letter"] not in referenced:
            report.warnings.append(
                f"datum {entry['letter']} is in the file but no tolerance references it; "
                "OCCT-based readers (including Draftwright) will not see it"
            )
    if intent.part.get("material"):
        # OCCT writes it as the CAx-IF material practice's 'material name'.
        # No density: OCCT writes its unit wrongly (g^3.cm^2).
        materials = XCAFDoc_DocumentTool.MaterialTool_s(loaded.doc.Main())
        name = TCollection_HAsciiString(intent.part["material"])
        empty = TCollection_HAsciiString("")
        materials.SetMaterial(loaded.label, materials.AddMaterial(name, empty, 0.0, empty, empty))
        report.written.append(f"material {intent.part['material']}")
    if intent.part.get("general_tolerance"):
        report.written.append(f"general tolerance {intent.part['general_tolerance']}")
    for key in requirements.PART_NOTES:
        if intent.part.get(key):
            report.written.append(f"{key.replace('_', ' ')} {intent.part[key]}")


_PART_MARK = "~part"


def _datum_name(letter: str, part: int) -> str:
    """The name a datum is written under: a part's own after the first."""
    return letter if part == 0 else f"{letter}{_PART_MARK}{part}"


def _part_datums(path: Path) -> None:
    """Give each part's datums their letters back (``_datum_name``)."""
    text = path.read_text()
    marked = re.compile(rf"(DATUM\([^;]*?'[^']*){re.escape(_PART_MARK)}\d+'")
    path.write_text(marked.sub(r"\1'", text))


def _drop_runout_zones(path: Path) -> int:
    """Remove OCCT's malformed runout zone definitions from ``path``, and the
    orientations, angles and units left referenced by nothing; return how many
    entities went."""
    text = path.read_text()
    entity = re.compile(r"^#(\d+)\s*=\s*(.*?);\s*$\n?", re.S | re.M)
    bodies = {m.group(1): m.group(2) for m in entity.finditer(text)}
    gone = {i for i, body in bodies.items() if body.startswith("RUNOUT_ZONE_DEFINITION(")}
    if not gone:
        return 0
    orphanable = (
        "RUNOUT_ZONE_ORIENTATION(",
        "PLANE_ANGLE_MEASURE_WITH_UNIT(",
        "( NAMED_UNIT(*) PLANE_ANGLE_UNIT()",
    )
    while True:
        used = {
            r for i, body in bodies.items() if i not in gone for r in re.findall(r"#(\d+)", body)
        }
        more = {
            i
            for i, body in bodies.items()
            if i not in gone and i not in used and body.startswith(orphanable)
        }
        if not more:
            break
        gone |= more
    path.write_text(entity.sub(lambda m: "" if m.group(1) in gone else m.group(0), text))
    return len(gone)


_UNTYPED = re.compile(r"^(#(\d+)\s*=\s*)MEASURE_WITH_UNIT\(\s*([-+0-9.Ee]+)\s*,", re.M)


def _type_limits(path: Path) -> None:
    """Give the limits OCCT writes untyped their type: length."""
    text = path.read_text()
    limits = {r for m in _TOLERANCE_VALUE.finditer(text) for r in m.groups()}
    path.write_text(
        _UNTYPED.sub(
            lambda m: (
                f"{m.group(1)}LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE({m.group(3)}),"
                if m.group(2) in limits
                else m.group(0)
            ),
            text,
        )
    )


#: Tolerances that are geometric_tolerance_with_datum_reference and nothing else.
_DATUMED = frozenset(
    {
        "ANGULARITY_TOLERANCE",
        "CIRCULAR_RUNOUT_TOLERANCE",
        "COAXIALITY_TOLERANCE",
        "CONCENTRICITY_TOLERANCE",
        "PARALLELISM_TOLERANCE",
        "PERPENDICULARITY_TOLERANCE",
        "SYMMETRY_TOLERANCE",
        "TOTAL_RUNOUT_TOLERANCE",
    }
)
_COMPLEX = re.compile(r"^(#\d+\s*=\s*)\((.*?)\)\s*;", re.S | re.M)


def _partials(body: str) -> dict[str, str] | None:
    """A complex entity's partial records by name, or None if it will not parse."""
    out: dict[str, str] = {}
    i = 0
    while i < len(body):
        if body[i].isspace():
            i += 1
            continue
        name = re.match(r"([A-Z_0-9]+)\s*\(", body[i:])
        if not name:
            return None
        depth, j, quoted = 0, i + name.end() - 1, False
        for j in range(i + name.end() - 1, len(body)):
            c = body[j]
            if c == "'":
                quoted = not quoted
            elif not quoted and c == "(":
                depth += 1
            elif not quoted and c == ")":
                depth -= 1
                if depth == 0:
                    break
        out[name.group(1)] = body[i + name.end() : j].strip()
        i = j + 1
    return out


def _simple_tolerances(path: Path) -> None:
    """Write a tolerance with datums and no modifier as the simple entity it is,
    as NIST's files do."""

    def simple(m: re.Match[str]) -> str:
        parts = _partials(m.group(2))
        rest = set(parts or ()) - {
            "GEOMETRIC_TOLERANCE",
            "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE",
        }
        if not parts or len(parts) != 3 or len(rest) != 1:
            return m.group(0)
        (kind,) = rest
        if kind not in _DATUMED or parts[kind]:
            return m.group(0)
        attrs = (
            f"{parts['GEOMETRIC_TOLERANCE']},{parts['GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE']}"
        )
        return f"{m.group(1)}{kind}({attrs});"

    path.write_text(_COMPLEX.sub(simple, path.read_text()))


def _write_failures(writer: STEPCAFControl_Writer) -> str:
    """What OCCT's checks found wrong in the entities it wrote, by entity type."""
    checks = writer.ChangeWriter().WS().LastRunCheckList()
    found: set[str] = set()
    checks.Start()
    while checks.More():
        check = checks.Value()
        entity = check.Entity()
        name = entity.DynamicType().Name() if entity is not None else "file"
        found |= {f"{name}: {check.CFail(i, True)}" for i in range(1, check.NbFails() + 1)}
        checks.Next()
    return "; ".join(sorted(found)) or "no reason given"


def verify(path: Path, intent: Intent, face_count: int, part: int = 0) -> None:
    """Read ``path`` back and check every requirement and datum reached its ``part``-th part."""
    text = path.read_text(errors="replace")
    if "AP242" not in text[:2000]:
        raise VerificationError("file was not written as AP242")
    back = load(path, part=part)
    if back.binding.face_count != face_count:
        raise VerificationError("face count changed on write")
    found = [e.to_dict() for e in read_existing(back)]
    problems = []
    for datum in _referenced_datums(intent):
        if not any(
            e["kind"] == "datum" and e["type"] == datum["letter"] and e["faces"] == datum["faces"]
            for e in found
        ):
            problems.append(f"datum {datum['letter']} missing or on the wrong faces")
    limits = _file_limits(text)
    for req in intent.requirements:
        faces = sorted(int(i) for i in req.get("faces", ()))
        if req["kind"] == "size":
            # Presence only: OCCT's reader garbles the value of a dimension whose
            # deviations are both below nominal (Ø70 g6 reads back as 35.0145).
            # The limits are checked in the file text below.
            if not any(e["kind"] == "dimension" and e["faces"] == faces for e in found):
                problems.append(f"size on {faces} missing")
            if not any(
                abs(lo - req["lower"]) < 1e-9 and abs(hi - req["upper"]) < 1e-9 for lo, hi in limits
            ):
                problems.append(f"limits {req['upper']}/{req['lower']} not in file")
        elif req["kind"] == "location":
            reference = sorted(int(i) for i in req["reference"])
            # As for a size, OCCT garbles the value read back when both
            # deviations are below nominal; the limits are checked in the text.
            below = not req.get("basic") and req["upper"] < 0
            if not any(
                e["kind"] == "dimension"
                and e["type"].startswith("Location")
                and sorted(e["faces"]) == reference
                and sorted(e.get("faces2", [])) == faces
                and (below or abs((e.get("value") or 0) - req["distance"]) < 1e-6)
                for e in found
            ):
                problems.append(f"location {req['distance']:g} to {faces} missing or wrong")
            if not req.get("basic") and not any(
                abs(lo - req["lower"]) < 1e-9 and abs(hi - req["upper"]) < 1e-9 for lo, hi in limits
            ):
                problems.append(f"limits {req['upper']}/{req['lower']} not in file")
        elif req["kind"] in _GEOM_TYPES:
            ok = any(
                e["kind"] == "geometric_tolerance"
                and e["type"] == _GEOM_NAMES[req["kind"]]
                and e["faces"] == faces
                and abs(e["value"] - req["tolerance"]) < 1e-9
                and e.get("datums", []) == list(req.get("datums", ()))
                for e in found
            )
            if not ok:
                problems.append(f"{req['kind']} on {faces} missing or wrong")
    if problems:
        raise VerificationError("; ".join(problems))


def _referenced_datums(intent: Intent) -> list[dict[str, Any]]:
    """Datums some tolerance refers to. OCCT's reader imports only these."""
    used = {letter for req in intent.requirements for letter in req.get("datums", ())}
    return [d for d in intent.datums if d["letter"] in used]


_MEASURE = re.compile(
    r"#(\d+)\s*=\s*(?:LENGTH_)?MEASURE_WITH_UNIT\(\s*(?:LENGTH_MEASURE\(\s*)?([-+0-9.Ee]+)"
)
_TOLERANCE_VALUE = re.compile(r"TOLERANCE_VALUE\(\s*#(\d+)\s*,\s*#(\d+)\s*\)")


def _file_limits(text: str) -> list[tuple[float, float]]:
    """(lower, upper) of every TOLERANCE_VALUE in the file, as STEP states them."""
    measures = {m.group(1): float(m.group(2)) for m in _MEASURE.finditer(text)}
    return [
        (measures[m.group(1)], measures[m.group(2)])
        for m in _TOLERANCE_VALUE.finditer(text)
        if m.group(1) in measures and m.group(2) in measures
    ]


def _verify_appended(
    path: Path, intent: Intent, appended: requirements.Appended, part: int = 0
) -> None:
    """Every appended requirement is in the file, on its part's faces; the material too."""
    text = re.sub(r"\s*\n\s*", " ", path.read_text(errors="replace"))
    problems = []
    for label, words in appended.texts.items():
        if f"'{p21.escape(words.replace(chr(39), chr(39) * 2))}'" not in text:
            problems.append(f"{label} text not in file")
    faces = requirements.face_entities(path, part)
    by_id = {face: index for index, face in faces.items()}
    for label, wanted in appended.faces.items():
        kind = label.rsplit(" ", 1)[0]
        found = {
            by_id.get(int(face))
            for face in re.findall(
                rf"GEOMETRIC_ITEM_SPECIFIC_USAGE\('{re.escape(kind)}','',#\d+,#\d+,#(\d+)\)", text
            )
        }
        if not set(wanted) <= found:
            problems.append(f"{label} is not on faces {wanted}")
    material = intent.part.get("material")
    if material and not re.search(
        # OCCT puts a long name on a line of its own, after the bracket.
        rf"DESCRIPTIVE_REPRESENTATION_ITEM\(\s*'{re.escape(p21.escape(material))}'",
        text,
    ):
        problems.append("material not in file")
    if problems:
        raise VerificationError("; ".join(problems))
