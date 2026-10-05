"""Gap rules: which questions a part needs, and how answers become intent.

Rules are deterministic and decide completeness. Defaults come from standards
and simple geometry; an AI may propose different answers, but a part is only
complete when every question here has an answer.

Questions are recomputed from the analysis and the answers so far, because
some depend on earlier answers (a hole's position tolerance depends on whether
it is a clearance, tapped or fitted hole, and on which datums exist).
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field, replace
from typing import Any

from . import choices, extras, general, locations, materials, standards

RULES_VERSION = 1

NONE = "none"
GENERAL = "general"

#: How holes are located: by position tolerances from datums, or by ± dimensions
#: on the drawing under the general tolerance, which needs no datums.
LOCATING = ("position tolerances", "± dimensions")

#: Surface roughness Ra in micrometres, finest first, and edge notes.
FINISHES = ("Ra 0.4", "Ra 0.8", "Ra 1.6", "Ra 3.2", "Ra 6.3", "Ra 12.5")
EDGES = ("Break sharp edges 0.2 max", "Break sharp edges 0.5 max", "Deburr only", "No edge note")
#: The part's notes, beside its material and general tolerance; an answer that
#: says there is nothing to note writes nothing.
NOTES = ("surface_finish", "coating", "heat_treatment", "edges")
_NOTHING = {"", "none", "none (as machined)", "as machined", "no edge note"}

#: Datums beyond A, B and C, asked only once the user adds them.
EXTRA_DATUMS = "DEFGH"

#: How much each question needs a person, so a part can be reviewed rather
#: than interrogated: ``required`` has no default and must be answered;
#: ``check`` has a default the rules doubt, with a ``reason``; ``routine`` has
#: a default from a standard or the geometry, accepted unless changed;
#: ``refine`` is an optional tightening that defaults to none or general.
ATTENTION = ("required", "check", "routine", "refine")


@dataclass(frozen=True)
class Question:
    id: str
    kind: str  # choice | text | faces
    prompt: str
    faces: tuple[int, ...]  # what to highlight
    default: Any = None
    options: tuple[str, ...] = ()
    why: str = ""
    feature: str | None = None
    #: For a position: the datum frame it is measured from, e.g. ``A|B|C``.
    frame: str | None = None
    #: How much a person needs to look at it; see ``ATTENTION``.
    attention: str = "routine"
    #: Why a ``check`` question was flagged.
    reason: str = ""
    #: When identical features are asked about once: each one's id and faces.
    #: Each still gets its own requirements.
    parts: tuple[tuple[str, tuple[int, ...]], ...] = ()
    #: For a text question: common answers to offer as it is typed; any text is accepted.
    suggestions: tuple[dict[str, Any], ...] = ()
    #: For a hole or turned diameter: each option's purpose, label, code and callout.
    choices: tuple[dict[str, Any], ...] = ()
    #: The feature's diameter, against which a typed spec is checked.
    diameter: float | None = None
    #: Why the default is what it is, in a sentence: shown as a choice made for
    #: the person, to keep or change.
    basis: str = ""

    def to_dict(self) -> dict[str, Any]:
        out = {
            "id": self.id,
            "kind": self.kind,
            "prompt": self.prompt,
            "faces": list(self.faces),
            "default": list(self.default) if isinstance(self.default, tuple) else self.default,
            "why": self.why,
        }
        if self.options:
            out["options"] = list(self.options)
        if self.feature:
            out["feature"] = self.feature
        if self.frame:
            out["frame"] = self.frame
        out["attention"] = self.attention
        if self.reason:
            out["reason"] = self.reason
        if self.parts:
            out["features"] = [feature for feature, _ in self.parts]
        if self.suggestions:
            out["suggestions"] = list(self.suggestions)
        if self.choices:
            out["choices"] = list(self.choices)
        if self.basis:
            out["basis"] = self.basis
        return out


@dataclass
class Intent:
    binding: dict[str, Any]
    part: dict[str, Any] = field(default_factory=dict)
    datums: list[dict[str, Any]] = field(default_factory=list)
    requirements: list[dict[str, Any]] = field(default_factory=list)
    schema: int = 1
    rules_version: int = RULES_VERSION
    #: The dimensions the general tolerance covers (``general.dimensions``):
    #: listed for inspection and shown, not written into the STEP.
    general: list[dict[str, Any]] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "schema": self.schema,
            "rules_version": self.rules_version,
            "binding": self.binding,
            "part": self.part,
            "datums": self.datums,
            "requirements": self.requirements,
            "general": self.general,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Intent:
        if data.get("schema") != 1:
            raise ValueError(f"unsupported intent schema {data.get('schema')!r}")
        return cls(
            data["binding"],
            data.get("part", {}),
            data.get("datums", []),
            data.get("requirements", []),
            data["schema"],
            data.get("rules_version", 0),
            data.get("general", []),
        )


# --------------------------------------------------------------------------
# Questions
# --------------------------------------------------------------------------


def questions(analysis: dict[str, Any], answers: dict[str, Any]) -> list[Question]:
    """Every question the part needs, given the answers so far.

    Datums exist to measure tolerances from, so they are proposed only when a
    feature needs a frame -- a hole or pattern for anything but the general
    tolerance -- and asked after the features they serve. Where only added
    orientations need them, only the letters those cite are proposed. Datums
    the person added or removed are always asked.
    """
    cited = "".join(sorted(extras.cited(answers)))
    qs = _questions(analysis, answers, propose="ABC" + cited)
    located = _answer(answers, qs, "part.locating") != LOCATING[1] and any(
        q.id.startswith("hole.function:") and _answer(answers, qs, q.id) != GENERAL for q in qs
    )
    # An answered runout is measured from the part's axis: the frame is needed.
    runout = any(
        q.id.startswith("diameter.runout:") and _answer(answers, qs, q.id) not in (None, NONE)
        for q in qs
    )
    if not located and not runout:
        qs = _questions(analysis, answers, propose=cited)
    return qs


def _questions(
    analysis: dict[str, Any], answers: dict[str, Any], *, propose: str
) -> list[Question]:
    faces = {f["id"]: f for f in analysis["faces"]}
    features = analysis["features"]
    # What the file already states is not asked again (merge.py adds to it).
    covered = _existing_faces(analysis, "dimension", "Size_") | _existing_faces(analysis, "note")
    positioned = _existing_faces(analysis, "geometric_tolerance", "Position")
    toleranced = _existing_faces(analysis, "geometric_tolerance")
    in_file = _file_datums(analysis)
    stated = analysis.get("part", {})
    out: list[Question] = [
        Question(
            "part.material",
            "text",
            "What material is the part made from?",
            (),
            why="Every drawing states the material; it also decides what tolerances are sensible.",
            suggestions=tuple(materials.suggestions()),
        ),
        Question(
            "part.general_tolerance",
            "choice",
            "Which general tolerance applies to dimensions without their own?",
            (),
            "ISO 2768-m",
            standards.GENERAL_TOLERANCES,
            "Covers every dimension without its own tolerance, so most features need no question.",
            basis="ISO 2768-m (medium) is the usual general tolerance for machined parts.",
        ),
    ]
    out += [
        Question(
            "part.surface_finish",
            "choice",
            "What surface finish applies where none is given?",
            (),
            "Ra 3.2",
            FINISHES,
            "Ra 3.2 is ordinary machining; finer costs more. Faces that need better, such "
            "as bores and seals, can be given their own.",
            basis="Ra 3.2 is what ordinary machining gives without extra passes.",
        ),
        Question(
            "part.coating",
            "text",
            "Is the part coated or treated?",
            (),
            "None (as machined)",
            why="A coating changes the size of every face it covers, and the price.",
            suggestions=tuple(materials.suggested(materials.COATINGS)),
        ),
        Question(
            "part.heat_treatment",
            "text",
            "Is the part heat treated?",
            (),
            "None",
            why="Hardening is done after machining, and is written on the drawing.",
            suggestions=tuple(materials.suggested(materials.HEAT_TREATMENTS)),
        ),
        Question(
            "part.edges",
            "choice",
            "What should be done to sharp edges?",
            (),
            EDGES[0],
            EDGES,
            "Sharp edges cut hands and chip; a standard note says how far to break them.",
            basis="Breaking edges to 0.2 is the common standard note.",
        ),
    ]
    # A file with its own datums locates by them; the choice is for a bare part.
    if not in_file and any(f["family"] in ("holes", "hole_patterns") for f in features):
        out.append(
            Question(
                "part.locating",
                "choice",
                "How are holes located?",
                (),
                LOCATING[0],
                LOCATING,
                "Position tolerances locate each hole from datums, for parts that must fit "
                "together. ± dimensions locate them on the drawing under the general "
                "tolerance, and need no datums unless a perpendicularity or parallelism "
                "is added.",
                basis="Position tolerances, as holes usually take parts that must line up.",
            )
        )
    out = [q for q in out if not stated.get(q.id.removeprefix("part."))]
    for key, extra in extras.given(answers):
        extras.check(key, extra, faces)
    asked_datums = _datum_questions(faces, features, answers, in_file, propose=propose)
    datums = _datums(answers, asked_datums, faces, in_file)
    controls = {x for x in datums if _datum_control(x, datums, faces)}
    by_position = _answer(answers, out, "part.locating") != LOCATING[1]
    frames = _frames(list(datums)) if by_position else []

    def located(hole) -> list[str]:
        bores = _bores_of(hole, faces)
        if set(bores) & positioned or not by_position:
            return []
        return _hole_frames(bores, datums, controls, frames)

    pattern_members = {
        m for f in features if f["family"] == "hole_patterns" for m in f.get("members", ())
    }
    by_id = {f["id"]: f for f in features}
    # (target, hole, frames) for every hole or group asked about, then merged
    # where identical: two equal bolt circles are one question.
    holes: list[tuple[dict, dict, list[str]]] = []
    turned: list[dict] = []
    for feature in features:
        family = feature["family"]
        if family == "holes" and feature["id"] not in pattern_members:
            if set(_bores_of(feature, faces)) & covered:
                continue
            holes.append((feature, feature, located(feature)))
        elif family == "hole_patterns" and feature.get("members"):
            # Sizing one of a set of identical holes sizes the set: NIST CTC-01
            # attaches its "4X Ø25" to one hole.
            if any(set(_bores_of(by_id[m], faces)) & covered for m in feature["members"]):
                continue
            # A member chosen as a datum leaves its set and is asked about on its own.
            alone = [m for m in feature["members"] if _datum_of(_bores_of(by_id[m], faces), datums)]
            for m in alone:
                holes.append((by_id[m], by_id[m], located(by_id[m])))
            members = [m for m in feature["members"] if m not in alone]
            if not members:
                continue
            target = {
                **feature,
                "members": members,
                "faces": [i for m in members for i in by_id[m]["faces"]],
            }
            positions = any(set(_bores_of(by_id[m], faces)) & positioned for m in members)
            holes.append((target, by_id[members[0]], [] if positions else frames))
        elif family == "turned_steps" and not set(feature["faces"]) & covered:
            turned.append(feature)

    for group in _same(holes, lambda t: _hole_key(t, faces, datums)):
        target, hole, located_frames = group[0]
        if len(group) > 1:
            target = {
                **target,
                "faces": [i for t, _, _ in group for i in t["faces"]],
                "members": [m for t, _, _ in group for m in t.get("members", ()) or [t["id"]]],
                "groups": len(group),
                "parts": tuple(
                    (t["id"], _bores(t["faces"], hole["record"]["diameter"], faces))
                    for t, _, _ in group
                ),
            }
        edges = None if by_position else list(_default_datums(faces, features)[1:])
        out.extend(_hole_questions(target, hole, faces, answers, located_frames, edges))
    axial, axis_faces = _runout_frame(datums, faces, features)
    for group in _same(turned, lambda f: round(float(f["record"]["diameter"]), 6)):
        out.append(_turned_question(group, faces))
        # Not the diameter that is the axis itself: it would run out from itself.
        if axial and not any(set(f["faces"]) & axis_faces for f in group):
            out.append(_runout_question(group, faces, axial))

    out.extend(asked_datums)
    out.extend(_datum_control_questions(datums, faces, toleranced))
    # A typed answer is refused here, where it is given, not when the file is written.
    for q in out:
        if q.kind == "choice" and q.id in answers:
            choices.check(q.id, answers[q.id], q.options, q.diameter)
    return [_reviewed(q, faces) for q in out]


def _reviewed(q: Question, faces: dict[int, dict]) -> Question:
    """``q`` with its attention level; the policy for all questions is here."""
    if q.default is None:
        return replace(q, attention="required")
    if q.id.startswith("hole.function:hole_patterns") and q.default == GENERAL:
        # A single odd hole is usually just a hole. A group of identical ones is
        # usually for fasteners, and left general it gets no position tolerance.
        return replace(
            q,
            attention="check",
            reason="a group of identical holes matching no standard clearance or "
            "tap-drill size; if they take fasteners they need a position tolerance",
        )
    if q.id.startswith("hole.function:") and str(q.default).startswith("tapped:"):
        # A thread is a strong claim to make from a diameter alone: the size
        # matches a tap drill, but a plain hole can be that size too.
        size = str(q.default).partition(":")[2]
        return replace(
            q,
            attention="check",
            reason=f"⌀{_fmt(q.diameter or 0)} is the tap-drill size for {size}, so it is "
            f"assumed tapped {size}; if it is not threaded, choose what it is for",
        )
    if q.id == "datum.A":
        doubt = _datum_a_doubt(faces)
        if doubt:
            return replace(q, attention="check", reason=doubt)
    if (
        q.id.startswith(("diameter.fit:", "diameter.runout:"))
        or q.id == "datum.A.flatness"
        or q.id.endswith(".control")
    ):
        return replace(q, attention="refine")
    return q


def _datum_a_doubt(faces: dict[int, dict]) -> str:
    """Why the largest plane may not be the primary datum: another plane, facing
    elsewhere, is nearly as large."""
    groups: list[tuple[dict, float]] = []
    for f in sorted((f for f in faces.values() if f["kind"] == "plane"), key=lambda f: -f["area"]):
        for i, (lead, area) in enumerate(groups):
            if _coplanar(f, lead):
                groups[i] = (lead, area + f["area"])
                break
        else:
            groups.append((f, f["area"]))
    groups.sort(key=lambda g: -g[1])
    if len(groups) > 1 and groups[1][1] >= 0.8 * groups[0][1]:
        return (
            f"face {groups[1][0]['id']} is nearly as large as face {groups[0][0]['id']}; "
            "the primary datum is whichever the part is mounted on"
        )
    return ""


def open_questions(qs: list[Question], answers: dict[str, Any]) -> list[Question]:
    return [q for q in qs if q.id not in answers]


def with_defaults(qs: list[Question], answers: dict[str, Any]) -> dict[str, Any]:
    """``answers`` plus the default of every unanswered question that has one."""
    merged = dict(answers)
    for q in qs:
        if q.id not in merged and q.default is not None:
            merged[q.id] = list(q.default) if isinstance(q.default, tuple) else q.default
    return merged


def _datum_questions(
    faces: dict[int, dict], features: list[dict], answers, in_file, *, propose: str
) -> list[Question]:
    """The letters in ``propose``, A to C with their defaults; any letter the
    person has given or removed, always. A file with datums has its reference
    frame: only letters it lacks are asked."""
    why = (
        "Datums fix the part for measurement; tolerances refer to them. A datum is "
        "one face, several coplanar faces, or a hole or boss (its axis)."
    )
    defaults: dict[str, tuple[int, ...]] = {}
    turned = any(f["family"] == "turned_steps" for f in features)
    bases = {
        "A": "The largest flat face: usually what the part sits on or is clamped by.",
        "B": "The part's axis: the largest diameter square to A."
        if turned
        else "The largest flat face square to A.",
        "C": "The largest flat face square to A and B.",
    }
    if propose and not in_file:
        a, b, c = _default_datums(faces, features)
        defaults = {x: d for x, d in zip("ABC", (a, b, c), strict=True) if x in propose}
        # A letter beyond C has no default: it is asked, empty, to be picked.
        defaults |= {x: () for x in propose if x not in "ABC"}
    out = []
    for letter in "ABC" + EXTRA_DATUMS:
        if letter in in_file or (letter not in defaults and f"datum.{letter}" not in answers):
            continue
        default = defaults.get(letter, ())
        if letter == "A":
            prompt = "Which face(s), hole or boss is datum A (primary)?"
        else:
            prompt = f"Which face(s), hole or boss is datum {letter}? (empty for none)"
        out.append(
            Question(
                f"datum.{letter}",
                "faces",
                prompt,
                default,
                default,
                why=why
                if letter in "ABC"
                else "An extra datum, for features located from something other than A.",
                basis=bases.get(letter, "") if default else "",
            )
        )
    return out


def _datum_control_questions(datums, faces, toleranced) -> list[Question]:
    """How well each datum feature must be made: a datum is only as good as its form.
    Not asked of a datum the file already tolerances."""
    out = []
    a = datums.get("A")
    if a and a["kind"] == "plane" and not set(a["faces"]) & toleranced:
        out.append(
            Question(
                "datum.A.flatness",
                "choice",
                "Flatness required on datum A?",
                tuple(a["faces"]),
                NONE,
                (NONE, "0.01", "0.02", "0.05", "0.1"),
                "Datum A is usually the face the part is clamped or mounted on.",
            )
        )
    for letter in "BC":
        control = _datum_control(letter, datums, faces)
        if not control or set(datums[letter]["faces"]) & toleranced:
            continue
        out.append(
            Question(
                f"datum.{letter}.control",
                "choice",
                f"{control['kind'].capitalize()} {'(Ø) ' if control['diametral'] else ''}"
                f"required on datum {letter}, relative to {'|'.join(control['datums'])}?",
                tuple(datums[letter]["faces"]),
                NONE,
                (NONE, "0.01", "0.02", "0.05", "0.1"),
                "Orients (and for a tertiary hole, locates) the datum feature, so the frame "
                "it builds is repeatable.",
            )
        )
    return out


def _datum_control(letter: str, datums, faces) -> dict[str, Any] | None:
    """The tolerance that relates datum B or C to the datums before it.

    A tertiary hole is located from A|B. Otherwise the feature is held square to
    the earlier datums it is square to: a plane C square to A and B is
    perpendicular to A|B. None when v1 has no characteristic for the geometry --
    a feature parallel to A would need parallelism.
    """
    if letter not in "BC" or letter not in datums or "A" not in datums:
        return None
    if letter == "C" and "B" not in datums:
        return None
    datum = datums[letter]
    axis = datum["kind"] == "axis"
    if letter == "C" and axis:
        return {"kind": "position", "datums": ["A", "B"], "diametral": True}
    refs = [r for r in "AB"[: "BC".index(letter) + 1] if _square(datum, datums[r], faces)]
    if not refs or refs[0] != "A":
        return None
    return {"kind": "perpendicularity", "datums": refs, "diametral": axis}


def _default_datums(faces: dict[int, dict], features: list[dict]):
    planes = sorted((f for f in faces.values() if f["kind"] == "plane"), key=lambda f: -f["area"])
    if not planes:
        return (), (), ()
    a = planes[0]
    a_ids = tuple(sorted(p["id"] for p in planes if _coplanar(p, a)))
    if any(f["family"] == "turned_steps" for f in features):
        # A turned part: B is the largest cylinder on the axis A is square to.
        cylinders = sorted(
            (
                f
                for f in faces.values()
                if f["kind"] == "cylinder" and _parallel(f["direction"], a["direction"])
            ),
            key=lambda f: -f["area"],
        )
        b_ids = tuple(sorted(f["id"] for f in cylinders if _coaxial(f, cylinders[0])))
        return a_ids, b_ids, ()
    square = [p for p in planes[1:] if _perpendicular(p["direction"], a["direction"])]
    if not square:
        return a_ids, (), ()
    b = square[0]
    third = [p for p in square[1:] if _perpendicular(p["direction"], b["direction"])]
    return a_ids, (b["id"],), ((third[0]["id"],) if third else ())


def _hole_questions(target, hole, faces, answers, frames, edges=None) -> list[Question]:
    diameter = float(hole["record"]["diameter"])
    bores = _bores(target["faces"], diameter, faces)
    count = len(target.get("members", ())) or 1
    label = f"{count}× Ø{_fmt(diameter)} holes" if count > 1 else f"Ø{_fmt(diameter)} hole"
    if target.get("groups"):
        label += f" in {target['groups']} identical groups"
    parts = target.get("parts", ())
    options = [GENERAL]
    # Clearance for any bolt the hole plausibly passes (ISO 273 fine to coarse);
    # where none does, a clearance hole for a bolt not stated.
    bolts = standards.clearance_bolts(diameter)
    tapped = standards.match_size(diameter, standards.TAP_DRILL)
    options.extend(f"clearance:{b}" for b in bolts)
    if tapped:
        options.append(f"tapped:{tapped}")
    if not bolts and standards.largest_bolt_below(diameter):
        options.append("clearance")
    options.extend(f"fit:{fit}" for fit in standards.HOLE_FITS)
    default = options[1] if bolts or tapped else GENERAL
    qid = f"hole.function:{target['id']}"
    out = [
        Question(
            qid,
            "choice",
            f"What {'are' if count > 1 else 'is'} the {label} for?",
            bores,
            default,
            tuple(options),
            "A clearance, tapped or fitted hole each need different size and position tolerances.",
            target["id"],
            parts=parts,
            choices=tuple(choices.describe(qid, options, diameter)),
            diameter=diameter,
            basis=_function_basis(default, diameter, bolts, tapped),
        )
    ]
    function = answers.get(qid, default)
    if function != GENERAL and frames:
        frame = frames[0]
        if len(frames) > 1:
            fid = f"hole.frame:{target['id']}"
            out.append(
                Question(
                    fid,
                    "choice",
                    f"Which datums locate the {label}?",
                    bores,
                    frames[0],
                    tuple(frames),
                    "The frame the position is measured from; the datums the mating part "
                    "locates on.",
                    target["id"],
                    basis="The part's whole frame, A then B then C.",
                )
            )
            frame = answers.get(fid, frames[0])
            if frame not in frames:
                raise ValueError(f"{fid}: {frame!r} is not one of {frames}")
        tol = _default_position(function, diameter)
        out.append(
            Question(
                f"hole.position:{target['id']}",
                "choice",
                f"Position tolerance (Ø) for the {label}, relative to {frame}?",
                bores,
                _fmt(tol),
                (NONE, *sorted({_fmt(tol), "0.05", "0.1", "0.2", "0.5"}, key=float)),
                "Locates the hole axis. Default from the fastener formula: floating "
                "T = H − F (H this hole, F the bolt; with no bolt stated, the largest "
                "standard bolt through it), fixed T = (H − F)/2; fitted holes get 0.05.",
                target["id"],
                frame,
                parts=parts,
                basis=_position_basis(function, diameter, tol),
            )
        )
    # Located by ± dimensions: how closely, from the part's edge faces.
    if edges is not None and locations.references(edges, hole["record"]["axis"], faces):
        out.append(
            Question(
                f"hole.location:{target['id']}",
                "choice",
                f"How closely are the {label} located?",
                bores,
                locations.TOLERANCES[0],
                locations.TOLERANCES,
                "Their distances from the part's edges: the general tolerance, or a "
                "tighter ± written on each.",
                target["id"],
                parts=parts,
            )
        )
    return out


def _turned_question(group: list[dict], faces) -> Question:
    """One question for turned diameters of one size; each keeps its own fit."""
    feature = group[0]
    d = float(feature["record"]["diameter"])

    def cylinders(f):
        return tuple(i for i in f["faces"] if faces[i]["kind"] == "cylinder") or tuple(f["faces"])

    times = f"{len(group)}× " if len(group) > 1 else ""
    thread = standards.thread_size(d)
    options = (
        GENERAL,
        *standards.SHAFT_FITS,
        *((f"thread:{thread}",) if thread else ()),
        *(f"knurl:{pattern}" for pattern in standards.KNURL_PATTERNS),
    )
    return Question(
        f"diameter.fit:{feature['id']}",
        "choice",
        f"Is the turned {times}Ø{_fmt(d)} diameter a fit, a thread or knurled?",
        tuple(i for f in group for i in cylinders(f)),
        GENERAL,
        options,
        "Functional diameters need a fit (bearing, seal, press fit); a diameter at a "
        "metric size may be threaded; a grip may be knurled. The rest are covered by "
        "the general tolerance.",
        feature["id"],
        parts=tuple((f["id"], cylinders(f)) for f in group) if len(group) > 1 else (),
        choices=tuple(choices.describe(f"diameter.fit:{feature['id']}", options, d)),
        diameter=d,
    )


#: Runout zones offered for a turned diameter, beside none.
RUNOUTS = ("0.01", "0.02", "0.05", "0.1")


def _runout_frame(datums, faces, features) -> tuple[str | None, set[int]]:
    """The frame a turned diameter's runout is measured from -- the datums up
    to and including the part's axis: those given, else those that would be
    proposed (a face, then the axis) -- and the axis datum's faces. None for a
    part with no datum axis."""
    if datums:
        letters = list(datums)
        axes = [x for x in letters if datums[x]["kind"] == "axis"]
        if not axes:
            return None, set()
        frame = "|".join(letters[: letters.index(axes[0]) + 1])
        return frame, set(datums[axes[0]]["faces"])
    a, b, _ = _default_datums(faces, features)
    try:
        if a and b and datum_kind(a, faces) == "plane" and datum_kind(b, faces) == "axis":
            return "A|B", set(b)
    except ValueError:
        return None, set()
    return None, set()


def _runout_question(group: list[dict], faces, frame: str) -> Question:
    """How true a turned diameter runs about the part's axis; none by default."""
    feature = group[0]
    d = float(feature["record"]["diameter"])
    times = f"{len(group)}× " if len(group) > 1 else ""

    def cylinders(f):
        return tuple(i for i in f["faces"] if faces[i]["kind"] == "cylinder") or tuple(f["faces"])

    return Question(
        f"diameter.runout:{feature['id']}",
        "choice",
        f"Runout of the turned {times}Ø{_fmt(d)} relative to {frame}?",
        tuple(i for f in group for i in cylinders(f)),
        NONE,
        (NONE, *RUNOUTS),
        "How far the diameter may wobble as the part turns on its axis: for bearing "
        "seats, seals and anything that spins.",
        feature["id"],
        frame,
        parts=tuple((f["id"], cylinders(f)) for f in group) if len(group) > 1 else (),
    )


