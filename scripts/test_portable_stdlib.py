#!/usr/bin/env python3
"""Emit every portable fixture for WebAssembly and Cortex-M0 at O0 and O3.

This checks object generation, not execution on a board or a WebAssembly host.
Firmware supplies startup, memory helpers, and soft-float compiler support.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    parser.add_argument("--fixture", type=Path, action="append")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    fixtures = args.fixture or sorted((ROOT / "tests/stdlib").glob("*.dodo"))
    if not fixtures:
        raise SystemExit("No portable fixtures found")
    records = []
    with tempfile.TemporaryDirectory(prefix="dodo-portable-stdlib-") as directory:
        for source in fixtures:
            for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"]:
                for optimization in (0, 3):
                    output = Path(directory) / f"{source.stem}-{target}-O{optimization}.o"
                    command = [str(args.compiler.resolve()), "build", str(source.resolve()),
                               "--emit", "obj", "--target", target, "-O", str(optimization), "-o", str(output)]
                    result = subprocess.run(command, capture_output=True, text=True, timeout=120)
                    if result.returncode or not output.is_file() or output.stat().st_size == 0:
                        raise RuntimeError(f"Failed: {' '.join(command)}\n{result.stdout}\n{result.stderr}")
                    data = output.read_bytes()
                    magic = b"\x00asm" if target.startswith("wasm") else b"\x7fELF"
                    if not data.startswith(magic):
                        raise RuntimeError(f"Wrong object format: {output}")
                    records.append({"fixture": source.name, "target": target,
                                    "optimization": optimization, "bytes": len(data)})
                    print(f"PASS {target}: {source.name} -O{optimization}", flush=True)
    if args.report:
        args.report.write_text(json.dumps({"objects": len(records), "fixtures": records}, indent=2) + "\n")
    print(f"Passed {len(records)} object compilations across {len(fixtures)} portable fixtures.")


if __name__ == "__main__":
    main()
