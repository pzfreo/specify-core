# Release checklist

Before publishing a GitHub release or approving a rerun, open each package's
registry settings and verify that its GitHub Actions trusted publisher matches
the entry below. Use `pzfreo` as the repository owner and `specify-core` as the
repository name where the registry asks for them separately.

| Registry | Package | Repository | Workflow file | Environment |
| --- | --- | --- | --- | --- |
| npm | `@draftwright/specify-core-wasm` | `pzfreo/specify-core` | `release.yml` | `npm` |
| npm | `@draftwright/specify-ui` | `pzfreo/specify-core` | `release.yml` | `npm` |
| crates.io | `specify-core` | `pzfreo/specify-core` | `crates.yml` | `crates-io` |
| PyPI | `specify-core` | `pzfreo/specify-core` | `python.yml` | `pypi` |

On npm, check **Allow npm publish** for both packages. The release jobs publish
directly. npm CLI 11.15.0 and later can list settings with
`npm trust list <package>` using an authenticated account with package write
access and 2FA enabled. The maintainer verifies these settings in advance because
the jobs use OIDC only after approval.
[npm CLI documentation](https://docs.npmjs.com/cli/v11/commands/npm-trust/)

For version alignment and the GitHub release flag, follow
[Cutting a pre-release](CONTRIBUTING.md#cutting-a-pre-release).
