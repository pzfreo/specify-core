# specify-core-wasm

Browser bindings for specify-core. See [the WASM guide](https://github.com/pzfreo/specify-core/blob/main/docs/wasm.md)
for the JSON API, build instructions and installation, from npm
(`npm install @draftwright/specify-core-wasm`) or from a release archive.

Initialize the web module, then use `IntentEngine`, `review` and `interpret`.
Errors are thrown as strings. STEP input must be decompressed. The constructor opens a file
of one distinct part with one placement; for an assembly or a part placed more than once, list
the parts with `IntentEngine.parts(name, bytes)` and open one with
`IntentEngine.openPart(name, bytes, part)`. `engine.table()` gives the feature table;
`engine.checkEdits(editsJson)` checks an edit list against the file and `engine.edit(editsJson)`
applies it in memory, all or nothing, with the upstream verified writer: the new bytes, or every
entry that cannot be applied.

Apache-2.0; see [LICENSE](https://github.com/pzfreo/specify-core/blob/main/LICENSE) and
[NOTICE](https://github.com/pzfreo/specify-core/blob/main/NOTICE). The bundled haecceity and
quiddity ([pzfreo/quiddity](https://github.com/pzfreo/quiddity)) are Apache-2.0 too.

Snapshot faces include usable unique authored STEP `f:` names.
Shared/generic labels retain geometric matching and are omitted from analysis. Keep the entire
`evidence()` object with external answers: named parts use schema 3; missing or
duplicate names are unresolved without geometric fallback. See
[face-name matching and compatibility](https://github.com/pzfreo/specify-core/blob/main/docs/face-names.md).

Answer sidecars (with geometry evidence) can be exported before writing:
`engine.exportAnswers(answersJson, approvalsJson?)` returns an answer map as JSON: the answers
given plus exactly the defaults approved (`{"<part>": {"<id>": value}}`).
Open questions and carry blocks do not throw. See
[answer files](https://github.com/pzfreo/specify-core/blob/main/docs/answers.md#export-a-sidecar).
