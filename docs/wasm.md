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
| `engine.writePmi(answersJson, projectBase64?)` | `{bytes, source_sha256, report}`; `bytes` is an array of byte values. Previews answers with defaults, writes and verifies PMI and stored answers in memory. `report` is the upstream writer's report; its `output` is the constructor's name. |
| `review(analysisJson, answersJson)` | Same review composition without a session or geometry anchors. Calls `specify_core::api::review`. |
| `interpret(analysisJson, answersJson, questionId, text)` | `{value, code, callout?}` for a choice question; includes a diameter callout for `hole.function:` and `diameter.fit:`. |
| `start()` | Installs `console_error_panic_hook`; initialization also calls it automatically. |

Questions use `to_presented_dict()` with `diameter` and `depth` added when present. `open`
is the list of ids returned by `rules::open_questions`. `intent` is `api::preview(...).to_dict()`;
`problems` comes from `problems::problems`. Native front ends can use `api::review` directly.

An optional project is written through `writer::write_bytes_with` as an `AttributeSet` with
`name: "draftwright editor project"`, `on: None`, one text item with that same name and the
supplied base64 text, and `description`/`general_property` both `None`. The editor's check is
preserved exactly: nonempty ASCII alphanumerics or `+`, `/`, `=` only (no decoding or JSON
validation). Omit the argument for no project attribute. Rewriting a resumed part replaces
previous extras, so resupply the project on every write that should retain it.

## Build and verify

Rust comes from `rust-toolchain.toml`. Install wasm-pack 0.15.0 and the WASM target:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --no-opt --locked
cargo build --locked --release -p specify-core-wasm --example parity_reference
node scripts/wasm-parity.mjs
```

The root `[profile.wasm-release]` inherits release with `lto = true`, `codegen-units = 1`
and `opt-level = 3`. `--no-opt` skips wasm-pack's additional wasm-opt pass, so the package
uses this Cargo profile without depending on a separate optimizer installation.
Generated `pkg/` is ignored by Git.

Measured with Rust 1.99, wasm-pack 0.15.0 and the committed lockfile:

| WASM binary | Bytes | MiB |
| --- | ---: | ---: |
| Raw | 12,258,045 | 11.69 |
| gzip, level 9 | 3,329,999 | 3.18 |
| Brotli, quality 11 | 2,176,781 | 2.08 |

These measure `specify_core_wasm_bg.wasm`, excluding the small JS/TypeScript/metadata files.
Release CI prints current measurements; sizes can change with compiler or dependency updates.

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

## Switching the editor

Keep the editor's local build until a tagged release actually contains these assets. Download
and checksum a pinned archive in the editor's dependency/update step, copy `pkg/` into its
served WASM directory, and change imports from `dw_intent.js` to `specify_core_wasm.js` (and
any explicit WASM URL from `dw_intent_bg.wasm` to `specify_core_wasm_bg.wasm`). Keep the same
initialization, `IntentEngine`, JSON calls and optional project argument. Then remove
`crates/intent`, its copied writer, workspace membership and its WASM build from editor CI.
The editor's separate drawing engine and its WASM build remain independently managed.
