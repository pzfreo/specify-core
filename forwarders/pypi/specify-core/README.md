# specify-core: renamed to specify-pmi

`specify-core` is now [`specify-pmi`](https://pypi.org/project/specify-pmi/). This package depends
on it and `import specify_core` forwards to `specify_pmi` with a DeprecationWarning; it will not be
updated. The `specify-core` command is installed by `specify-pmi`.

```
pip uninstall specify-core && pip install specify-pmi
```

Source: <https://github.com/pzfreo/specify-pmi>
