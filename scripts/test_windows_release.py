#!/usr/bin/env python3
"""Extract a Windows release ZIP and test its compiler natively or with Wine."""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import zipfile


WINDOWS_LIBRARIES = {
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "dbghelp.dll",
    "kernel32.dll", "msvcrt.dll", "ntdll.dll", "ole32.dll", "oleaut32.dll",
    "psapi.dll", "rpcrt4.dll", "secur32.dll", "shell32.dll", "ucrtbase.dll",
    "user32.dll", "userenv.dll", "version.dll", "ws2_32.dll",
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--runner", help="Wine executable when testing on Linux")
    parser.add_argument("--linker", default="clang", help="Windows C compiler driver")
    parser.add_argument("--link-arg", action="append", default=[], help="Extra linker driver argument")
    parser.add_argument("--llvm-readobj", default="llvm-readobj", help="Host LLVM binary inspection tool")
    args = parser.parse_args()
    match = re.fullmatch(r"dodo-(.+)-x86_64-pc-windows-msvc.zip", args.archive.name)
    if not match:
        parser.error("expected a dodo-VERSION-x86_64-pc-windows-msvc.zip archive")
    version = match[1]
    runner = [args.runner] if args.runner else []

    # Spaces and a working directory outside the checkout exercise relocation.
    with tempfile.TemporaryDirectory(prefix="dodo Windows release ") as directory:
        scratch = Path(directory)
        with zipfile.ZipFile(args.archive) as archive:
            if archive.testzip() is not None:
                raise RuntimeError("Corrupt release archive")
            archive.extractall(scratch)
        bundle = scratch / args.archive.stem
        compiler = bundle / "dodo.exe"
        for name in ("dodo.exe", "INSTALL.txt", "LICENSE", "licenses/LLVM-LICENSE.txt", "licenses/libxml2-Copyright",
                     "licenses/Unicode-LICENSE.txt", "licenses/rust/COPYRIGHT-library.html"):
            if not (bundle / name).is_file():
                raise RuntimeError(f"Release is missing {name}")

        headers = subprocess.check_output(
            [args.llvm_readobj, "--file-headers", "--coff-imports", str(compiler)], text=True,
        )
        if "Machine: IMAGE_FILE_MACHINE_AMD64" not in headers:
            raise RuntimeError("Release compiler is not a Windows x86-64 executable")
        imports = re.findall(r"^\s*Name: (\S+\.dll)\s*$", headers, re.MULTILINE | re.IGNORECASE)
        forbidden = [name for name in imports if name.lower() not in WINDOWS_LIBRARIES
                     and not name.lower().startswith("api-ms-win-")]
        if forbidden or not imports:
            raise RuntimeError(f"Unexpected Windows release DLL dependencies: {forbidden or headers}")
        print(f"PASS Windows system DLLs only: {', '.join(imports)}", flush=True)

        def run(*command: str, tools: bool = False) -> str:
            env = dict(os.environ)
            env.pop("LLVM_SYS_221_PREFIX", None)
            if os.name == "nt" and not tools:
                # Windows os.environ keys are uppercase after copying to a dict.
                env["PATH"] = str(Path(env["SYSTEMROOT"]) / "System32")
            result = subprocess.run([*runner, *map(str, command)], cwd=scratch, env=env,
                                    capture_output=True, text=True, timeout=120)
            if result.returncode:
                raise RuntimeError(f"Failed: {command}\n{result.stdout}\n{result.stderr}")
            return result.stdout.strip()

        assert run(compiler, "--version") == f"dodo {version} (LLVM 22, BSD-2-Clause)"
        # Test sources come from the checkout and stay outside the extracted bundle.
        examples = Path(__file__).resolve().parent.parent / "examples"
        for name in ("hello.dodo", "io.dodo", "bytes.dodo"):
            shutil.copy2(examples / name, scratch / name)
        hello = "hello.dodo"
        portable = "io.dodo"
        run(compiler, "check", hello)
        run(compiler, "check", "bytes.dodo")
        print("PASS extracted compiler: version, source checks, embedded standard library", flush=True)

        link = ["--linker", args.linker]
        for argument in args.link_arg:
            link.extend(["--link-arg", argument])
        for optimization in ("0", "3"):
            for emit, filename in (("llvm-ir", "hello.ll"), ("bitcode", "hello.bc"),
                                   ("asm", "hello.s"), ("obj", "hello.obj")):
                run(compiler, "build", hello, "--emit", emit, "-O", optimization, "-o", filename)
                assert (scratch / filename).stat().st_size > 0, filename
            assert (scratch / "hello.obj").read_bytes().startswith(b"\x64\x86"), "expected x64 COFF"
            run(compiler, "build", portable, "--emit", "obj", "--target", "wasm32-unknown-unknown",
                "-O", optimization, "-o", "hello.wasm")
            assert (scratch / "hello.wasm").read_bytes().startswith(b"\x00asm")
            run(compiler, "build", hello, "-O", optimization, "-o", "hello.exe", *link, tools=True)
            assert run(scratch / "hello.exe") == "Hello, world!"
            assert run(compiler, "run", hello, "-O", optimization, *link, tools=True) == "Hello, world!"
            print(f"PASS -O{optimization}: IR, bitcode, assembly, COFF, WebAssembly, build and run", flush=True)

    print(f"Verified Windows release: {args.archive}")


if __name__ == "__main__":
    main()
