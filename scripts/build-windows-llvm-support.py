#!/usr/bin/env python3
"""Add the static XML library and notices omitted by the LLVM Windows archive."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tarfile


ROOT = Path(__file__).resolve().parent.parent


def download(url: str, destination: Path, checksum: str) -> None:
    if not destination.is_file():
        print(f"Downloading {url}", flush=True)
        subprocess.run([
            "curl.exe" if os.name == "nt" else "curl",
            "--fail", "--location", "--retry", "5", "--retry-all-errors",
            "--connect-timeout", "30", "--max-time", "300", "--retry-max-time", "600",
            "--remove-on-error", "--output", str(destination), url,
        ], check=True)
    with destination.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != checksum:
        destination.unlink()
        raise ValueError(f"Download checksum mismatch: {url}; run again to retry")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--llvm-prefix", type=Path, default=os.environ.get("LLVM_SYS_221_PREFIX"))
    parser.add_argument("--cmake-toolchain", type=Path, help="MSVC cross toolchain when building on Linux")
    args = parser.parse_args()
    if not args.llvm_prefix or not (args.llvm_prefix / "bin/llvm-config.exe").is_file():
        parser.error("set --llvm-prefix or LLVM_SYS_221_PREFIX to the extracted Windows LLVM archive")
    if os.name != "nt" and not args.cmake_toolchain:
        parser.error("cross compilation requires --cmake-toolchain")
    prefix = args.llvm_prefix.resolve()
    cache = ROOT / "target/windows-llvm-support"
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / "libxml2.tar.gz"
    # Match LLVM's llvmorg-22.1.8/llvm/utils/release/build_llvm_release.bat.
    download("https://gitlab.gnome.org/GNOME/libxml2/-/archive/v2.9.12/libxml2-v2.9.12.tar.gz",
             archive, "98bfa7a9a5e2a75638422050740448ee9f02bf4dc2075c9822d7747d5ff9e617")
    source = cache / "libxml2-v2.9.12"
    if not source.is_dir():
        with tarfile.open(archive) as tar:
            tar.extractall(cache, filter="data")
    build = cache / "build"
    generator = (["-G", "Ninja", f"-DCMAKE_TOOLCHAIN_FILE={args.cmake_toolchain.resolve()}"]
                 if args.cmake_toolchain else ["-G", "Visual Studio 17 2022", "-A", "x64"])
    disabled = (
        "C14N CATALOG DEBUG DOCB FTP HTML HTTP ICONV ICU ISO8859X LEGACY LZMA MEM_DEBUG "
        "MODULES PATTERN PROGRAMS PUSH PYTHON READER REGEXPS RUN_DEBUG SCHEMAS SCHEMATRON "
        "TESTS THREAD_ALLOC VALID WRITER XINCLUDE XPATH XPTR ZLIB"
    ).split()
    subprocess.run([
        "cmake", "-S", str(source), "-B", str(build), *generator,
        "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded",
        "-DBUILD_SHARED_LIBS=OFF", f"-DCMAKE_INSTALL_PREFIX={prefix}",
        *[f"-DLIBXML2_WITH_{option}=OFF" for option in disabled],
        *[f"-DLIBXML2_WITH_{option}=ON" for option in ("OUTPUT", "SAX1", "THREADS", "TREE")],
    ], check=True)
    subprocess.run(["cmake", "--build", str(build), "--config", "Release", "--parallel", "2"], check=True)
    subprocess.run(["cmake", "--install", str(build), "--config", "Release"], check=True)
    # llvm-config reports xml2s.lib even though libxml2 installs libxml2s.lib.
    shutil.copy2(prefix / "lib/libxml2s.lib", prefix / "lib/xml2s.lib")
    shutil.copy2(source / "Copyright", prefix / "libxml2-Copyright")
    download("https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.8/llvm/LICENSE.TXT",
             prefix / "LICENSE.txt", "8d85c1057d742e597985c7d4e6320b015a9139385cff4cbae06ffc0ebe89afee")
    print(f"Windows LLVM support libraries and notices: {prefix}")


if __name__ == "__main__":
    main()
