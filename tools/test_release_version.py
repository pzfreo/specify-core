"""Offline tests for the release checks used by the publishing workflows."""

import io
import os
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

from release_version import check_python_version, check_release, lookup_prerelease


class ReleaseVersionTests(unittest.TestCase):
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
