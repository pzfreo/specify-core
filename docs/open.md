# `specify-core open`: the local app contract

The Specify page serves the Specify app, the engine and one STEP file (two for a diff) from
this computer on 127.0.0.1 and opens the browser at the app. This page is the contract between
that page server (src/open.rs) and the app (ui/app, built in this repository) that it serves.

The page server is started in one of two ways, with the same code and the same contract:

- **By an AI agent** (the usual way): Specify is set up once as the agent's stdio MCP server,
  `specify-core mcp`; the agent opens a part and calls the `show` tool, which starts the page
  server for that session and returns its address (docs/session.md).
- **By hand**, with no agent: `specify-core open FILE` or `specify-core open --diff OLD NEW`,
  below.

```text
specify-core open FILE [--read-only | --in-place] [--port N] [--no-browser]
                       [--app DIR] [--wasm DIR]
specify-core open --diff OLD NEW [--in-place] [--port N] [--no-browser]
                       [--app DIR] [--wasm DIR]
```

`FILE`, `OLD` and `NEW` are `.step` or `.stp` files, or either gzipped. `--read-only` and
`--in-place` cannot be combined; `--read-only` cannot be combined with `--diff`. Every path
the command shows or returns (`saveTarget`, a save's `path`, its notices) is the file as given,
made absolute, never resolved: a link stays the link, and nothing is shown in a canonical form
such as Windows' `\\?\C:\…`.

The run has one of three modes, given to the app as `config.mode`:

| Mode | Command | The page |
| --- | --- | --- |
| `specify` | `open FILE` | shows the part's requirements and gaps and saves the edits made to them |
| `read-only` | `open FILE --read-only` | shows the part and its PMI; no Save |
| `diff` | `open --diff OLD NEW` | shows what changed beside `NEW`, opened as `open NEW` opens it; Save writes `NEW` specified |

In `diff` mode everything below that speaks of "the file" or `FILE` means `NEW`; `OLD` is
only read.

The session lives in the process that serves the page (the *session*, below, and
docs/session.md): the file as last saved, its stored answers and their revision. The page
collects its edits itself and sends them all at once with Save as one edit list per edited part (ADR P11).
The agent's answers reach the page as they are made. In what follows, "the
command" is that process: `specify-core open`, or `specify-core mcp` after `show`.

## Addresses

Everything is under `http://127.0.0.1:<port>/<token>/`, where `<token>` is 32 lowercase hex
characters drawn from the operating system's random source for each run. The command prints

```text
Open part.step: http://127.0.0.1:<port>/<token>/
```

and opens the browser there (not with `--no-browser`). (Under an agent nothing is printed: the
`show` tool returns the address, and opens the browser unless asked not to.) `<token>`
without its trailing slash is redirected (308) to it. The app must use relative URLs only: it is served under the token, never
at `/`.

| Request | Answer |
| --- | --- |
| `GET <token>/` | the app's `index.html` |
| `GET <token>/<path>` | the static app: the file at `<path>` in the app directory |
| `GET <token>/wasm/<path>` | the engine: the file at `<path>` in the wasm-pack `pkg/` directory |
| `GET <token>/file` | the STEP file's bytes, as on disk (gzipped if the file is), `application/octet-stream` |
| `GET <token>/file?rev=old` | `diff` mode: `OLD`'s bytes; `?rev=new` (or no query) is `NEW`; any other `rev` is 404 |
| `GET <token>/file?rev=saved` | the bytes the session's last successful save wrote, as on disk now, its absolute path in `X-Specify-Path`, below |
| `GET <token>/config` | the app's settings, below; also the page's heartbeat |
| `GET <token>/answers` | the session's answers, below |
| `GET <token>/events` | the session's changes as server-sent events, below |
| `GET <token>/diff` | `diff` mode: the comparison, as WASM `diff()` gives it, below |
| `POST <token>/save` | an edit list applied to the file, all or nothing, and written to disk, below |
| `POST <token>/closed` | the page went away, below |

A query string is ignored, except `rev` on `file`. `file` serves the input as it was opened, or
as the last `--in-place` save left it. Outside `diff` mode `?rev=old` is 404, and `diff` is 404.

`file?rev=saved` serves the file the session's last successful save wrote (from the page or
from an AI client's `save`: it is one session), so the page can work from the file as saved
(the session's `part.specified.step`, or `-2`, … when that name was taken, or the input after an `--in-place` save). It works the same in
every mode, `diff` included:

- before any save in this run it is 404 (`{"error": …}`); `--read-only` never saves, so it is
  always 404;
- the bytes are read from disk when asked for and served only if they hash (SHA-256) to what the
  save stored: a file changed on disk since is 409, one removed is 404, and one that cannot be
  read is 500, each `{"error": …, "path": …}`; other bytes are never served;
- `X-Specify-Path` is the file's absolute path as the save's `path` and `saveTarget` give it,
  with `%` and any byte outside printable ASCII percent-encoded as UTF-8 (`decodeURIComponent`
  gives the path back; a path of plain characters is unchanged).

### `config`

```json
{
  "api": 2,
  "mode": "specify",
  "name": "part.step",
  "path": "/home/me/parts/part.step",
  "part": null,
  "oldName": null,
  "oldPath": null,
  "readOnly": false,
  "wasmUrl": "wasm/specify_core_wasm.js",
  "saveTarget": "/home/me/parts/part.specified.step",
  "specifyCore": "0.3.0"
}
```

- `api`: the version of this contract, an integer: 1 is 0.2.0's, 2 is 0.3.0's (Save sends an
  edit list; there is no `PUT answers`). It changes only in a minor release, when the contract
  changes in a way an app built for the old one cannot follow; an app checks it and says
  plainly which version it needs.
- `mode`: `"specify"`, `"read-only"` or `"diff"` (above).
- `name`: the file's name (`NEW`'s in `diff` mode).
- `path`: the opened file's absolute path (`NEW`'s in `diff` mode), as given, not resolved
  (above).
