"""Command line: analyse, mesh, questions, write."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import api
from .rules import IncompleteError, open_questions


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="specify-core")
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("analyse", help="recognise features and read existing PMI")
    p.add_argument("step", type=Path)
    p.add_argument("-o", "--output", type=Path, required=True, help="analysis JSON")

    sub.add_parser("serve", help="answer JSON-line requests on stdin (for a host process)")

    p = sub.add_parser("mesh", help="triangles ranged by face index, for a picker")
    p.add_argument("step", type=Path)
    p.add_argument("-o", "--output", type=Path, required=True, help="mesh JSON")

    p = sub.add_parser("questions", help="list the questions, given answers so far")
    p.add_argument("analysis", type=Path)
    p.add_argument("--answers", type=Path, help="answers JSON: {question id: value}")
    p.add_argument("--open", action="store_true", help="only unanswered questions")

    p = sub.add_parser("write", help="write intent as AP242 PMI")
    p.add_argument("step", type=Path)
    p.add_argument("analysis", type=Path)
    p.add_argument("answers", type=Path)
    p.add_argument("-o", "--output", type=Path, required=True)
    p.add_argument("--accept-defaults", action="store_true", help="take defaults for the rest")
    p.add_argument("--intent", type=Path, help="also write the intent JSON here")

    args = parser.parse_args(argv)
    if args.command == "serve":
        from .serve import serve

        return serve()
    if args.command == "analyse":
        analysis = api.analyse(args.step)
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
        mesh = api.mesh(args.step)
        args.output.write_text(json.dumps(mesh, separators=(",", ":")))
        print(json.dumps({"faces": len(mesh["faces"]), "triangles": len(mesh["indices"]) // 3}))
        if mesh["missing"]:
            print(f"warning: faces {mesh['missing']} did not triangulate", file=sys.stderr)
        return 0

    analysis = json.loads(args.analysis.read_text())
    answers = json.loads(args.answers.read_text()) if args.answers else {}
    try:
        if args.command == "questions":
            qs = api.questions(analysis, answers)
            if args.open:
                qs = open_questions(qs, answers)
            print(json.dumps([q.to_dict() for q in qs], indent=1, ensure_ascii=False))
            return 0
        intent = api.apply(analysis, answers, accept_defaults=args.accept_defaults)
    except IncompleteError as exc:
        print(f"incomplete: {exc}", file=sys.stderr)
        return 2
    except ValueError as exc:
        # An answer that cannot be used: a bad datum, or a frame no longer offered.
        print(f"invalid answer: {exc}", file=sys.stderr)
        return 2
    if args.intent:
        args.intent.write_text(json.dumps(intent.to_dict(), indent=1))
    report = api.write(args.step, intent, args.output, answers=answers)
    print(json.dumps(report.to_dict(), indent=1, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
