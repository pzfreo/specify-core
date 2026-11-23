# @draftwright/specify-ui

Part of [specify-core](../README.md) (moved here from pzfreo/specify-ui in 0.3.0; its history is kept under `ui/`).
Changes are in specify-core's [CHANGELOG](../CHANGELOG.md).

Two React components for the browser, on specify-core-wasm (the engine, built from [crates/specify-core-wasm](../crates/specify-core-wasm)):

- **Specify review** (`SpecifyReview`): the workspace for answering a part's Specify questions, with the part, its PMI
  callouts, the characteristics table and the AS9102 / CSV exports.
- **STEP diff** (`StepDiff`, `AssemblyDiffView`): compares two revisions of a STEP file (or assembly) with Specify's
  diff, lists what changed, draws the part and its PMI changes in 3D, and links the two ways.

There is no drawing engine here; drawings are Draftwright's.

## Versions

From 0.3.0 this package is released with specify-core, at the same version (0.2.1 and 0.2.2 were skipped): `@draftwright/specify-ui`
X.Y.Z depends on `@draftwright/specify-core-wasm` X.Y.Z exactly.

## Upgrading from 0.1

0.2.0 breaks 0.1 hosts in two ways:

- **Engine:** it needs `@draftwright/specify-core-wasm` **0.2.0** or later, whose API is not compatible with 0.1.x.
- **Exports follow ADR P9:** the Answers (JSON) export holds only the answers given, kept or confirmed. Inspection
  exports require approval for defaults. From 0.3.0, the app’s inspection exports read the saved STEP (ADR P11).

A host that implements only the original three engine methods keeps its 0.1 display and exports otherwise.
`SpecifyEngine.inspection` is optional.

New in 0.2.0: the local app (`specify-core open` / an agent's `show`), with its `specify-ui-app.json` manifest (see
"The local app"). From 0.3.0 it is built into the specify-core command rather than released as an archive.

## Install

```sh
npm install @draftwright/specify-ui
```