def _function_basis(default: str, diameter: float, bolts, tapped) -> str:
    """Why a hole's purpose defaults to what it does."""
    d = _fmt(diameter)
    if default.startswith("clearance:"):
        bolt = default.partition(":")[2]
        also = f"; it is also the {tapped} tap drill" if tapped else ""
        return f"⌀{d} is an ISO 273 clearance hole for {bolt}{also}."
    if default.startswith("tapped:"):
        return f"⌀{d} is the tap-drill size for {default.partition(':')[2]}."
    return ""  # left to the general tolerance: nothing was chosen


def _position_basis(function: str, diameter: float, tol: float) -> str:
    """How a hole's default position tolerance was worked out."""
    kind, _, size = str(function).partition(":")
    t = _fmt(tol)
    if kind == "fit":
        return f"⌀{t}: a fitted hole is located closely."
    if kind == "tapped":
        h = _fmt(standards.CLEARANCE[size])
        return (
            f"⌀{t} = (H − F)/2 for a fixed {size} screw: the mating part's ⌀{h} "
            f"clearance hole less the screw, halved."
        )
    bolt = size or standards.largest_bolt_below(diameter)
    if bolt:
        return (
            f"⌀{t} = H − F for a floating {bolt} bolt: this ⌀{_fmt(diameter)} hole less the bolt."
        )
    return f"⌀{t}: no bolt is known, so a common value."


