"""specify-core's command line and serve protocol on the synthetic parts, for tests/cli.rs.

Run with specify-core's venv, from the repository root:

    /Users/paul/repos/specify-core/.venv/bin/python tests/fixtures/cli_oracle.py \
        | gzip -n -9 > tests/fixtures/cli_oracle.json.gz

Each run is the real `specify-core` executable (a child process, cwd the repository root,
inputs named by their repository paths): its exit code, stdout and stderr, and the text of the
files it was asked to write that are compared (the intent). Inputs it reads that are not
committed parts are made first in a scratch directory, written `{tmp}` in arguments and
outputs: `{"artifact": [case, name]}` is an oracle artifact (`tests/fixtures/oracle/<case>/
<name>.json.gz`, Python's own analysis), `{"json": value}` a JSON file, `{"text": s}` a file.

Normalisations: the scratch directory is written `{tmp}`; a traceback on stderr (Python's
uncaught exception) is written as its last line after `Traceback: `; OpenCascade's transfer
statistics, which its STEP writer prints on stdout (lines with ANSI colour escapes, before the
report), are cut from `stdout` and counted in `stdout_noise` (lines cut); in serve replies a
failure's `trace` is `(omitted)`.

The serve session is one child (`specify-core serve`) fed `lines` in turn: the protocol's
answers to malformed input, non-object requests, missing fields, unknown ops and refusals, and
preview anchoring across files: `{lines: [{line, reply}], exit}`, each line sent as is and its
reply (None for a blank line, which gets none), and the child's exit code once stdin closes.

`programs`: the executables specify-core installs (its `[project.scripts]`).

`json_text`: how Python's `json.dumps(value, indent=1)`, which writes the analysis and intent
files, writes a non-finite float and non-ASCII text: `{name: text}`.

Deterministic: run again, the output is the same.
"""

import gzip
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests/fixtures/oracle"
PARTS = "tests/fixtures/parts"
EXE = str(Path(sys.executable).parent / "specify-core")
BASE = {"part.material": "steel"}

sys.path.insert(0, "/Users/paul/repos/specify-core/src")


def artifact(case, name):
    return json.loads(gzip.decompress((ORACLE / case / f"{name}.json.gz").read_bytes()))


def step(stem):
    return f"{PARTS}/{stem}.step"


FILES = {
    # stem: number of parts
    "a_file_with_two_parts_must_say_which": 2,
    "a_hole_along_x_is_labelled_from_its_rim": 1,
    "a_placed_part_is_meshed_in_the_frame_the_analysis_uses": 1,
    "assembly": 2,
    "default_datum_a_takes_every_coplanar_face": 1,
    "four_corner_holes_are_quiddity_rectangle_with_members": 1,
    "identical_holes_no_pattern_claims_form_a_set": 1,
    "pin": 1,
    "plate": 1,
    "stack": 3,
    "wrapped": 1,
}


def case_of(stem, k, count):
    return f"parts/{stem}#{k}" if count > 1 else f"parts/{stem}"


