"""Python's choices, standards and materials outputs over a grid, for tests/choices.rs.

Run once with specify-core's venv, from the repository root:

    /Users/paul/repos/specify-core/.venv/bin/python tests/fixtures/choices_oracle.py \
        | gzip -n -9 > tests/fixtures/choices_oracle.json.gz

The grid: every hole diameter in the TAP_DRILL and CLEARANCE tables and 20 odd diameters;
at each, the options the rules offer a hole and a turned diameter (built here as rules.py builds
them), each option's code and callout, describe() of both questions, and check() of each option;
then interpret() of typed texts (those of the Python tests and more, with variations of case and
spacing) for holes (with no options, the offered ones, and three drill depths), turned diameters,
zones, locations, frames and the general tolerance; check() of non-text and non-offered answers;
split() and code() of stored values. Also the standards lookups and the materials tables.

Each result is {"ok": value} or {"err": message, "type": exception name}. Deterministic.
"""

import json
import sys

sys.path.insert(0, "/Users/paul/repos/specify-core/src")

from specify_core import choices, materials, standards  # noqa: E402

GENERAL = choices.GENERAL
HOLE = "hole.function:x"
SHAFT = "diameter.fit:x"

ODD = [0.5, 1.0, 1.5, 3.0, 4.0, 6.0, 7.3, 8.0, 10.0, 12.0, 15.0, 19.99, 25.4, 33.3, 47.0, 62.0,
       100.0, 250.0, 500.0, 600.0]
DIAMETERS = sorted(set(standards.TAP_DRILL.values()) | set(standards.CLEARANCE.values()) | set(ODD))


def run(fn, *args):
    try:
        return {"ok": fn(*args)}
    except Exception as exc:  # noqa: BLE001 - the oracle records whatever Python raises
        return {"err": str(exc), "type": type(exc).__name__}


def hole_options(diameter):
    """As rules._hole_questions builds them."""
    options = [GENERAL]
    bolts = standards.clearance_bolts(diameter)
    tapped = standards.match_size(diameter, standards.TAP_DRILL)
    options.extend(f"clearance:{b}" for b in bolts)
    if tapped:
        options.append(f"tapped:{tapped}")
    if not bolts and standards.largest_bolt_below(diameter):
        options.append("clearance")
    options.extend(f"fit:{fit}" for fit in standards.HOLE_FITS)
    return options


def turned_options(d):
    """As rules._turned_question builds them."""
    thread = standards.thread_size(d)
    return [
        GENERAL,
        *standards.SHAFT_FITS,
        *((f"thread:{thread}",) if thread else ()),
        *(f"knurl:{pattern}" for pattern in standards.KNURL_PATTERNS),
    ]


