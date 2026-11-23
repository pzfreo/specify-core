# 0.3.1: the review panel as a component other apps can embed

**Agreed by Paul, 2026-11-23.** Planned for 0.3.1. Paul set its scope on 2026-11-23 as everything the editor needs to switch to Specify 0.3: this panel, hand-picked datums, free-form `extra.*` requirements, and BOM editing with CSV row exclusions. The grouped fillet rows move to 0.3.2, on quiddity 0.5.6. Work order: the Enter/lost-update fix, then the npm release guard, then 0.3.1. Tests may not be weakened, and it needs an independent review before it's ready.

**One review component (Paul, 2026-11-23).** "I asked to PORT the old UI to a standalone package, not to rewrite it." `SpecifyPanel` is the 0.2 `SpecifyReview` ported to the 0.3 engine, and exported. Its screens, inputs, question order and keyboard are kept. Only the engine layer changes: answers become pending edits, rows are stated, gap or decided none, and Save writes the edits together. The same component serves three hosts: `specify-core open` (from 0.3.0-rc.2), the package export (0.3.1) and the editor (0.3.1). There is no second panel; `ui/app/Panel.tsx` is removed in rc.2.

## What you'll get

Another app, such as the editor, can show Specify's review inside its own page. That's the ported 0.2 review that `specify-core open` shows from rc.2.

- The app gives the panel a STEP file.
- You review and edit requirements exactly as in Specify:
  - each feature with its requirements, each one stated, a gap or decided none;
  - change and remove on stated rows;
  - Add / None needed on gaps;
  - Add all suggestions;
  - the bar of pending edits.
- When the app saves, it gets the new file back. What it does with the file is up to the app: keep it, upload it, download it.

The panel never saves on its own and never touches the page around it. It doesn't open files, show a Save button or decide where files go. The app keeps its own Save button, its own part list for an assembly, and its own layout.

## Why it's needed

- Today the panel lives only inside Specify's own app (`ui/app`). That app is tied to the `specify-core open` command, which saves over local HTTP.
- `@draftwright/specify-ui` exports `SpecifyReview`, which is still the 0.2 question-by-question review (questions, answers, preview). Its three methods can't do 0.3.0's edits.
- So no other app can offer 0.3.0's editing today.

## How the app and the panel talk

**The app gives:**
- the file's bytes and name;
- where the WASM engine is (`wasmUrl`), or an engine it already made;
- for an assembly, which part to show. The app switches parts; the panel shows one part at a time and keeps every part's pending edits.
- optionally, its own 3D view as a `view: HostView`, the same option StepDiff and SpecifyReview take (see [ui/docs/host-view.md](../ui/docs/host-view.md)). With no view passed, the panel uses the package's default view.

**The 3D view and the panel stay in step, through `HostView`:**
- selecting a row asks the view to show, colour and frame that row's faces;
- a face picked in the view selects its row;
- drawn-PMI overlays over a host-owned camera stay under "Later", as they do for `HostView` itself.

**The panel tells the app:**
- what's pending: the edit lists, per part;
- whether it's busy (checking an edit) and whether there's anything to save;

**The app asks the panel:**
- **save**: the panel applies all pending edits in the browser, all or nothing. It gives back the new file's bytes, or the refused entries with their reasons. Those reasons show on their rows, as they do today.
- **discard**: drop the pending edits.
- **load**: reopen the panel on new bytes, for example after the app has stored the file.

All of the writing happens in the browser, through the engine 0.3.0 already ships (`edit`, `checkEdits`, `openPart`). No server is involved, and the result is the same file the command line would write from the same edit list.

## What stays the same

- `specify-core open` keeps its page. That page becomes one user of the component: its Save posts to the local session exactly as today, so agents, `rev` and refusals are unchanged.
- `SpecifyReview` keeps its props: the ported component is `SpecifyReview` itself, exported as `SpecifyPanel`, so the editor's props (slots, focusRequest, toReview, onKeep, tints, onPick and so on) keep working.
- The browser engine's worker can check an edit list today but can't apply one. Adding `edit` to the worker is part of this work.
- The edit list is the same everywhere: command line, MCP, Python, WASM and now the component.

## Decisions (Paul, 2026-11-23)

1. **Name:** `SpecifyPanel`.
2. **The Characteristics tab and the CSV/AS9102 exports stay in the component** (Paul, 2026-11-23; this reverses the morning's decision, because the editor needs them). They are built from the file's PMI. **Agent drafts** stay in `specify-core open` only; the component shows host-supplied pending edits through a generic prop.
3. **Saving runs in the browser by default.** The app can also pass its own save function and get the edit lists to apply elsewhere. `open` uses that to keep saving through its session.
4. **The 3D view is the host's, if it wants** (Amy's requirement). `SpecifyPanel` takes the same optional `view: HostView` as StepDiff and SpecifyReview (ui/docs/host-view.md). Selecting a row shows, colours and frames its faces in that view, and a face picked there selects its row. With no view passed, the panel uses the package's default view. Drawn-PMI overlays over a host camera stay under "Later", as for `HostView`.
5. **Restored as in 0.2:** one-click accept of a suggestion and its alternatives, general-dimension call-outs, and the CSV inclusion ticks. Datums are picked with the 0.2 chips, with pencil and bin on datums the file already states. "Added by you" is shown but disabled in rc.2, and enabled in 0.3.1 with `extra.*`.
6. **Identical parts of an assembly** (Paul, 2026-11-23). Copies group when the engine proves identical geometry and their stated PMI and saved decisions match on corresponding faces. An edit applies to every copy in the group in the same all-or-nothing Save. The row says "applies to N identical parts" and the bar lists the edit once. **Split off this copy** makes subsequent edits on that copy independent until Save. Differences in saved PMI or decisions keep the copies separate on reopen. Splitting leaves the STEP assembly structure as it was.

## How we'll know it works

- A small test page embeds the component with no Specify app around it. It opens GRM-05 and an assembly, then:
  - changes, removes and fills a requirement, then saves;
  - checks that the returned bytes read back as stated;
  - checks that every entity it didn't touch is byte-identical.
- The `open` page's existing browser tests pass unchanged on the component.
- The editor embeds it. That is the real trial, and it is the editor's work, after this ships.