Peer dependencies, which the host provides (the package bundles none of them): `react` and `react-dom` ^19,
`three` 0.184 to 0.187 and `three-cad-viewer` ~5.1.3 (this package's tests run on three 0.184.0; its package test also installs it on 0.186.1 and 0.187.0). `three-cad-viewer`
5.1.3's build contains its own copy of three (r184), so a host that uses this package ships that copy as well as its
own, whatever its three version (asked upstream: bernhard-42/three-cad-viewer#52).

The Specify engine comes in as a dependency, `@draftwright/specify-core-wasm` (the same version as this package). It runs in a worker; the host's bundler builds that worker and copies the engine's `.wasm`
beside it, so a host passes nothing. Two things the host's build must allow:

- **Vite**: in development, keep the package out of dependency pre-bundling, or the worker's URL breaks:
  `optimizeDeps: { exclude: ['@draftwright/specify-ui'] }`. Production builds need nothing.
- **Content Security Policy**: the engine needs `'wasm-unsafe-eval'` in `script-src`, and `worker-src 'self'`.

**The font**: the package loads none. The notes and drawn PMI use the CSS variable `--specify-mono-font`, else
`'IBM Plex Mono'` if the host has loaded it (Draftwright drawings' lettering, SIL OFL), else the system monospace.
A host sets its own with, for example, `:root { --specify-mono-font: 'JetBrains Mono', monospace; }`.

### Serving the engine yourself (`wasmUrl`)

A host that serves the engine itself (from a CDN, or a build it pins) passes `wasmUrl`: the URL of the
`specify_core_wasm.js` module (not a folder), resolved against the page, with `specify_core_wasm_bg.wasm` beside it.
Both files are in the `@draftwright/specify-core-wasm` package, or in the `specify-core-wasm-<version>.tgz` asset of a
[specify-core release](https://github.com/pzfreo/specify-core/releases). For example, Draftwright's editor has
served them as `/wasm/intent/specify_core_wasm.js` and `/wasm/intent/specify_core_wasm_bg.wasm` from its own
`public/`, installed by a checksum-pinned script, and passed
`wasmUrl="/wasm/intent/specify_core_wasm.js"`. A cross-origin `wasmUrl` needs the CSP's `script-src` (and
`connect-src`, for the `.wasm`) to allow it. Use the engine of the package's own version; the components are tested
with that one.

The default engine is still imported (the worker loads it when no `wasmUrl` is given), so the host's build still
emits its ~16 MB `.wasm`, although the page never fetches it. To leave it out, alias the package to a stub, in Vite:

```js
// vite.config.js
resolve: { alias: { '@draftwright/specify-core-wasm': fileURLToPath(new URL('./src/no-bundled-engine.js', import.meta.url)) } }
// src/no-bundled-engine.js
export default function init() { throw new Error('This host serves the Specify engine itself: pass wasmUrl.'); }
```

## Usage

`StepDiff` compares two revisions of a STEP file:

```tsx
import { StepDiff } from '@draftwright/specify-ui';
import '@draftwright/specify-ui/style.css';

<StepDiff
  before={{ name: 'bracket.step', bytes: oldBytes }}   // Uint8Array; stepBytes(file) reads a File, .step.gz too
  after={{ name: 'bracket.step', bytes: newBytes }}
  onEvent={name => console.log(name)}                   // 'diff_viewed' | 'diff_failed' | 'diff_change_picked'
  // wasmUrl="/wasm/intent/specify_core_wasm.js"        // only when serving the engine yourself
/>
```

`SpecifyReview` is the review workspace. It renders into elements the host lays out (`slots`), and talks to the
engine through a `SpecifyEngine` (`questions`, `interpret`, `preview`) that the host implements, usually over
`createIntentEngine()`, the engine in a worker (a `WorkerClient`, with `request(operation, args)`). Its `session` is the
part opened: its analysis, mesh, questions and answers.

```tsx
import { SpecifyReview, type SpecifyEngine, type SpecifySlots } from '@draftwright/specify-ui';
import '@draftwright/specify-ui/style.css';

const slots: SpecifySlots = { view: viewElement, panel: panelElement, exports: exportsElement };

<SpecifyReview
  engine={engine}                    // your SpecifyEngine
  session={session}                  // the open part (PmiLabSession)
  slots={slots}
  externalAnswers={answers}          // the answers so far
  externalReview={review}            // Specify's review of them
  disabled={false}
  onExport={action => action()}      // run an export (gate it on sign-in, say)
/>
```

Hosts without characteristic-number metadata retain the original sequential preview balloons and
CSV/AS9102 contents. STEP-aware previews and saved-file inspections explicitly mark their rows
as using STEP numbering, including unnumbered settings.

In the local app, **Inspection list (CSV)** and **AS9102 FAI (Excel)** describe only the last saved
STEP, or the opened STEP in read-only mode. Before the first Save they are disabled and say
“Save first: the inspection files describe the saved STEP”. After an in-place Save the label names
the source file. Further edits do not change either export. There is no Answers (JSON) export in 0.3.0.

The app reads the bytes from `GET file?rev=saved` for inspection exports after Save. The
session serves those bytes for both in-place saves and separate copies.

The shared component keeps the original three-method host contract. `SpecifyEngine.inspection`
is optional and called only when an inspection export runs. Display always uses `engine.preview`;
keeping suggestions and receiving host reviews never requests inspection. Without `inspection`,
shared-component exports retain the host's preview behaviour. `inspectionIntent` remains an optional
export override. The local app owns its saved-file buttons and suppresses shared preview-download and CSV-inclusion controls with `inspectionExports={false}`.

Stored STEP identifiers supply inspection numbers, including PMI absent from the interview review.
Material belongs on AS9102 Form 2 only. Form 3 contains numbered characteristics only.

The local app sends the explicit list to the session’s Save API. A host can check its edit
list with the worker’s `check-edits` operation before saving.

On non-persisted `pagehide`, the local app cancels pending answer requests and saves before sending
`closed`. Cancellation prevents queued work from starting and avoids waiting for a slow write during
unload. An already dispatched request may have reached the command before the browser aborts it.
The specify-core 0.2.0 `closed` endpoint starts a 90-second idle countdown. It does not revoke the
session or reject later saves. Any later request, including a save, cancels that countdown. Browser
cancellation therefore cannot guarantee command-side arrival order or retract an accepted save.

### Read-only mode

`<SpecifyReview readOnly />` (default `false`) is a PMI viewer: the 3D view with the PMI that is set, and a list of it.
The 3D view keeps View PMI (Semantic / Graphical / Both), its navigation, transparency and Show menu, and draws only
what is set: no defaults to check (the dimensions the general tolerance covers stay under Show, off at first). The panel reads "PMI (view only)" and lists the datums that
are set (a datum chosen is lit on the part) and the characteristics that are set, numbered as their balloons, with what the general tolerance covers folded into one row; a row,
a face or a label on the part selects the other. A part setting counts as set when the file states it or the saved
answers give it; a default does not. With nothing set it says "No PMI is set on this part."

Everything about questions is removed from the page, not disabled: the questions, answering, suggestions, problems,
tick boxes, edit and call-out links, and the CSV and AS9102 exports (the `slots.question`, `slots.exports` and
`slots.more` portals render nothing). The host's `after` node is still shown. Pass the PMI as drawn in `drawn`, built
with `drawnFromScene`, `withoutNotesBlock` and `hasGraphics` (exported, with the `DrawnPmi` type) from the engine's
`scene` call, as the editor does.

`engine.questions`, `engine.interpret` and `onKeep` are never called, and a part opened with saved answers does not
re-commit them. `engine.preview` is called only to label the answers given. `onPick` still fires. `readOnlyEngine(engine)`
is the wrapper `SpecifyReview` uses: its `questions` and `interpret` reject with `{ code: 'read_only' }` without
calling yours, and `preview` passes through; use it for your own read-only code around an engine. `StepDiff` and
`AssemblyDiffView` change nothing and are view-only already.

The helpers `characteristics`, `as9102`, `xlsx`, the diff and PMI-diff functions and the `types/pmi` types are
exported too; see `src/index.ts`.

## The local app (`specify-core open`, or an agent's `show`)

`app/` is Specify on your own computer: the page served on 127.0.0.1 under the contract in
[docs/open.md](../docs/open.md) and [docs/session.md](../docs/session.md). An AI agent with Specify as its MCP server (`specify-core mcp`) opens a part,
answers what it was told and calls `show`; by hand, `specify-core open part.step` (or `open --diff old.step
new.step`) serves the same page.

**The review panel** uses the original `SpecifyReview` controls with the 0.3 edit list. It shows each requirement
as **stated** with its stored value and balloon numbers, a **gap** with its suggestion, or **none needed**.
The engine reads stated requirements and decisions from the STEP. The review asks about Process first, then
uses the file and the pending answers to ask which gaps apply after each edit. **Holes located by** and an
assembly part's **Parts list** appear when the engine asks about them. Requirements on no recognised feature
are listed last under **Other PMI in the file**.

- A stated row has two matching, uncoloured icons: a pencil (change) and a bin (remove). Change opens one line
  under the row with the current value first ("Now: ⌖ 0.2 A") and a box for the new one; the line says that a change
  of limits keeps the number and a different measurement gets a new one. The bin asks once, in one line, the only red
  in the panel: "Remove ⌖ 0.2 A? Number #4 is retired and never reused." Values read as a drawing writes them
  ("⌖ 0.2 A", "Ø3.3"), in the rows, the line and the bar. A removal names the requirement's characteristic numbers,
  or, for one without a number (the material, another tool's PMI), the row's or item's id:
  `items.remove: ["#4", {"id": "part.material"}]`.
- A gap offers **Keep** for its suggestion and one-click alternatives. **Add** opens its controls for another
  value. Use the choice chips, type a specification, or pick faces on the part. The material, coating and
  heat-treatment catalogues support the arrow keys, Enter, Tab and Esc.
- **None needed** records a decision. **Undo** takes back a pending edit or clears a stored decision. A refusal
  shows the reason on the row. Use the Datum chips to pick or add datums. **Added by you** shows where free-form
  requirements will go in the next release. The 3D view still offers call-outs for individual general dimensions.
- The general tolerance's line is its own editor (#228): a class (ISO 2768-m and so on), or a block row by row
  (decimal places and their tolerance, Add row), an angles line and separate note fields.
  The part's drawing notes also display one per line. It writes one line of text, the form
  the CLI and edit lists take: `.XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°` (inch places written `.XX`,
  millimetre places `X.XX`, no places `X` with its unit, `X ±.5 in`, as the engine spells them; a new block starts
  in the file's length unit; `src/specify/general-tolerance.ts`).
- "Other PMI in the file" rows have the bin only: removed by number, or, without one, by the engine's id for the
  item (`{"id": "file_pmi:#<STEP entity>"}`, the table's `file_pmi[].id`). On any queued removal,
  **Undo** restores the row and takes the removal out of the bar.
- Every edit is checked against the selected part and that part's queued edits before it joins the
  bar (the engine's `checkEdits`). A refusal says why on the row. **Add all suggestions** queues the
  selected part's suggestions, including None needed. It skips gaps without suggestions and names
  any refused entries.
- Use the part picker to review and edit each part of an assembly. Switching keeps all pending
  edits, and the one bar names the part on every entry. **Save** sends them all in one request
  (`{rev, edits}` for a single part file, `{rev, partEdits: [{part, edits}, …]}` for an assembly).
  The page session saves all of them together or reports the part and entry that prevented the
  save. Parts outside the edit lists keep their entity bytes. **Discard** drops every pending edit.
  With an empty bar Save says “Nothing to save.” Only unsaved edits are highlighted. After a save
  the selected part opens from the saved file. The bar also lists characteristic numbers retired by the save.
- Page edits follow engine-proven identical definitions with matching stated PMI and saved decisions.
  The row shows how many copies it covers.
  The bar lists the originating edit once with its copy count. Undo drops that edit from every affected list.
  **Split off this copy** makes subsequent edits to the selected definition independent until Save.
  Differences in saved PMI or decisions keep the definitions separate on reopen.
  Several placements of a single STEP definition share that definition's edit list.

`SpecifyReview` owns the rows, validation, pending lists, and edit bar. The local app supplies the part picker,
session state, agent drafts, and a save callback. A file host can also use the browser writer and receive the
saved bytes. Engine and host callbacks may run back-to-back before a React render. Read current host state
through refs in each callback.

The page receives the agent's working edits through the session. They appear in the same
pending bar with part names and are highlighted on their rows. **Save** writes the combined
lists. **Discard** drops the page edits and the agent's working edits. A working edit also
has **Undo** on its row and in the bar. A different answer to a stated row shows the
refusal and the change needed. **Save** excludes it and **Discard** clears it. An agent
can propose a savable change with MCP `check`, `stage: true` and an explicit
`change` entry with `to` and `expect`.
If you edit the same row, the bar says your edit replaces the agent's answer.
The Characteristics tab and its CSV and AS9102 exports read the file's stored PMI. The inclusion ticks control
which rows are exported. A saved file with no characteristics still has an exportable inspection list. After edits the 3D view can preview the proposed requirements. Save writes the specified STEP
(beside it as `part.specified.step` by default, or over the input with `--in-place`). Save never refuses for questions
still open (many AP242 files have no complete answer to everything): it writes the edits listed and says which
questions are still open. Specify's suggestions are written only when the person adds them (ADR P9).

Read only (`--read-only`) shows the part's PMI. In diff mode the page shows what changed (StepDiff) and lists the new revision's requirements
on features that moved or changed size, to check; the session is the new revision's own (nothing is carried:
ADR P11), and Save writes the new revision. An assembly is shown a part at a time. It offers the inspection list (CSV) and AS9102
workbook as downloads; it has no account, AI chat or analytics, stores nothing in the browser, and requests nothing
but its own origin; IBM Plex Mono is bundled.

`npm run build:app` builds it into `dist-app/` (relative URLs, no inline scripts; the engine is not included: the
command serves its own at `config.wasmUrl`). From 0.3.0 specify-core builds `dist-app/` and the WASM into the command
itself (its release binaries, Python wheels and crate; [docs/open.md](../docs/open.md#where-the-app-and-the-engine-come-from)),
so `specify-core open` and MCP `show` download nothing; no app archive is attached to the release. (Apps up to 0.2.0
were released from pzfreo/specify-ui, which 0.2.1 downloads.) To try a local build: `specify-core open --app dist-app
--wasm public/wasm/intent part.step`, or rebuild specify-core, which then has it built in.

The build also writes `specify-ui-app.json` at the root of `dist-app/`:
`{"name":"specify-ui-app","version":"<package.json version>","api":2}`. specify-core builds in only an app of its own
version, and checks `api` for the app it serves (built in or `--app`). `api` is the session API, defined once as `SESSION_API` in
`app/session-api.ts`, shared by the manifest and the page's compatibility check. The page refuses
to load a review when `config.api` differs, naming both versions and asking for an update.
An absent `config.api` is treated as API 1.

`npm run test:app` builds it and drives it in Chromium under the real command (`SPECIFY_CORE`, default
`specify-core` on `PATH`; it needs the session contract): `test/app-browser.mjs` by hand (the panel's Add, None
needed, Undo, Change, Remove and Discard, Save, the saved file read back with `specify-core analyse`, reopening,
read only, single- and multi-part assembly saves and atomic refusal, "Other PMI" removed by number, another tool's file left byte for byte, diff on the
enclosure fixtures, an `--in-place` save over a file changed on disk, the event stream refused and regained) and
`test/app-agent.mjs` as an agent (`specify-core mcp` over stdio: open, answer, `show`, both saves, an assembly, a
symbolic link, a diff). `test/app-panel.test.mjs` (in `npm test`) covers the panel's reading of the table, its edit
list and the general tolerance's text. Both check that no request leaves 127.0.0.1. CI and the release workflow run it against the command built
from the same commit (`target/release/specify-core`), and a release attaches only an app that passed. It takes about three minutes (it waits for the heartbeat and for the
command to stop after `closed`).

## Development

Node 22. The engine is the in-repo WASM build: build it first, from the repository root, with
`wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --locked` (`npm ci` links
`@draftwright/specify-core-wasm` to `../crates/specify-core-wasm/pkg`). The app's tests need the command from
the same checkout: `cargo build --release --bin specify-core`.

```sh
npm ci
npm run fetch:wasm     # copies the in-repo build into public/wasm/intent (the browser test's own checks)
npm run build          # typecheck, vite library build, declarations
npm test               # unit tests (node --experimental-strip-types)
npm run test:browser   # builds test/harness and drives <StepDiff> in Chromium (Playwright), on the
                       # dependency's engine and again with wasmUrl
npm run test:package   # packs the tarball, builds test/package-host on it, and runs the browser test there
SPECIFY_CORE=../target/release/specify-core npm run test:app   # the local app under the command
```

The browser test needs a Chromium for Playwright (`npx playwright install chromium`, or set `CHROMIUM_PATH`).

In the repository `package.json` depends on `@draftwright/specify-core-wasm` as `file:../crates/specify-core-wasm/pkg`.
The published tarball depends on it at the package's own version instead: `scripts/pack.mjs` packs it so (release.yml
and `npm run test:package` both pack with it), and refuses a WASM build of another version.

## Releasing

CI (specify-core's `.github/workflows/ci.yml`) runs these suites on each pull request that touches `ui/` or a shared
file, and on each push to main. Publishing a specify-core GitHub release tagged `v<version>` runs
`.github/workflows/release.yml`: it builds the WASM, checks the tag (a `v*` tag on main, matching `package.json`
and the WASM build), runs the suites on that WASM, builds the app into the command from the tagged commit and tests
it there, and packs the library. The library is published once its engine is on npm, after approval
in the `npm` environment, with npm trusted publishing (OIDC, with provenance). The `npm` environment must have a
required reviewer and be restricted to `v*` tags.

Mark a release tagged `v0.3.0-rc.1` as pre-release on GitHub. Both npm packages publish
that version under `next`, and `latest` continues to select the stable release. See
[Cutting a pre-release](../CONTRIBUTING.md#cutting-a-pre-release) for the lockstep version updates.

The trusted publisher on npmjs.com is organization `pzfreo`, repository `specify-core`, workflow `release.yml`,
environment `npm` (repository `specify-ui` up to 0.2.0). The workflow passes no npm token. Once a release has
published through the trusted publisher, set "Require two-factor authentication and disallow tokens" on the package.

## Licence

Apache-2.0, Copyright (C) 2026 Betwixt Limited (trading as Draftwright). See `LICENSE` and `NOTICE`, which also
records third-party material (`third_party/`, `test/fixtures/README.md`).
