#!/usr/bin/env python3
"""Check the shared libraries required by an ELF compiler binary."""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys


LINUX_RELEASE_LIBRARIES = {"libc.so.6", "libm.so.6", "libgcc_s.so.1", "ld-linux-x86-64.so.2"}


def check(binary: Path, require_static: bool, require_release: bool) -> None:
    if not binary.is_file():
        raise ValueError(f"not a regular file: {binary}")
    with binary.open("rb") as source:
        if source.read(4) != b"\x7fELF":
            raise ValueError(f"not an ELF binary: {binary}")

    result = subprocess.run(
        [
            "readelf", "--file-header", "--program-headers", "--dynamic", "--wide",
            "--", str(binary),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        env={**os.environ, "LC_ALL": "C"},
    )
    # readelf can report malformed ELF structures on stderr while exiting zero.
    if result.returncode or result.stderr.strip():
        detail = result.stderr.strip() or f"exit status {result.returncode}"
        raise ValueError(f"readelf could not inspect {binary}: {detail}")
    if not re.search(r"^\s*Type:\s+(?:EXEC|DYN)\b", result.stdout, re.MULTILINE):
        raise ValueError(f"not an ELF executable or shared object: {binary}")
    if "Program Headers:" not in result.stdout:
        raise ValueError(f"ELF binary has no program headers: {binary}")

    needed_lines = [line for line in result.stdout.splitlines() if "(NEEDED)" in line]
    dependencies = []
    for line in needed_lines:
        match = re.search(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", line)
        if match is None:
            raise ValueError(f"unrecognized ELF dependency: {line.strip()}")
        dependencies.append(match.group(1))

    has_interpreter = re.search(r"^\s*INTERP\s", result.stdout, re.MULTILINE) is not None
    interpreter = re.search(r"\[Requesting program interpreter: ([^\]]+)\]", result.stdout)
    if has_interpreter and interpreter is None:
        raise ValueError(f"could not read ELF program interpreter: {binary}")

    if require_static:
        forbidden = dependencies
    elif require_release:
        forbidden = [name for name in dependencies if name not in LINUX_RELEASE_LIBRARIES]
    else:
        forbidden = [name for name in dependencies if "llvm" in name.lower()]
    problems = []
    if forbidden:
        problems.append("shared libraries: " + ", ".join(forbidden))
    if require_static and has_interpreter:
        problems.append("an ELF program interpreter (PT_INTERP)")
    elif require_release and interpreter and interpreter.group(1) != "/lib64/ld-linux-x86-64.so.2":
        problems.append("a nonstandard ELF program interpreter: " + interpreter.group(1))
    if problems:
        raise ValueError(f"{binary} still requires " + " and ".join(problems))

    if require_static:
        print(f"{binary}: no shared libraries or ELF program interpreter")
    elif require_release:
        remaining = ", ".join(dependencies) or "none"
        print(f"{binary}: only standard Linux runtime libraries: {remaining}")
    else:
        remaining = ", ".join(dependencies) or "none"
        print(f"{binary}: no shared LLVM dependency; remaining shared libraries: {remaining}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--release", action="store_true",
        help="allow only the standard x86-64 Linux C, math, and unwind runtimes",
    )
    mode.add_argument(
        "--static", action="store_true",
        help="reject every shared library and ELF program interpreter",
    )
    args = parser.parse_args()
    try:
        check(args.binary, args.static, args.release)
    except (OSError, ValueError) as error:
        print(f"linkage check failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