- `part`: the part index the page is shown for: under an agent, the part (`doc`) `show` was
  called for, in a file of several parts; otherwise `null` (`specify-core open`, or a file of
  one part).
- `oldName`, `oldPath`: `OLD`'s name and absolute path in `diff` mode, otherwise `null`.
- `readOnly`: saves are refused (`--read-only`); hide or disable Save.
  `true` exactly when `mode` is `"read-only"`.
- `wasmUrl`: the engine's entry module, relative to the app.
- `saveTarget`: the absolute path the next save would write (the input itself with
  `--in-place`), or `null` when read-only. It changes after each save. A save may still take
  another name if a file appears there first.
- `specifyCore`: this command's version.

### `save`

The review checks each edit against the file before listing it. It uses the stored requirements and
pending edits to update the preview and decide which questions apply. Save sends the combined
edits to the command, which applies them all or nothing using the same writer as
`specify-core write STEP EDITS.json` (ADR P11). The request carries the edit list and revision.

`POST <token>/save` with a JSON body:

```json
{"rev": 7, "edits": {"fill": {"hole.position:holes:12": "0.2"},
                     "items": {"remove": ["#17"]}, "none": ["part.coating"]}}
```

- `rev`: the session revision the page last saw (from `answers` or `events`). If the session
  has moved on since, the save is refused (409, with the current `rev`) so the page never
  saves over changes it has not shown. The session checks the revision again under the same
  lock it uses to commit the file and publish the new session state. The page refreshes
  after a conflict so the person can save again.
