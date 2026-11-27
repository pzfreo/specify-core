"""Decide whether a release can publish and select its registry version or dist-tag."""

import argparse
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
from urllib.error import HTTPError
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


def check_python_version(tag: str, version: str) -> str:
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
    return str(expected)


def cargo_tag() -> str:
    with open("Cargo.toml", "rb") as manifest:
        return "v" + tomllib.load(manifest)["package"]["version"]


def read_python_version() -> str:
    with open("crates/specify-pmi-py/pyproject.toml", "rb") as pyproject:
        return tomllib.load(pyproject)["project"]["version"]


@dataclass(frozen=True)
class Decision:
    publish: bool = True
    dist_tag: str = ""
    python_version: str = ""


def decide_publish(
    channel: str,
    tag: str,
    prerelease: bool | None,
    *,
    latest: str = "",
    python_version: str | None = None,
    registry_status: str | None = None,
) -> Decision:
    check_release(tag, prerelease, require_release=channel in ("github", "npm"))
    expected = check_python_version(tag, python_version) if python_version is not None else ""
    if registry_status == "200":
        return Decision(publish=False)
    if registry_status not in (None, "404"):
        raise ValueError(f"{channel} answered {registry_status} for {tag}: publish refused.")
    if channel == "pypi":
        return Decision(python_version=expected)
    if channel == "npm":
        version = tag.removeprefix("v")
        if "-" in version:
            return Decision(dist_tag="next")
        if latest.strip() == "none":
            # The package has never been published: its first release is latest.
            return Decision(dist_tag="latest")
        if not latest.strip():
            raise ValueError(f"npm latest is missing for {tag}: publish refused.")
        ordered = subprocess.check_output(["sort", "-V"], input=f"{latest}\n{version}\n", text=True)
        if ordered.splitlines()[-1] != version:
            return Decision(dist_tag=f"release-{version}")
        return Decision(dist_tag="latest")
    return Decision()


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
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--channel", choices=("github", "npm", "crates", "pypi"))
    mode.add_argument("--check-lockstep", action="store_true")
    parser.add_argument("--tag")
    parser.add_argument("--prerelease", choices=("true", "false", "missing"),
                        help="Use metadata already resolved by the release check")
    parser.add_argument("--latest", default="")
    parser.add_argument("--registry-status")
    args = parser.parse_args()
    if args.channel and args.tag is None:
        parser.error("--tag is required with --channel")
    try:
        if args.check_lockstep:
            check_python_version(cargo_tag(), read_python_version())
            return
        # The PyPI backfill builds main rather than a tag. Its Cargo version names the release.
        tag = cargo_tag() if args.channel == "pypi" and not args.tag else args.tag
        is_release = os.environ["GITHUB_EVENT_NAME"] == "release"
        if args.prerelease is not None:
            prerelease = {"true": True, "false": False, "missing": None}[args.prerelease]
        elif is_release:
            event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
            prerelease = event["release"]["prerelease"]
        else:
            prerelease = lookup_prerelease(tag)
        # Releases check the tagged checkout on every channel. Dispatch backfills can
        # run from main while publishing an older tag, so skip the pyproject comparison then.
        python_version = read_python_version() if is_release else None
        decision = decide_publish(args.channel, tag, prerelease, latest=args.latest,
                                  python_version=python_version, registry_status=args.registry_status)
    except (ValueError, OSError) as error:
        raise SystemExit(f"::error::{error}") from None
    if not decision.publish:
        print(f"{tag} is already on {args.channel}, skipping publication.")
    prerelease_output = {True: "true", False: "false", None: "missing"}[prerelease]
    output = (
        f"publish={str(decision.publish).lower()}\n"
        f"prerelease={prerelease_output}\n"
        f"dist_tag={decision.dist_tag}\n"
        f"python_version={decision.python_version}\n"
    )
    if path := os.environ.get("GITHUB_OUTPUT"):
        with open(path, "a", encoding="utf-8") as outputs:
            outputs.write(output)
    else:
        print(output, end="")


if __name__ == "__main__":
    main()
