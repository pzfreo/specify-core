"""``specify-core serve``: a long-lived child process speaking JSON lines.

A host that cannot import specify-core -- one that pins an older
Quiddity through Draftwright -- runs this in specify-core's own environment and
sends one request per line on stdin; one reply per line comes back. Requests
carry everything they need (the analysis is passed back in, not kept here), so
a crashed or restarted child loses nothing.

    {"id": 1, "op": "analyse", "step": "/path/part.step"}
    -> {"id": 1, "ok": true, "result": {"analysis": {...}, "mesh": {...}}}
    {"id": 2, "op": "questions", "analysis": {...}, "answers": {...}}
    {"id": 3, "op": "preview", "analysis": {...}, "answers": {...}, "step": "/path/part.step"}
    -> the intent, with "anchors" for its labels when the step is given
    {"id": 5, "op": "interpret", "analysis": ..., "answers": ..., "question": id, "text": "M6"}
    -> {"id": 5, "ok": true, "result": {"value": "clearance:M6", "code": ..., "callout": ...}}
    {"id": 4, "op": "write", "step": ..., "analysis": ..., "answers": ...,
     "output": "/path/out.step", "accept_defaults": true}
    -> the report; the answers are stored in the file, which "analyse" of it
       then returns as "resume": {"answers": ...} (see ``resume``)
    -> {"id": 4, "ok": false, "error": {"code": "incomplete", "message": ..., "missing": [...]}}

OCCT prints progress to stdout from C++, which would corrupt the replies, so
the process moves file descriptor 1 onto stderr and writes replies to a private
duplicate of the original stdout.
"""

from __future__ import annotations

import json
import os
import sys
import traceback
from collections import OrderedDict
from pathlib import Path
from typing import Any, TextIO

from . import anchors, api, choices
from .load import load
from .problems import problems
from .rules import IncompleteError, open_questions

PROTOCOL = 1


def handle(request: dict[str, Any]) -> dict[str, Any]:
    """One request to its result; raises on failure."""
    op = request.get("op")
    if op == "hello":
        return {"protocol": PROTOCOL}
    if op == "analyse":
        loaded = load(Path(request["step"]))
        return {"analysis": api.analyse(loaded), "mesh": api.mesh(loaded)}
    analysis = request["analysis"]
    answers = request.get("answers", {})
    if op == "questions":
        qs = api.questions(analysis, answers)
        return {
            "questions": [q.to_dict() for q in qs],
            "open": [q.id for q in open_questions(qs, answers)],
            "problems": problems(analysis, answers),
        }
    if op == "interpret":
        # What someone typed for a question, read as the answer it means.
        qs = api.questions(analysis, answers)
        q = next((q for q in qs if q.id == request["question"]), None)
        if q is None or q.kind != "choice":
            raise ValueError(f"no choice question {request['question']!r}")
        value = choices.interpret(q.id, str(request["text"]), q.options, q.diameter)
        out = {"value": value, "code": choices.code(value)}
        if q.diameter is not None and q.id.startswith(("hole.function:", "diameter.fit:")):
            out["callout"] = choices.callout(value, q.diameter, q.id.startswith("hole."))
        return out
    if op == "preview":
        intent = api.preview(analysis, answers).to_dict()
        if request.get("step"):
            # Where each label lands, worked out on the solid; the page falls
            # back to its own guess for any face without one.
            intent["anchors"] = _label_anchors(Path(request["step"]), analysis, intent)
        return {"intent": intent}
    if op == "write":
        intent = api.apply(analysis, answers, accept_defaults=bool(request.get("accept_defaults")))
        # The answers as given, defaults not filled in: resumed, a default not
        # chosen stays a default and follows the rules as they then are.
        report = api.write(Path(request["step"]), intent, Path(request["output"]), answers=answers)
        return {"report": report.to_dict(), "intent": intent.to_dict()}
    raise ValueError(f"unknown op {op!r}")


#: Parts loaded for anchoring, by path, most recent last; and their anchors.
_PARTS: OrderedDict[str, Any] = OrderedDict()
_ANCHORS: dict[tuple[str, str], dict[str, Any]] = {}
_KEEP = 2


def _label_anchors(step: Path, analysis: dict[str, Any], intent: dict[str, Any]) -> dict[str, Any]:
    loaded = _PARTS.get(str(step))
    if loaded is None:
        loaded = load(step)
        _PARTS[str(step)] = loaded
        while len(_PARTS) > _KEEP:
            gone, _ = _PARTS.popitem(last=False)
            for k in [k for k in _ANCHORS if k[0] == gone]:
                del _ANCHORS[k]
    _PARTS.move_to_end(str(step))
    if loaded.binding.to_dict() != analysis.get("binding"):
        return {}
    groups = [d["faces"] for d in intent["datums"]] + [r["faces"] for r in intent["requirements"]]
    groups += [e.get("faces", []) for e in analysis.get("existing", [])]
    wanted = {anchors.key(g): g for g in groups if g}
    missing = [g for k, g in wanted.items() if (str(step), k) not in _ANCHORS]
    if missing:
        for k, found in anchors.anchors(loaded, missing, _diagonal(loaded)).items():
            _ANCHORS[(str(step), k)] = found
    return {k: _ANCHORS[(str(step), k)] for k in wanted if (str(step), k) in _ANCHORS}


def _diagonal(loaded) -> float:
    from OCP.Bnd import Bnd_Box
    from OCP.BRepBndLib import BRepBndLib

    box = Bnd_Box()
    BRepBndLib.Add_s(loaded.shape, box)
    lo, hi = box.CornerMin(), box.CornerMax()
    return ((hi.X() - lo.X()) ** 2 + (hi.Y() - lo.Y()) ** 2 + (hi.Z() - lo.Z()) ** 2) ** 0.5 or 1.0


def reply(request: dict[str, Any]) -> dict[str, Any]:
    """A request's reply: its result, or a structured error."""
    out: dict[str, Any] = {"id": request.get("id")}
    try:
        out |= {"ok": True, "result": handle(request)}
    except IncompleteError as exc:
        out |= {"ok": False, "error": _error("incomplete", exc, missing=exc.missing)}
    except (ValueError, KeyError) as exc:
        # An answer specify-core refuses, or a malformed request.
        out |= {"ok": False, "error": _error("invalid", exc)}
    except Exception as exc:  # noqa: BLE001 -- the child must outlive one bad part
        out |= {"ok": False, "error": _error("failed", exc, trace=traceback.format_exc())}
    return out


def _error(code: str, exc: Exception, **extra: Any) -> dict[str, Any]:
    message = str(exc) if not isinstance(exc, KeyError) else f"missing field {exc}"
    return {"code": code, "message": message, **extra}


def serve(stdin: TextIO | None = None, replies: TextIO | None = None) -> int:
    """Answer requests until stdin closes."""
    if replies is None:
        replies = os.fdopen(os.dup(1), "w", buffering=1)
        os.dup2(2, 1)  # OCCT's C++ output now goes to stderr
        sys.stdout = sys.stderr
    for line in stdin or sys.stdin:
        if not line.strip():
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError as exc:
            out = {"id": None, "ok": False, "error": {"code": "invalid", "message": str(exc)}}
        else:
            out = reply(request)
        replies.write(json.dumps(out, separators=(",", ":")) + "\n")
        replies.flush()
    return 0
