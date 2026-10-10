"""What every part of the capture shares: where things are, how artifacts are written, how a
Python failure is recorded, and the provenance stamped into each case's ``meta.json``."""

from __future__ import annotations

import datetime
import gzip
import hashlib
import importlib.metadata
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path
from typing import Any

#: This repository's root (tools/capture/common.py -> ../..).
ROOT = Path(__file__).resolve().parents[2]
PARTS = ROOT / "tests" / "fixtures" / "parts"
NIST = ROOT / "tests" / "fixtures" / "nist"
ORACLE = ROOT / "tests" / "fixtures" / "oracle"
#: The reference implementation, read-only; its venv runs this tool. The environment variable
#: SPECIFY_CORE names another checkout of it (a branch to capture from): its ``src`` is then
#: loaded in place of the one the venv installs, and its revision is the one recorded.
SPECIFY_CORE = Path(os.environ.get("SPECIFY_CORE", "/Users/paul/repos/specify-core"))
if "SPECIFY_CORE" in os.environ:
    sys.path.insert(0, str(SPECIFY_CORE / "src"))
#: Quiddity's STEP corpus: QUIDDITY_CORPUS, else this default. Never searched for elsewhere.
DEFAULT_CORPUS = Path("/Users/paul/repos/quiddity/tests/corpus")

#: Input file names the capture takes, in each root.
INPUT_SUFFIXES = (".step", ".stp", ".step.gz", ".stp.gz")

#: The answers every case is applied and written with: the one question no part can default
#: (specify-core's tests use the same). Recorded in each meta.json.
BASE_ANSWERS = {"part.material": "steel"}

#: What a written STEP's FILE_NAME time stamp is replaced with in ``written.step.gz``, so a
#: capture is reproducible; nothing reads the time stamp.
FIXED_TIME_STAMP = "2000-01-01T00:00:00"


def corpus_dir() -> Path:
    return Path(os.environ.get("QUIDDITY_CORPUS", DEFAULT_CORPUS))


def strip_input_suffix(name: str) -> str:
    """``cgb202.step.gz`` -> ``cgb202``: case names drop the STEP suffix and any ``.gz``."""
    for suffix in sorted(INPUT_SUFFIXES, key=len, reverse=True):
        if name.lower().endswith(suffix):
            return name[: -len(suffix)]
    raise ValueError(f"{name}: not a STEP input ({', '.join(INPUT_SUFFIXES)})")


def is_input(path: Path) -> bool:
    return path.is_file() and path.name.lower().endswith(INPUT_SUFFIXES)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


# --------------------------------------------------------------------------------------------
# Failures: a Python refusal or crash is an oracle result, recorded in place of the value.
# --------------------------------------------------------------------------------------------


def error(exc: BaseException) -> dict[str, Any]:
    """``{error: class, message}``, plus ``missing`` and ``part`` for an IncompleteError."""
    out: dict[str, Any] = {"error": type(exc).__name__, "message": str(exc)}
    if hasattr(exc, "missing"):
        out["missing"] = list(exc.missing)
        out["part"] = getattr(exc, "part", None)
    return out


def attempt(fn: Callable[[], Any]) -> Any:
    """``fn()``, or its failure as ``error``."""
    try:
        return fn()
    except Exception as exc:  # noqa: BLE001 -- every failure is a result here
        return error(exc)


def failed(value: Any) -> bool:
    """Whether ``value`` is a recorded failure rather than a result."""
    return isinstance(value, dict) and "error" in value and "message" in value


# --------------------------------------------------------------------------------------------
# Artifacts
# --------------------------------------------------------------------------------------------


def plain(value: Any) -> Any:
    """A JSON round trip of ``value``: tuples become lists, as any reader of the JSON sees it."""
    return json.loads(json.dumps(value))


