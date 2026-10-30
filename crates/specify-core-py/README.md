# specify-core (Python, on the Rust engine)

The Python package `specify_core` with the Rust engine behind it: specify-core's own API, by
the same names and with the same return shapes, and none of its dependencies (quiddity,
build123d, OCP). Built with [maturin](https://www.maturin.rs/) as a mixed project: the native
module `specify_core._native` (`src/lib.rs`) and the package around it
(`python/specify_core/`).

So far: `parts`, `analyse` and `questions` (with `Question`). `analyse` takes a path, not a
`LoadedPart`.

```bash
uv venv /tmp/specify-py && uv pip install --python /tmp/specify-py/bin/python maturin pytest
VIRTUAL_ENV=/tmp/specify-py /tmp/specify-py/bin/maturin develop --release \
  -m crates/specify-core-py/Cargo.toml
QUIDDITY_CORPUS=/path/to/quiddity/tests/corpus /tmp/specify-py/bin/python -m pytest tests/python
```

`maturin build --release -m crates/specify-core-py/Cargo.toml` builds the wheel: abi3, so one
wheel per platform serves CPython 3.12 to 3.14.

`tests/python` holds specify-core's own tests of these steps (at cc341dd, the revision the
oracle was captured from), on the committed parts `tools/make_parts.py` built; the tests of the
spool and NIST CTC-03 skip without Quiddity's corpus. A test that fails by a recorded
difference is a strict xfail naming its ledger entry (`tests/python/conftest.py`, `KNOWN`).

Licensed under AGPL-3.0-or-later; bundled haecceity/quiddity are AGPL-3.0-only.
