"""Capture one input file: every artifact of every part, as tests/fixtures/oracle/README.md lays
them out. Each file is captured in a process of its own (``capture.py`` runs a pool), so OCCT's
caches and serve's anchor cache cannot carry anything from one file into another."""

from __future__ import annotations

import json
import shutil
from pathlib import Path
from typing import Any

from .common import (
    BASE_ANSWERS,
    FIXED_TIME_STAMP,
    NIST,
    PARTS,
    attempt,
    corpus_dir,
    failed,
    fixed_time_stamp,
    materialise,
    plain,
    provenance,
    reset_dir,
    scratch_dir,
    scrub,
    sha256,
    strip_input_suffix,
    today,
    write_bytes_gz,
    write_json_gz,
)

#: Kinds of input: where each lives, and whether its written STEP is kept (``written.step.gz``)
#: and a serve transcript made.
KINDS = {
    "parts": {"root": PARTS, "keep_written": True, "serve": True},
    "nist": {"root": NIST, "keep_written": True, "serve": False},
    "corpus": {"root": None, "keep_written": False, "serve": False},
}

#: What each meta.json says was done to Python's output, so a reader knows what is not raw.
NORMALISATIONS = [
    "Paths: the input file's path is written {input} and every output path {output} in every "
    "artifact (messages, the write report's output, serve requests).",
    "Non-finite floats, which Python's json writes as bare NaN/Infinity tokens, are written "
    '{"$float": "NaN" | "Infinity" | "-Infinity"}.',
    f"written.step.gz: the FILE_NAME time stamp is replaced by {FIXED_TIME_STAMP}.",
    "serve transcripts: an error's traceback ('trace') is replaced by '(omitted)'; the analyse "
    "reply's mesh is replaced by its summary (as mesh.json).",
]


def root_of(kind: str) -> Path:
    root = KINDS[kind]["root"]
    return corpus_dir() if root is None else root


def case_name(kind: str, rel: str, part: int | None = None) -> str:
    """``parts/plate``, ``corpus/cadgenbench_inputs/cgb202`` (``.gz`` dropped),
    ``parts/assembly#1`` for a part of a file of several."""
    name = f"{kind}/{strip_input_suffix(rel)}"
    return name if part is None else f"{name}#{part}"


def capture_file(kind: str, rel: str, oracle: Path) -> dict[str, Any]:
    """Capture input ``rel`` (relative to its kind's root) into ``oracle``; returns a summary
    (case names written and the failures recorded) for the run's log."""
    from specify_core import api

    source = root_of(kind) / rel
    stored = source.read_bytes()
    scratch = scratch_dir()
    try:
        path = materialise(source, scratch)
        out = scratch / "written.step"
        subs = {str(path): "{input}", str(source): "{input}", str(scratch): "{output}"}
        base = {
            "kind": kind,
            "input": rel,
            "input_sha256": sha256(stored),
        }
        if path != source:
            base["decompressed_sha256"] = sha256(path.read_bytes())
        base |= provenance()
        base |= {
            "command": f"tools/capture.py case {_display(kind, rel)}",
            "captured": today(),
            "answers": BASE_ANSWERS,
            "normalisations": NORMALISATIONS,
        }

        name = case_name(kind, rel)
        for stale in list(oracle.glob(_glob_escape(name) + "#*")):
            shutil.rmtree(stale)
        file_dir = oracle / name
        reset_dir(file_dir)

        parts = attempt(lambda: api.parts(path))
        count = len(parts) if not failed(parts) else 1
        summary: dict[str, Any] = {"cases": [name], "failures": []}
        artifacts: dict[str, Any] = {"parts": parts}
        analyses: dict[int, Any] = {}
        if count > 1:
            # api.analyse of a file of several parts refuses without a part: a result too.
            artifacts["analysis"] = attempt(lambda: api.analyse(path))
            for k in range(count):
                part_name = case_name(kind, rel, k)
                part_dir = oracle / part_name
                reset_dir(part_dir)
                analyses[k] = _capture_part(
                    path, k, part_dir, {**base, "case": part_name, "part": k}, subs
                )
                summary["cases"].append(part_name)
        else:
            analyses[0] = _capture_part(path, None, file_dir, None, subs, artifacts)

        # The write: every part with the base answers and defaults, its answers stored, as
        # serve's write does; then the written file read back part by part.
        if all(not failed(a) for a in analyses.values()):
            artifacts["write"] = attempt(lambda: _write(path, analyses, out))
            if not failed(artifacts["write"]):
                artifacts["readback"] = readback(out)
                if KINDS[kind]["keep_written"]:
                    write_bytes_gz(file_dir / "written.step.gz", fixed_time_stamp(out.read_bytes()))
        else:
            artifacts["write"] = {"skipped": "an analysis failed"}
        if KINDS[kind]["serve"]:
            artifacts["serve"] = _transcript(path, count, scratch / "served.step")

        meta = {**base, "case": name, "part": None, "parts": count}
        _save(file_dir, meta, artifacts, subs)
        for case in summary["cases"]:
            for artifact in sorted((oracle / case).glob("*.json.gz")):
                if artifact.name == "meta.json.gz":
                    continue
                value = json.loads(_gunzip(artifact))
                if failed(value):
                    summary["failures"].append(f"{case}/{artifact.name}: {value['error']}")
        return summary
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def _capture_part(
    path: Path,
    part: int | None,
    folder: Path,
    meta: dict[str, Any] | None,
    subs: dict[str, str],
    artifacts: dict[str, Any] | None = None,
) -> Any:
    """Analysis, questions, problems, preview, apply, mesh summary and anchors of one part,
    saved in ``folder`` (with ``meta``; for a file of one part, into ``artifacts``, saved with
    the file's). Returns the analysis (JSON round-tripped), or its failure."""
    from specify_core import api, rules, serve
    from specify_core.load import load
    from specify_core.problems import problems

    out: dict[str, Any] = {}
    analysis = attempt(lambda: plain(api.analyse(path, part)))
    out["analysis"] = analysis
    if not failed(analysis):
        out["questions"] = {
            "none": attempt(lambda: _questions(analysis, {})),
            "defaults": attempt(
                lambda: _questions(analysis, plain(rules.with_all_defaults(analysis, {})))
            ),
        }
        out["problems"] = attempt(lambda: plain(problems(analysis, {})))
        preview = attempt(lambda: plain(api.preview(analysis, {}).to_dict()))
        out["preview"] = preview
        out["apply"] = attempt(
            lambda: plain(api.apply(analysis, BASE_ANSWERS, accept_defaults=True).to_dict())
        )
        out["mesh"] = attempt(lambda: mesh_summary(api.mesh(path, part)))
        if not failed(preview):

            def anchors() -> dict[str, Any]:
                serve._PARTS.clear()
                serve._ANCHORS.clear()
                loaded = load(path, part=part)
                return plain(
                    {
                        "diagonal": serve._diagonal(loaded),
                        "anchors": serve._label_anchors(path, analysis, preview),
                    }
                )

            out["anchors"] = attempt(anchors)
    if artifacts is not None:
        artifacts.update(out)
    else:
        _save(folder, meta, out, subs)
    return analysis