def _default_position(function: str, diameter: float) -> float:
    kind, _, size = function.partition(":")
    if kind == "fit":
        return 0.05
    if kind == "tapped":
        # Fixed fastener: the mating part has the medium clearance hole.
        return round((standards.CLEARANCE[size] - standards.bolt_diameter(size)) / 2, 2)
    # Floating fastener: this hole's own float, H - F.
    size = size or standards.largest_bolt_below(diameter)
    return round(diameter - standards.bolt_diameter(size), 2) if size else 0.1


# --------------------------------------------------------------------------
# Answers to intent
# --------------------------------------------------------------------------


def apply(analysis: dict[str, Any], answers: dict[str, Any]) -> Intent:
    """Turn a complete set of answers into intent. Refuses while any is missing."""
    qs = questions(analysis, answers)
    missing = [q.id for q in open_questions(qs, answers)]
    if missing:
        raise IncompleteError(missing)
    return _intent(analysis, answers, qs)


def with_all_defaults(analysis: dict[str, Any], answers: dict[str, Any]) -> dict[str, Any]:
    """``answers`` plus every default, to a fixed point: some questions only
    exist once an earlier default is taken."""
    while True:
        merged = with_defaults(questions(analysis, answers), answers)
        if merged == answers:
            return answers
        answers = merged


