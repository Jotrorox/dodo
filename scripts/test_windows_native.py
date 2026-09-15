#!/usr/bin/env python3
"""Build and execute hosted platform fixtures on native Windows x64.

Requires a Windows Dodo compiler, Clang and the Visual Studio C++/Windows SDK
libraries. Unlike test_stdlib_windows.py, this uses the normal executable build
path, including the embedded adapters and the Windows CRT startup.
"""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = (
    "os/fs_checks.dodo",
    "os/fs_modes.dodo",
    "os/fs_path_checks.dodo",
    "os/fs_windows_checks.dodo",
    "os/hosted_fs.dodo",
    "os/process_windows_checks.dodo",
    "os/hosted_process.dodo",
    "os/console_windows_checks.dodo",
    "os/thread_checks.dodo",
    "os/sync_checks.dodo",
    "net/native_checks.dodo",
    "net/backpressure.dodo",
    "os/process_handles_windows_checks.dodo",
)


def run(command, *, cwd, env, timeout=120):
    # Files also keep a leaked child pipe from blocking output collection.
    with tempfile.TemporaryFile() as log:
        with subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                              stdout=log, stderr=log) as process:
            try:
                code = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired as error:
                # Kill descendants before the parent so captured children cannot
                # outlive a hung fixture or keep its temporary files open.
                try:
                    subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                   timeout=20, check=False)
                finally:
                    process.kill()
                    process.wait(timeout=20)
                log.seek(0)
                raise RuntimeError(f"Timed out after {timeout}s: {command}\n"
                                   f"{log.read().decode('utf-8', errors='replace')}") from error
        log.seek(0)
        output = log.read().decode("utf-8", errors="replace")
    if code:
        raise RuntimeError(f"Failed ({code}): {command}\n{output}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path,
                        default=ROOT / "target/x86_64-pc-windows-msvc/release/dodo.exe")
    parser.add_argument("--linker", default="clang", help="Windows C compiler driver")
    parser.add_argument("--fixture", type=Path, action="append",
                        help="Run only this fixture (repeatable)")
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("this suite requires native Windows; use test_stdlib_windows.py for Wine")
    compiler = args.compiler.resolve()
    if not compiler.is_file():
        parser.error(f"build the Windows compiler first: {compiler}")
    linker = shutil.which(args.linker)
    if linker is None:
        parser.error(f"Windows C compiler driver not found: {args.linker}")
    fixtures = ([path.resolve() for path in args.fixture] if args.fixture else
                [ROOT / "tests" / name for name in FIXTURES])
    for fixture in fixtures:
        if not fixture.is_file():
            parser.error(f"fixture not found: {fixture}")
    env = dict(os.environ, DODO_PARENT_ONLY="must not leak")
    executions = 0
    with tempfile.TemporaryDirectory(prefix="dodo native Windows ") as directory:
        scratch = Path(directory)
        child = scratch / "os child.exe"
        run([linker, "-std=c11", "-O2", str(ROOT / "tests/support/os_child.c"),
             "-o", str(child)], cwd=scratch, env=env)
        for index, source in enumerate(fixtures):
            for optimization in (0, 3):
                work = scratch / f"{index}-{source.stem}-O{optimization}"
                work.mkdir()
                shutil.copy2(child, work / child.name)
                child_work = work / "child cwd é"
                child_work.mkdir()
                shutil.copy2(child, child_work / child.name)
                (child_work / "cwd-marker").write_bytes(b"fixture")
                exe = work / f"{source.stem}.exe"
                print(f"RUN native Windows x64: {source.name} -O{optimization}", flush=True)
                run([str(compiler), "build", str(source), "--target", "x86_64-pc-windows-msvc",
                     "-O", str(optimization), "-o", str(exe), "--linker", linker,
                     "--link-arg=-Wl,/stack:8388608"], cwd=work, env=env)
                run([str(exe)], cwd=work, env=env)
                executions += 1
                print(f"PASS native Windows x64: {source.name} -O{optimization}", flush=True)
    print(f"Passed {executions} native Windows executions across {len(fixtures)} fixtures.")


if __name__ == "__main__":
    main()
