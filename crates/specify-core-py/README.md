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
/tmp/specify-py/bin/python -m pytest tests/python
```

Licensed under AGPL-3.0-or-later; bundled haecceity/quiddity are AGPL-3.0-only.
