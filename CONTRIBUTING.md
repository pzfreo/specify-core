# Contributing to specify-core

Thank you for helping. Bug reports with a STEP file that shows the problem are
the most useful contribution; if the part cannot be shared, a small synthetic
part that reproduces it works as well.

## Before a pull request

- Open an issue first for anything larger than a small fix, so the approach
  can be agreed before you spend time on it.
- Add a test that fails without your change.
- Run `uv run ruff check src tests tools`, `uv run ruff format --check src tests tools`
  and `uv run pytest`.

## Contributor licence agreement

specify-core is licensed under the GNU Affero General Public License v3.0 or
later, and may also be offered under other terms. So that it can be, every
contributor is asked to agree to the
[Contributor License Agreement](https://gist.github.com/pzfreo/942bdf3dbf5dc98f2c95acfec2e3b8b8). CLA Assistant asks you
to sign it, once, when you open your first pull request; you keep the copyright
in your contributions.