def runs():
    """Every command line run: (name, argv, inputs, files to record)."""
    out = []

    def run(name, argv, inputs=None, files=()):
        out.append({"name": name, "argv": argv, "inputs": inputs or {}, "files": list(files)})

    for stem, count in FILES.items():
        s = step(stem)
        run(f"{stem}/parts", ["parts", s])
        pairs, inputs = [], {}
        for k in range(count):
            part = ["--part", str(k)] if count > 1 else []
            run(f"{stem}#{k}/analyse", ["analyse", s, *part, "-o", "{tmp}/analysis.json"])
            run(f"{stem}#{k}/mesh", ["mesh", s, *part, "-o", "{tmp}/mesh.json"])
            a = {"artifact": [case_of(stem, k, count), "analysis"]}
            run(f"{stem}#{k}/questions", ["questions", "{tmp}/a.json"], {"a.json": a})
            run(
                f"{stem}#{k}/questions-open",
                ["questions", "{tmp}/a.json", "--open", "--answers", "{tmp}/q.json"],
                {"a.json": a, "q.json": {"json": BASE}},
            )
            inputs[f"a{k}.json"] = a
            inputs[f"q{k}.json"] = {"json": BASE}
            pairs += [f"{{tmp}}/a{k}.json", f"{{tmp}}/q{k}.json"]
        run(
            f"{stem}/write",
            ["write", s, *pairs, "-o", "{tmp}/out.step", "--accept-defaults",
             "--intent", "{tmp}/intent.json"],
            inputs,
            ["intent.json"],
        )

    plate, pin, assembly = step("plate"), step("pin"), step("assembly")
    pa = {"artifact": ["parts/plate", "analysis"]}
    asm = {f"a{k}.json": {"artifact": [f"parts/assembly#{k}", "analysis"]} for k in (0, 1)}
    # Refusals and failures.
    run("errors/analyse-an-assembly-without-part", ["analyse", assembly, "-o", "{tmp}/a.json"])
    run("errors/analyse-no-such-part", ["analyse", plate, "--part", "3", "-o", "{tmp}/a.json"])
    run("errors/analyse-negative-part", ["analyse", plate, "--part", "-1", "-o", "{tmp}/a.json"])
    run("errors/analyse-no-such-file", ["analyse", "no/such.step", "-o", "{tmp}/a.json"])
    run("errors/mesh-an-assembly-without-part", ["mesh", assembly, "-o", "{tmp}/m.json"])
    run("errors/parts-no-such-file", ["parts", "no/such.step"])
    run(
        "errors/questions-answers-not-json",
        ["questions", "{tmp}/a.json", "--answers", "{tmp}/q.json"],
        {"a.json": pa, "q.json": {"text": "{"}},
    )
    run("errors/questions-no-such-analysis", ["questions", "{tmp}/none.json"])
    run(
        "errors/questions-a-refused-datum",
        ["questions", "{tmp}/a.json", "--answers", "{tmp}/q.json"],
        {"a.json": pa, "q.json": {"json": {"datum.A": [99999]}}},
    )
    run(
        "errors/write-unanswered",
        ["write", plate, "{tmp}/a.json", "{tmp}/q.json", "-o", "{tmp}/out.step"],
        {"a.json": pa, "q.json": {"json": BASE}},
    )
    run(
        "errors/write-no-material",
        ["write", plate, "{tmp}/a.json", "{tmp}/q.json", "-o", "{tmp}/out.step",
         "--accept-defaults"],
        {"a.json": pa, "q.json": {"json": {}}},
    )
    run(
        "errors/write-an-unanswered-part-is-named",
        ["write", assembly, "{tmp}/a0.json", "{tmp}/q0.json", "{tmp}/a1.json", "{tmp}/q1.json",
         "-o", "{tmp}/out.step", "--accept-defaults"],
        {**asm, "q0.json": {"json": BASE}, "q1.json": {"json": {}}},
    )
    run(
        "errors/write-a-refused-answer-names-the-part",
        ["write", assembly, "{tmp}/a0.json", "{tmp}/q0.json", "{tmp}/a1.json", "{tmp}/q1.json",
         "-o", "{tmp}/out.step", "--accept-defaults"],
        {**asm, "q0.json": {"json": BASE}, "q1.json": {"json": {**BASE, "datum.A": [99999]}}},
    )
    run(
        "errors/write-another-files-analysis",
        ["write", pin, "{tmp}/a.json", "{tmp}/q.json", "-o", "{tmp}/out.step",
         "--accept-defaults"],
        {"a.json": pa, "q.json": {"json": BASE}},
    )
    run(
        "errors/write-into-a-missing-directory",
        ["write", plate, "{tmp}/a.json", "{tmp}/q.json", "-o", "{tmp}/none/out.step",
         "--accept-defaults"],
        {"a.json": pa, "q.json": {"json": BASE}},
    )
    run(
        "errors/write-the-intent-before-a-refused-write",
        ["write", pin, "{tmp}/a.json", "{tmp}/q.json", "-o", "{tmp}/out.step",
         "--accept-defaults", "--intent", "{tmp}/intent.json"],
        {"a.json": pa, "q.json": {"json": BASE}},
        ["intent.json"],
    )
    # Usage.
    usage = [
        [], ["bogus"], ["analyse"], ["analyse", plate], ["analyse", plate, "-o"],
        ["analyse", plate, "-o", "{tmp}/a.json", "--part", "q"], ["mesh"], ["parts"],
        ["questions"], ["write", plate], ["write", plate, "{tmp}/a.json", "-o", "{tmp}/o.step"],
        ["write", plate, "a", "b", "c", "-o", "{tmp}/o.step"], ["serve", "extra"],
        ["analyse", plate, "-o", "{tmp}/a.json", "z"], ["questions", "a", "--bogus"],
        ["write", plate, "a", "b", "-o", "{tmp}/o.step", "--intent"],
        ["-h"], ["parts", "-h"], ["analyse", "--help"], ["mesh", "-h"], ["questions", "-h"],
        ["write", "-h"], ["serve", "-h"],
    ]
    for i, argv in enumerate(usage):
        run(f"usage/{i:02d} {' '.join(argv)}".rstrip(), argv)
    # Abbreviated and attached option forms, as argparse takes them.
    run("forms/output-equals-and-part-abbreviated",
        ["analyse", f"--output={{tmp}}/a.json", "--pa", "1", assembly])
    run("forms/short-option-attached", ["mesh", pin, "-o{tmp}/m.json"])
    return out


