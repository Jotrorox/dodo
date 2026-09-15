#!/usr/bin/env python3
"""Prepare static LLVM dependencies, relocatable llvm-config output, and notices."""

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
    parser.add_argument("--llvm-prefix", type=Path, default=os.environ.get("LLVM_SYS_231_PREFIX"))
    parser.add_argument("--cmake-toolchain", type=Path, help="MSVC cross toolchain when building on Linux")
    args = parser.parse_args()
    if not args.llvm_prefix or not (args.llvm_prefix / "bin/llvm-config.exe").is_file():
        parser.error("set --llvm-prefix or LLVM_SYS_231_PREFIX to the extracted Windows LLVM archive")
    if os.name != "nt" and not args.cmake_toolchain:
        parser.error("cross compilation requires --cmake-toolchain")
    prefix = args.llvm_prefix.resolve()
    cache = ROOT / "target/windows-llvm-support"
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / "libxml2.tar.gz"
    # Match LLVM's llvmorg-23.1.1/llvm/utils/release/build_llvm_release.bat.
    download("https://gitlab.gnome.org/GNOME/libxml2/-/archive/v2.9.12/libxml2-v2.9.12.tar.gz",
             archive, "98bfa7a9a5e2a75638422050740448ee9f02bf4dc2075c9822d7747d5ff9e617")
    source = cache / "libxml2-v2.9.12"
    if not source.is_dir():
        with tarfile.open(archive) as tar:
            tar.extractall(cache, filter="data")
    build = cache / "build"
    generator = (["-G", "Ninja", f"-DCMAKE_TOOLCHAIN_FILE={args.cmake_toolchain.resolve()}"]
                 if args.cmake_toolchain else ["-G", "Visual Studio 17 2022", "-A", "x64"])

    def build_static(source: Path, build: Path, options: list[str]) -> None:
        subprocess.run([
            "cmake", "-S", str(source), "-B", str(build), *generator,
            "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded",
            "-DBUILD_SHARED_LIBS=OFF", f"-DCMAKE_INSTALL_PREFIX={prefix}", *options,
        ], check=True)
        subprocess.run(["cmake", "--build", str(build), "--config", "Release", "--parallel", "2"], check=True)
        subprocess.run(["cmake", "--install", str(build), "--config", "Release"], check=True)

    disabled = (
        "C14N CATALOG DEBUG DOCB FTP HTML HTTP ICONV ICU ISO8859X LEGACY LZMA MEM_DEBUG "
        "MODULES PATTERN PROGRAMS PUSH PYTHON READER REGEXPS RUN_DEBUG SCHEMAS SCHEMATRON "
        "TESTS THREAD_ALLOC VALID WRITER XINCLUDE XPATH XPTR ZLIB"
    ).split()
    build_static(source, build, [
        *[f"-DLIBXML2_WITH_{option}=OFF" for option in disabled],
        *[f"-DLIBXML2_WITH_{option}=ON" for option in ("OUTPUT", "SAX1", "THREADS", "TREE")],
    ])
    # llvm-config reports xml2s.lib even though libxml2 installs libxml2s.lib.
    shutil.copy2(prefix / "lib/libxml2s.lib", prefix / "lib/xml2s.lib")
    shutil.copy2(source / "Copyright", prefix / "libxml2-Copyright")

    # LLVM 23 enables compression in its Windows release. These archives are
    # omitted too; match the versions/checksums in build_llvm_release.bat.
    for name, url, checksum, subdirectory, options, notices in (
        ("zlib-1.3.2", "https://github.com/madler/zlib/releases/download/v1.3.2/zlib-1.3.2.tar.gz",
         "bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16", ".",
         ["-DZLIB_BUILD_TESTING=OFF", "-DZLIB_BUILD_SHARED=OFF", "-DZLIB_BUILD_STATIC=ON", "-DZLIB_INSTALL=ON"],
         [("LICENSE", "zlib-LICENSE")]),
        ("zstd-1.5.7", "https://github.com/facebook/zstd/releases/download/v1.5.7/zstd-1.5.7.tar.gz",
         "eb33e51f49a15e023950cd7825ca74a4a2b43db8354825ac24fc1b7ee09e6fa3", "build/cmake",
         ["-DZSTD_BUILD_PROGRAMS=OFF", "-DZSTD_BUILD_TESTS=OFF", "-DZSTD_BUILD_STATIC=ON",
          "-DZSTD_BUILD_SHARED=OFF", "-DZSTD_USE_STATIC_RUNTIME=ON"],
         [("LICENSE", "zstd-LICENSE"), ("COPYING", "zstd-COPYING")]),
    ):
        archive = cache / f"{name}.tar.gz"
        download(url, archive, checksum)
        source = cache / name
        if not source.is_dir():
            with tarfile.open(archive) as tar:
                # Zstandard's disabled CLI tests contain symlinks, which need
                # extra privileges to extract on Windows.
                members = (member for member in tar
                           if not member.name.startswith(f"{name}/tests/"))
                tar.extractall(cache, members=members, filter="data")
        build_static(source / subdirectory, cache / f"{name}-build", options)
        for original, installed in notices:
            shutil.copy2(source / original, prefix / installed)

    if os.name == "nt":
        # llvm-sys searches the versioned name first. Keep the official executable
        # intact and delegate to it, correcting only --system-libs output. LLVM 23
        # embeds the release builder's S: drive path to zstd_static.lib, which rustc
        # interprets as a library rename instead of a linkable library name.
        subprocess.run([
            "rustc", "--edition=2024", "--crate-name", "dodo_llvm_config",
            str(ROOT / "scripts/windows-llvm-config.rs"), "-O",
            "-C", "target-feature=+crt-static",
            "-o", str(prefix / "bin/llvm-config-23.exe"),
        ], check=True)
    download("https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-23.1.1/llvm/LICENSE.TXT",
             prefix / "LICENSE.txt", "8d85c1057d742e597985c7d4e6320b015a9139385cff4cbae06ffc0ebe89afee")
    print(f"Windows LLVM support libraries and notices: {prefix}")


if __name__ == "__main__":
    main()
