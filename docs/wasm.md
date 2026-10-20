# Browser WASM package

`specify-core-wasm` is a workspace member in `crates/specify-core-wasm`. The root remains
`specify-core`, with its existing CLI and tests; the default workspace member is the root,
so ordinary `cargo build` and `cargo test` retain their scope. The binding is a `cdylib`
(and an `rlib` for native verification). It calls the same core rules, geometry and verified
PMI writer as native Specify; there is no copied writer.

## Install a release

Each published GitHub release whose tag starts with `v` gets
`specify-core-wasm-<version>.tgz` and `specify-core-wasm-<version>.tgz.sha256`. The archive
contains `pkg/`: the web ES module, `.wasm`, TypeScript declarations, package metadata and
README. Nothing is published to npm. Pin an exact release and verify the archive before
extracting it:

```sh
version=0.1.0-alpha.3 # choose a release that includes the WASM assets
archive="specify-core-wasm-${version}.tgz"
gh release download "v${version}" --repo pzfreo/specify-core-rust \
  --pattern "$archive" --pattern "$archive.sha256"
sha256sum -c "$archive.sha256" # macOS: shasum -a 256 -c "$archive.sha256"
tar -xzf "$archive"
```

Serve the JS and WASM together (prefer `application/wasm` for the latter), retaining their
filenames. Initialization fetches the `.wasm` beside the JS unless given an explicit URL:

```js
import init, { IntentEngine, review, interpret } from './pkg/specify_core_wasm.js';
await init();
const engine = new IntentEngine('plate.step', new Uint8Array(await file.arrayBuffer()));
try {
  const { analysis, mesh, provenance } = JSON.parse(engine.snapshot());
  const answersJson = JSON.stringify({ 'part.material': 'steel' });
  const state = JSON.parse(engine.review(answersJson));
  const result = JSON.parse(engine.writePmi(answersJson, projectBase64));
  const stepBytes = Uint8Array.from(result.bytes);
  // Download stepBytes; present result.report and state.problems to the user.
} finally {
  engine.free();
}
```

## JavaScript API

All structured inputs and outputs are **JSON strings**, except the constructor's byte input
(`Uint8Array`). Failures throw **strings**. `free()` releases a session's WASM memory.

| Export | Behaviour |
| --- | --- |
| `new IntentEngine(name, bytes)` | Opens one STEP part. Refuses gzip, any count other than one distinct part, any count other than one placement, and a mismatch between haecceity's ordered face sources and the STEP definition. `name` labels the input; it is never read as a path. |
| `engine.snapshot()` | `{analysis, mesh, provenance: {source_sha256, face_entities, part: 0}}`. |
| `engine.review(answersJson)` | `{questions, open, intent, problems}`; adds geometry anchors to `intent.anchors`. |
| `engine.writePmi(answersJson, projectBase64?)` | `{bytes, source_sha256, report}`; `bytes` is an array of byte values. Writes and verifies answered PMI and stored answers in memory. Unanswered defaults remain suggestions. `report` is the upstream writer's report; its `output` is the constructor's name. |
| `review(analysisJson, answersJson)` | Same review composition without a session or geometry anchors. Calls `specify_core::api::review`. |
| `interpret(analysisJson, answersJson, questionId, text)` | `{value, code, callout?}` for a choice question; includes a diameter callout for `hole.function:` and `diameter.fit:`. |
| `start()` | Installs `console_error_panic_hook`; initialization also calls it automatically. |

Questions use `to_presented_dict()` with `diameter` and `depth` added when present. `open`
is the list of ids returned by `rules::open_questions`. `intent` is `api::preview(...).to_dict()`;
`problems` comes from `problems::problems`. Native front ends can use `api::review` directly.