def materialise(inputs, tmp):
    for name, spec in inputs.items():
        path = tmp / name
        if "artifact" in spec:
            path.write_text(json.dumps(artifact(*spec["artifact"])))
        elif "json" in spec:
            path.write_text(json.dumps(spec["json"]))
        else:
            path.write_text(spec["text"])


def scrub(text, tmp):
    return text.replace(str(tmp), "{tmp}")


def denoised(text):
    """stdout without OpenCascade's statistics (every line up to the last with an ANSI escape),
    and how many lines were cut."""
    lines = text.splitlines(keepends=True)
    noisy = [i for i, line in enumerate(lines) if "\x1b[" in line]
    if not noisy:
        return text, 0
    return "".join(lines[noisy[-1] + 1 :]), noisy[-1] + 1


def stderr_of(text, tmp):
    text = scrub(text, tmp)
    if text.startswith("Traceback"):
        return "Traceback: " + text.strip().splitlines()[-1] + "\n"
    return text


def run_all():
    results = []
    for r in runs():
        tmp = Path(tempfile.mkdtemp(prefix="cli-oracle-")).resolve()
        try:
            materialise(r["inputs"], tmp)
            argv = [a.replace("{tmp}", str(tmp)) for a in r["argv"]]
            done = subprocess.run(
                [EXE, *argv], cwd=ROOT, capture_output=True, text=True, timeout=600
            )
            files = {}
            for name in r["files"]:
                p = tmp / name
                files[name] = scrub(p.read_text(), tmp) if p.exists() else None
            stdout, noise = denoised(scrub(done.stdout, tmp))
            results.append(
                {
                    **r,
                    "exit": done.returncode,
                    "stdout": stdout,
                    "stdout_noise": noise,
                    "stderr": stderr_of(done.stderr, tmp),
                    "written": files,
                }
            )
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
    return results


