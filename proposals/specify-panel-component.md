# 0.3.1: the review panel as a component other apps can embed

**Agreed by Paul, 2026-11-23.** Planned for 0.3.1. It's built after the CI-speed work and once 0.3.0-rc.1 has published, alongside the other 0.3.1 items. Tests may not be weakened, and it needs an independent review before it's ready.

## What you'll get

Another app, such as the editor, can show Specify's 0.3.0 review panel inside its own page. That's the panel `specify-core open` shows today.

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

**The panel tells the app:**
- what's pending: the edit lists, per part;
- whether it's busy (checking an edit) and whether there's anything to save;
- for highlighting on the app's own 3D view, if it has one: which faces a row is about, when you click its label.

**The app asks the panel:**
- **save**: the panel applies all pending edits in the browser, all or nothing. It gives back the new file's bytes, or the refused entries with their reasons. Those reasons show on their rows, as they do today.
- **discard**: drop the pending edits.
- **load**: reopen the panel on new bytes, for example after the app has stored the file.

All of the writing happens in the browser, through the engine 0.3.0 already ships (`edit`, `checkEdits`, `openPart`). No server is involved, and the result is the same file the command line would write from the same edit list.

## What stays the same

- `specify-core open` keeps its page. That page becomes one user of the component: its Save posts to the local session exactly as today, so agents, `rev` and refusals are unchanged.
- `SpecifyReview` stays exported and unchanged in 0.3.1, so existing users don't break. The changelog marks it as the old review.
- The browser engine's worker can check an edit list today but can't apply one. Adding `edit` to the worker is part of this work.
- The edit list is the same everywhere: command line, MCP, Python, WASM and now the component.

## Decisions (Paul, 2026-11-23)

1. **Name:** `SpecifyPanel`.
2. **Agent drafts and the Characteristics tab** stay out of the component for 0.3.1. They depend on the local session (agents) and on the 3D review. The `open` page keeps both.
3. **Saving runs in the browser by default.** The app can also pass its own save function and get the edit lists to apply elsewhere. `open` uses that to keep saving through its session.

## How we'll know it works

- A small test page embeds the component with no Specify app around it. It opens GRM-05 and an assembly, then:
  - changes, removes and fills a requirement, then saves;
  - checks that the returned bytes read back as stated;
  - checks that every entity it didn't touch is byte-identical.
- The `open` page's existing browser tests pass unchanged on the component.
- The editor embeds it. That is the real trial, and it is the editor's work, after this ships.