def preview(analysis: dict[str, Any], answers: dict[str, Any]) -> Intent:
    """The intent the answers so far amount to, for showing while they are given.

    Defaults stand in for unanswered questions -- datum defaults feed every
    position frame -- and every datum and requirement says which question it
    came from and whether that question was answered (``"default": true`` when
    not). A question with neither answer nor default contributes nothing.
    """
    merged = with_all_defaults(analysis, answers)
    intent = _intent(analysis, merged, questions(analysis, merged))
    for item in intent.datums + intent.requirements:
        if "question" in item:
            item["default"] = item["question"] not in answers
    return intent


def _intent(analysis: dict[str, Any], answers: dict[str, Any], qs: list[Question]) -> Intent:
    """Intent from the answered questions among ``qs``; each item names its question."""
    faces = {f["id"]: f for f in analysis["faces"]}
    features = {f["id"]: f for f in analysis["features"]}
    intent = Intent(analysis["binding"])
    intent.part = {
        key: answers[f"part.{key}"]
        for key in ("material", "general_tolerance", *NOTES)
        if str(answers.get(f"part.{key}") or "").strip().lower() not in _NOTHING
    }
    datums = _datums(answers, qs, faces, _file_datums(analysis))
    intent.datums = [
        {"letter": x, "faces": d["faces"], "existing": True}
        if d.get("existing")
        else {"letter": x, "faces": d["faces"], "question": f"datum.{x}"}
        for x, d in datums.items()
    ]

    for q in qs:
        if q.id not in answers:
            continue
        value = answers[q.id]
        before = len(intent.requirements)
        if q.id.startswith("hole.function:"):
            for fid, bores in q.parts or ((q.feature, q.faces),):
                feature = features[fid]
                hole = feature if feature["family"] == "holes" else features[feature["members"][0]]
                intent.requirements.extend(_hole_requirements(value, bores, feature, hole))
        elif q.id.startswith("hole.location:") and locations.parse(value):
            for fid, _ in q.parts or ((q.feature, q.faces),):
                intent.requirements.extend(
                    locations.dimensions(
                        _members(features[fid], features, faces),
                        list(_default_datums(faces, analysis["features"])[1:]),
                        faces,
                        tolerance=locations.parse(value),
                    )
                )
        elif q.id.startswith("hole.position:") and value != NONE:
            function = str(_answer(answers, qs, f"hole.function:{q.feature}"))
            # Basic dimensions from the frame's planes say where "true position" is.
            planes = [
                tuple(datums[x]["faces"])
                for x in q.frame.split("|")
                if x in datums and datums[x]["kind"] == "plane"
            ]
            for fid, _ in q.parts or ((q.feature, q.faces),):
                intent.requirements.extend(
                    locations.dimensions(
                        _members(features[fid], features, faces), planes, faces, tolerance=None
                    )
                )
            for fid, bores in q.parts or ((q.feature, q.faces),):
                intent.requirements.append(
                    {
                        "kind": "position",
                        "feature": fid,
                        "faces": list(bores),
                        "tolerance": float(value),
                        "diametral": True,
                        "datums": q.frame.split("|"),
                        # A clearance hole's position at maximum material:
                        # bonus tolerance as the hole grows, and a gauge pin checks it.
                        **({"mmc": True} if function.startswith("clearance") else {}),
                    }
                )
        elif q.id.startswith("diameter.runout:") and value != NONE:
            letters = q.frame.split("|")
            if all(x in datums for x in letters):
                for fid, cylinders in q.parts or ((q.feature, q.faces),):
                    intent.requirements.append(
                        {
                            "kind": "runout",
                            "feature": fid,
                            "faces": list(cylinders),
                            "tolerance": float(value),
                            "datums": letters,
                        }
                    )
        elif q.id.startswith("diameter.fit:") and value != GENERAL:
            d = float(features[q.feature]["record"]["diameter"])
            for fid, cylinders in q.parts or ((q.feature, q.faces),):
                intent.requirements.append(_turned_requirement(value, d, fid, cylinders, features))
        elif q.id == "datum.A.flatness" and value != NONE:
            intent.requirements.append(
                {"kind": "flatness", "faces": list(q.faces), "tolerance": float(value)}
            )
        elif q.id.endswith(".control") and value != NONE:
            letter = q.id.split(".")[1]
            control = _datum_control(letter, datums, faces)
            intent.requirements.append(
                {
                    "kind": control["kind"],
                    "feature": f"datum {letter}",
                    "faces": list(q.faces),
                    "tolerance": float(value),
                    "diametral": control["diametral"],
                    "datums": control["datums"],
                }
            )
        for req in intent.requirements[before:]:
            req["question"] = q.id
    for key, extra in extras.given(answers):
        req = extras.requirement(key, extra, faces, datums)
        if req is not None:
            intent.requirements.append(req)
    tolerance = _answer(answers, qs, "part.general_tolerance") or analysis.get("part", {}).get(
        "general_tolerance"
    )
    if tolerance:
        intent.general = general.dimensions(analysis, datums, intent.requirements, tolerance)
    return intent


