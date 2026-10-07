#!/usr/bin/env python3
"""Install the official LLVM archives and tools used by Dodo's Linux releases."""

import argparse
import hashlib
import os
from pathlib import Path
import platform
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parent.parent
VERSION = "23.1.1"
ARCHIVE_NAME = f"LLVM-{VERSION}-Linux-X64"
ARCHIVE_SHA256 = "832aeb58d105de1cabc7b982dd2c65de0610f7377df48ae8fc2dd8e97420a15c"
LICENSE_SHA256 = "8d85c1057d742e597985c7d4e6320b015a9139385cff4cbae06ffc0ebe89afee"


def download(url: str, destination: Path) -> None:
    subprocess.run([
        "curl", "--fail", "--location", "--retry", "5", "--retry-all-errors",
        "--connect-timeout", "30", "--max-time", "1200", "--retry-max-time", "1800",
        "--remove-on-error", "--output", str(destination), url,
    ], check=True)


def verify(path: Path, checksum: str) -> None:
    with path.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != checksum:
        raise ValueError(f"Checksum mismatch: {path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--llvm-prefix", type=Path,
                        default=os.environ.get("LLVM_SYS_231_PREFIX", ROOT / "target/llvm-linux"))
    parser.add_argument("--archive", type=Path, help="Use an already downloaded archive (checksum verified)")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("requires an x86-64 Linux host")
    prefix = args.llvm_prefix.resolve()

    with tempfile.TemporaryDirectory(prefix="dodo-llvm-") as temporary:
        archive = args.archive or Path(temporary) / f"{ARCHIVE_NAME}.tar.xz"
        if not args.archive:
            download(f"https://github.com/llvm/llvm-project/releases/download/llvmorg-{VERSION}/"
                     f"{ARCHIVE_NAME}.tar.xz", archive)
        verify(archive, ARCHIVE_SHA256)
        prefix.mkdir(parents=True, exist_ok=True)
        # Keep the static LLVM libraries, C API headers, debugger and Windows
        # test tools; omit unrelated executables and Clang/MLIR static libraries.
        members = [
            "include/llvm", "include/llvm-c", "lib/libLLVM*.a", "lib/libPolly*.a",
            "lib/clang/23/include", "bin/llvm-config", "bin/llvm-dwarfdump",
            "bin/clang", "bin/clang-23", "bin/lld", "bin/ld.lld", "bin/lld-link",
        ]
        subprocess.run([
            "tar", "-xJf", str(archive), "-C", str(prefix), "--strip-components=1", "--no-same-owner",
            "--wildcards", *[f"{ARCHIVE_NAME}/{member}" for member in members],
        ], check=True)
        download(f"https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-{VERSION}/llvm/LICENSE.TXT",
                 prefix / "LICENSE.txt")
        verify(prefix / "LICENSE.txt", LICENSE_SHA256)

    # Existing debugger fixtures use the versioned Ubuntu tool name.
    dwarf = prefix / "bin/llvm-dwarfdump-23"
    if not dwarf.exists():
        dwarf.symlink_to("llvm-dwarfdump")
    config = str(prefix / "bin/llvm-config")
    version = subprocess.check_output([config, "--version"], text=True).strip()
    if version != VERSION:
        raise ValueError(f"Expected LLVM {VERSION}, got {version}")
    libraries = subprocess.check_output([config, "--link-static", "--system-libs"], text=True).strip()
    if "z3" in libraries.lower():
        raise ValueError(f"The Linux release toolchain must not require Z3: {libraries}")
    print(f"LLVM {version}: {prefix}\nSystem libraries: {libraries}")


if __name__ == "__main__":
    main()