- `edits` (an object for a single-part file, `{}` for none): the edit list, as `specify-core write STEP EDITS.json` takes it:
  `fill` gaps, `change` stated feature-and-kinds (with the value the person saw as `expect`),
  `items.remove` requirements by characteristic number, `none` feature-and-kinds decided none,
  `accept_suggestions`, gaps filled with Specify's suggestion, and `clear`. Ids are the feature
  table's (`table` in the analysis), a choice with no row (`part.locating`), or an added
  requirement (`extra.<name>`). `clear` takes back a stored decision that leaves no PMI (a decided none, or a choice such as `part.locating`), so the feature-and-kind is a gap, or the choice unanswered, again; it never touches PMI (that is `change` and `items.remove`). A fill of `extra.<name>` with a requirement's spec (`{kind, faces, value, …}`, as an answer gave it) adds that requirement. Anything the list does not name is left as it is, byte for byte.
  A suggestion is written only when the list names it (ADR P9).

For an assembly send `partEdits` in place of `edits`. It is a nonempty array with one
`{part, edits}` object for each edited part. Use the distinct part's zero-based index as listed
by `parts` or the page's part picker. A definition placed several times is edited once.
The lists apply in part-index order and each part's characteristic numbers stay local to it.
The original request form stays supported under session API 2. MCP `save` and `check` also
accept the `partEdits` form with an open document for the file.

```json
{"rev": 7, "partEdits": [{"part": 0, "edits": {"fill": {"part.material": "Steel"}}},
                        {"part": 2, "edits": {"none": ["part.coating"]}}]}
```

**Save is all or nothing across the file.** The command stages the lists in memory through
the assembly writer and commits once. Requirement edits preserve other parts’ and placements’
entity bytes.
If an entry cannot be applied, the response names the part and entry and the save writes
nothing. The page includes the agent's savable edits in its lists. For a stated row, an agent
can stage an explicit `change` with `to` and `expect`. A differing answer appears on the row
as a refusal and can be discarded. An explicit page edit to the same question takes
precedence, and the bar shows that it replaces the draft. The command refuses a request
that omits a savable draft so an older page cannot silently discard it. Successful saves
clear the working edits. Characteristics and the 3D view read the saved STEP.

The file is written from the file as the session's last save left it (the input before any), to
a temporary file next to the input and renamed into place:

- by default in the directory of `FILE` as given (beside a link, not in its target's
  directory) under the first free name of `<stem>.specified.step`, `<stem>.specified-2.step`,
  `<stem>.specified-3.step`, … (`<stem>` is the name without `.gz` and `.step`/`.stp`:
  `part.stp` and `part.step.gz` both save as `part.specified.step`, uncompressed). An existing
  file is never overwritten, even by a concurrent save. Later saves of the session update the
  copy its last save wrote, while that copy is still as the save left it; a copy removed or
  changed by anyone else is left alone, and the save takes the next free name;
- with `--in-place` over the input. The first save of the run first copies the original to
  the first free name of `<FILE>.bak`, `<FILE>.bak-2`, … (an existing backup is never
  overwritten); later saves in the same run replace the file and make no further backup, so
  the `.bak` holds the file as it was before the run. The replacement keeps the original's
  permissions. If `FILE` is a link, the link is followed: its target's contents are replaced
  (written beside the target and renamed onto it), the link is kept, and the `.bak` is beside
  the link, named after it. A gzipped input is written gzipped.

After a save the session works on the file it wrote: its answers are those the saved file
stores. A change to the file or to its stored answers advances `rev`. The success response
includes the answers at that revision, so the page can save again before an event arrives.

The request: `Content-Type: application/json` (415 otherwise), `Content-Length` and no
`Transfer-Encoding` (411), at most 16 MB (413), the whole body within 30 s and 1 s more per MiB
(408), and `Origin: http://127.0.0.1:<port>` exactly (403 otherwise, and when `Origin` is
absent).

Answers:

- `200 {"path": "<absolute path written>", "bytes": <length written>, "rev": <n>, "report": {…},
  "parts": {"<part>": {…}}, "complete": <bool>, "missing": ["<question id>", …]}`, `report` being `write`'s report for an
  edit list (with `retired`, the characteristic numbers the save retired, and `unchanged`).
  For `partEdits`, `report` contains `output`, `unchanged` for the whole save, and `parts`,
  an array of `{part, name, report}` with a writer report for each requested part.
  `missing` lists required questions the saved file still leaves open (a file with gaps is valid);
