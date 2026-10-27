# STEP diff action

On each pull request that changes a STEP file, this action compares the file with the base
branch's version using `specify-core diff`. It keeps one comment on the pull request up to date
with:

- what changed in the part, in words;
- a picture of the new part, coloured by what changed;
- its volume, area and bounding box before and after;
- the [Specify](../README.md) answers carried to the new revision, and those to check.

## Adding it to a repository

Add `.github/workflows/step-diff.yml`:

```yaml
name: STEP diff
on:
  pull_request:
    paths: ['**.step', '**.stp', '**.STEP', '**.STP', '**.answers.json']
permissions:
  contents: write       # the pictures, on the step-diff-images branch
  pull-requests: write  # the comment
jobs:
  diff:
    runs-on: ubuntu-latest
    steps:
      - uses: pzfreo/specify-core-rust/step-diff@main
```

That is all. The action checks the repository out itself.

- **What it compares:** each STEP file the pull request changes, as it would be once merged,
  against the base branch's version. A renamed file is compared with its old path only when the
  file is unchanged by the rename; otherwise it is reported as one part removed and another added.
  Added and removed parts are listed. A file Specify can't read is reported with the error, and
  the other files are still compared. An assembly isn't compared yet: specify-core asks for one
  of its parts.

- **Answers:** a part's answers live beside it as `<name>.answers.json` (for `bracket.step`,
  `bracket.answers.json`). The base branch's answers are the ones carried. The comment holds
  the carried answers to commit. Without an answers file, the answers Specify wrote into the
  old STEP file as PMI are carried.
- **Pictures:** they are pushed to an orphan branch, `step-diff-images`, under
  `pr-<number>/<commit>/`, so that the comment can show them.
- **Version:** `@main` runs the latest specify-core release, and a release tag (`@v0.1.0-alpha.10`)
  runs that release. `with: version: v0.1.0-alpha.9` picks one explicitly.
- **Limits:** it runs on `ubuntu-latest`. A pull request from a fork gets a read-only token,
  so there the action passes without commenting, with a notice saying so.
