# Contributing to Specify

Contributions are welcome: bug reports, fixes, features and documentation.

Before review, run `tools/verify.sh`. It runs what CI runs on a pull request (the Rust suite, the WASM
checks, the UI and app jobs against this checkout's build, the wheel's pytest and the crate package) and
ends with `VERIFY: PASS` or `VERIFY: FAIL (<steps>)`; paste that output in the pull request. Commit first: its crate-package step refuses uncommitted changes, as CI's does.
`tools/verify.sh --dry-run` lists the steps. It uses cargo-nextest for the Rust suite.
Point `SPECIFY_CORPUS` at the [pzfreo/quiddity-corpus](https://github.com/pzfreo/quiddity-corpus)
revision pinned in `.github/workflows/ci-rust.yml` (default `../quiddity/tests/corpus`).
Linux links every workspace target and builds one test archive for its five shards.
The five long corpus sweeps run in eight input-file groups each. All parts from one file stay
in the same group. The writer checks every group's differences against the full ledger and
checks each entry for staleness in its case or witness group. A local verification run covers
all groups.
macOS uses one job. It builds and runs the floating-point test binaries on ordinary
PRs and runs the full suite on main, manual runs and PRs with `release*` heads.
Full macOS runs are outside the 25-minute target. Docs-only PRs skip Rust, WASM, UI
and app jobs unless they touch a dependency listed in the workflow's path filter.

A change to the WASM API also needs its `api.d.ts` snapshot and a CHANGELOG `### WASM API` entry
(`python3 tools/check_wasm_api.py --base-ref origin/main`). CI's `wasm-checks` job runs that and the
size and parity checks. The UI and app jobs can start as soon as `wasm-build` uploads the package.
The UI browser suite and the packed-host suite run in parallel. All three packed-host jobs install
the same tarball with different Three.js versions. The app browser and agent jobs share one bundled
command. `tools/verify.sh` runs the same checks against this checkout's builds.

A change that alters a recorded expectation (fixtures, oracle departures, known differences)
says why for each change. Don't weaken a test to make it pass.

## Cutting a pre-release

Verify the registry settings in the [release checklist](RELEASING.md) before
publishing any GitHub release or approving a rerun.

1. For `0.3.0-rc.1`, set that version in the root, WASM and Python `Cargo.toml` files and
   the two exact `specify-core` dependencies (`=0.3.0-rc.1`). Refresh `Cargo.lock`.
   Set the same SemVer version in `ui/package.json` and `ui/package-lock.json`.
2. Set `crates/specify-core-py/pyproject.toml` to the PEP 440 version `0.3.0rc1`.
3. Once the version commit is on main and `ci.yml` has passed on it, tag it `v0.3.0-rc.1`
   and publish a GitHub release marked **pre-release**. Use the pre-release flag for every
   tag with a SemVer pre-release suffix. A final tag requires a full GitHub release.
4. After the usual environment approvals, the release workflows attach the CLI binaries and
   WASM archive, publish `0.3.0-rc.1` to crates.io and `0.3.0rc1` to PyPI, and publish
   `@draftwright/specify-core-wasm` and `@draftwright/specify-ui` at `0.3.0-rc.1` under
   npm's `next` dist-tag. Use `@next` to select a pre-release. A plain `npm install` of
   either package continues to select the stable version under `latest`.

## Test files and data

Add only files you created yourself or whose licence allows redistribution, and record each
file's source and licence beside it (a fixture README or NOTICE entry). Never add customer,
confidential or NDA-covered files, or geometry extracted from them, to the repository, to
issues or to pull requests. For a bug seen on such a part, share a reduced synthetic part that
reproduces it instead.

## Developer Certificate of Origin

Contributions are accepted under the project's licence, Apache-2.0 (its section 5 covers
contributions). Each commit must be signed off to certify the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote the
change, or otherwise have the right to submit it under that licence. Sign off with
`git commit -s`, which adds a line with your real name and email:

```
Signed-off-by: Your Name <you@example.com>
```

A check on pull requests from forks confirms every commit is signed off. To sign off commits
already made, `git fetch upstream && git rebase --signoff upstream/main` (with `upstream` this repository) and force-push the branch.