def _questions(analysis: dict[str, Any], answers: dict[str, Any]) -> dict[str, Any]:
    from specify_core import rules

    qs = rules.questions(analysis, answers)
    return plain(
        {
            "answers": answers,
            "questions": [q.to_dict() for q in qs],
            "open": [q.id for q in rules.open_questions(qs, answers)],
        }
    )


def _write(path: Path, analyses: dict[int, Any], out: Path) -> dict[str, Any]:
    """Each part applied with the base answers and defaults, and written with its answers
    stored (serve's write; several parts together, as serve's write of ``parts``)."""
    from specify_core import api
    from specify_core.rules import IncompleteError

    intents = []
    for k, analysis in analyses.items():
        try:
            intents.append(api.apply(analysis, BASE_ANSWERS, accept_defaults=True))
        except IncompleteError as exc:
            if len(analyses) == 1:
                raise
            raise IncompleteError(exc.missing, k) from None
    if len(intents) == 1:
        report = api.write(path, intents[0], out, answers=dict(BASE_ANSWERS))
    else:
        report = api.write_parts(path, [(i, dict(BASE_ANSWERS)) for i in intents], out)
    return plain(report.to_dict())


def readback(step: Path) -> Any:
    """The written file read back directly, one entry per part (list index = part): what
    specify-core reads of its PMI (``existing``), its part settings (``part``) and its stored
    answers (``resume``). Not ``api.analyse``: for a file with answers it can resume, that
    returns ``existing: []`` and ``part: {}``. A failing call gives its failure in place."""
    from specify_core.load import load_all

    count = attempt(lambda: len(load_all(step)))
    if failed(count):
        return count
    return [_readback_part(step, k) for k in range(count)]


def _readback_part(step: Path, k: int) -> dict[str, Any]:
    from specify_core import existing, resume
    from specify_core.load import load

    loaded = attempt(lambda: load(step, part=k))
    if failed(loaded):
        return {"existing": loaded, "part": loaded, "resume": loaded}
    return plain(
        {
            "existing": attempt(lambda: [e.to_dict() for e in existing.read_existing(loaded)]),
            "part": attempt(lambda: existing.part_settings(loaded)),
            "resume": attempt(lambda: resume.answers_for(loaded)),
        }
    )


