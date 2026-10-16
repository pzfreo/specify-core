#!/usr/bin/env python3
"""Measure the optimized package, and enforce the committed raw WASM ceiling."""

import argparse
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def measure(path):
    # Node is already required by parity CI. Its zlib avoids a Python Brotli dependency.
    result = subprocess.run([
        "node", "--input-type=module", "-e",
        "import {readFileSync} from 'node:fs';"
        "import {gzipSync,brotliCompressSync,constants} from 'node:zlib';"
        "const b=readFileSync(process.argv[1]);"
        "console.log(JSON.stringify({raw_bytes:b.length,"
        "gzip_bytes:gzipSync(b,{level:9}).length,"
        "brotli_bytes:brotliCompressSync(b,{params:{[constants.BROTLI_PARAM_QUALITY]:11}}).length}));",
        str(path),
    ], check=True, text=True, capture_output=True)
    return json.loads(result.stdout)


def check(sizes, budget):
    ceiling = budget["ceiling_bytes"]
    headroom = ceiling - sizes["raw_bytes"]
    print(f"WASM size: raw {sizes['raw_bytes']:,} bytes; gzip (level 9) {sizes['gzip_bytes']:,}; Brotli (quality 11) {sizes['brotli_bytes']:,}.")
    print(f"WASM ceiling: {ceiling:,} bytes; headroom {headroom:,} bytes ({headroom / ceiling:.1%} of ceiling).")
    if headroom < 0:
        print("ERROR: optimized WASM exceeds tools/wasm-size-budget.json. Reduce size or deliberately edit the reviewed ceiling.")
        return 1
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wasm", type=Path, default=ROOT / "crates/specify-core-wasm/pkg/specify_core_wasm_bg.wasm")
    parser.add_argument("--budget", type=Path, default=ROOT / "tools/wasm-size-budget.json")
    args = parser.parse_args()
    try:
        return check(measure(args.wasm), json.loads(args.budget.read_text()))
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"WASM size check failed: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
