#!/usr/bin/env python3
"""Cross-compile portable and hosted std fixtures and execute real PE files in Wine.

Requires a built host dodo, clang, lld-link, Wine, and MinGW import libraries.
Hosted fixtures also require MinGW headers. Portable fixtures retain their
freestanding startup and trap on failed runtime checks; hosted thread/child
fixtures link the required C runtime.
LLVM's stack probe handles large Windows stack frames.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def tool(names):
    for name in names:
        found = shutil.which(name)
        if found:
            return found
    raise SystemExit(f"Required tool not found: {' or '.join(names)}")


def run(arguments, *, env=None, timeout=120, cwd=None):
    # Wine's background services can inherit stdout/stderr. A log file avoids
    # waiting for those services to close pipes after the tested process exits.
    with tempfile.TemporaryFile(mode="w+") as log:
        try:
            result = subprocess.run(arguments, stdout=log, stderr=log, env=env, timeout=timeout, cwd=cwd)
        except subprocess.TimeoutExpired as error:
            log.seek(0)
            raise RuntimeError(f"Command timed out after {timeout}s: {' '.join(map(str, arguments))}\n"
                               f"{log.read()}") from error
        log.seek(0)
        output = log.read()
    if result.returncode:
        raise RuntimeError(f"Command failed ({result.returncode}): {' '.join(map(str, arguments))}\n"
                           f"{output}")
    return result


def run_console(arguments, *, env, cwd):
    # Use files, not captured pipes: Wine services can inherit process handles.
    with tempfile.TemporaryFile() as input_file, tempfile.TemporaryFile() as output_file, tempfile.TemporaryFile() as error_file:
        input_file.write(b"abc\r\ntail")
        input_file.seek(0)
        result = subprocess.run(arguments, stdin=input_file, stdout=output_file,
                                stderr=error_file, env=env, cwd=cwd, timeout=120)
        output_file.seek(0)
        error_file.seek(0)
        stdout, stderr = output_file.read(), error_file.read()
        expected_stdout = b"Hello, world!\n42\ntext line\n42\n"
        expected_stderr = b"io: Closed code=9 transferred=0\n"
        if result.returncode or stdout != expected_stdout or stderr != expected_stderr:
            raise RuntimeError(f"Console fixture failed ({result.returncode}): {arguments}\n"
                               f"stdout={stdout!r}\nstderr={stderr!r}")


RUNTIME = r'''
typedef __SIZE_TYPE__ size_t;
/* COFF floating-point marker. Arithmetic itself remains generated machine code;
   this symbol introduces no C runtime, libm initialization, or dependency. */
int _fltused = 0;
__declspec(dllimport) void __stdcall ExitProcess(unsigned int code);
extern int dodo_main(void) __asm__("dodo.PACKAGE.main");
void *memcpy(void *destination, const void *source, size_t length) {
    unsigned char *d = destination;
    const unsigned char *s = source;
    for (size_t i = 0; i < length; ++i) d[i] = s[i];
    return destination;
}
void *memmove(void *destination, const void *source, size_t length) {
    unsigned char *d = destination;
    const unsigned char *s = source;
    if ((size_t)d < (size_t)s) {
        for (size_t i = 0; i < length; ++i) d[i] = s[i];
    } else {
        for (size_t i = length; i != 0; --i) d[i - 1] = s[i - 1];
    }
    return destination;
}
void *memset(void *destination, int value, size_t length) {
    unsigned char *d = destination;
    for (size_t i = 0; i < length; ++i) d[i] = (unsigned char)value;
    return destination;
}
int memcmp(const void *left, const void *right, size_t length) {
    const unsigned char *a = left, *b = right;
    for (size_t i = 0; i < length; ++i) {
        if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
    }
    return 0;
}
void mainCRTStartup(void) { ExitProcess((unsigned int)dodo_main()); }
'''


def runtime_source(source):
    """Match the fixture's Dodo entry ABI; void has no exit-code register."""
    package = re.search(r"^package\s+([A-Za-z_][A-Za-z_0-9]*)\s*$", source, re.MULTILINE)
    entry = re.search(r"^(?:pub\s+)?fn\s+main\s*\(\s*\)\s*(?:->\s*(i32|void)\s*)?\{",
                      source, re.MULTILINE)
    if package is None or entry is None:
        raise RuntimeError("Windows fixture needs a package and main() returning void or i32")
    runtime = RUNTIME.replace("PACKAGE", package.group(1))
    if entry.group(1) != "i32":
        runtime = runtime.replace("extern int dodo_main(void)", "extern void dodo_main(void)")
        runtime = runtime.replace("ExitProcess((unsigned int)dodo_main());", "dodo_main(); ExitProcess(0);")
    return runtime