- `400 {"error": "…"}`: missing `rev`, invalid JSON, or invalid request shape, including
  both edit forms, an empty `partEdits` array or repeated part indices. A malformed edit list
  also returns `failures` with its entries and reasons;
- `409 {"error": "…", "rev": <current>}`: the session moved on, or read-only, or (`--in-place`)
  the file was changed on disk by another program since it was opened (nothing written);
- `422 {"error": "…", "failures": [{"entry", "reason"}, …]}`: entries that cannot be applied,
  every one named. Part-scoped failures also carry `part` (the numeric index) and `editEntry`
  (the raw edit path). The qualified `entry` names the part for display. An omitted draft is
  also reported as a refusal. Nothing is written;
- `403 {"error": "…"}`: under an agent, the save would write outside the directories the agent's
  server may use (`--allow`); nothing written.

The command prints `Saved <path> (<bytes> bytes).` for each save.

(Contract change, 2026-11-21, `api` 2: Save sent `{rev, acceptDefaults}` and the command wrote
the session's answers, which the page changed with `PUT answers`. Save now sends the edit list
and `PUT answers` is gone (405). Contract change, 2026-11-18: until then the page wrote the STEP
with its own engine and sent the bytes as `application/octet-stream`.)

## The session: `answers` and `events`

The command holds the answers for the run: one answer map (as in `docs/answers.md`) per part,
by part index (`"0"` for a single part), and a revision number `rev` that goes up by one on
every change. At the start it holds:

- `specify` and `read-only`: the answers stored in `FILE`, when Specify wrote it (resume, as
  `analyse` reports them: decided none, and choices that write no PMI), otherwise none;
- `diff`: the answers stored in `NEW`, as for `specify` (nothing is carried from `OLD`: ADR P11).

`GET <token>/answers` →

```json
{"rev": 7, "parts": {"0": {"part.material": "6061-T6", "datum.A": [4]}}}
```

`drafts`, when present, maps part indices to working edit lists with `fill`, `none` and
`clear`. The page puts these edits in its pending bar with the part names. It sends them
alongside its own edits in Save.

`POST <token>/discard` uses the same Origin and body rules as Save. Send `{rev}` to drop
every working edit or `{rev, part, id}` to undo one working edit. It restores the saved answers
at one revision without writing a file. The response has the `GET answers` shape. A stale
revision returns 409 with the current `rev`. A read-only page cannot discard.