def mesh_summary(mesh: dict[str, Any]) -> dict[str, Any]:
    """A mesh by its properties, not its triangles: face count, triangles per face, missing
    faces, bbox, each face's mesh area, and the edge polylines' count and total length."""
    positions, indices = mesh["positions"], mesh["indices"]

    def point(i: int) -> tuple[float, float, float]:
        return positions[3 * i], positions[3 * i + 1], positions[3 * i + 2]

    areas = []
    for start, count in mesh["faces"]:
        area = 0.0
        for t in range(start, start + count):
            a, b, c = (point(indices[3 * t + j]) for j in range(3))
            u = [b[i] - a[i] for i in range(3)]
            v = [c[i] - a[i] for i in range(3)]
            n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
            area += 0.5 * (n[0] ** 2 + n[1] ** 2 + n[2] ** 2) ** 0.5
        areas.append(area)
    length = 0.0
    for line in mesh["edges"]:
        for j in range(3, len(line), 3):
            length += sum((line[j + i] - line[j - 3 + i]) ** 2 for i in range(3)) ** 0.5
    return plain(
        {
            "format": mesh["format"],
            "version": mesh["version"],
            "binding": mesh["binding"],
            "bbox": mesh["bbox"],
            "face_count": len(mesh["faces"]),
            "triangles": [count for _, count in mesh["faces"]],
            "missing": mesh["missing"],
            "areas": areas,
            "edges": {"count": len(mesh["edges"]), "length": length},
        }
    )


def _transcript(path: Path, count: int, out: Path) -> list[dict[str, Any]]:
    """One serve session through ``serve.reply`` (the same handler the child process runs,
    in process): hello, parts, and per part analyse, questions, interpret, preview with the
    step; then a write of every part. Each entry is ``{request, reply}``."""
    from specify_core import choices, serve

    serve._PARTS.clear()
    serve._ANCHORS.clear()
    session: list[dict[str, Any]] = []
    next_id = iter(range(1, 1000))

    def ask(request: dict[str, Any]) -> dict[str, Any]:
        request = {"id": next(next_id), **request}
        reply = plain(serve.reply(plain(request)))
        if not reply.get("ok") and "trace" in reply.get("error", {}):
            reply["error"]["trace"] = "(omitted)"
        shown = plain(reply)
        if reply.get("ok") and "mesh" in reply.get("result", {}):
            shown["result"]["mesh"] = mesh_summary(reply["result"]["mesh"])
        session.append({"request": request, "reply": shown})
        return reply

    ask({"op": "hello"})
    ask({"op": "parts", "step": str(path)})
    analyses = []
    for k in range(count):
        request: dict[str, Any] = {"op": "analyse", "step": str(path)}
        if count > 1:
            request["part"] = k
        analysed = ask(request)
        if not analysed.get("ok"):
            return session
        analysis = analysed["result"]["analysis"]
        analyses.append(analysis)
        asked = ask({"op": "questions", "analysis": analysis, "answers": {}})
        # Typed as its default's code: the first hole or diameter whose default is a spec.
        typed = next(
            (
                q
                for q in asked.get("result", {}).get("questions", [])
                if q["kind"] == "choice"
                and q["id"].startswith(("hole.function:", "diameter.fit:"))
                and isinstance(q.get("default"), str)
                and q["default"] != "general"
            ),
            None,
        )
        if typed is not None:
            ask(
                {
                    "op": "interpret",
                    "analysis": analysis,
                    "answers": {},
                    "question": typed["id"],
                    "text": choices.code(typed["default"]),
                }
            )
        ask({"op": "preview", "analysis": analysis, "answers": BASE_ANSWERS, "step": str(path)})
    write: dict[str, Any] = {
        "op": "write",
        "step": str(path),
        "output": str(out),
        "accept_defaults": True,
    }
    if count > 1:
        write["parts"] = [{"analysis": a, "answers": BASE_ANSWERS} for a in analyses]
    else:
        write |= {"analysis": analyses[0], "answers": BASE_ANSWERS}
    ask(write)
    return session


def _save(
    folder: Path, meta: dict[str, Any], artifacts: dict[str, Any], subs: dict[str, str]
) -> None:
    meta = {**meta, "artifacts": sorted(artifacts)}
    write_json_gz(folder / "meta.json.gz", scrub(meta, subs))
    for name, value in artifacts.items():
        write_json_gz(folder / f"{name}.json.gz", scrub(value, subs))


def _display(kind: str, rel: str) -> str:
    """The input as the command line names it: committed inputs by their repository path,
    corpus inputs under $QUIDDITY_CORPUS."""
    if kind == "corpus":
        return f"$QUIDDITY_CORPUS/{rel}"
    return str((root_of(kind) / rel).relative_to(PARTS.parents[2]))


def _glob_escape(name: str) -> str:
    return "".join(f"[{c}]" if c in "*?[" else c for c in name)


def _gunzip(path: Path) -> bytes:
    import gzip

    return gzip.decompress(path.read_bytes())
