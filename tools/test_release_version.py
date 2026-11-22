"""Offline tests for the release checks used by the publishing workflows."""

import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

from release_version import check_python_version, check_release, decide_publish, lookup_prerelease


class ReleaseVersionTests(unittest.TestCase):
    def test_workflow_publish_decisions(self):
        script = Path(__file__).with_name("release_version.py").resolve()
        jobs = (
            ("wasm", "github", False),
            ("npm", "npm", True),
            ("ui-guard", "github", False),
            ("ui-npm", "npm", True),
            ("crates-bundle", "crates", False),
            ("crates-publish", "crates", True),
            ("PyPI", "pypi", False),
            ("cli-attach", "github", False),
        )
        rc = "0.3.0-rc.1"
        final = "0.3.0"
        cases = [
            (rc, True, "0.3.0rc1", "", "next", "0.3.0rc1", True),
            (rc, False, "0.3.0rc1", "0.2.3", "", "", False),
            (final, True, final, "0.2.3", "", "", False),
            (final, False, final, "0.2.3", "latest", final, True),
            (final, False, final, "0.3.1", "release-0.3.0", final, True),
            (final, False, final, "", "", final, True),
            (rc, True, final, "0.2.3", "", "", False),
        ]
        header = ("tag", "flag", "pyproject", "npm-latest", *(job[0] for job in jobs))
        actual_rows = []
        expected_rows = []
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            pyproject = root / "crates/specify-core-py/pyproject.toml"
            pyproject.parent.mkdir(parents=True)
            event = root / "event.json"
            output = root / "output"
            env = {**os.environ, "GITHUB_EVENT_NAME": "release",
                   "GITHUB_EVENT_PATH": str(event), "GITHUB_OUTPUT": str(output)}
            for version, flag, python_version, latest, dist_tag, pypi_version, eligible in cases:
                tag = f"v{version}"
                flag_value = str(flag).lower()
                event.write_text(json.dumps({"release": {"tag_name": tag, "prerelease": flag}}))
                pyproject.write_text(f'[project]\nversion = "{python_version}"\n')
                actual = [tag, flag_value, python_version, latest or "(missing)"]
                expected = actual.copy()
                for name, channel, queries_registry in jobs:
                    command = [sys.executable, str(script), "--channel", channel, f"--tag={tag}"]
                    if queries_registry:
                        command.extend((f"--prerelease={flag_value}", "--registry-status=404"))
                    if channel == "npm":
                        command.append(f"--latest={latest}")
                    output.unlink(missing_ok=True)
                    result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
                    with self.subTest(job=name, tag=tag, flag=flag, pyproject=python_version):
                        if result.returncode:
                            self.assertEqual(result.returncode, 1)
                            self.assertIn("::error::", result.stderr)
                            self.assertFalse(output.exists())
                            actual.append("fail")
                        else:
                            fields = dict(line.split("=", 1) for line in output.read_text().splitlines())
                            self.assertEqual(fields["prerelease"], flag_value)
                            status = "publish" if fields["publish"] == "true" else "skip"
                            detail = fields["dist_tag"] or fields["python_version"]
                            actual.append(f"{status}/{detail}" if detail else status)
                    detail = {"npm": dist_tag, "pypi": pypi_version}.get(channel, "")
                    if not eligible or (channel == "npm" and not dist_tag):
                        expected.append("fail")
                    else:
                        expected.append(f"publish/{detail}" if detail else "publish")
                actual_rows.append(actual)
                expected_rows.append(expected)
        print()
        for row in (header, *actual_rows):
            print(" | ".join(row))
        self.assertEqual(actual_rows, expected_rows)

    def test_cargo_and_pyproject_versions_must_be_lockstep(self):
        script = Path(__file__).with_name("release_version.py").resolve()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "Cargo.toml").write_text('[package]\nversion = "0.3.0-rc.1"\n')
            pyproject = root / "crates/specify-core-py/pyproject.toml"
            pyproject.parent.mkdir(parents=True)
            for version, exit_code in (("0.3.0", 1), ("0.3.0rc1", 0)):
                with self.subTest(pyproject=version):
                    pyproject.write_text(f'[project]\nversion = "{version}"\n')
                    result = subprocess.run([sys.executable, str(script), "--check-lockstep"],
                                            cwd=root, capture_output=True, text=True)
                    self.assertEqual(result.returncode, exit_code, result.stderr)
                    if exit_code:
                        self.assertIn("v0.3.0-rc.1 requires Python version 0.3.0rc1", result.stderr)

    def test_dispatch_uses_the_requested_tag_without_comparing_main_pyproject(self):
        script = Path(__file__).with_name("release_version.py").resolve()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            pyproject = root / "crates/specify-core-py/pyproject.toml"
            pyproject.parent.mkdir(parents=True)
            pyproject.write_text('[project]\nversion = "0.3.0"\n')
            output = root / "outputs"
            env = {**os.environ, "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_OUTPUT": str(output)}
            for channel in ("github", "npm", "crates", "pypi"):
                with self.subTest(channel=channel):
                    output.unlink(missing_ok=True)
                    result = subprocess.run(
                        [sys.executable, str(script), "--channel", channel, "--tag=v0.2.3-rc.1",
                         "--prerelease=true"], cwd=root, env=env, capture_output=True, text=True,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("publish=true", output.read_text().splitlines())

    def test_registry_skip_and_error(self):
        for channel in ("npm", "crates"):
            with self.subTest(channel=channel):
                self.assertFalse(decide_publish(channel, "v0.3.0-rc.1", True, registry_status="200").publish)
                with self.assertRaisesRegex(ValueError, "answered 503"):
                    decide_publish(channel, "v0.3.0-rc.1", True, registry_status="503")

    def test_release_flags_must_match_tags(self):
        check_release("v0.2.3", False)
        check_release("v0.3.0", False)
        check_release("v0.3.0-rc.1", True)

        with self.assertRaisesRegex(ValueError, "v0.3.0-rc.1 must be marked pre-release"):
            check_release("v0.3.0-rc.1", False)
        with self.assertRaisesRegex(ValueError, "v0.3.0 must be marked as a full release"):
            check_release("v0.3.0", True)

    def test_dispatch_without_a_github_release(self):
        check_release("v0.2.3", None)
        with self.assertRaisesRegex(ValueError, "v0.2.3 requires a GitHub release"):
            check_release("v0.2.3", None, require_release=True)
        with self.assertRaisesRegex(ValueError, "v0.3.0-rc.1 requires a GitHub release marked pre-release"):
            check_release("v0.3.0-rc.1", None)

    def test_python_versions_must_match_tags(self):
        check_python_version("v0.2.3", "0.2.3")
        check_python_version("v0.3.0", "0.3.0")
        check_python_version("v0.3.0-rc.1", "0.3.0rc1")

        with self.assertRaisesRegex(ValueError, "requires Python version 0.3.0rc1"):
            check_python_version("v0.3.0-rc.1", "0.3.0")
        with self.assertRaisesRegex(ValueError, "requires Python version 0.3.0rc1"):
            check_python_version("v0.3.0-rc.1", "0.3.0rc2")
        with self.assertRaisesRegex(ValueError, "requires Python version 0.3.0"):
            check_python_version("v0.3.0", "0.3.0rc1")

    @patch.dict(os.environ, {
        "GITHUB_API_URL": "https://api.example.com",
        "GITHUB_REPOSITORY": "owner/repo",
        "GH_TOKEN": "test",
    })
    @patch("release_version.urlopen")
    def test_dispatch_uses_release_metadata_and_distinguishes_api_errors(self, urlopen):
        urlopen.return_value = io.BytesIO(b'{"prerelease": true}')
        self.assertIs(lookup_prerelease("v0.3.0-rc.1"), True)
        request = urlopen.call_args.args[0]
        self.assertEqual(request.full_url, "https://api.example.com/repos/owner/repo/releases/tags/v0.3.0-rc.1")

        urlopen.side_effect = HTTPError(request.full_url, 404, "Not Found", {}, None)
        self.assertIsNone(lookup_prerelease("v0.3.0"))
        urlopen.side_effect = HTTPError(request.full_url, 403, "Forbidden", {}, None)
        with self.assertRaises(HTTPError):
            lookup_prerelease("v0.3.0")