`GET <token>/events` is a stream of server-sent events (`text/event-stream`, for
`EventSource`). On connecting it sends the current state (an `answers` event whose data is
`answers`' body with `"source": "start"`: `{"rev": 7, "parts": {…}, "source": "start"}`), then
one event per change:

```text
event: answers
id: 8
data: {"rev": 8, "part": 0, "answers": {…}, "source": "mcp", "draft": {"fill": {"part.material": "6061-T6"}}}
```

(A part joining the session, when the agent opens another part of the file, is a change too: a
new `rev` and an event with `"joined": true`, its `answers` possibly `{}`; a page holding an
older `rev` then fails its next check and re-reads.)

A part event's `draft` is that part's current working list. Its absence clears the part's
draft. A save or whole-file discard may include `"drafts": {}` to clear the entire bar
as soon as its first part event arrives.

`source` is `"page"` (the page's save or discard), `"mcp"` (the agent's tools, docs/session.md) or
`"start"` (the first event).
Every open page receives every change, its own included. One `rev` can carry several `answers`
events, one per part (a save's, for every part whose stored answers it changed): a page skips an event it already
has by its `rev` and `part` together, not by `rev` alone.
A comment line (`: keep-alive`) is sent every 15 s. A stream that falls more than 1,000 events
behind is closed; reconnecting, it gets the full state. A reconnecting `EventSource` gets the full
current state again; `Last-Event-ID` is ignored. A `save` also sends

```text
event: saved
data: {"path": "…", "bytes": 12345, "rev": 8, "source": "page"}
```

so every page shows where the file was written, whoever asked.

At most 4 event streams are open at once (a fifth gets 503 with `Retry-After: 1`); they count
toward the 16 connections.

## `diff` mode

`specify-core open --diff OLD NEW` runs the same comparison as `specify-core diff OLD NEW`
when it starts: from the two files' PMI only (docs/diff.md). Nothing is carried from `OLD`
(ADR P11); the session is `NEW`'s, as `open NEW` starts it.

**The comparison comes from the command's `diff`, not from the engine in the page.** The
command's comparison is the one `specify-core diff` and MCP's `diff` give, so the page, the AI
and a CI run cannot disagree. The page does not compare the parts again.

`GET <token>/diff` → exactly what WASM `diff()` returns for the two files with `scenes` true
(docs/wasm.md, the `diff(...)` row), computed natively by the same code when the session
starts: for a part `{report, markdown, view}`, `report` being `diff --pmi-scenes`'s
`report.json` (docs/diff.md), `markdown` the report headed with `NEW`'s name, and `view` the
new and old meshes, the `faces` classes, `gone`, `motion` and `changes`; for an assembly
`{report, markdown, parts}`, each part `{name, status, old, new}` with its `report` when it is
in both files and, when it changed, its `markdown` and `view`. The PMI scenes are
`report.pmi.scenes`, `{old, new}`, each a `PmiScene` (docs/wasm.md, "PMI scenes"); in an
assembly, each compared part's `parts[i].report.pmi.scenes`. (Contract addition, 2026-11-18:
`GET diff` includes the PMI scenes, as `diff()` with `scenes` true.) The page's StepDiff
draws from it without comparing again. (Contract change, 2026-11-18: `GET diff` was
report.json; it is now `{report, markdown, view}` (assembly `{report, markdown, parts}`), as
WASM `diff()`.) A save changes `NEW`, and the report is not recomputed. Save applies its edit
list to `NEW` and writes it (beside it, or over it with `--in-place`).

## The agent's side

Under `specify-core mcp` the agent reads and changes the same session through MCP tools
(docs/session.md): its answers reach the page as `answers` events with `source: "mcp"`. Its
`save` tool applies an edit list as the page's Save does and sends the same `saved` event.

### Heartbeat and `closed`

The engine runs in the page, so looking at the part and making edits make no requests (Save
does). While the page is open, visible or not, the app sends `GET <token>/config` every 30 s (the heartbeat; the answer may be used or
ignored). Any request resets the idle time and cancels a pending close.

`POST <token>/closed`, with the same `Origin` rule as `save` (any body, ignored; 204). Send it
with `navigator.sendBeacon` on `pagehide` only when `!event.persisted`: a page going into the
back/forward cache is not closed, and sends nothing. After `closed` the command stops once no
request has arrived and no event stream has been open for 90 s, printing `Closed.`; a reload, or another open tab's heartbeat,
keeps it running (90 s, because browsers run a long-hidden tab's timers about once a
minute). Without `closed` (a forgotten server, no page sending heartbeats) it stops
after 30 minutes with no request, printing `Stopped: no request for 30 minutes.`.

(Contract addition, after the first version: the heartbeat, the `!event.persisted` rule and
the 90 s delay, which was 3 s.)

Under an agent, "the command stops" means the page server stops: the MCP server and its
session carry on, nothing is printed on stdout (it carries the MCP protocol; notices go to
stderr), and the agent can call `show` again for a new address. When the MCP server exits, its
page servers stop with it.

## Which answers have a body

- A JSON `{"error": "…"}` body (`application/json`), with the extra fields given above: every
  refusal of `POST <token>/save` (400, 403, 408, 409, 411, 413, 415,
  422, 500), a refused `events` stream (503), a `POST <token>/closed` from another origin
  (403), and `file?rev=saved` when it serves nothing (404, 409, 500).
- JSON: `config`, `answers`, `diff` and a successful `save` (200).
- `events`: `text/event-stream`, never closed by the command but at exit.
- No body (`Content-Length: 0`): `closed` (204), a wrong `Host` (403), a `GET` from another
  origin (403), any other token or path (404), a 404 under the token, a wrong method (405,
  with `Allow`), a request that cannot be read (400), the 308 redirect and 503.

## Rules for every request

- `Host` must be exactly `127.0.0.1:<port>` (403 otherwise; `localhost` is refused), so a page
  that reaches the port through DNS rebinding gets nothing.
- Any other token or path outside it: 404 with no body.
- A `GET` carrying an `Origin` other than the app's own: 403.
- Static files are only files inside the app or engine directory: `..`, `.`, empty segments,
  encoded slashes, backslashes and colons are refused, and a link leading outside is not
  followed (404).
- Every answer to the app's requests (app and engine files, `file`, `config`, `save`,
  `closed`, and a 404 under the token) carries `Cache-Control: no-store`,
  `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` and

  ```text
  Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval';
    style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:;
    connect-src 'self' blob:;
    worker-src 'self' blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'none';
    form-action 'none'
  ```

  so the app must load nothing from elsewhere (no CDN fonts or scripts) and use no inline
  scripts.
