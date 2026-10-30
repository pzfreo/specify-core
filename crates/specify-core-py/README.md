# specify-core (Python, on the Rust engine)

The Python package `specify_core` with the Rust engine behind it: specify-core's own API, by
the same names and with the same return shapes, and none of its dependencies (quiddity,
build123d, OCP). Built with [maturin](https://www.maturin.rs/) as a mixed project: the native
module `specify_core._native` (`src/lib.rs`) and the package around it
(`python/specify_core/`).

The API: `load` (a `LoadedPart`), `parts`, `analyse`, `questions`, `apply`, `preview`, `write`,
`write_parts` and `mesh`, with `Question`, `Intent`, `IncompleteError`, `WriteReport`,
`VerificationError`, `rules.with_all_defaults`, `existing.read_existing`/`part_settings`
(`ExistingPmi`), `problems.problems`, `choices` (`interpret`, `check`, `describe`, `code`,
`callout`, `SpecError`) and `serve.reply`. Each step that reads a file takes a path or a
`LoadedPart`, a handle on the part held by the engine: it has `path`, `binding`, `part_count`
and `name`, but none of OpenCascade's objects (`faces`, `face`, `index_of`, `shape`).

The command line: the `specify-core` console script (or `python -m specify_core`) runs the
engine's (`specify_core::cli`, the binary's code) in the Python process, `serve` included;
`cli.main(argv)` returns its exit code. specify-core's second name, `pmi-assist`, is not kept.

```bash
uv venv /tmp/specify-py && uv pip install --python /tmp/specify-py/bin/python maturin pytest
VIRTUAL_ENV=/tmp/specify-py /tmp/specify-py/bin/maturin develop --release \
  -m crates/specify-core-py/Cargo.toml
QUIDDITY_CORPUS=/path/to/quiddity/tests/corpus /tmp/specify-py/bin/python -m pytest tests/python
```

`maturin build --release -m crates/specify-core-py/Cargo.toml` builds the wheel: abi3, so one
wheel per platform serves CPython 3.12 to 3.14.

`tests/python` holds specify-core's own tests (at cc341dd, the revision the oracle was captured
from), on the committed parts `tools/make_parts.py` built; the tests of the spool and NIST
CTC-03 skip without Quiddity's corpus (`QUIDDITY_CORPUS`), and those of NIST's PMI files
without them (`NIST_PMI`). A test that fails by a recorded difference is a strict xfail naming
its ledger entry (`tests/python/conftest.py`, `KNOWN`); the tests that do not run here, and the
ones ported onto the public API, are listed in `tests/python/PYTHON_ONLY.md`.

Licensed under Apache-2.0 (LICENSE, NOTICE: the repository's, copied here for the wheel).