def _turned_requirement(value: str, d: float, fid: str, cylinders, features) -> dict[str, Any]:
    """A turned diameter's answer: a fit, an external thread or a knurl."""
    base = {"feature": fid, "faces": list(cylinders)}
    kind, _, spec = value.partition(":")
    if kind == "thread":
        record = features[fid]["record"]
        length = record.get("hi") - record.get("lo") if record.get("hi") is not None else None
        return {
            "kind": "thread",
            **base,
            "side": "external",
            "designation": spec,
            "pitch": standards.PITCH[spec],
            "spec": f"{spec}x{standards.PITCH[spec]:g}",
            "class": standards.EXTERNAL_THREAD_CLASS,
            "hand": "RH",
            "length": length,
            "nominal": d,
        }
    if kind == "knurl":
        return {
            "kind": "knurl",
            **base,
            "pattern": spec,
            "pitch": standards.KNURL_PITCH,
            "diameter": d,
        }
    upper, lower = standards.fit_limits(value, d)
    return {"kind": "size", **base, "nominal": d, "fit": value, "upper": upper, "lower": lower}


class IncompleteError(ValueError):
    def __init__(self, missing: list[str]) -> None:
        super().__init__(f"{len(missing)} question(s) unanswered: {', '.join(missing)}")
        self.missing = missing