- Content types: `.html` text/html, `.js`/`.mjs` text/javascript, `.wasm` application/wasm,
  `.css` text/css, `.json`/`.map` application/json, `.svg` image/svg+xml, `.png` image/png,
  `.jpg`/`.jpeg` image/jpeg, `.webp` image/webp, `.ico` image/x-icon, `.woff2` font/woff2,
  `.woff` font/woff, `.ttf` font/ttf, `.txt` text/plain; anything else
  application/octet-stream.
- A directory, with or without a trailing slash, is not served (404): there is no index but
  the app's own `index.html` at `<token>/`.
- The token is compared in constant time.
- At most 16 connections are answered at once; more get 503 with `Retry-After: 1`.

## Where the app and the engine come from

The app (built from [ui/](../ui/)) and its WASM engine are built into the `specify-core`
command ([ADR P12](https://github.com/pzfreo/draftwright-project/blob/main/adr/P12-specify-ships-whole.md)):
the release binaries, the Python wheels and the crates.io crate carry them, gzipped (about
6 MB), and `open` and MCP `show` serve them from the binary. Nothing is downloaded or cached:
the page works offline from the first run, and the app, the engine and the command are always
the same version.

`open` and MCP `show` use the same resolution rules:

- **App.** `--app DIR` takes precedence over `SPECIFY_APP_DIR`; the directory must contain
  `index.html`. With neither, the built-in app is served.
- **Engine.** `--wasm DIR` takes precedence over `SPECIFY_WASM_DIR`; either points to a
  wasm-pack `pkg/` directory served at `wasm/`. With neither, an `--app` directory's own
  `wasm/` is served if it has one, else the built-in engine.
- **Session API.** An app's `specify-ui-app.json` (at its root) names it (`specify-ui-app`),
  its `version`, and an integer `api` that must match `config.api`. The built-in app always
  has one; an `--app` with one is checked the same way, and an older local build without one
  gives a warning and continues.

MCP `show` reads the overrides from the server process's environment.

**Building it in.** With the `bundled-app` feature (on by default), `build.rs` builds in the
app from `ui/dist-app` (else `bundle/app`, the copy a published crate carries) when its
manifest names the crate's version, and the engine from `crates/specify-core-wasm/pkg` (else
`bundle/wasm`) when its `package.json` names the crate's version. A development build has them once they are built:

```sh
wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --locked
(cd ui && npm ci && npm run build:app)
cargo build --release
```

A build without them still compiles (with a build warning), and its `open` asks for `--app`
(or a release: `pip install specify-core`). `SPECIFY_REQUIRE_BUNDLE=1` makes a missing app or
engine a build error (the release jobs set it). `cargo test` builds in the small fake app and
engine of `tests/fixtures/bundle` instead (the `bundle-test-fixture` feature).
`tools/open_smoke.py BINARY STEP` checks that a binary serves its built-in app and engine with
no network.