BASE_TEXTS = [
    # test_choices.py
    "H7", "h6", "JS7", "M6", "clearance M6", "M2", "tapped M2x0.4", "none", "G6", "m6", "M10",
    "diamond knurl", "⌀0.150", "f", "M8 tapped", "clearance M8", "M6 clearance", "g6", "M6x0.75",
    "M7", "knurl", "-1", "M2 x 0.4, usable thread depth 2.3 mm", "M2 tapped, 2 deep",
    "M2x0.4-6H 1.5mm min full thread", "M2 tapped .5 deep", "M2 tapped 2deep", "M2 tapped 2mmdeep",
    "M2-6H", "tapped:M2@2.3", "M2 x 0.4, usable thread depth 3.15 mm", "M2 tapped -2 deep",
    "M2 tapped 1e1 deep", "M2 3.15", "M2 tapped 2 deep 3 long", "M2 2 in deep", "M2-6G",
    "M6 clearance, 10 deep", "M6 clearance H11", "M6 clearance ISO 273", "M6 thread", "h7", "M12",
    # test_threads.py and more
    "M5 tapped 5.5 deep", "M5 tapped, 5.5 deep", "M5 tapped@5.5", "tapped:M5@5.5",
    "M5 tapped 2 cm deep", "M5 tapped 2 thou", "M5 tapped 2mm", "M5 tapped 2mmx deep",
    "M5 tapped 1.2.3 deep", "M5-6h", "M5 6H", "M5 tapped iso 261 4 deep",
    "M5 tapped xiso 261 4 deep", "M5 tapped ISO 965-1 4 deep", "M5 clearance bolt tapped",
    "M5 screw", "M5 pass", "M24", "M1.6", "M1.60", "M01.6", "M2.5", "M3 thread", "M٦", "h٧",
    "M6 thread 10 long", "clearance", "nan", "inf", "0.1mm", "M4 tapped 3 deep 1", "M4 tapped @3",
    "M4 @3", "M4 tapped 3 usable", "M4 tapped 3 lengthy", "M4 tapped 3 mmdeep",
    "M4 tapped 3 depthx", "M4 tapped .", "M4 tapped -.5 deep", "M4 tapped 3_0 deep",
    "M4 tapped 3 in", "M4 tapped 3 inch deep", "M4 tapped 3 m deep", "M8x1.25",
    "M8 x 1.25 tapped", "M8x1.250", "M20x2.5 clearance", "Ø6.6", "H7 fit", "JS 7", "js 6", "Js6",
    "p6", "n6 fit", "H12", "k8", "k7", "fit: H8", "fit:js6", "thread: M6", "thread:M6", "M6 × 1",
    "m6x1", "M 6", "straight knurl", "diamond", "straight", "knurled", "0.1", "0", "11", "1e-1",
    "ø0.2", "±0.1", "+-0.05", "general", "gen", "plain", "nothing", "no", "", "   ", "A", "A B",
    "a, b, c", "B|A", "ISO 2768-m", "iso2768 f", "2768-c", "v", "x", "H07", "M6\tclearance",
    "M2 tapped 2 deep\n", "M100", "M99999999999999999999", "M2 tapped 2.0000001 deep", "H7 6.6",
    "clearance M6 for an M6 bolt", "M3 tapped 4 min", "M3 tapped 4 minimum", "M3 @ 4",
    "M3 tapped 4mmfull", "M3 tapped 4 mmfull", "M3 tapped 4 full", "M3 tapped 4 mm full thread",
    "M3 tapped -6H", "M3-6H-4 deep", "M10 thread", "M10 knurl", "diamond m10", "1.5", "10.0001",
]


def variations(text):
    out = [text, text.upper(), text.lower(), "  " + text.replace(" ", "   ") + " ",
           text.replace(" ", "")]
    return list(dict.fromkeys(out))


TEXTS = list(dict.fromkeys(v for t in BASE_TEXTS for v in variations(t)))

options_table = []


def options_index(options):
    options = list(options)
    if options not in options_table:
        options_table.append(options)
    return options_table.index(options)


cases = []
for d in DIAMETERS:
    holes, shafts = hole_options(d), turned_options(d)
    h, s = options_index(holes), options_index(shafts)
    cases.append(["describe", HOLE, h, d, run(choices.describe, HOLE, holes, d)])
    cases.append(["describe", SHAFT, s, d, run(choices.describe, SHAFT, shafts, d)])
    for value in holes + shafts:
        cases.append(["callout", value, d, True, run(choices.callout, value, d, True)])
        cases.append(["callout", value, d, False, run(choices.callout, value, d, False)])
    for qid, options, idx in ((HOLE, holes, h), (SHAFT, shafts, s)):
        for value in options:
            cases.append(["check", qid, value, idx, d, None, run(choices.check, qid, value, options, d)])
    empty = options_index(())
    for text in TEXTS:
        for depth in (None, 3.5, 12.0):
            cases.append(
                ["interpret", HOLE, text, empty, d, depth, run(choices.interpret, HOLE, text, (), d, depth)]
            )
        cases.append(
            ["interpret", HOLE, text, h, d, None, run(choices.interpret, HOLE, text, holes, d, None)]
        )
        cases.append(
            ["interpret", SHAFT, text, s, d, None, run(choices.interpret, SHAFT, text, shafts, d, None)]
        )

ZONES = [
    ("hole.position:x", ()),
    ("diameter.runout:x", ()),
    ("datum.B.control", ()),
    ("datum.A.flatness", ()),
    ("hole.location:x", ()),
    ("hole.frame:x", ("A", "A|B", "A|B|C", "B|A")),
    ("part.general_tolerance", standards.GENERAL_TOLERANCES),
    ("part.general_tolerance", ("ISO 2768-f", "ISO 2768-m")),
    ("part.material", ()),
    (HOLE, ("general", "clearance:M6", "fit:H7")),
    (SHAFT, ("general", "h6", "m6", "thread:M6", "knurl:diamond")),
]
for qid, options in ZONES:
    idx = options_index(options)
    for text in TEXTS:
        cases.append(["interpret", qid, text, idx, None, None, run(choices.interpret, qid, text, options, None)])