def _hole_requirements(value: str, bores, feature, hole) -> list[dict[str, Any]]:
    if value == GENERAL:
        return []
    kind, _, spec = value.partition(":")
    d = float(hole["record"]["diameter"])
    base = {"feature": feature["id"], "faces": list(bores)}
    if kind == "fit":
        upper, lower = standards.fit_limits(spec, d)
        return [{"kind": "size", **base, "nominal": d, "fit": spec, "upper": upper, "lower": lower}]
    if kind == "tapped":
        depth = hole["record"].get("depth")
        through = hole["record"].get("bottom") == "through"
        return [
            {
                "kind": "thread",
                **base,
                "side": "internal",
                "designation": spec,
                "pitch": standards.PITCH[spec],
                "spec": f"{spec}x{standards.PITCH[spec]:g}",
                "class": standards.INTERNAL_THREAD_CLASS,
                "hand": "RH",
                "through": through,
                "drill_point": hole["record"].get("bottom") == "drill_point",
                "drill_diameter": d,
                "drill_depth": depth,
                "full_thread": depth
                if through or depth is None
                else standards.full_thread(depth, spec),
            }
        ]
    # Clearance: the hole as drawn is the nominal; H11 bounds it.
    upper, lower = standards.fit_limits("H11", d)
    size = {"kind": "size", **base, "nominal": d, "fit": "H11", "upper": upper, "lower": lower}
    if spec:
        size["clearance_for"] = spec
    return [size]


