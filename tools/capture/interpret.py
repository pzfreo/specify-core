"""Typed specs: every input specify-core's tests type at ``choices`` (test_choices.py, and the
location tolerances of test_locations.py), with Python's answer or refusal, written to
``tests/fixtures/oracle/interpret.json.gz`` as a list of entries:

    {"from": test, "fn": "interpret", "question": id, "text": typed, "options": [...],
     "diameter": d | null, "depth": d | null, "result": value | {error, message}}

``fn`` is ``interpret`` or ``check`` (same arguments; ``text`` is the stored value checked),
``code`` (``value`` -> text), ``callout`` (``value``, ``diameter``, ``hole``) or ``describe``
(``question``, ``options``, ``diameter`` -> the choices). test_call_out.py's typed values are
added requirements, read by the rules: they are scenarios (``callout_*``), not entries here.
"""

from __future__ import annotations

from typing import Any

from .common import attempt, plain

HOLE = "hole.function:x"
SHAFT = "diameter.fit:x"
HOLE_OPTIONS = ["general", "clearance:M6", "fit:H7", "fit:H8"]
SRC = "test_choices.py::"


def _interpret(
    source: str,
    qid: str,
    text: str,
    options: list[str],
    d: float | None,
    depth: float | None = None,
    fn: str = "interpret",
) -> dict[str, Any]:
    return {
        "from": source,
        "fn": fn,
        "question": qid,
        "text": text,
        "options": options,
        "diameter": d,
        "depth": depth,
    }