NOT_TEXT = [0.1, 3, True, False, None, ["general"], {"value": "general"}, [], {}, 1e16, -0.0,
            123456789012345678, "x" * 50,
            [1, 2, 3, 4, 5, 6, 7, [8, [9, [10, [11, [12, [13]]]]]]],
            {"e": 1, "d": 2, "c": "a'b", "b": [], "a": {"z": None}}, ["\x00" * 12], "it's \"x\""]
STORED = ["tapped:M2@2.30", "tapped:M2@x", "tapped:M2@2.3", "tapped:M2", "fit:H07", "fit:H7",
          "clearance", "clearance:M6", "M6", "general", "knurl:diamond", "thread:M2", "g6", "", " ",
          "tapped:M2@", "tapped:M2@inf", "tapped:M2@nan"]
for d in (1.6, 6.6, 10.0):
    holes = hole_options(d)
    h = options_index(holes)
    for value in NOT_TEXT + STORED:
        for depth in (None, 3.5):
            cases.append(["check", HOLE, value, h, d, depth, run(choices.check, HOLE, value, holes, d, depth)])
    shafts = turned_options(d)
    s = options_index(shafts)
    for value in NOT_TEXT + STORED:
        cases.append(["check", SHAFT, value, s, d, None, run(choices.check, SHAFT, value, shafts, d)])

VALUES = STORED + ["tapped:M5@5.5", "tapped:M5@0", "tapped:M5@-0", "knurl:straight", "thread:M10",
                   "clearance:", "fit:", "a:b:c@1@2", "tapped:M2@1_0", "tapped:M2@ 2 ",
                   "fit:H7@3", "knurl:ǆx", "tapped:M7", "foo"]
# Every character whose title case is not its upper case (str.capitalize title-cases), and some
# whose are the same.
TITLED = [c for c in range(sys.maxunicode + 1) if chr(c).title() != chr(c).upper()]
VALUES += [f"knurl:{chr(c)}xY" for c in TITLED + [0x61, 0xe9, 0x3c3, 0x10d0 - 1, 0x1c90, 0x149]]
for value in VALUES:
    # The depth as its repr: JSON has no inf or nan.
    split = run(lambda v: [*choices.split(v)[:2], None if choices.split(v)[2] is None else repr(choices.split(v)[2])], value)
    cases.append(["split", value, split])
    cases.append(["code", value, run(choices.code, value)])
    for d in (1.6, 6.6, 600.0):
        for hole in (True, False):
            cases.append(["callout", value, d, hole, run(choices.callout, value, d, hole)])

# Standards: lookups over a fine sweep of diameters, and every fit at every range edge.
sweep = sorted({round(0.05 * k, 2) for k in range(0, 1300)} | set(DIAMETERS))
standards_cases = [
    [
        d,
        standards.match_size(d, standards.TAP_DRILL),
        standards.match_size(d, standards.CLEARANCE),
        standards.clearance_bolts(d),
        standards.largest_bolt_below(d),
        standards.thread_size(d),
    ]
    for d in sweep
]
FITS = ["H5", "H6", "H7", "H8", "H9", "H10", "H11", "H12", "JS6", "js7", "Js6", "f7", "g6", "h6",
        "h11", "k6", "k7", "k8", "m6", "n6", "p6", "H07", "H7\n", "H7 ", "Z7", "H3", "", "h"]
NOMINALS = [0.0, 0.001, 1.0, 3.0, 3.0001, 6.0, 10.0, 10.01, 18.0, 30.0, 47.0, 50.0, 80.0, 120.0,
            180.0, 250.0, 315.0, 400.0, 499.99, 500.0, 500.01, -1.0]
fits = [[fit, n, run(lambda f, x: list(standards.fit_limits(f, x)), fit, n)] for fit in FITS for n in NOMINALS]
full_threads = [
    [depth, size, standards.full_thread(depth, size)]
    for size in standards.PITCH
    for depth in (0.0, 1.0, 2.0, 3.5, 4.0, 5.0, 8.0, 10.0, 12.3, 30.0)
]

json.dump(
    {
        "options": options_table,
        "cases": cases,
        "standards": standards_cases,
        "fits": fits,
        "full_threads": full_threads,
        "materials": materials.suggestions(),
        "coatings": materials.suggested(materials.COATINGS),
        "heat_treatments": materials.suggested(materials.HEAT_TREATMENTS),
    },
    sys.stdout,
    ensure_ascii=False,
    separators=(",", ":"),
)
sys.stdout.write("\n")
