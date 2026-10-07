#!/usr/bin/env python3
"""Check the shared libraries required by an ELF or Mach-O compiler binary."""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys


LINUX_RELEASE_LIBRARIES = {"libc.so.6", "libm.so.6", "libgcc_s.so.1"}
# The standard glibc program interpreter for each supported ELF machine.
LINUX_INTERPRETERS = {
    "Advanced Micro Devices X86-64": "/lib64/ld-linux-x86-64.so.2",
    "AArch64": "/lib/ld-linux-aarch64.so.1",
}


# 64-bit Mach-O (either byte order) and universal ("fat") binaries.
MACHO_MAGICS = {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe"}
# Every macOS installation provides these; anything else (for example a
# Homebrew prefix) would make a release depend on the build machine.
MACOS_SYSTEM_PREFIXES = ("/usr/lib/", "/System/Library/")
MH_EXECUTE = 2
MH_DYLIB = 6


def check_macho(binary: Path, require_static: bool, require_release: bool) -> None:
    with binary.open("rb") as source:
        header = source.read(16)
    # Thin 64-bit images store the file type in their fourth header word;
    # universal binaries are checked per architecture by otool.
    if len(header) < 16:
        raise ValueError(f"truncated Mach-O binary: {binary}")
    if header[:4] != b"\xca\xfe\xba\xbe":
        order = "little" if header[:4] == b"\xcf\xfa\xed\xfe" else "big"
        filetype = int.from_bytes(header[12:16], order)
        if filetype not in (MH_EXECUTE, MH_DYLIB):
            raise ValueError(f"not a Mach-O executable or dynamic library: {binary}")
    if require_static:
        raise ValueError(f"macOS does not support fully static executables: {binary}")
    result = subprocess.run(
        # Apple otool rejects "--"; an absolute path cannot be read as a flag.
        ["otool", "-L", str(binary.resolve())],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
        env={**os.environ, "LC_ALL": "C"},
    )
    if result.returncode or result.stderr.strip():
        detail = result.stderr.strip() or f"exit status {result.returncode}"
        raise ValueError(f"otool could not inspect {binary}: {detail}")
    # The first line names the binary (and each architecture of a fat binary).
    dependencies = sorted({
        line.strip().split(" (compatibility version")[0]
        for line in result.stdout.splitlines()[1:]
        if line.startswith(("\t", " ")) and "(compatibility version" in line
    })
    if require_release:
        forbidden = [name for name in dependencies if not name.startswith(MACOS_SYSTEM_PREFIXES)]
    else:
        forbidden = [name for name in dependencies if "llvm" in name.lower()]
    if forbidden:
        raise ValueError(f"{binary} still requires shared libraries: " + ", ".join(forbidden))
    remaining = ", ".join(dependencies) or "none"
    if require_release:
        print(f"{binary}: only macOS system libraries: {remaining}")
    else:
        print(f"{binary}: no shared LLVM dependency; remaining shared libraries: {remaining}")


def check(binary: Path, require_static: bool, require_release: bool) -> None:
    if not binary.is_file():
        raise ValueError(f"not a regular file: {binary}")
    with binary.open("rb") as source:
        magic = source.read(4)
    if magic in MACHO_MAGICS:
        check_macho(binary, require_static, require_release)
        return
    if magic != b"\x7fELF":
        raise ValueError(f"not an ELF or Mach-O binary: {binary}")

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

    machine = re.search(r"^\s*Machine:\s+(.+?)\s*$", result.stdout, re.MULTILINE)
    standard_interpreter = LINUX_INTERPRETERS.get(machine.group(1)) if machine else None

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
        allowed = set(LINUX_RELEASE_LIBRARIES)
        if standard_interpreter:
            allowed.add(Path(standard_interpreter).name)
        forbidden = [name for name in dependencies if name not in allowed]
    else:
        forbidden = [name for name in dependencies if "llvm" in name.lower()]
    problems = []
    if forbidden:
        problems.append("shared libraries: " + ", ".join(forbidden))
    if require_static and has_interpreter:
        problems.append("an ELF program interpreter (PT_INTERP)")
    elif require_release and standard_interpreter is None:
        problems.append("an unsupported ELF machine: " + (machine.group(1) if machine else "unknown"))
    elif require_release and interpreter and interpreter.group(1) != standard_interpreter:
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
        help="allow only the standard Linux C, math, and unwind runtimes, or macOS system libraries",
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
