# Specify sessions: the agent and the page together

Specify on a person's computer is **agent-first**. The person sets Specify up once as a
normal stdio MCP server in their AI agent, with no addresses or tokens:

```sh
claude mcp add specify -- specify-core mcp
```

The agent opens a part (or two revisions of it) through MCP, and Specify holds that file in
the MCP server's process: the *session*. When the person wants to see the part, the agent
calls `show`, which serves the Specify page (docs/open.md) for that same session on 127.0.0.1
and opens the browser. Both sides change the file the same way: each sends an edit list
(fill gaps, change what the file states, remove requirements by number, decide none), applied
all or nothing, and each save is what the other reads next.

`specify-core open FILE` and `specify-core open --diff OLD NEW` are the manual way in: the same
session and the same page, with no agent attached.

## The session

One session per opened file (per pair, for a diff), held in memory by the MCP server:

- the file as its last save left it (the input before any);
- the answers, one answer map (docs/answers.md) per part: those the file stores (decided none,
  and choices that write no PMI), plus any the agent set with `answer` and has not saved;
- `rev`, a number that goes up by one on every change, whoever makes it;
- the recent changes (the last 256), so a side can ask what changed since a `rev`;
- for a diff, the revision report;
- where Save writes: beside the file by default, or over it when the session was opened
  `in_place` (keeping a `.bak`), as docs/open.md's `save` says.

It starts from the answers stored in the file (a file Specify wrote resumes them), for a diff
too: nothing is carried from the old revision (ADR P11). After each save it works on the file
saved, and its answers are those that file stores. It lasts as long as the MCP server
process; nothing is written to disk unless a save or an `answers_file` asks for it (docs/mcp.md's
memory rule).

MCP's documents are parts (`doc`, one per part, as today). All the parts of one file share
its session and its `rev`. The page and MCP can save an assembly's part lists together as one
file revision.

## MCP tools

The existing tools (docs/mcp.md) work on the session, plus these:

| Tool | Arguments | Result |
| --- | --- | --- |
| `open` | as today, plus `old?` (a revision to compare from), `in_place?` (on a session already open, changes where Save writes only when given) | as today, plus `rev`; with `old`, the diff's `verdict`, `summary` and `report` too; the session is the file's own, nothing carried |
| `questions`, `review`, `characteristics`, `read_pmi` | as today | as today, plus `rev`: they read the session as it is now |
| `answer` | as today | as today, plus the new `rev`; the page receives it as an `answers` event with `source: "mcp"`. These answers become working edits in the page's pending bar |
| `changes` | `doc`, `since` (a `rev`) | `rev`, and `changes`: `[{rev, part, source, set: {id: answer}, cleared: [id]}]` after `since`, oldest first. If `since` is older than the changes kept, `full: true` and the whole `answers` instead (every part's, by part index, as the page's `GET answers` gives `parts`) |
| `show` | `doc`, `browser?` (default `true`), `read_only?` | `url`, `mode`. Starts the page server for this session on 127.0.0.1 (random port and token), or returns the running one's address; opens the browser unless `browser: false`. A session opened with `old` is shown in `diff` mode: the page shows what changed beside the new file, and the person may edit and save the new revision. `read_only: true` makes that page view-only (no Save from the page). Only `read_only: true` is refused for a session opened with `old` or `in_place` (as `open` refuses `--read-only` with `--diff` or `--in-place`). A page already running in the other mode is never replaced: `show` is refused with its `url` and `mode` until the person has closed it |
| `save` | `doc`, `edits` for that part or `partEdits: [{part, edits}, …]` for the file, `rev?` (the session revision the agent last saw) | `path`, `bytes`, `rev`, `report` (with `retired` and `unchanged`), `complete`, `missing`: the list applied to the file all or nothing and written with the same naming as the page's Save (docs/open.md). One entry that cannot be applied writes nothing, and the `invalid` error's `failures` names every failing entry and why. A `rev` the session has left is `stale`. The page gets the same `saved` event |
| `check` | `doc`, either edit form above | `applicable`, `entries`, `unchanged`, `failures`, `message`, `rev`. Checks the same lists as `save` without writing |
| `discard` | `doc`, `rev?` | `rev`, `parts`. Drops working answers on all parts of the file and restores the saved answers without writing |

`write_pmi`, which wrote the whole intent of the answers to a path of the agent's choosing, is
gone (ADR P11: a write applies an edit list). `specify-core write STEP EDITS.json -o OUT` writes
an edit list to another path.

A suggestion is written only when the list names it, in `fill` with the value the person saw
or in `accept_suggestions` (ADR P9: "we shouldn't create the default, which then becomes
gospel"): the agent lists only what the person saw and confirmed. Anything the list does not
name is left as it is, byte for byte, whoever wrote it.

An equal `fill` on a decided-none row is unchanged. Two answers meaning none count as
equal. To replace a stored decision, put its ID in both `clear` and `fill` in the same list.
For example, `{"clear":["part.coating"],"fill":{"part.coating":"Anodised"}}` clears the
decision first and fills the resulting gap.

For an editable BOM row, `stated` covers both native product data and an explicit
override written as a STEP property. Use `change` with `to` and the row's `value`
as `expect`. A different `fill` or a `clear` is refused with that guidance. BOM
proposals with source `default` remain gaps and take `fill` or `accept_suggestions`.

Working answers become fills, decisions of none, or clears of stored decisions. A different
answer for a stated row is refused with the value the file states and the guarded change to
make. To stage the change explicitly for the panel, use MCP `check` with `stage: true` and a
`change` entry with `to` and `expect`.
To replace a stored decision with MCP `answer`, explicitly clear its ID and set the new value.
The draft carries both edits to the save.
MCP `save` and `check` combine savable drafts from every part with the explicit lists at
one revision. An explicit edit to the same question takes precedence. The panel's bar marks
when a local edit replaces a draft. For example, `save {doc, edits: {}}` applies the working
edits alone. The page shows refused answers on their rows, and its Save skips them. MCP
`discard` and the page's **Discard** drop drafts and refusals without writing. The page also
offers **Undo** for a single draft or refusal.

An agent that wants to follow the person's saves calls `changes` with the last `rev` it saw
(for example after the person says they are done, or before each step); MCP over stdio has no
way to push to the agent unasked.

## Lifetimes

- **The MCP server process** owns the sessions. When it exits, they are gone (unless saved or
  kept in an `answers_file`) and any page server it started stops.
- **The page server** runs while a page is open. After the page closes it stops as
  docs/open.md says (90 s after `closed`, or 30 minutes with no request). `show` can be called
  again at any time, for example after the tab was closed, and starts a new one (a new address).
  Several sessions may each have a page.
- **`specify-core open`** holds one session and one page server, and exits when the page
  server stops.

## Rules kept from MCP and the page

- Paths: `open`, `save` and the rest stay inside `--allow` (docs/mcp.md). Save writes beside
  the file, which is inside it.
- Stale files: a file changed on disk after `open` is refused as `stale`, except by the
  session's own `in_place` save, which records what it wrote. A `save` during which the
  session changed (an agent's answer, which a save without `rev` does not guard) is refused as
  `stale` too, nothing written: read and save again.
- A save applies its whole list or nothing; a write that fails leaves the file and the
  session as they were.
- The page server's rules (exact Host, token, Origin on changes, CSP, connection caps) are
  docs/open.md's, unchanged. There is no MCP endpoint over HTTP in the page server: the agent
  talks to the session over its own stdio connection only.
