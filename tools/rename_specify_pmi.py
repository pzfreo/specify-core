#!/usr/bin/env python3
"""Rename specify-core to specify-pmi across the repository (proposals/specify-pmi-rename.md).

Re-runnable: run it from the repository root on a clean tree (it is safe to run twice). It
`git mv`s the paths, then rewrites the text, then normalises the lock files. Commit the script
and its output separately, so the output can be regenerated on top of other changes:

    git checkout -B cl/rename-specify-pmi origin/main
    python3 tools/rename_specify_pmi.py

Names that stay (the plan's "Unchanged" list, plus what is not this project):
  * `specify-core:written:v1` and `specify-core original`: the STEP provenance data;
  * `SPECIFY_CORE` in the Python-oracle tooling (it names a checkout of the Python project);
  * `specify-core-python` (another repository) and `/Users/paul/repos/specify-core` (a local path);
  * the Python-oracle tools and fixtures, and the port-era design docs, which describe the Python
    project that was called specify-core (only this repository's own names change in them).
In the other source and docs, a prose line about Python or "port of" keeps a bare `specify-core`
(the Python original) and changes only the technical forms (backticked, a path, a URL, a suffix).
"""
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Files where only this repository's own names change (-rust, -wasm, -py): they are about the
# Python project that was called specify-core.
SKIP_BARE = (
    "tools/capture/",
    "tools/capture.py",
    "tools/known_differences/",
    "tools/make_parts.py",
    "tests/fixtures/",
    "docs/design.md",
    "docs/known-differences.md",
    "docs/questions.md",
    "docs/upstream-needs.md",
    "docs/next-session.md",
    "proposals/",
    "CHANGELOG.md",
)
# Exceptions to SKIP_BARE: a built bundle that is this project's own engine.
NOT_SKIPPED = ("tests/fixtures/bundle/wasm/",)

# Files where every occurrence is this project (no Python-line protection).
FULL = (
    "README.md", "RELEASING.md", "CONTRIBUTING.md", "docs/python.md", "docs/wasm.md",
    "crates/", "ui/", ".github/", "Cargo.toml", "Cargo.lock", "build.rs", "step-diff/",
    "scripts/", "tools/", "tests/fixtures/bundle/wasm/",
)

# Files that read the CLI's path from SPECIFY_CORE (the binary, not the Python checkout).
ENV_FILES = (
    ".github/workflows/ci.yml", ".github/workflows/release.yml", "README.md", "ui/README.md",
    "ui/test/app-agent.mjs", "ui/test/app-browser.mjs", "tools/verify.sh",
)

PYTHONISH = re.compile(r"\bPython\b|\bport of\b|\bported\b|\boracle\b|\bargparse\b|\.py\b|\bPython's", re.I)
MASKS = [
    "specify-core:written", "specify-core original", "specify-core-python",
    "/Users/paul/repos/specify-core", "?? process.env.SPECIFY_CORE",
    '"specify-core" => "specify-pmi"',
    "The oracle's `specify-core` executable",
]


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


# The 0.3.x `specify-core` alias (a binary, a console script, an archive copy and their notice)
# keeps its name: these lines are left as they are.
ALIAS_LINE = re.compile(
    r'specify-core is now|specify_core_alias|BIN_EXE_specify-core|^name = "specify-core"|'
    r'specify-core-copy|specify-core\$\{\{|^specify-core = |which\("specify-core"|specify-core\{\}|'
    r'old command name|`specify-core` still works|named `specify-core`|'
    r'`specify-core` (alias|console script)|specify-core alias|``specify-core`` console|'
    r'"specify-core"\.as_ref|`specify-core`: the old name|'
    r'cargo install specify-core|cargo uninstall specify-core|installed `specify-core` crate|'
    r'`specify-core` forwarder'
)


def new_path(p):
    if p == "src/specify_core_alias.rs":
        return p
    return p.replace("specify-core-wasm", "specify-pmi-wasm").replace("specify_core_wasm", "specify_pmi_wasm") \
        .replace("specify-core-py", "specify-pmi-py").replace("specify_core", "specify_pmi")


def under(path, prefixes):
    return any(path == p or path.startswith(p) for p in prefixes)


def own_names(s):
    """This repository's own names, in every file."""
    s = s.replace("specify-core-rust", "specify-pmi")
    s = s.replace("specify-core-wasm", "specify-pmi-wasm").replace("specify_core_wasm", "specify_pmi_wasm")
    return s.replace("specify-core-py", "specify-pmi-py")


TECH_HYPHEN = re.compile(r"(?<=[`/@$_])specify-core|specify-core(?=-[a-z0-9]|\b/|\.git\b|`)")


def bare_line(line, protect_python):
    line = line.replace("specify_cores_types", "specify_pmi_types").replace("specify_core", "specify_pmi")
    if protect_python and PYTHONISH.search(line):
        return TECH_HYPHEN.sub("specify-pmi", line)
    return line.replace("specify-core", "specify-pmi")