Defaults and their reasons remain in `questions`, and unanswered ids remain in `open`.
They contribute no PMI to review or `writePmi` until their values are supplied in answers;
in particular, an unanswered general tolerance is absent from `intent.part`, so a drawing
can continue reporting `general_tolerance_missing`. Existing file datums remain available.
The shared #16 settling path still records its automatic ±/plain answers in `state.auto`;
it does not accept datum or general-tolerance suggestions. Diagnostic `problems` continues
to inspect the proposed scheme. These are behaviour changes with no signature changes.

An optional project is written through `writer::write_bytes_with` as an `AttributeSet` with
`name: "draftwright editor project"`, `on: None`, one text item with that same name and the
supplied base64 text, and `description`/`general_property` both `None`. The editor's check is
preserved exactly: nonempty ASCII alphanumerics or `+`, `/`, `=` only (no decoding or JSON
validation). Omit the argument for no project attribute. Rewriting a resumed part replaces
previous extras, so resupply the project on every write that should retain it.

## Build and verify

Rust comes from `rust-toolchain.toml`. The checks use Python 3.11+ and Node (CI uses Node 24).
Install wasm-pack 0.15.0 and the WASM target:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --locked
python3 tools/check_wasm_api.py
python3 tools/wasm_size.py
python3 tools/check_quiddity_pin.py
python3 -m unittest discover -s tools -p 'test_*.py'
cargo build --locked --release -p specify-core-wasm --example parity_reference
node scripts/wasm-parity.mjs
```

The root `[profile.wasm-release]` inherits release with `lto = true`, `codegen-units = 1`
and `opt-level = 3`. Both workflows also run wasm-pack's additional wasm-opt pass. Do not
use `--no-opt` for the size check or release package. wasm-pack downloads its optimizer
when needed. If its default cache is unwritable, set `WASM_PACK_CACHE` to a writable directory.
Generated `pkg/` is ignored by Git; its normalized TypeScript API is reviewed separately.

Measured with Rust 1.99, wasm-pack 0.15.0, wasm-opt 117, Node 26.9.0 and the committed lockfile
on 2026-10-16:

| WASM binary | Bytes | MiB |
| --- | ---: | ---: |
| Raw, optimized | 10,197,823 | 9.73 |
| gzip, level 9 | 3,157,943 | 3.01 |
| Brotli, quality 11 | 2,148,100 | 2.05 |
| Raw ceiling | 11,800,000 | 11.25 |

These measure `specify_core_wasm_bg.wasm`, excluding the small JS/TypeScript/metadata files.
CI and releases print current measurements and the remaining size headroom; sizes can change
with compiler or dependency updates. The committed baseline and ceiling are in
`tools/wasm-size-budget.json` (see safeguards below).

File loaders, disk writers (including staging names, process ids and clocks), and the JSON-line
service are cfg-gated out on `wasm32`. Memory loading, analysis, reviews, meshing, anchors and
verified writing remain available. Native APIs and behaviour are unchanged. At the pinned
kernel revision, `cargo tree --target wasm32-unknown-unknown` contains neither `uuid` nor
`getrandom`, so no JS randomness feature or backend override is needed. If a future kernel
introduces them, enable the appropriate browser feature only in WASM target dependencies.

The Node check loads the **web release package** directly, then runs a native reference on a
plate, a turned pin and a placed part. It compares snapshots (analysis, mesh, provenance),
session/free reviews, choice interpretation, bare/project STEP bytes and reports, and resumed
answers. STEP bytes, reports, ids and strings compare exactly; geometric floating values allow
relative/absolute error of `1e-10` for native/WASM math differences. Mesh checks compare
metadata, per-face vertex sets, area and oriented area: circular planar caps can choose
different Delaunay diagonals on native and WASM while describing the same surface. It also checks thrown-string
errors, invalid project input, gzip/assembly/multiple-placement refusals, and reports raw,
gzip (level 9) and Brotli (quality 11) WASM sizes.

CI builds and runs this check on pull requests and pushes to `main`. The release workflow
checks that both crate versions equal the `v` tag, builds with the same profile, reruns parity,
archives `pkg/`, computes SHA-256, and uploads both files with `gh` and `GITHUB_TOKEN`. Only
that release job has `contents: write`; other jobs retain read-only contents access. Release
preparation must update both package versions and the binding's exact core dependency together.

## Release safeguards

### Reviewed WASM API

`crates/specify-core-wasm/api.d.ts` is the reviewed snapshot of wasm-pack's
`pkg/specify_core_wasm.d.ts`. `tools/check_wasm_api.py` removes generated comments (including
version noise), normalizes whitespace and sorts declarations and class/interface members.
It retains the public signatures and initialization types. After an intentional API change:

```sh
wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --locked
python3 tools/check_wasm_api.py --update
```

Review and commit the snapshot diff, and add a **new bullet** under `### WASM API` inside
`## Unreleased` in `CHANGELOG.md`. An existing note or a note in a released section does not
count. CI compares against the PR base or the commit before a main push; changing the snapshot
also triggers this requirement, even when it already matches the generated declarations.
A generated/snapshot mismatch fails unless the same change adds that changelog bullet.
The check prints the diff and snapshot update command. Locally `--base-ref <commit>` selects
the comparison commit (default `HEAD`).