CHILD_STARTUP = r'''
#include <stdio.h>
__declspec(dllimport) void __stdcall ExitProcess(unsigned int);
__declspec(dllimport) int __getmainargs(int *, char ***, char ***, int, int *);
__declspec(dllimport) FILE *__iob_func(void);
/* MinGW's stdio declarations use the UCRT spelling. The controlled child uses
   the matching legacy msvcrt FILE layout, selected by its target headers. */
FILE *__cdecl child_iob(unsigned int index) {
    return &__iob_func()[index];
}
FILE *(*__imp___acrt_iob_func)(unsigned int) = child_iob;
void __main(void) {}
extern int main(int, char **);
void mainCRTStartup(void) {
    int argc, startup = 0; char **argv, **environment;
    if (__getmainargs(&argc, &argv, &environment, 0, &startup)) ExitProcess(100);
    ExitProcess((unsigned int)main(argc, argv));
}
'''


def native_sources(source):
    """Follow bundled imports so independent packages link only their adapters."""
    seen, runtime = set(), set()

    def visit(path):
        if path in seen:
            return
        seen.add(path)
        for name in re.findall(r'^\s*import\s+"([^"]+)"', path.read_text(), re.MULTILINE):
            if not name.startswith(("std/", "core/", "alloc/")):
                continue
            if name.endswith("/native"):
                name = name[:-6] + "windows"
            dependency = ROOT / "stdlib" / (name + ".dodo")
            if dependency.is_file():
                boundary = dependency.parent / "runtime.c"
                if boundary.is_file() and dependency.stem in ("linux", "windows"):
                    runtime.add(boundary)
                visit(dependency)

    visit(source)
    return sorted(runtime)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    parser.add_argument("--kernel32", type=Path, help="Path to the MinGW libkernel32.a import library")
    parser.add_argument("--mingw-include", type=Path, help="Target MinGW headers (needed for hosted native boundaries)")
    parser.add_argument("--fixture", type=Path, action="append", help="Run only this fixture (repeatable)")
    parser.add_argument("--wine", type=Path, help="Use a specific Wine binary, including a local unpacked installation")
    parser.add_argument("--wineserver", type=Path, help="Matching Wine server for --wine")
    parser.add_argument("--report", type=Path, help="Write a JSON record of successful fixture executions")
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    if not compiler.is_file():
        raise SystemExit(f"Build the compiler first: {compiler}")
    clang = tool(["clang-23", "clang"])
    linker = tool(["lld-link-22", "lld-link"])
    wine = str(args.wine.absolute()) if args.wine else tool(["wine64", "wine", "/usr/lib/wine/wine64"])
    wineserver = str(args.wineserver.absolute()) if args.wineserver else tool(["wineserver", "/usr/lib/wine/wineserver64", "/usr/lib/wine/wineserver"])
    wine_data = Path(wine).parent.parent / "share/wine"
    wine_inf = wine_data / "wine.inf"
    if wine_inf.is_file() and ".winmd" in wine_inf.read_text(errors="replace").lower() and not (wine_data / "winmd").is_dir():
        raise SystemExit(f"Wine data files are missing from {wine_data / 'winmd'}. "
                         "Install the matching Wine data package (wine-common on Fedora) before running a fresh prefix.")
    candidates = [args.kernel32] if args.kernel32 else [
        Path("/usr/x86_64-w64-mingw32/sys-root/mingw/lib/libkernel32.a"),
        Path("/usr/x86_64-w64-mingw32/lib/libkernel32.a"),
    ]
    kernel32 = next((path.resolve() for path in candidates if path and path.is_file()), None)
    if kernel32 is None:
        raise SystemExit("MinGW libkernel32.a not found; provide --kernel32 or install the MinGW development files")
    fixtures = [path.resolve() for path in args.fixture] if args.fixture else (
        sorted((ROOT / "tests/stdlib").glob("*.dodo")) +
        sorted(path for path in (ROOT / "tests/os").glob("*.dodo")
               if "_linux_" not in path.name and path.name not in (
                   "process_checks.dodo", "env_native_checks.dodo", "thread_failures.dodo")))
    if not fixtures:
        raise SystemExit("No standard-library fixtures found")
    executions = 0
    records = []
    with tempfile.TemporaryDirectory(prefix="dodo-windows-stdlib-") as directory:
        scratch = Path(directory)
        (scratch / "wine").mkdir()
        env = dict(os.environ, WINEPREFIX=str(scratch / "wine"), WINEARCH="win64", WINEDEBUG="-all")
        env.update(DODO_ENV_TEST="parent  é", DODO_PARENT_ONLY="must not leak",
                   DODO_HOSTED_EMPTY="", DODO_HOSTED_VALUE="hé!!")
        env.pop("DODO_HOSTED_ABSENT", None)
        env.pop("DISPLAY", None)
        # Keep Wine services alive across fixtures; terminate only our own prefix.
        subprocess.run([wineserver, "-p"], env=env, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, check=True, timeout=20)
        try:
            stack_probe = scratch / "chkstk.obj"
            run([clang, "--target=x86_64-pc-windows-msvc", "-c", str(ROOT / "tests/support/windows_chkstk.S"),
                 "-o", str(stack_probe)])
            native_objects = {}
            hosted = any(native_sources(source) for source in fixtures)
            includes = [args.mingw_include] if args.mingw_include else [
                kernel32.parent.parent / "include",
                Path("/usr/x86_64-w64-mingw32/include"),
            ]
            headers = next((path for path in includes if path and (path / "windows.h").is_file()), None)
            if hosted and headers is None:
                raise RuntimeError("Hosted Windows fixtures require MinGW target headers; provide --mingw-include")
            cflags = [clang, "--target=x86_64-w64-windows-gnu", "-std=c11", "-O2", "-fno-stack-protector"]
            if headers:
                cflags += ["-isystem", str(headers)]
            for boundary in sorted({path for source in fixtures for path in native_sources(source)}):
                for optimization in (0, 3):
                    obj = scratch / (boundary.parent.name + f"-native-O{optimization}.obj")
                    run([*cflags, f"-O{optimization}", "-Wall", "-Wextra", "-Werror", "-c", str(boundary), "-o", str(obj)])
                    native_objects[boundary, optimization] = obj
            # GNU-targeted C probes use the same x64 convention under this name.
            probe_alias = scratch / "probe-alias.S"
            probe_alias.write_text(".text\n.globl ___chkstk_ms\n___chkstk_ms:\n jmp __chkstk\n")
            alias_object = scratch / "probe-alias.obj"
            run([clang, "--target=x86_64-pc-windows-msvc", "-c", str(probe_alias), "-o", str(alias_object)])
            child = ROOT / "tests/support/os_child.c"
            if any("process" in source.stem for source in fixtures):
                child_obj, child_start_obj = scratch / "child.obj", scratch / "child-start.obj"
                child_start = scratch / "child-start.c"
                child_start.write_text(CHILD_STARTUP)
                run([*cflags, "-D__USE_MINGW_ANSI_STDIO=0", "-c", str(child), "-o", str(child_obj)])
                run([*cflags, "-c", str(child_start), "-o", str(child_start_obj)])
                run([linker, "/nologo", "/nodefaultlib", "/entry:mainCRTStartup", "/subsystem:console", "/machine:x64",
                     f"/out:{scratch / 'os child.exe'}", str(child_obj), str(child_start_obj), str(stack_probe), str(alias_object),
                     str(kernel32), str(kernel32.parent / "libmsvcrt.a")])
            for source in fixtures:
                boundaries = native_sources(source)
                # Portable fixtures link without a CRT, even with a Windows
                # target triple whose automatic panic strategy uses the CRT.
                panic = "auto" if boundaries else "trap"
                startup = scratch / "startup.c"
                startup.write_text(runtime_source(source.read_text()))
                runtime_object = scratch / "startup.obj"
                run([clang, "--target=x86_64-pc-windows-msvc", "-ffreestanding", "-fno-builtin", "-fno-stack-protector",
                     "-O2", "-c", str(startup), "-o", str(runtime_object)])
                for optimization in (0, 3):
                    obj = scratch / f"{source.stem}-O{optimization}.obj"
                    exe = obj.with_suffix(".exe")
                    run([str(compiler), "build", str(source), "--emit", "obj", "--target", "x86_64-pc-windows-msvc",
                         "--panic", panic, "-O", str(optimization), "-o", str(obj)])
                    run([linker, "/nologo", "/nodefaultlib", "/entry:mainCRTStartup", "/subsystem:console", "/machine:x64",
                         "/stack:8388608", f"/out:{exe}", str(runtime_object), str(stack_probe), str(alias_object), str(obj), str(kernel32),
                         *[str(native_objects[path, optimization]) for path in boundaries],
                         *([str(kernel32.parent / "libmsvcrt.a")] if boundaries else []),
                         *([str(kernel32.parent / "libws2_32.a")]
                           if any(path.parent.name == "net" for path in boundaries) else [])])
                    if exe.read_bytes()[:2] != b"MZ":
                        raise RuntimeError(f"Linker did not produce a Windows executable: {exe}")
                    work = scratch / f"work-{source.stem}-{optimization}"
                    work.mkdir()
                    if "process" in source.stem:
                        shutil.copyfile(scratch / "os child.exe", work / "os child.exe")
                        child_work = work / "child cwd é"
                        child_work.mkdir()
                        shutil.copyfile(scratch / "os child.exe", child_work / "os child.exe")
                        (child_work / "cwd-marker").write_bytes(b"fixture")
                    extra_args = ["space arg", "é", ""] if source.stem in ("env_checks", "hosted_env") else []
                    if source.stem == "console_checks":
                        run_console([wine, str(exe)], env=env, cwd=work)
                    else:
                        run([wine, str(exe), *extra_args], env=env, cwd=work)
                    executions += 1
                    records.append({"fixture": source.name, "optimization": optimization,
                                    "target": "x86_64-pc-windows-msvc", "exit_code": 0})
                    print(f"PASS Windows x64 / Wine: {source.name} -O{optimization}", flush=True)
        finally:
            subprocess.run([wineserver, "-k"], env=env, capture_output=True, timeout=20)
            subprocess.run([wineserver, "-w"], env=env, capture_output=True, timeout=20)
    if args.report:
        args.report.write_text(json.dumps({"executions": executions, "fixtures": records}, indent=2) + "\n")
    print(f"Passed {executions} Windows executions across {len(fixtures)} fixtures.")


if __name__ == "__main__":
    main()
