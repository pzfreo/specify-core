# @draftwright/specify-core-wasm: renamed to @draftwright/specify-pmi-wasm

`@draftwright/specify-core-wasm` is now
[`@draftwright/specify-pmi-wasm`](https://www.npmjs.com/package/@draftwright/specify-pmi-wasm).
This package re-exports it and will not be updated.

```
npm uninstall @draftwright/specify-core-wasm && npm install @draftwright/specify-pmi-wasm
```

The wasm asset is now named `specify_pmi_wasm_bg.wasm` (it was `specify_core_wasm_bg.wasm`). This
package does not ship it; bundlers resolve it through the new package.

Source: <https://github.com/pzfreo/specify-pmi>