### WASM size ceiling

`python3 tools/wasm_size.py` measures the optimized `specify_core_wasm_bg.wasm`: raw bytes,
gzip at level 9, and Brotli at quality 11. It uses Node's built-in zlib, already needed for
parity, with no extra Python packages. Both workflows fail if **raw optimized bytes** exceed
`ceiling_bytes` in `tools/wasm-size-budget.json`; equality passes. Every run prints all three
sizes and headroom in bytes and as a percentage of the ceiling. Compressed figures are
recorded for transfer-size visibility and do not have separate limits.

The initial ceiling is approximately 15% above the measured baseline, rounded upward to
100,000 bytes: 11,800,000 bytes, leaving 1,602,177 bytes of initial headroom.
Raising it requires a deliberate, reviewed JSON edit; the size checker never
updates the budget automatically. When rebaselining, record the three measurements and
tool versions together and explain the increase in the changelog.

### Matching editor kernels

The editor passes STEP files between Specify's browser package and Draftwright's drawing
package. Different quiddity kernels can number the same faces differently.
`python3 tools/check_quiddity_pin.py` reads **both** `haecceity` and `quiddity` pins from the
root `Cargo.toml` and compares them with `pzfreo/draftwright-rust`'s latest published release.
Prereleases count; drafts do not. Tags and abbreviated revisions are resolved to commits,
so a full rev and a tag at that same commit match. Internal disagreement between the two
kernel crates also warns. The check prints each package's pins and the matching commit.

Public GitHub requests work without authentication; `GITHUB_TOKEN` (or `GH_TOKEN`) is used
when set. CI runs this on main pushes. Before a release upload, the release workflow also
passes `--release "$TAG"`: a mismatch emits a GitHub Actions `::warning::` annotation and
uses `gh release edit` to append a `Kernel note:` line listing each package's pins. Reruns
replace that line without duplicating it or changing the other release notes. A mismatch
never fails a release, because one repository necessarily releases first after a kernel bump.
Network or release-note update failures also warn without blocking the upload. The command
without `--release` only reads GitHub and never edits a release.

## Switching the editor

Keep the editor's local build until a tagged release actually contains these assets. Download
and checksum a pinned archive in the editor's dependency/update step, copy `pkg/` into its
served WASM directory, and change imports from `dw_intent.js` to `specify_core_wasm.js` (and
any explicit WASM URL from `dw_intent_bg.wasm` to `specify_core_wasm_bg.wasm`). Keep the same
initialization, `IntentEngine`, JSON calls and optional project argument. Then remove
`crates/intent`, its copied writer, workspace membership and its WASM build from editor CI.
The editor's separate drawing engine and its WASM build remain independently managed.
