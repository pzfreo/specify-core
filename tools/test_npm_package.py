import json
import os
import shutil
import pathlib
import tempfile
import unittest

import npm_package
from release_version import decide_publish


def write_pkg(root: pathlib.Path, **meta) -> pathlib.Path:
    pkg = root / "pkg"
    pkg.mkdir()
    for name in npm_package.MODULE_FILES + npm_package.LEGAL_FILES:
        (pkg / name).write_text("x", encoding="utf-8")
    base = {
        "name": "specify-pmi-wasm",
        "version": "0.1.17",
        "license": "Apache-2.0",
        "repository": {"type": "git", "url": "https://github.com/pzfreo/specify-pmi"},
        "main": "specify_pmi_wasm.js",
        "files": list(npm_package.MODULE_FILES),
    }
    base.update(meta)
    (pkg / "package.json").write_text(json.dumps(base), encoding="utf-8")
    return pkg


class NpmPackageTests(unittest.TestCase):
    def test_publish_dist_tags(self):
        for version, latest, expected in [
            ("0.2.3", "0.2.3", "latest"),
            ("0.2.3", "0.3.0", "release-0.2.3"),
            ("0.3.0", "0.2.3", "latest"),
            ("0.3.0-rc.1", "0.2.3", "next"),
            ("0.3.0-rc.1", "0.3.0", "next"),
            ("0.3.0-rc.1", "", "next"),
        ]:
            with self.subTest(version=version, latest=latest):
                decision = decide_publish("npm", f"v{version}", "-" in version, latest=latest)
                self.assertTrue(decision.publish)
                self.assertEqual(decision.dist_tag, expected)

    def test_final_publish_needs_npm_latest(self):
        with self.assertRaisesRegex(ValueError, "npm latest is missing for v0.3.0"):
            decide_publish("npm", "v0.3.0", False)

    def test_scopes_the_name_and_keeps_the_module(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg = write_pkg(pathlib.Path(tmp))
            npm_package.prepare(pkg, "0.1.17")
            meta = json.loads((pkg / "package.json").read_text(encoding="utf-8"))
            self.assertEqual(meta["name"], "@draftwright/specify-pmi-wasm")
            self.assertEqual(meta["publishConfig"], {"access": "public", "provenance": True})
            self.assertEqual(meta["repository"]["url"], "https://github.com/pzfreo/specify-pmi")
            self.assertEqual(meta["main"], "specify_pmi_wasm.js")
            self.assertEqual(meta["version"], "0.1.17")
            self.assertIn("NOTICE", meta["files"])

    def test_refuses_a_version_other_than_the_tag(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg = write_pkg(pathlib.Path(tmp))
            with self.assertRaises(SystemExit):
                npm_package.prepare(pkg, "0.1.18")

    def test_refuses_a_missing_module_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg = write_pkg(pathlib.Path(tmp))
            (pkg / "specify_pmi_wasm_bg.wasm").unlink()
            with self.assertRaises(SystemExit):
                npm_package.prepare(pkg)

    def test_refuses_a_package_without_notice(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg = write_pkg(pathlib.Path(tmp))
            (pkg / "NOTICE").unlink()
            with self.assertRaises(SystemExit):
                npm_package.prepare(pkg)

    @unittest.skipUnless(shutil.which("npm"), "npm not installed")
    def test_check_pack_takes_a_relative_directory(self):
        # The release job passes `release/pkg`, which npm alone would read as a GitHub repository.
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "release").mkdir()
            pkg = write_pkg(root / "release")
            npm_package.prepare(pkg, "0.1.17")
            cwd = os.getcwd()
            os.chdir(root)
            try:
                packed = npm_package.check_pack(pathlib.Path("release/pkg"))
            finally:
                os.chdir(cwd)
            self.assertIn("NOTICE", packed)
            self.assertIn("specify_pmi_wasm_bg.wasm", packed)


if __name__ == "__main__":
    unittest.main()
