"""specify-core's command line, the Rust engine's (``specify_core::cli``).

``main`` takes specify-core's arguments and returns its exit code, as specify-core's
``cli.main`` does. Its output goes to the process's stdout and stderr; where ``sys.stdout`` or
``sys.stderr`` is not the process's own (a test capturing them, a host redirecting them),
it runs as a child (``python -m specify_core``) and its output is written to them.
"""

from __future__ import annotations

import io
import signal
import subprocess
import sys

from . import _native


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else list(argv)
    if _is_fd(sys.stdout, 1) and _is_fd(sys.stderr, 2):
        sys.stdout.flush()
        sys.stderr.flush()
        return _native.cli(args)
    done = subprocess.run(
        [sys.executable, "-m", "specify_core", *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    sys.stdout.write(done.stdout)
    sys.stderr.write(done.stderr)
    return done.returncode


def run() -> None:
    """The ``specify-core`` console script: Ctrl-C ends it, as it ends the binary."""
    signal.signal(signal.SIGINT, signal.SIG_DFL)
    sys.exit(main())


def _is_fd(stream, fd: int) -> bool:
    try:
        return stream.fileno() == fd
    except (AttributeError, ValueError, io.UnsupportedOperation):
        return False
