#!/usr/bin/env python3
"""Check the reviewed wasm-bindgen declarations and API changelog acknowledgement."""

import argparse
import difflib
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SNAPSHOT = Path("crates/specify-core-wasm/api.d.ts")
GENERATED = Path("crates/specify-core-wasm/pkg/specify_core_wasm.d.ts")
# Preserve string literals while discarding generated documentation/version comments.
TOKENS = re.compile(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|/\*.*?\*/|//[^\n]*', re.S)
SPACE = re.compile(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|\s+')


def compact(source):
    return SPACE.sub(lambda m: " " if m[0][0].isspace() else m[0], source).strip()


def statements(source):
    """Split declarations/members without splitting nested types or string literals."""
    parts, start, depth = [], 0, 0
    strings = {match.start(): match.end() for match in TOKENS.finditer(source)}
    index = 0
    while index < len(source):
        if index in strings:
            index = strings[index]
            continue
        char = source[index]
        if char in "{([":
            depth += 1
        elif char in "})]":
            depth -= 1
        block = re.match(r"(?:export\s+)?(?:declare\s+)?(?:class|interface)\b", source[start:].lstrip())
        if depth == 0 and (char == ";" or (char == "}" and block)):
            parts.append(source[start:index + 1].strip())
            start = index + 1
        index += 1
    if source[start:].strip():
        parts.append(source[start:].strip())
    return parts


def normalize(source):
    source = TOKENS.sub(lambda m: " " if m[0].startswith(("/*", "//")) else m[0], source)
    declarations = []
    for declaration in statements(source):
        # Sort class/interface members too: wasm-bindgen may reorder its export map.
        match = re.match(r"((?:export\s+)?(?:declare\s+)?(?:class|interface)\b[^{}]*\{)(.*)\}\s*$", declaration, re.S)
        if match:
            members = sorted(compact(member) for member in statements(match[2]))
            declaration = compact(match[1]) + "\n" + "\n".join("  " + member for member in members) + "\n}"
        else:
            declaration = compact(declaration)
        declarations.append(declaration)
    return "\n\n".join(sorted(declarations)) + "\n"


def api_notes(changelog):
    """Only bullets under Unreleased / WASM API acknowledge an API change."""
    unreleased = wasm_api = False
    notes = set()
    for line in changelog.splitlines():
        if line.startswith("## "):
            unreleased = line[3:].strip().lower() in ("unreleased", "[unreleased]")
            wasm_api = False
        elif line.startswith("### "):
            wasm_api = unreleased and line[4:].strip().lower() == "wasm api"
        elif unreleased and wasm_api and re.match(r"^[-*] \S", line):
            notes.add(line.strip())
    return notes


def git_file(root, ref, path):
    result = subprocess.run(["git", "show", f"{ref}:{path}"], cwd=root, text=True, capture_output=True)
    if result.returncode:
        # A newly introduced file has no contents at the comparison commit.
        listing = subprocess.run(["git", "ls-tree", ref, "--", str(path)], cwd=root, text=True, capture_output=True, check=True)
        if listing.stdout:
            raise RuntimeError(result.stderr.strip())
        return ""
    return result.stdout


def check(root, generated, snapshot, base_ref):
    actual = normalize(generated.read_text())
    expected = snapshot.read_text() if snapshot.exists() else ""
    mismatch = actual != expected
    # Also catch a snapshot update that would otherwise silently bypass the gate.
    previous = git_file(root, base_ref, snapshot.relative_to(root))
    changed = mismatch or expected != previous
    if not changed:
        print("WASM API: generated declarations match the reviewed snapshot.")
        return 0
    if mismatch:
        print("".join(difflib.unified_diff(expected.splitlines(True), actual.splitlines(True), fromfile=str(snapshot), tofile=str(generated))), end="")
    changelog = root / "CHANGELOG.md"
    added_notes = api_notes(changelog.read_text()) - api_notes(git_file(root, base_ref, Path("CHANGELOG.md")))
    print("WASM API changed. Update the reviewed snapshot with: python3 tools/check_wasm_api.py --update")
    if added_notes:
        print("WASM API: acknowledged by a new Unreleased / WASM API changelog bullet.")
        return 0
    print("ERROR: add a new bullet under '### WASM API' in CHANGELOG.md's '## Unreleased' section in the same change.")
    return 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--update", action="store_true", help="replace the reviewed snapshot with normalized generated declarations")
    parser.add_argument("--base-ref", default="HEAD", help="commit before this change (CI supplies the PR base or push's previous commit; default HEAD)")
    args = parser.parse_args()
    generated, snapshot = ROOT / GENERATED, ROOT / SNAPSHOT
    try:
        if args.update:
            snapshot.write_text(normalize(generated.read_text()))
            print(f"Updated {SNAPSHOT}; review the diff and add an Unreleased / WASM API changelog bullet.")
            return 0
        return check(ROOT, generated, snapshot, args.base_ref)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"WASM API check failed: {error}. Build with wasm-pack first and use --update to refresh the snapshot.")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