def rewrite(path, text):
    masked = text
    for i, m in enumerate(MASKS):
        masked = masked.replace(m, f"\0{i}\0")
    masked = own_names(masked)
    if under(path, NOT_SKIPPED) or not under(path, SKIP_BARE):
        protect = not under(path, FULL)
        if path in ENV_FILES:
            masked = re.sub(r"SPECIFY_CORE\b", "SPECIFY_PMI", masked)
        masked = "".join(
            l if ALIAS_LINE.search(l) else bare_line(l, protect)
            for l in masked.splitlines(keepends=True)
        )
    for i, m in enumerate(MASKS):
        masked = masked.replace(f"\0{i}\0", m)
    return masked


def special_edits(path, text):
    """Edits that are not a plain replacement. Each is applied once (it checks its own result)."""
    if path == "Cargo.toml":
        text = text.replace(" (Rust port of specify-pmi)", "")
    if path == "src/cli.rs":
        # clap_complete's bash case names carry the program name's `-` as `__subcmd__`.
        text = text.replace("specify__subcmd__core__subcmd__", "specify__subcmd__pmi__subcmd__")
        text = text.replace("`specify__core__subcmd__write`", "`specify__pmi__subcmd__write`")
        text = text.replace('"specify__core__subcmd__"', '"specify__pmi__subcmd__"')
    if path == "tests/cli.rs":
        # The Python oracle's executable is the retired `specify-core`: this crate's is `specify-pmi`.
        text = text.replace(
            '        let program = program.as_str().unwrap();\n        if !ours.contains(program) {',
            '        // The oracle\'s `specify-core` executable is this crate\'s `specify-pmi`.\n'
            '        let program = match program.as_str().unwrap() {\n'
            '            "specify-core" => "specify-pmi",\n'
            '            other => other,\n'
            '        };\n'
            '        if !ours.contains(program) {',
        )
    if path in ("ui/test/app-agent.mjs", "ui/test/app-browser.mjs"):
        text = text.replace("process.env.SPECIFY_PMI ?? 'specify-pmi'",
                            "process.env.SPECIFY_PMI ?? process.env.SPECIFY_CORE ?? 'specify-pmi'")
    if path == "CHANGELOG.md":
        text = text.replace("All notable changes to specify-core-rust.", "All notable changes to specify-pmi (formerly specify-core and specify-core-rust).", 1)
        marker = "- **Renamed: specify-core is now specify-pmi.**"
        entry = (
            marker + " Every package, the command and the repository:\n"
            "  the crate and the command are `specify-pmi` (library `specify_pmi`); the WASM crate is\n"
            "  `specify-pmi-wasm` and its npm package `@draftwright/specify-pmi-wasm` (generated\n"
            "  `specify_pmi_wasm.js` and `specify_pmi_wasm_bg.wasm`); the Python distribution is\n"
            "  `specify-pmi` and its module `specify_pmi`; the release assets are `specify-pmi-<target>…`;\n"
            "  the repository is `pzfreo/specify-pmi` (GitHub redirects the old links).\n"
            "  `@draftwright/specify-ui` keeps its name. The old names get forwarding releases that\n"
            "  re-export the new ones. The `SPECIFY_CORE` test variable that names the command's\n"
            "  binary is now `SPECIFY_PMI`; `SPECIFY_CORE` is still read for one release. Nothing in the\n"
            "  files changes: `_specify` in STEP, the edit lists and session `api` 2 are as before.\n"
            "  Entries below keep the names they were written under.\n"
        )
        if marker not in text:
            text = text.replace("Read first:\n\n", "Read first:\n\n" + entry, 1)
    return text


def main():
    if git("status", "--porcelain").strip() and "--force" not in sys.argv:
        sys.exit("working tree is not clean (commit or stash first, or pass --force)")
    # 1. Paths.
    for p in git("ls-files").splitlines():
        n = new_path(p)
        if n != p:
            (ROOT / n).parent.mkdir(parents=True, exist_ok=True)
            git("mv", p, n)
    # 2. Text.
    changed = 0
    for p in git("ls-files").splitlines():
        if p.endswith((".wasm", ".png", ".jpg", ".gz", ".step", ".stp", ".ico")):
            continue
        f = ROOT / p
        try:
            text = f.read_text(encoding="utf-8")
        except (UnicodeDecodeError, IsADirectoryError, FileNotFoundError):
            continue
        if p == "tools/rename_specify_pmi.py":
            continue
        out = special_edits(p, text if p == "CHANGELOG.md" else rewrite(p, text))
        if out != text:
            f.write_text(out, encoding="utf-8")
            changed += 1
    print(f"rewrote {changed} files")
    # 3. Lock files: cargo re-sorts Cargo.lock for the new names.
    subprocess.run(["cargo", "metadata", "--format-version", "1", "--offline"], cwd=ROOT,
                   env={**__import__("os").environ, "RUSTC_WRAPPER": ""}, stdout=subprocess.DEVNULL)
    lock = ROOT / "ui/package-lock.json"
    if lock.exists():
        json.loads(lock.read_text())  # must still parse


if __name__ == "__main__":
    main()
