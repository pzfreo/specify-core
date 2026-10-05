"""Add PMI to a STEP file that already has some, leaving what it has untouched.

OCCT cannot write back the PMI it reads. Found on the NIST AP242 test parts:
every geometric tolerance is written with magnitude 0 (its reader returns 0 for
a magnitude held in a complex entity); a 60° angle is written as 3437.7°; a
location dimension it read one end of is written with a null reference; a
tessellated presentation likewise; a datum it could not resolve loses its faces.

So a part with PMI is loaded geometry only, the intent is written by OCCT into a
scratch file exactly as for a bare part, and the new PMI is transplanted into
the original file's text: appended before its ENDSEC, numbered after its last
entity. The original's entities are not touched.

What is transplanted: every entity of a PMI type in the scratch file, the
entities ``requirements.append`` added, and whatever those reference that is not
the part (units, contexts, measures). References to the part go to the
original's own entities: a face to the face with the same index, the
representation holding the solid, the part's product_definition_shape and
product_definition. A datum letter the original already has is the original's
datum: the scratch file's datum feature for it is dropped and references go to
the original's DATUM. Any other reference into the part is an error rather than
a guess.
"""

from __future__ import annotations

import re
from pathlib import Path

from .requirements import _anchors, entities_of, face_entities

#: Entity types of semantic PMI and its links to faces, as OCCT writes them.
PMI_TYPES = frozenset(
    {
        "DATUM",
        "DATUM_FEATURE",
        "DATUM_SYSTEM",
        "DATUM_REFERENCE_COMPARTMENT",
        "DATUM_REFERENCE_ELEMENT",
        "GEOMETRIC_TOLERANCE",
        "FLATNESS_TOLERANCE",
        "PERPENDICULARITY_TOLERANCE",
        "PARALLELISM_TOLERANCE",
        "CIRCULAR_RUNOUT_TOLERANCE",
        "TOTAL_RUNOUT_TOLERANCE",
        "POSITION_TOLERANCE",
        "TOLERANCE_ZONE",
        "TOLERANCE_ZONE_FORM",
        "DIMENSIONAL_SIZE",
        "DIMENSIONAL_LOCATION",
        "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION",
        "SHAPE_DIMENSION_REPRESENTATION",
        "PLUS_MINUS_TOLERANCE",
        "TOLERANCE_VALUE",
        "LIMITS_AND_FITS",
        "SHAPE_ASPECT",
        "COMPOSITE_SHAPE_ASPECT",
        "SHAPE_ASPECT_RELATIONSHIP",
        "GEOMETRIC_ITEM_SPECIFIC_USAGE",
        "ITEM_IDENTIFIED_REPRESENTATION_USAGE",
    }
)
#: The material, and its link to the name; not the part's validation properties.
_MATERIAL = re.compile(r"PROPERTY_DEFINITION\(\s*'material property'")

#: Entity types of the part itself; PMI may only reach them through the anchors.
PART_TYPES = frozenset(
    {
        "ADVANCED_FACE",
        "FACE_SURFACE",
        "MANIFOLD_SOLID_BREP",
        "BREP_WITH_VOIDS",
        "CLOSED_SHELL",
        "OPEN_SHELL",
        "EDGE_CURVE",
        "ORIENTED_EDGE",
        "EDGE_LOOP",
        "VERTEX_POINT",
        "FACE_BOUND",
        "FACE_OUTER_BOUND",
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "SHAPE_REPRESENTATION",
        "SHAPE_DEFINITION_REPRESENTATION",
        "PRODUCT",
        "PRODUCT_DEFINITION",
        "PRODUCT_DEFINITION_FORMATION",
        "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE",
        "PRODUCT_DEFINITION_SHAPE",
        "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
        "STYLED_ITEM",
        "OVER_RIDING_STYLED_ITEM",
    }
)

_REF = re.compile(r"#(\d+)")
_TYPE = re.compile(r"([A-Z][A-Z0-9_]*)\s*\(")
_LETTER = re.compile(r"'([^']*)'\s*\)$")
_SCHEMA = re.compile(r"FILE_SCHEMA\s*\(.*?\)\s*;", re.S)
_PMI = re.compile(r"\b(DATUM|DIMENSIONAL_SIZE|DIMENSIONAL_LOCATION|[A-Z_]*TOLERANCE)\s*\(")


class MergeError(RuntimeError):
    pass


def carries_pmi(path: Path) -> bool:
    """Whether the STEP file at ``path`` already has semantic PMI."""
    return _PMI.search(path.read_text(errors="replace")) is not None


def datum_letters(path: Path) -> set[str]:
    """The datum letters the STEP file at ``path`` defines."""
    return set(_datums(_entities(path.read_text(errors="replace"))))