# --------------------------------------------------------------------------
# Helpers
# --------------------------------------------------------------------------


def _bores(face_ids, diameter: float, faces) -> tuple[int, ...]:
    """The cylinder faces among ``face_ids`` at ``diameter`` -- a hole's bore, not its
    drill point or counterbore."""
    return tuple(i for i in face_ids if _is_bore(faces[i], diameter))


def _is_bore(face: dict, diameter: float) -> bool:
    return face["kind"] == "cylinder" and abs(2 * face.get("radius", 0) - diameter) < 1e-6


def _existing_faces(analysis, kind: str, prefix: str = "") -> set[int]:
    """Faces the file's PMI of ``kind`` (and type starting ``prefix``) is on."""
    return {
        i
        for e in analysis.get("existing", ())
        if e["kind"] == kind and e["type"].startswith(prefix)
        for i in e["faces"]
    }


def _file_datums(analysis) -> dict[str, tuple[int, ...]]:
    """The file's datum letters and their faces; none where they cannot be read."""
    out: dict[str, tuple[int, ...]] = {}
    for e in analysis.get("existing", ()):
        if e["kind"] == "datum" and (e["faces"] or e["type"] not in out):
            out[e["type"]] = tuple(e["faces"])
    return out


def _datums(answers, qs, faces, in_file) -> dict[str, dict[str, Any]]:
    """Each datum, in letter order, as its faces and ``plane`` or ``axis``. The
    file's own come first; one whose faces cannot be read cannot be cited."""
    out = {}
    for letter in "ABC" + EXTRA_DATUMS:
        if letter in in_file:
            try:
                kind = datum_kind(in_file[letter], faces) if in_file[letter] else None
            except ValueError:
                kind = None
            if kind:
                out[letter] = {"faces": list(in_file[letter]), "kind": kind, "existing": True}
            continue
        chosen = _answer(answers, qs, f"datum.{letter}")
        if not chosen:
            continue
        try:
            _check_faces(chosen, faces)
            kind = datum_kind(chosen, faces)
        except ValueError as exc:
            raise ValueError(f"datum {letter}: {exc}") from None
        out[letter] = {"faces": sorted(int(i) for i in chosen), "kind": kind}
    return out


