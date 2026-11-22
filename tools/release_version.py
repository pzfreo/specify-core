"""Check the GitHub release flag and, for PyPI, the version in pyproject.toml."""

import argparse
import json
import os
from pathlib import Path
import re
import tomllib
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


def check_release(tag: str, prerelease: bool | None, require_release: bool = False) -> None:
    has_suffix = "-" in tag
    if prerelease is None:
        if require_release or has_suffix:
            expected = " marked pre-release" if has_suffix else ""
            raise ValueError(f"{tag} requires a GitHub release{expected}.")
    elif has_suffix != prerelease:
        expected = "marked pre-release" if has_suffix else "marked as a full release"
        raise ValueError(f"GitHub release {tag} must be {expected} to match its tag.")


def check_python_version(tag: str, version: str) -> None:
    semver = tag.removeprefix("v")
    try:
        from packaging.version import Version
    except ModuleNotFoundError:
        # Without packaging, accept the exact PEP 440 spelling for final, alpha, beta and RC tags.
        match = re.fullmatch(r"([0-9]+\.[0-9]+\.[0-9]+)(?:-(alpha|beta|rc)\.?([0-9]+))?", semver)
        if match is None:
            raise ValueError(f"Tag {tag} has no supported PEP 440 version.") from None
        base, stage, number = match.groups()
        suffix = {"alpha": "a", "beta": "b", "rc": "rc"}
        expected = base if stage is None else f"{base}{suffix[stage]}{int(number)}"
        matches = version == expected
    else:
        expected = Version(semver)
        if expected.is_prerelease != ("-" in semver):
            raise ValueError(f"Tag {tag} has no equivalent PEP 440 release status.")
        matches = Version(version) == expected
    if not matches:
        raise ValueError(f"Tag {tag} requires Python version {expected}, but pyproject.toml has {version}.")


def lookup_prerelease(tag: str) -> bool | None:
    # Dispatches have no release payload. A 404 permits a tag-only final backfill
    # where the caller allows it. Other API failures must stop publishing.
    api = os.environ["GITHUB_API_URL"]
    repo = os.environ["GITHUB_REPOSITORY"]
    request = Request(
        f"{api}/repos/{repo}/releases/tags/{quote(tag, safe='')}",
        headers={"Authorization": f"Bearer {os.environ['GH_TOKEN']}"},
    )
    try:
        with urlopen(request, timeout=30) as response:
            return json.load(response)["prerelease"]
    except HTTPError as error:
        error.close()
        if error.code != 404:
            raise
        return None


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--require-release", action="store_true")
    parser.add_argument("--python", action="store_true")
    args = parser.parse_args()
    # The PyPI backfill builds main rather than a tag. Its Cargo version names the release.
    tag = args.tag
    if args.python and not tag:
        with open("Cargo.toml", "rb") as manifest:
            tag = "v" + tomllib.load(manifest)["package"]["version"]
    try:
        if os.environ["GITHUB_EVENT_NAME"] == "release":
            event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
            prerelease = event["release"]["prerelease"]
        else:
            prerelease = lookup_prerelease(tag)
        check_release(tag, prerelease, args.require_release)
        if args.python:
            with open("crates/specify-core-py/pyproject.toml", "rb") as pyproject:
                version = tomllib.load(pyproject)["project"]["version"]
            check_python_version(tag, version)
    except (ValueError, URLError) as error:
        raise SystemExit(f"::error::{error}") from None


if __name__ == "__main__":
    main()