def transplant(source: Path, scratch: Path, output: Path, appended: int) -> int:
    """Write ``source`` plus the PMI of ``scratch`` to ``output``; return how many
    entities were added. ``appended`` is how many entities ``requirements.append``
    added at the end of ``scratch``."""
    text = source.read_text(errors="replace")
    scratch_text = scratch.read_text(errors="replace")
    old = _entities(text)
    new = _entities(scratch_text)

    anchors = {}
    theirs, ours = _anchors(new), _anchors(old)
    for key in ("brep", "pds", "pd"):
        anchors[theirs[key]] = ours[key]
    their_faces, our_faces = face_entities(scratch), face_entities(source)
    if set(their_faces) != set(our_faces):
        raise MergeError("the scratch file's faces are not the original's")
    for index, face in their_faces.items():
        anchors[face] = our_faces[index]

    old_datums, new_datums = _datums(old), _datums(new)
    for letter, datum in new_datums.items():
        if letter in old_datums:
            anchors[datum] = old_datums[letter]
    dropped = _datum_features(new, {d for d in anchors if d in new_datums.values()})

    order = list(new)
    skip = dropped | set(anchors)
    materials = {i for i in order if _MATERIAL.match(new[i])}
    materials |= {
        i
        for i in order
        if new[i].startswith("PROPERTY_DEFINITION_REPRESENTATION(")
        and _refs(new[i])[0] in materials
    }
    kept = [i for i in order if (_types(new[i]) & PMI_TYPES or i in materials) and i not in skip]
    kept += order[len(order) - appended :] if appended else []
    chosen: set[int] = set()
    stack = list(kept)
    while stack:
        i = stack.pop()
        if i in chosen:
            continue
        chosen.add(i)
        for r in _refs(new[i]):
            if r in anchors or r in chosen:
                continue
            if r in dropped:
                raise MergeError(f"#{r} of a datum the file already has is still referenced")
            if _types(new[r]) & PART_TYPES:
                raise MergeError(f"new PMI refers to #{r}, a part of the model, not one it maps")
            stack.append(r)

    first = max(old) + 1
    numbers = {i: first + n for n, i in enumerate(i for i in order if i in chosen)}
    numbers.update(anchors)
    lines = [
        f"#{numbers[i]}={_REF.sub(lambda m: f'#{numbers[int(m.group(1))]}', new[i])};"
        for i in order
        if i in chosen
    ]

    end = text.rfind("ENDSEC;")
    if end < 0 or "END-ISO-10303-21" not in text[end:]:
        raise MergeError(f"{source} does not end as a Part 21 file does")
    head = text[:end]
    schema = _SCHEMA.search(head)
    if schema is None:
        raise MergeError(f"{source} names no schema")
    if "AP242" not in schema.group(0):
        # The added PMI is AP242's; the original's geometry is valid in it.
        ap242 = _SCHEMA.search(scratch_text).group(0)
        head = head[: schema.start()] + ap242 + head[schema.end() :]
    output.write_text(head + "\n".join(lines) + "\n" + text[end:])
    return len(lines)


def _entities(text: str) -> dict[int, str]:
    return {int(i): body for i, body in entities_of(text)}


def _refs(body: str) -> list[int]:
    return [int(x) for x in _REF.findall(body)]


def _types(body: str) -> set[str]:
    """The entity type names of ``body``, every part of a complex entity included."""
    if body.lstrip().startswith("("):
        return set(_TYPE.findall(body))
    match = _TYPE.match(body)
    return {match.group(1)} if match else set()


def _datums(entities: dict[int, str]) -> dict[str, int]:
    """Datum letter -> its DATUM entity."""
    out = {}
    for i, body in entities.items():
        if body.startswith("DATUM(") and (m := _LETTER.search(body)):
            out.setdefault(m.group(1), i)
    return out


def _datum_features(entities: dict[int, str], datums: set[int]) -> set[int]:
    """The feature of each of ``datums``, its parts, and their links to faces."""
    relations = []
    for i, body in entities.items():
        if body.startswith("SHAPE_ASPECT_RELATIONSHIP("):
            refs = _refs(body)
            relations.append((i, refs[-2], refs[-1]))
    out: set[int] = set()
    aspects = [a for _, a, d in relations if d in datums]
    out |= {i for i, _, d in relations if d in datums}
    while aspects:
        aspect = aspects.pop()
        if aspect in out:
            continue
        out.add(aspect)
        for i, relating, related in relations:
            if relating == aspect:
                out.add(i)
                aspects.append(related)
    for i, body in entities.items():
        if (
            body.startswith(
                ("GEOMETRIC_ITEM_SPECIFIC_USAGE(", "ITEM_IDENTIFIED_REPRESENTATION_USAGE(")
            )
            and _refs(body)[0] in out
        ):
            out.add(i)
    return out