def _finite(value: Any) -> Any:
    """NaN and infinities as ``{"$float": "NaN" | "Infinity" | "-Infinity"}``: Python's json
    writes bare tokens RFC 8259 does not have (and serde_json refuses); the marker keeps the
    fact visible to a comparison instead of losing it."""
    if isinstance(value, float) and not math.isfinite(value):
        return {
            "$float": "NaN" if math.isnan(value) else ("Infinity" if value > 0 else "-Infinity")
        }
    if isinstance(value, dict):
        return {k: _finite(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_finite(v) for v in value]
    return value


def scrub(value: Any, subs: dict[str, str]) -> Any:
    """``value`` with each path in ``subs`` replaced by its placeholder in every string, so a
    fixture does not depend on where the capture ran (``{input}``, ``{output}``)."""
    if isinstance(value, str):
        for path, token in sorted(subs.items(), key=lambda kv: -len(kv[0])):
            value = value.replace(path, token)
        return value
    if isinstance(value, dict):
        return {scrub(k, subs): scrub(v, subs) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [scrub(v, subs) for v in value]
    return value


def write_json_gz(path: Path, value: Any) -> None:
    """``value`` as gzipped JSON, keys in Python's order, reproducibly (no gzip time stamp)."""
    text = json.dumps(_finite(value), ensure_ascii=False, sort_keys=False, allow_nan=False)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(gzip.compress(text.encode() + b"\n", compresslevel=9, mtime=0))


def write_bytes_gz(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(gzip.compress(data, compresslevel=9, mtime=0))


def fixed_time_stamp(step: bytes) -> bytes:
    """``step`` with its FILE_NAME time stamp replaced by ``FIXED_TIME_STAMP``."""
    return re.sub(
        rb"(FILE_NAME\s*\(\s*'(?:[^']|'')*'\s*,\s*')[^']*(')",
        lambda m: m.group(1) + FIXED_TIME_STAMP.encode() + m.group(2),
        step,
        count=1,
    )


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def materialise(path: Path, scratch: Path) -> Path:
    """``path`` itself, or for a ``.gz`` its bytes decompressed to ``scratch`` (same name
    without ``.gz``): neither engine reads gzip, so both see the decompressed file."""
    if not path.name.lower().endswith(".gz"):
        return path
    out = scratch / path.name[:-3]
    out.write_bytes(gzip.decompress(path.read_bytes()))
    return out


def scratch_dir() -> Path:
    """A fresh temporary directory, under SC_CAPTURE_TMP if set."""
    base = os.environ.get("SC_CAPTURE_TMP")
    if base:
        Path(base).mkdir(parents=True, exist_ok=True)
    # Resolved, so the path Python is given is the one its messages repeat (/var -> /private/var).
    return Path(tempfile.mkdtemp(prefix="capture-", dir=base)).resolve()


# --------------------------------------------------------------------------------------------
# Provenance
# --------------------------------------------------------------------------------------------


def _git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(repo), *args], capture_output=True, text=True, check=True
    ).stdout.strip()


def _quiddity_source() -> str:
    """Where specify-core's quiddity came from: the wheel uv.lock pins (it has no git revision
    of its own in the venv: it is the PyPI release)."""
    lock = (SPECIFY_CORE / "uv.lock").read_text()
    block = re.search(r'\[\[package\]\]\nname = "quiddity"\n(.*?)\n\n', lock, re.S)
    wheel = (
        re.search(r'url = "([^"]+\.whl)", hash = "(sha256:[0-9a-f]+)"', block.group(1))
        if block
        else None
    )
    if wheel is None:
        return "unknown (not found in specify-core's uv.lock)"
    return f"{wheel.group(1).rsplit('/', 1)[-1]} {wheel.group(2)} (PyPI, pinned by uv.lock)"


def provenance() -> dict[str, Any]:
    """The versions and revisions the oracle was captured with."""
    dirty = _git(SPECIFY_CORE, "status", "--porcelain", "--untracked-files=no", "--", "src")
    return {
        "specify_core": {
            "version": importlib.metadata.version("specify-core"),
            "git": _git(SPECIFY_CORE, "rev-parse", "HEAD"),
            "src_modified": bool(dirty),
        },
        "quiddity": {
            "version": importlib.metadata.version("quiddity"),
            "source": _quiddity_source(),
        },
        "python": sys.version.split()[0],
    }


def today() -> str:
    return datetime.datetime.now(datetime.UTC).date().isoformat()