def _same(items: list, key) -> list[list]:
    """``items`` grouped by ``key``, groups in order of first appearance."""
    groups: dict[Any, list] = {}
    for item in items:
        groups.setdefault(key(item), []).append(item)
    return list(groups.values())


def _hole_key(item, faces, datums) -> tuple:
    """When two hole targets are the same question: holes alike in size, depth,
    bottom, entry and direction, as many of them, located the same way. A hole
    chosen as a datum is never merged -- it is specified as that datum."""
    target, hole, frames = item
    if _datum_of(_bores_of(hole, faces), datums):
        return ("datum", target["id"])
    r = hole["record"]
    return (
        _rounded(r.get("diameter")),
        _rounded(r.get("depth")),
        r.get("bottom"),
        bool(r.get("cbore")),
        bool(r.get("spotface")),
        bool(r.get("csink")),
        tuple(_rounded(abs(v)) for v in r.get("axis") or ()),
        len(target.get("members", ())) or 1,
        target["family"],
        tuple(frames),
    )


def _rounded(value):
    return None if value is None else round(float(value), 6)


def _members(feature, features, faces) -> list[tuple[str, dict, tuple[int, ...]]]:
    """Each hole of a hole or pattern feature: its id, record and bore faces."""
    holes = [features[m] for m in feature.get("members") or ()] or [feature]
    return [(h["id"], h["record"], _bores_of(h, faces)) for h in holes]


def _bores_of(hole, faces) -> tuple[int, ...]:
    return _bores(hole["faces"], hole["record"]["diameter"], faces)


def _datum_of(bores, datums) -> str | None:
    """The axis datum ``bores`` form, if the hole was chosen as one."""
    return next(
        (
            x
            for x, d in datums.items()
            if d["kind"] == "axis" and bores and set(bores) <= set(d["faces"])
        ),
        None,
    )


def _frames(letters: list[str]) -> list[str]:
    """Datum frames to offer: the primary frame, then each extra datum in A's place.
    None without A: a position needs a primary datum."""
    if "A" not in letters:
        return []
    primary = [x for x in letters if x in "ABC"]
    return ["|".join(primary)] + ["|".join([x, *primary[1:]]) for x in letters if x in EXTRA_DATUMS]


def _hole_frames(bores, datums, controls, frames) -> list[str]:
    """Frames a hole can be positioned from. A datum hole whose datum control
    locates or orients it gets none; one without a control keeps its position,
    from the frames with its own letter taken out."""
    letter = _datum_of(bores, datums)
    if letter is None:
        return frames
    if letter in controls:
        return []
    own = ["|".join(x for x in f.split("|") if x != letter) for f in frames]
    return list(dict.fromkeys(f for f in own if f))


def _square(feature, datum, faces) -> bool:
    """Whether ``feature`` is perpendicular to ``datum``: planes by their normals,
    an axis to a plane when it runs along the normal."""
    u = faces[feature["faces"][0]]["direction"]
    v = faces[datum["faces"][0]]["direction"]
    return _perpendicular(u, v) if feature["kind"] == datum["kind"] else _parallel(u, v)


def datum_kind(face_ids, faces) -> str:
    """``plane`` for coplanar faces, ``axis`` for the coaxial cylinders of one hole
    or boss. Anything else cannot be a datum here."""
    chosen = [faces[int(i)] for i in face_ids]
    first = chosen[0]
    if all(f["kind"] == "plane" for f in chosen) and all(_coplanar(f, first) for f in chosen):
        return "plane"
    if all(f["kind"] == "cylinder" for f in chosen) and all(_coaxial(f, first) for f in chosen):
        return "axis"
    raise ValueError(
        f"faces {list(face_ids)} are neither coplanar faces nor one hole or boss, "
        "so they cannot form one datum"
    )


def _coplanar(f, g) -> bool:
    n = f["direction"]
    offset = sum((a - b) * c for a, b, c in zip(f["centroid"], g["centroid"], n, strict=True))
    return _same_direction(n, g["direction"]) and abs(offset) < 1e-4


def _coaxial(f, g) -> bool:
    if abs(f["radius"] - g["radius"]) > 1e-6 or not _parallel(f["direction"], g["direction"]):
        return False
    d = [a - b for a, b in zip(f["origin"], g["origin"], strict=True)]
    u = g["direction"]
    cross = (d[1] * u[2] - d[2] * u[1], d[2] * u[0] - d[0] * u[2], d[0] * u[1] - d[1] * u[0])
    return math.hypot(*cross) < 1e-4


def _same_direction(a, b) -> bool:
    return sum(x * y for x, y in zip(a, b, strict=True)) > 1.0 - 1e-6


def _answer(answers, qs, qid):
    if qid in answers:
        return answers[qid]
    for q in qs:
        if q.id == qid:
            return q.default
    return None


def _check_faces(chosen, faces) -> None:
    for i in chosen:
        if int(i) not in faces:
            raise ValueError(f"face {i} does not exist")


def _parallel(a, b) -> bool:
    return abs(abs(sum(x * y for x, y in zip(a, b, strict=True))) - 1.0) < 1e-6


def _perpendicular(a, b) -> bool:
    return abs(sum(x * y for x, y in zip(a, b, strict=True))) < 1e-6


def _fmt(value: float) -> str:
    return (
        f"{value:.3f}".rstrip("0").rstrip(".")
        if not math.isclose(value, round(value))
        else f"{value:g}"
    )
