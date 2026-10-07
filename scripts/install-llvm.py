#!/usr/bin/env python3
"""Install the official LLVM archives and tools used by Dodo's Linux releases."""

import argparse
import fnmatch
import hashlib
import os
from pathlib import Path
import platform
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parent.parent
VERSION = "23.1.1"
# Official release archives by host system and machine. Digests match the ones
# GitHub publishes for the llvmorg-23.1.1 release assets. The official macOS
# archive is not listed: its static libraries are ThinLTO bitcode, which Rust's
# own LLVM cannot link. macOS builds use Homebrew's llvm@23 instead.
ARCHIVES = {
    ("Linux", "x86_64"): (f"LLVM-{VERSION}-Linux-X64",
                          "832aeb58d105de1cabc7b982dd2c65de0610f7377df48ae8fc2dd8e97420a15c"),
    ("Linux", "aarch64"): (f"LLVM-{VERSION}-Linux-ARM64",
                           "3fbaaa6a1f147557a4095b9911f8dc2d4c745a11863982f76ee20e248c190a80"),
}
LICENSE_SHA256 = "8d85c1057d742e597985c7d4e6320b015a9139385cff4cbae06ffc0ebe89afee"


def download(url: str, destination: Path) -> None:
    subprocess.run([
        "curl", "--fail", "--location", "--retry", "5", "--retry-all-errors",
        "--connect-timeout", "30", "--max-time", "1200", "--retry-max-time", "1800",
        "--remove-on-error", "--output", str(destination), url,
    ], check=True)


def verify(path: Path, checksum: str) -> None:
    # Chunked rather than hashlib.file_digest: macOS ships Python 3.9.
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1 << 20), b""):
            digest.update(chunk)
    actual = digest.hexdigest()
    if actual != checksum:
        raise ValueError(f"Checksum mismatch: {path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--llvm-prefix", type=Path,
                        default=os.environ.get("LLVM_SYS_231_PREFIX", ROOT / "target/llvm"))
    parser.add_argument("--archive", type=Path, help="Use an already downloaded archive (checksum verified)")
    args = parser.parse_args()
    host = (platform.system(), platform.machine())
    if host not in ARCHIVES:
        parser.error("requires an x86-64 or AArch64 Linux host; on macOS, use brew install llvm@23")
    archive_name, archive_sha256 = ARCHIVES[host]
    prefix = args.llvm_prefix.resolve()

    with tempfile.TemporaryDirectory(prefix="dodo-llvm-") as temporary:
        archive = args.archive or Path(temporary) / f"{archive_name}.tar.xz"
        if not args.archive:
            download(f"https://github.com/llvm/llvm-project/releases/download/llvmorg-{VERSION}/"
                     f"{archive_name}.tar.xz", archive)
        verify(archive, archive_sha256)
        prefix.mkdir(parents=True, exist_ok=True)
        # Keep the static LLVM libraries, C API headers, debugger and Windows
        # test tools; omit unrelated executables and Clang/MLIR static libraries.
        members = [
            "include/llvm", "include/llvm-c", "lib/libLLVM*.a", "lib/libPolly*.a",
            "lib/clang/23/include", "bin/llvm-config", "bin/llvm-dwarfdump",
            "bin/clang", "bin/clang-23", "bin/lld", "bin/ld.lld", "bin/lld-link",
        ]
        # List first: GNU and BSD tar disagree on wildcard flags, and archives
        # for different hosts ship slightly different tool sets.
        listing = subprocess.run(["tar", "-tJf", str(archive)], check=True,
                                 capture_output=True, text=True).stdout.splitlines()
        patterns = [f"{archive_name}/{member}" for member in members]
        selected = [name for name in listing if not name.endswith("/") and any(
            fnmatch.fnmatchcase(name, pattern) or name.startswith(pattern + "/") for pattern in patterns)]
        for required in ("include/llvm-c", "lib/libLLVM*.a", "bin/llvm-config"):
            pattern = f"{archive_name}/{required}"
            if not any(fnmatch.fnmatchcase(name, pattern) or name.startswith(pattern + "/") for name in selected):
                raise ValueError(f"{archive_name} lacks {required}")
        members_file = Path(temporary) / "members.txt"
        members_file.write_text("\n".join(selected) + "\n")
        subprocess.run([
            "tar", "-xJf", str(archive), "-C", str(prefix), "--strip-components=1", "--no-same-owner",
            "-T", str(members_file),
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
        raise ValueError(f"The release toolchain must not require Z3: {libraries}")
    print(f"LLVM {version}: {prefix}\nSystem libraries: {libraries}")


if __name__ == "__main__":
    main()
