#!/usr/bin/env python3
"""Package a Linux or Windows compiler, examples, and dependency notices."""

from pathlib import Path
import argparse
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile


ROOT = Path(__file__).resolve().parent.parent


def output(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=("x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"),
                        default="x86_64-unknown-linux-gnu")
    parser.add_argument("--llvm-prefix", type=Path, default=os.environ.get("LLVM_SYS_221_PREFIX"),
                        help="LLVM installation, including its license notices")
    parser.add_argument("--runner", help="Run the Windows compiler with Wine when packaging on Linux")
    args = parser.parse_args()
    windows = args.target == "x86_64-pc-windows-msvc"
    if not args.llvm_prefix:
        parser.error("packaging requires --llvm-prefix or LLVM_SYS_221_PREFIX")
    if args.runner and not windows:
        parser.error("--runner is only supported for Windows packaging")

    metadata = json.loads(output("cargo", "metadata", "--locked", "--format-version", "1"))
    package = next(p for p in metadata["packages"] if Path(p["manifest_path"]) == ROOT / "Cargo.toml")
    version = package["version"]
    executable = "dodo.exe" if windows else "dodo"
    binary = Path(metadata["target_directory"]) / args.target / "release" / executable
    if not windows:
        subprocess.run([sys.executable, "scripts/check-linkage.py", str(binary), "--release"], cwd=ROOT, check=True)
    runner = [args.runner] if args.runner else []
    if output(*runner, str(binary), "--version") != f"dodo {version} (LLVM 22, BSD-2-Clause)":
        raise ValueError("Release binary version does not match Cargo.toml")

    destination = ROOT / "build/release-assets"
    destination.mkdir(parents=True, exist_ok=True)
    # Remove the manifest left by older versions of this script as well.
    (destination / "SHA256SUMS").unlink(missing_ok=True)
    name = f"dodo-{version}-{args.target}"
    with tempfile.TemporaryDirectory(prefix="dodo-package-") as temporary:
        bundle = Path(temporary) / name
        bundle.mkdir()
        shutil.copy2(binary, bundle / executable)
        for filename in ("LICENSE", "README.md", "CHANGELOG.md"):
            shutil.copy2(ROOT / filename, bundle / filename)
        shutil.copytree(ROOT / "examples", bundle / "examples")
        shutil.copytree(ROOT / "docs/src/content/docs", bundle / "docs/src/content/docs")
        notices = bundle / "licenses"
        notices.mkdir()
        shutil.copy2(ROOT / "stdlib/std/LICENSE.unicode", notices / "Unicode-LICENSE.txt")

        # Include build dependencies too, so the bundle records the locked toolchain.
        for dependency in metadata["packages"]:
            if dependency["id"] == package["id"]:
                continue
            directory = Path(dependency["manifest_path"]).parent
            files = [p for p in directory.rglob("*") if p.is_file()
                     and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"))]
            if dependency.get("license_file"):
                files.append(directory / dependency["license_file"])
            target = notices / f'{dependency["name"]}-{dependency["version"]}'
            target.mkdir()
            (target / "metadata.json").write_text(json.dumps({key: dependency.get(key) for key in
                ("name", "version", "license", "repository", "source")}, indent=2) + "\n")
            for source in sorted(set(files)):
                relative = source.relative_to(directory)
                copied = target / relative
                copied.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, copied)

        rust_docs = Path(output("rustc", "--print", "sysroot")) / "share/doc/rust"
        shutil.copytree(rust_docs / "licenses", notices / "rust/licenses")
        shutil.copy2(rust_docs / "COPYRIGHT-library.html", notices / "rust/COPYRIGHT-library.html")

        shutil.copy2(args.llvm_prefix / "LICENSE.txt", notices / "LLVM-LICENSE.txt")
        if windows:
            shutil.copy2(args.llvm_prefix / "libxml2-Copyright", notices / "libxml2-Copyright")
        else:
            # These files describe the native libraries installed by the Ubuntu CI job.
            system_packages = ("zlib1g-dev", "libzstd-dev", "libxml2-dev", "libffi-dev")
            for dependency in system_packages:
                copyright_file = Path("/usr/share/doc") / dependency / "copyright"
                shutil.copy2(copyright_file, notices / f"{dependency}-copyright")
            gcc_notices = sorted(Path("/usr/share/doc").glob("gcc-*-base/copyright"))
            if not gcc_notices:
                raise ValueError("Missing GCC runtime copyright notices")
            for source in gcc_notices:
                shutil.copy2(source, notices / f"{source.parent.name}-copyright")
            shutil.copytree("/usr/share/common-licenses", notices / "common-licenses")

        platform = "x86-64 Windows" if windows else "x86-64 Linux (Ubuntu 24.04 / glibc 2.39 or newer)"
        toolchain = (
            "To build or run native executables, install Clang and the Visual Studio C++ Build Tools\n"
            "with a Windows SDK, then use --linker clang or set DODO_CC=clang.\n"
            if windows else "To build or run native executables, install a C toolchain providing cc.\n"
        )
        (bundle / "INSTALL.txt").write_text(
            f"Dodo {version} for {platform}\n\n"
            f"Copy {executable} to a directory on PATH, then run dodo --version.\n"
            "LLVM is embedded; no LLVM installation is needed.\n"
            f"{toolchain}\n"
            "Documentation: https://jotrorox.github.io/dodo/\n"
            f"Source: https://github.com/Jotrorox/dodo/tree/v{version}\n\n"
            "Dodo is licensed under BSD-2-Clause; see LICENSE.\n"
            "Dependency copyright and license notices are included in licenses/.\n"
        )
        if windows:
            archive = destination / f"{name}.zip"
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as zip_archive:
                for source in sorted(bundle.rglob("*")):
                    if source.is_file():
                        zip_archive.write(source, source.relative_to(bundle.parent))
        else:
            archive = destination / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                tar.add(bundle, arcname=name)

    print(f"Packaged {archive} ({archive.stat().st_size:,} bytes)")


if __name__ == "__main__":
    main()
