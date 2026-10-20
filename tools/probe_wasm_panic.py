#!/usr/bin/env python3
"""Diagnose the target's panic strategy, with a fault injected into a disposable source copy.

Not a production feature or export. Run separately from parity; a trap may poison Rust's
borrow bookkeeping, so this process must not be used for the hostile-input regression.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    with tempfile.TemporaryDirectory(prefix="specify-panic-") as directory:
        copy = Path(directory) / "repo"
        shutil.copytree(ROOT, copy, ignore=shutil.ignore_patterns(".git", "target", "pkg", "__pycache__"))
        source = copy / "crates/specify-core-wasm/src/session.rs"
        text = source.read_text()
        needle = "    pub fn review(&self, answers: &Map<String, Value>) -> Result<Value, String> {"
        assert text.count(needle) == 1
        source.write_text(text.replace(needle, needle + '''
        if answers.contains_key("__panic_probe") {
            panic!("injected IntentEngine review panic (#26)");
        }
'''))
        env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "target"))
        subprocess.run(["wasm-pack", "build", "crates/specify-core-wasm", "--target", "web",
                        "--profile", "wasm-release", "--locked"], cwd=copy, env=env, check=True)
        subprocess.run(["node", "scripts/wasm-panic.mjs"], cwd=copy, env=env, check=True)


if __name__ == "__main__":
    main()
