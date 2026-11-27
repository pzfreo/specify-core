# Forwarding packages for the old `specify-core` names

Renamed to `specify-pmi` in 0.3.0. These three packages (0.3.1) point the old names at the new
ones. They are outside the Cargo workspace and are published by hand, once, after
`specify-pmi` 0.3.0 is out. Do not bump them with the main release.

- `crates/specify-core`: `cd crates/specify-core && cargo publish` (needs a crates.io token or the
  re-pointed trusted publisher).
- `pypi/specify-core`: `cd pypi/specify-core && python3 -m build && twine upload dist/*`. It declares
  no console script: the `specify-core` command comes from the `specify-pmi` wheel.
- `npm/specify-core-wasm`: `cd npm/specify-core-wasm && npm publish --access public`, then
  `npm deprecate` the older versions (message in the release notes).