def serve_lines():
    """The serve session's lines, as sent."""
    from specify_core import rules

    plate = artifact("parts/plate", "analysis")
    pin = artifact("parts/pin", "analysis")
    old_pin = json.loads(json.dumps(pin))
    old_pin["binding"].pop("part")
    typed = next(q.id for q in rules.questions(plate, {}) if q.id.startswith("hole.function:"))
    assembly = [artifact(f"parts/assembly#{k}", "analysis") for k in (0, 1)]
    requests = [
        {"id": 1, "op": "hello"},
        "{",
        [],
        None,
        3,
        3.5,
        True,
        "hello",
        "",
        {"id": 2},
        {"id": 3, "op": "bogus", "analysis": {}},
        {"id": 4, "op": "questions", "analysis": {"faces": [], "features": []},
         "answers": {"datum.A": [5]}},
        {"id": 5, "op": "write", "step": "x",
         "analysis": {"binding": {}, "faces": [], "features": [], "existing": []},
         "answers": {}, "output": "y"},
        {"id": 6, "op": "analyse", "step": "/no/such/file.step"},
        {"id": 7, "op": "parts"},
        {"id": 8, "op": "analyse", "step": step("assembly")},
        {"id": 9, "op": "interpret", "analysis": plate, "answers": {}, "question": "nope",
         "text": "M6"},
        {"id": 10, "op": "interpret", "analysis": plate, "answers": {}, "question": typed,
         "text": "zzz"},
        {"id": 11, "op": "interpret", "analysis": plate, "answers": {}, "question": typed},
        {"id": 12, "op": "preview", "analysis": pin, "answers": {}, "step": step("plate")},
        {"id": 13, "op": "preview", "analysis": old_pin, "answers": {}},
        {"id": 14, "op": "write", "step": step("assembly"), "output": "{tmp}/x.step",
         "accept_defaults": True,
         "parts": [{"analysis": assembly[0], "answers": BASE},
                   {"analysis": assembly[1], "answers": {}}]},
        {"id": 15, "op": "write", "step": step("assembly"), "output": "{tmp}/x.step",
         "accept_defaults": True,
         "parts": [{"analysis": assembly[0], "answers": BASE},
                   {"analysis": assembly[1], "answers": {"datum.A": [99999]}}]},
        {"id": 16, "op": "write", "step": step("pin"), "output": "{tmp}/x.step",
         "accept_defaults": True, "analysis": plate, "answers": BASE},
        {"id": 17, "op": "hello"},
    ]
    return [r if isinstance(r, str) and r in ("{", "") else json.dumps(r) for r in requests]


def serve_session():
    tmp = Path(tempfile.mkdtemp(prefix="cli-oracle-serve-")).resolve()
    try:
        child = subprocess.Popen(
            [EXE, "serve"], cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True,
        )
        session = []
        for line in serve_lines():
            sent = line.replace("{tmp}", str(tmp))
            child.stdin.write(sent + "\n")
            child.stdin.flush()
            if not sent.strip():
                session.append({"line": line, "reply": None})
                continue
            reply = json.loads(child.stdout.readline())
            if not reply.get("ok") and "trace" in reply.get("error", {}):
                reply["error"]["trace"] = "(omitted)"
            session.append({"line": line, "reply": json.loads(scrub(json.dumps(reply), tmp))})
        child.stdin.close()
        return {"lines": session, "exit": child.wait(timeout=60)}
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def programs():
    """The executables specify-core's package installs."""
    import tomllib

    project = tomllib.loads(Path("/Users/paul/repos/specify-core/pyproject.toml").read_text())
    return sorted(project["project"]["scripts"])


def json_text():
    """The analysis file's encoder on what it writes differently from a strict JSON encoder."""
    values = {
        "nan": float("nan"),
        "infinity": float("inf"),
        "minus_infinity": float("-inf"),
        "non_ascii": "Ø6.6 H11, ±0.1, Ra 0.8 µm, M5×0.8, ⌀4.2",
    }
    return {name: json.dumps(value, indent=1) for name, value in values.items()}


if __name__ == "__main__":
    out = {
        "runs": run_all(),
        "serve": serve_session(),
        "programs": programs(),
        "json_text": json_text(),
    }
    json.dump(out, sys.stdout, indent=1)
    sys.stdout.write("\n")
