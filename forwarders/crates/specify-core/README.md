# specify-core: renamed to specify-pmi

`specify-core` is now [`specify-pmi`](https://crates.io/crates/specify-pmi). This crate re-exports
it and its `specify-core` command forwards to `specify-pmi`; it will not be updated.

```
cargo uninstall specify-core && cargo install specify-pmi
```

Source: <https://github.com/pzfreo/specify-pmi>
