# Forwarding packages for the old `specify-core` names

Renamed to `specify-pmi` in 0.3.0. These three packages (0.3.1) point the old names at the new
ones. They are outside the Cargo workspace and are published once, after `specify-pmi` 0.3.0 is
out, by dispatching the registry's existing trusted-publishing workflow with `forwarders` ticked
(no other job runs): `crates.yml` (crate), `python.yml` (PyPI), `release.yml` (npm). Each
registry's trusted publisher for the old name must name that workflow and its environment
(`crates-io`, `pypi`, `npm`). Do not bump them with the main release.

- `crates/specify-core`: copied out of the checkout and `cargo publish`ed.
- `pypi/specify-core`: built with `python -m build`. It declares no console script: the
  `specify-core` command comes from the `specify-pmi` wheel.
- `npm/specify-core-wasm`: `npm publish --provenance --access public`. Then, by hand with a login
  (trusted publishing does not cover it): `npm deprecate @draftwright/specify-core-wasm
  "Renamed to @draftwright/specify-pmi-wasm; install that instead. 0.3.1 only forwards to it."`
