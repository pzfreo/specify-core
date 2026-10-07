"""Command line: parts, analyse, mesh, questions, write."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import api
from .merge import MergeError
from .rules import IncompleteError, open_questions
from .writer import VerificationError


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="specify-core")
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("parts", help="list an assembly's parts")
    p.add_argument("step", type=Path)

    p = sub.add_parser("analyse", help="recognise features and read existing PMI")
    p.add_argument("step", type=Path)
    p.add_argument("-o", "--output", type=Path, required=True, help="analysis JSON")
    p.add_argument("--part", type=int, help="which part of an assembly (see parts)")

    sub.add_parser("serve", help="answer JSON-line requests on stdin (for a host process)")

    p = sub.add_parser("mesh", help="triangles ranged by face index, for a picker")
    p.add_argument("step", type=Path)
    p.add_argument("-o", "--output", type=Path, required=True, help="mesh JSON")
    p.add_argument("--part", type=int, help="which part of an assembly (see parts)")

    p = sub.add_parser("questions", help="list the questions, given answers so far")
    p.add_argument("analysis", type=Path)
    p.add_argument("--answers", type=Path, help="answers JSON: {question id: value}")
    p.add_argument("--open", action="store_true", help="only unanswered questions")

    p = sub.add_parser("write", help="write intent as AP242 PMI")
    p.add_argument("step", type=Path)
    p.add_argument(
        "pairs",
        type=Path,
        nargs="+",
        metavar="ANALYSIS ANSWERS",
        help="analysis and answers JSON; in an assembly, a pair for each part to write",
    )
    p.add_argument("-o", "--output", type=Path, required=True)
    p.add_argument("--accept-defaults", action="store_true", help="take defaults for the rest")
    p.add_argument("--intent", type=Path, help="also write the intent JSON here")

    args = parser.parse_args(argv)
    if args.command == "serve":
        from .serve import serve

        return serve()
    if args.command == "parts":
        print(json.dumps(api.parts(args.step), indent=1, ensure_ascii=False))
        return 0
    if args.command == "analyse":
        analysis = api.analyse(args.step, args.part)
        args.output.write_text(json.dumps(analysis, indent=1))
        counts: dict[str, int] = {}
        for f in analysis["features"]:
            counts[f["family"]] = counts.get(f["family"], 0) + 1
        print(
            json.dumps(
                {
                    "faces": len(analysis["faces"]),
                    "features": counts,
                    "existing_pmi": len(analysis["existing"]),
                }
            )
        )
        return 0

    if args.command == "mesh":
        mesh = api.mesh(args.step, args.part)
        args.output.write_text(json.dumps(mesh, separators=(",", ":")))
        print(json.dumps({"faces": len(mesh["faces"]), "triangles": len(mesh["indices"]) // 3}))
        if mesh["missing"]:
            print(f"warning: faces {mesh['missing']} did not triangulate", file=sys.stderr)
        return 0

    if args.command == "questions":
        pairs = [(args.analysis, args.answers)]
    elif len(args.pairs) % 2:
        parser.error("write takes an analysis and its answers for each part")
    else:
        pairs = list(zip(args.pairs[::2], args.pairs[1::2], strict=True))
    written = []
    try:
        for analysis_path, answers_path in pairs:
            analysis = json.loads(analysis_path.read_text())
            answers = json.loads(answers_path.read_text()) if answers_path else {}
            if args.command == "questions":
                qs = api.questions(analysis, answers)
                if args.open:
                    qs = open_questions(qs, answers)
                print(json.dumps([q.to_dict() for q in qs], indent=1, ensure_ascii=False))
                return 0
            intent = api.apply(analysis, answers, accept_defaults=args.accept_defaults)
            written.append((intent, answers))
    except IncompleteError as exc:
        print(f"incomplete: {exc}", file=sys.stderr)
        return 2
    except ValueError as exc:
        # An answer that cannot be used: a bad datum, or a frame no longer offered.
        print(f"invalid answer: {exc}", file=sys.stderr)
        return 2
    if args.intent:
        intents = [intent.to_dict() for intent, _ in written]
        args.intent.write_text(json.dumps(intents[0] if len(intents) == 1 else intents, indent=1))
    try:
        if len(written) == 1:
            intent, answers = written[0]
            report = api.write(args.step, intent, args.output, answers=answers)
        else:
            report = api.write_parts(args.step, written, args.output)
    except ValueError as exc:
        # An intent for another file, or another loader's face numbering.
        print(f"cannot write: {exc}", file=sys.stderr)
        return 2
    except (VerificationError, MergeError) as exc:
        print(f"write failed, {args.output} left as it was: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(report.to_dict(), indent=1, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
