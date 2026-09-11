#!/usr/bin/env python3
"""Package the verified Ubuntu compiler, examples, and dependency notices."""

from pathlib import Path
import hashlib
import json
import shutil
import subprocess
import tarfile
import tempfile


ROOT = Path(__file__).resolve().parent.parent


def output(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def main() -> None:
    metadata = json.loads(output("cargo", "metadata", "--locked", "--format-version", "1"))
    package = next(p for p in metadata["packages"] if Path(p["manifest_path"]) == ROOT / "Cargo.toml")
    version = package["version"]
    binary = Path(metadata["target_directory"]) / "x86_64-unknown-linux-gnu/release/dodo"
    subprocess.run(["python3", "scripts/check-linkage.py", str(binary), "--release"], cwd=ROOT, check=True)
    if output(str(binary), "--version") != f"dodo {version} (LLVM 22, BSD-2-Clause)":
        raise ValueError("Release binary version does not match Cargo.toml")

    destination = ROOT / "build/release-assets"
    destination.mkdir(parents=True, exist_ok=True)
    name = f"dodo-{version}-x86_64-unknown-linux-gnu"
    with tempfile.TemporaryDirectory(prefix="dodo-package-") as temporary:
        bundle = Path(temporary) / name
        bundle.mkdir()
        shutil.copy2(binary, bundle / "dodo")
        for filename in ("LICENSE", "README.md", "CHANGELOG.md"):
            shutil.copy2(ROOT / filename, bundle / filename)
        shutil.copytree(ROOT / "examples", bundle / "examples")
        shutil.copytree(ROOT / "docs/src/content/docs", bundle / "docs/src/content/docs")
        notices = bundle / "licenses"
        notices.mkdir()

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

        # These files describe the native libraries installed by the Ubuntu CI job.
        system_packages = ("llvm-22-dev", "zlib1g-dev", "libzstd-dev", "libffi-dev", "libz3-dev")
        for dependency in system_packages:
            copyright_file = Path("/usr/share/doc") / dependency / "copyright"
            shutil.copy2(copyright_file, notices / f"{dependency}-copyright")
        gcc_notices = sorted(Path("/usr/share/doc").glob("gcc-*-base/copyright"))
        if not gcc_notices:
            raise ValueError("Missing GCC runtime copyright notices")
        for source in gcc_notices:
            shutil.copy2(source, notices / f"{source.parent.name}-copyright")
        shutil.copytree("/usr/share/common-licenses", notices / "common-licenses")

        (bundle / "INSTALL.txt").write_text(
            f"Dodo {version} for x86-64 Linux (Ubuntu 24.04 / glibc 2.39 or newer)\n\n"
            "Copy dodo to a directory on PATH, then run dodo --version.\n"
            "LLVM is embedded; no LLVM installation is needed.\n"
            "To build or run native executables, install a C toolchain providing cc.\n\n"
            "Documentation: https://jotrorox.github.io/dodo/\n"
            f"Source: https://github.com/Jotrorox/dodo/tree/v{version}\n\n"
            "Dodo is licensed under BSD-2-Clause; see LICENSE.\n"
            "Dependency copyright and license notices are included in licenses/.\n"
        )
        archive = destination / f"{name}.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(bundle, arcname=name)

    with archive.open("rb") as source:
        checksum = hashlib.file_digest(source, "sha256").hexdigest()
    (destination / "SHA256SUMS").write_text(f"{checksum}  {archive.name}\n")
    print(f"Packaged {archive} ({archive.stat().st_size:,} bytes)")


if __name__ == "__main__":
    main()