def entries() -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    typed = [
        (HOLE, "H7", 6.6),
        (HOLE, "h6", 6.6),
        (HOLE, "JS7", 6.6),
        (HOLE, "M6", 6.6),
        (HOLE, "clearance M6", 6.6),
        (HOLE, "M2", 1.6),
        (HOLE, "tapped M2x0.4", 1.6),
        (HOLE, "none", 6.6),
        (SHAFT, "G6", 10.0),
        (SHAFT, "m6", 10.0),
        (SHAFT, "M10", 10.0),
        (SHAFT, "diamond knurl", 10.0),
        ("hole.position:x", "⌀0.150", None),
        ("hole.position:x", "none", None),
        ("part.general_tolerance", "f", None),
    ]
    for qid, text, d in typed:
        options = ["ISO 2768-f", "ISO 2768-m"] if qid.startswith("part.") else []
        out.append(_interpret(SRC + "test_typed_specs_are_read_as_answers", qid, text, options, d))
    shaft_options = ["general", "h6", "m6", "thread:M6", "knurl:diamond"]
    for text in ("M6", "m6", "M6 thread"):
        out.append(
            _interpret(
                SRC + "test_on_a_shaft_capital_m_is_a_thread_and_lower_case_a_fit",
                SHAFT,
                text,
                shaft_options,
                6.0,
            )
        )
    out.append(
        _interpret(
            SRC + "test_on_a_shaft_capital_m_is_a_thread_and_lower_case_a_fit",
            HOLE,
            "h7",
            ["fit:H7"],
            8.0,
        )
    )
    refused = [
        (HOLE, "M8 tapped", 1.6),
        (HOLE, "clearance M8", 6.6),
        (HOLE, "M6 clearance", 12.0),
        (HOLE, "g6", 6.6),
        (HOLE, "M6x0.75", 5.0),
        (HOLE, "M7", 6.0),
        (SHAFT, "M6", 10.0),
        (SHAFT, "knurl", 10.0),
        ("hole.position:x", "-1", None),
    ]
    for qid, text, d in refused:
        out.append(
            _interpret(SRC + "test_specs_that_do_not_fit_the_part_are_refused", qid, text, [], d)
        )
    for text in (
        "M2 x 0.4, usable thread depth 2.3 mm",
        "M2 tapped, 2 deep",
        "M2x0.4-6H 1.5mm min full thread",
        "M2 tapped .5 deep",
        "M2 tapped 2deep",
        "M2 tapped 2mmdeep",
        "M2-6H",
        "tapped:M2@2.3",
    ):
        out.append(
            _interpret(
                SRC + "test_a_tapped_hole_keeps_a_typed_thread_depth", HOLE, text, [], 1.6, 3.5
            )
        )
    for text, depth in (
        ("M2 x 0.4, usable thread depth 3.15 mm", 3.5),
        ("M2 tapped, 2 deep", None),
        ("M2 tapped -2 deep", 3.5),
        ("M2 tapped 1e1 deep", 3.5),
        ("M2 3.15", 3.5),
        ("M2 tapped 2 deep 3 long", 3.5),
        ("M2 2 in deep", 3.5),
        ("M2-6G", 3.5),
    ):
        out.append(
            _interpret(
                SRC + "test_a_number_that_cannot_be_kept_is_refused_not_dropped",
                HOLE,
                text,
                [],
                1.6,
                depth,
            )
        )
    clearance = SRC + "test_a_clearance_hole_takes_no_depth_but_may_name_its_fit_and_standard"
    out.append(_interpret(clearance, HOLE, "M6 clearance, 10 deep", [], 6.6, 10.0))
    out.append(_interpret(clearance, HOLE, "M6 clearance H11", [], 6.6))
    out.append(_interpret(clearance, HOLE, "M6 clearance ISO 273", [], 6.6))
    stored = SRC + "test_a_stored_depth_written_otherwise_is_refused_with_what_it_reads_as"
    out.append(_interpret(stored, HOLE, "tapped:M2@2.30", [], 1.6, 3.5, fn="check"))
    out.append(_interpret(stored, HOLE, "tapped:M2@x", [], 1.6, 3.5, fn="check"))
    for text in ("0.03", "±0.1", "+-0.2", "none"):
        out.append(
            _interpret(
                "test_locations.py::test_a_typed_location_tolerance",
                "hole.location:x",
                text,
                [],
                None,
            )
        )
    # The serve child's interpret (test_threads.py::test_the_serve_child_reports_the_depth_it_kept)
    # on the pin's tap drill: Ø4.2, 8 deep.
    for text in ("M5 tapped, 5 mm full thread", "M5, usable thread depth 7 mm"):
        out.append(
            _interpret(
                "test_threads.py::test_the_serve_child_reports_the_depth_it_kept",
                HOLE,
                text,
                [],
                4.2,
                8.0,
            )
        )

    out.append(
        {
            "from": SRC + "test_each_option_has_a_purpose_a_code_and_a_callout",
            "fn": "describe",
            "question": HOLE,
            "options": HOLE_OPTIONS,
            "diameter": 6.6,
        }
    )
    out.append(
        {
            "from": SRC + "test_each_option_has_a_purpose_a_code_and_a_callout",
            "fn": "describe",
            "question": "hole.position:x",
            "options": ["0.1"],
            "diameter": 6.6,
        }
    )
    for value, d, hole in (
        ("fit:H7", 8.0, True),
        ("tapped:M2", 1.6, True),
        ("g6", 10.0, False),
        ("tapped:M2@2.3", 1.6, True),
        ("tapped:M5@5", 4.2, True),
    ):
        out.append(
            {
                "from": SRC
                + "test_each_option_has_a_purpose_a_code_and_a_callout; "
                + SRC
                + "test_a_thread_depth_is_named_in_the_code_and_callout",
                "fn": "callout",
                "value": value,
                "diameter": d,
                "hole": hole,
            }
        )
    for value in (
        "tapped:M2@2.3",
        "fit:H7",
        "clearance:M6",
        "general",
        "g6",
        "thread:M6",
        "knurl:diamond",
        "tapped:M5@5",
    ):
        out.append(
            {
                "from": SRC + "test_a_thread_depth_is_named_in_the_code_and_callout",
                "fn": "code",
                "value": value,
            }
        )
    return out


def capture() -> list[dict[str, Any]]:
    """Each entry with Python's result."""
    from specify_core import choices

    out = []
    for entry in entries():
        fn = entry["fn"]
        if fn in ("interpret", "check"):
            call = getattr(choices, fn)

            def result(e: dict[str, Any] = entry, call: Any = call) -> Any:
                return call(
                    e["question"], e["text"], tuple(e["options"]), e["diameter"], e["depth"]
                )

        elif fn == "describe":

            def result(e: dict[str, Any] = entry) -> Any:
                return choices.describe(e["question"], tuple(e["options"]), e["diameter"])

        elif fn == "callout":

            def result(e: dict[str, Any] = entry) -> Any:
                return choices.callout(e["value"], e["diameter"], e["hole"])

        else:

            def result(e: dict[str, Any] = entry) -> Any:
                return choices.code(e["value"])

        out.append({**entry, "result": plain(attempt(result))})
    return out
