"""A STEP specify-core wrote, opened again to be changed: its answers travel with it.

Writing a bare part, specify-core also stores the answers it was given in the
file, as a property of the product: ``PROPERTY_DEFINITION('pmi-assist answers',
...)`` holding one descriptive item, the answers as base64 JSON (Part 21 strings
need no escaping then). Reading such a file, the analysis is of its geometry
alone and carries the answers back (``analysis["resume"]``); writing it again
starts from that geometry, so its own PMI is replaced, never added to.

Answers name faces by index and features by Quiddity's ids. Both were found to
survive a write unchanged (the faces of a written file, read without its PMI,
are the original's in order), and the stored fingerprint of the faces is
checked before any answer is trusted. A file whose PMI came partly from
elsewhere (written by ``merge``) stores nothing: its own PMI cannot be told
apart from what specify-core added, so it is read as before.

In an assembly each part's answers are stored on that part, saying which part
they are for; a part with none stored has not been written.
"""

from __future__ import annotations

import base64
import hashlib
import json
import re
from pathlib import Path
from typing import Any

from .features import describe_faces
from .load import LoadedPart
from .requirements import _anchors, _Part21, entities_of, face_entities

SCHEMA = 1
# The stored property's name stays as first written, so files saved before the
# rename still resume.
_SAVED = re.compile(
    r"DESCRIPTIVE_REPRESENTATION_ITEM\(\s*'pmi-assist answers'\s*,\s*'([A-Za-z0-9+/=]*)'\s*\)"
)


def fingerprint(loaded: LoadedPart) -> str:
    """The faces in order, by kind and rounded centroid: what answers refer to."""
    faces = [(f.kind, [round(c, 3) for c in f.centroid]) for f in describe_faces(loaded)]
    return hashlib.sha256(json.dumps(faces).encode()).hexdigest()


def embed(path: Path, loaded: LoadedPart, answers: dict[str, Any]) -> None:
    """Store ``answers`` in the file at ``path``, written from ``loaded``."""
    payload = {
        "schema": SCHEMA,
        "answers": answers,
        "part": loaded.binding.part,
        "face_count": loaded.binding.face_count,
        "faces": fingerprint(loaded),
    }
    encoded = base64.b64encode(json.dumps(payload, separators=(",", ":")).encode()).decode()
    text = path.read_text()
    entities = {int(i): body for i, body in entities_of(text)}
    ids = _anchors(entities, face_entities(path, loaded.binding.part).values())
    out = _Part21(max(entities) + 1)
    item = out.add(f"DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers','{encoded}')")
    rep = out.add(f"REPRESENTATION('pmi-assist answers',(#{item}),#{ids['context']})")
    prop = out.add(f"PROPERTY_DEFINITION('pmi-assist answers','schema {SCHEMA}',#{ids['pd']})")
    out.add(f"PROPERTY_DEFINITION_REPRESENTATION(#{prop},#{rep})")
    marker = re.search(r"\nENDSEC;\s*\nEND-ISO-10303-21;\s*$", text)
    if marker is None:
        raise RuntimeError(f"{path} does not end as a Part 21 file does")
    path.write_text(text[: marker.start()] + "\n" + "\n".join(out.lines) + text[marker.start() :])


def saved(path: Path, part: int = 0) -> dict[str, Any] | None:
    """The answers stored in the file at ``path`` for its ``part``-th part, or None."""
    return stored(path).get(part)


def stored(path: Path) -> dict[int, dict[str, Any]]:
    """Every part's stored answers in the file at ``path``, by part. Answers saved
    before parts were told apart are part 0's."""
    out: dict[int, dict[str, Any]] = {}
    for found in _SAVED.finditer(re.sub(r"\s*\n\s*", "", path.read_text(errors="replace"))):
        try:
            payload = json.loads(base64.b64decode(found.group(1), validate=True))
        except (ValueError, json.JSONDecodeError):
            continue
        if not isinstance(payload, dict) or payload.get("schema") != SCHEMA:
            continue
        if not isinstance(payload.get("answers"), dict):
            continue
        part = payload.get("part", 0)
        if isinstance(part, int):
            out.setdefault(part, payload)
    return out


def answers_for(loaded: LoadedPart) -> dict[str, Any] | None:
    """The stored answers, if the file holds them for this part and its faces are
    the ones they were given for; None otherwise."""
    payload = saved(loaded.path, loaded.binding.part)
    if payload is None:
        return None
    if payload.get("face_count") != loaded.binding.face_count:
        return None
    if payload.get("faces") != fingerprint(loaded):
        return None
    return payload["answers"]
