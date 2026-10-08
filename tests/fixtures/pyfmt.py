"""Expected outputs for tests/pyfmt.rs: Python's own number formatting, float()
and str repr, and the Unicode character classes its `re` and `str` methods use.

Run once with specify-core's venv (Python 3.14, Unicode 16.0), from the
repository root:

    /Users/paul/repos/specify-core/.venv/bin/python tests/fixtures/pyfmt.py > tests/fixtures/pyfmt.json

Deterministic: the random values are seeded. Floats are written as their repr
(a string), so -0.0, inf and nan survive JSON.
"""

import json
import math
import random
import re
import struct
import sys
import unicodedata

rng = random.Random(286)


def values():
    out = [0.0, -0.0, math.inf, -math.inf, math.nan, 1.0, -1.0, 0.5, 1.5, 2.5, -2.5]
    out += [2.675, 0.125, 0.375, 1.0005, 2.0005, 1e-5, 1e-4, 1e-3, 1e15, 1e16, 1e17, 1e22]
    out += [123456.5, 999999.5, 9999995.0, 9.9999995, 0.00012345678, 5e-324, 1.7976931348623157e308]
    out += [2.2250738585072014e-308, 0.1, 0.2, 0.3, 1 / 3, 2 / 3, 6.6, 6.69, 8.015, 9.986]
    out += [1e-9, 1 - 1e-10, 2 + 1e-10, 100.0, 1234567.0, 0.0001, 0.00001, 123456789012345678.0]
    # Decimals a person types, and their halves, at the precisions used.
    for _ in range(500):
        digits = rng.choice([1, 2, 3, 4, 5, 6])
        k = rng.randint(-10**7, 10**7)
        out.append(k / 10**digits)
        out.append((k + 0.5) / 10**digits)
    # Small integers and their halves.
    out += [k / 2 for k in range(-40, 41)]
    # Arbitrary bit patterns over every magnitude.
    for _ in range(700):
        bits = rng.getrandbits(64)
        x = struct.unpack("<d", struct.pack("<Q", bits))[0]
        if not math.isnan(x):
            out.append(x)
    # Uniform values on plausible part sizes.
    for _ in range(450):
        out.append(rng.uniform(-500, 500))
        out.append(rng.uniform(0, 1) * 10 ** rng.randint(-8, 20))
    return out


def rules_fmt(value):
    try:
        return (
            f"{value:.3f}".rstrip("0").rstrip(".")
            if not math.isclose(value, round(value))
            else f"{value:g}"
        )
    except (OverflowError, ValueError):
        return None  # Python raises on inf and nan


ROUND_DIGITS = (0, 1, 2, 3, 4, 6, 9, -1, -2)

numbers = []
for x in values():
    numbers.append(
        {
            "x": repr(x),
            "g": f"{x:g}",
            "signed_g": f"{x:+g}",
            "fixed3": f"{x:.3f}",
            "fmt3": f"{x:.3f}".rstrip("0").rstrip("."),
            "fmt_rules": rules_fmt(x),
            # x is repr(x) already; a rounding that leaves x unchanged is null.
            "round": [None if repr(round(x, n)) == repr(x) else repr(round(x, n)) for n in ROUND_DIGITS],
        }
    )

FLOAT_TEXTS = [
    "1", "1.5", " 2.5 ", "+3", "-0", "-0.0", ".5", "5.", ".", "", " ", "e5", "1e5", "1E-05",
    "1e", "1e+", "inf", "-inf", "+Inf", "INFINITY", "infinity", "infinit", "nan", "NaN", "-nan",
    "1_000", "1_000.5", "1__0", "_1", "1_", "1_.5", "1._5", "1e1_0", "1.5_5", "0x10", "1,5",
    "١٢٣", "١.٥", " 1.5 ", "\u001c2\u001f", "1\u00002", "1 2", "--1", "+-1", "١_٢",
    "1e1000", "-1e1000", "1e-400", "0.1", "00012", "1.2.3", "⌀1", "1mm", "²", "½", "𝟓",
    "0.150", "   -0.25e-3   ", "nan(1)", "in", "１２", "1e５",
]
floats = []
for text in FLOAT_TEXTS:
    try:
        floats.append([text, repr(float(text))])
    except ValueError:
        floats.append([text, None])

REPR_TEXTS = [
    "", "a", "tapped:M2@2.30", "it's", 'say "x"', "both ' and \"", "back\\slash", "tab\tnl\nr\r",
    "\x00\x07\x1b\x7f", "\x85\xa0\xad", "é⌀×±µ", "  ​﻿", "\U0001f600",
    "\U000e0001", "", "\U0010ffff", "͸", "a" * 40, "Z9", "'\"", " ",
]
reprs = [[text, repr(text)] for text in REPR_TEXTS]


def ranges(predicate):
    out, start = [], None
    for c in range(sys.maxunicode + 2):
        inside = c <= sys.maxunicode and predicate(chr(c))
        if inside and start is None:
            start = c
        elif not inside and start is not None:
            out.append([start, c - 1])
            start = None
    return out


tables = {
    "unicode": unicodedata.unidata_version,
    # str.isspace, which str.split() and str.strip() use, and re's \s.
    "space": ranges(str.isspace),
    "re_space": ranges(lambda c: re.fullmatch(r"\s", c) is not None),
    # re's \d and \w on str patterns.
    "re_digit": ranges(lambda c: re.fullmatch(r"\d", c) is not None),
    "re_word": ranges(lambda c: re.fullmatch(r"\w", c) is not None),
    # The characters float() and int() read as decimal digits, by their zero.
    "decimal_zeros": [c for c in range(sys.maxunicode + 1) if unicodedata.decimal(chr(c), -1) == 0],
    "printable": ranges(str.isprintable),
}

json.dump(
    {"numbers": numbers, "round_digits": ROUND_DIGITS, "floats": floats, "reprs": reprs, **tables},
    sys.stdout,
    ensure_ascii=False,
    separators=(",", ":"),
)
sys.stdout.write("\n")
