#!/usr/bin/env python3
"""Cross-compile the portable core/alloc/std fixtures and execute real PE files in Wine.

Requires a built host dodo, clang, lld-link, Wine, and a MinGW kernel32 import
library. No Windows C runtime or allocator is linked. The tiny test startup
supplies compiler-generated memory helpers and forwards the Dodo exit status;
LLVM's stack probe handles large Windows stack frames.
"""
import argparse
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


def run(arguments, *, env=None, timeout=120):
    # Wine's background services can inherit stdout/stderr. A log file avoids
    # waiting for those services to close pipes after the tested process exits.
    with tempfile.TemporaryFile(mode="w+") as log:
        try:
            result = subprocess.run(arguments, stdout=log, stderr=log, env=env, timeout=timeout)
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


RUNTIME = r'''
typedef __SIZE_TYPE__ size_t;
// LLVM uses the MSVC floating-point marker even with a freestanding entry.
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    parser.add_argument("--kernel32", type=Path, help="Path to the MinGW libkernel32.a import library")
    parser.add_argument("--fixture", type=Path, action="append", help="Run only this fixture (repeatable)")
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    if not compiler.is_file():
        raise SystemExit(f"Build the compiler first: {compiler}")
    clang = tool(["clang-22", "clang"])
    linker = tool(["lld-link-22", "lld-link"])
    wine = tool(["wine64", "wine", "/usr/lib/wine/wine64"])
    wineserver = tool(["wineserver", "/usr/lib/wine/wineserver64", "/usr/lib/wine/wineserver"])
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
    fixtures = [path.resolve() for path in args.fixture] if args.fixture else sorted((ROOT / "tests/stdlib").glob("*.dodo"))
    if not fixtures:
        raise SystemExit("No standard-library fixtures found")
    executions = 0
    with tempfile.TemporaryDirectory(prefix="dodo-windows-stdlib-") as directory:
        scratch = Path(directory)
        (scratch / "wine").mkdir()
        env = dict(os.environ, WINEPREFIX=str(scratch / "wine"), WINEARCH="win64", WINEDEBUG="-all")
        env.pop("DISPLAY", None)
        # Keep Wine services alive across fixtures; terminate only our own prefix.
        subprocess.run([wineserver, "-p"], env=env, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, check=True, timeout=20)
        try:
            stack_probe = scratch / "chkstk.obj"
            run([clang, "--target=x86_64-pc-windows-msvc", "-c", str(ROOT / "tests/support/windows_chkstk.S"),
                 "-o", str(stack_probe)])
            for source in fixtures:
                declaration = re.search(r"^package\s+([A-Za-z_][A-Za-z_0-9]*)\s*$", source.read_text(), re.MULTILINE)
                if declaration is None:
                    raise RuntimeError(f"Missing package declaration: {source}")
                startup = scratch / "startup.c"
                startup.write_text(RUNTIME.replace("PACKAGE", declaration.group(1)))
                runtime_object = scratch / "startup.obj"
                run([clang, "--target=x86_64-pc-windows-msvc", "-ffreestanding", "-fno-builtin", "-fno-stack-protector",
                     "-O2", "-c", str(startup), "-o", str(runtime_object)])
                for optimization in (0, 3):
                    obj = scratch / f"{source.stem}-O{optimization}.obj"
                    exe = obj.with_suffix(".exe")
                    run([str(compiler), "build", str(source), "--emit", "obj", "--target", "x86_64-pc-windows-msvc",
                         "-O", str(optimization), "-o", str(obj)])
                    run([linker, "/nologo", "/nodefaultlib", "/entry:mainCRTStartup", "/subsystem:console", "/machine:x64",
                         f"/out:{exe}", str(runtime_object), str(stack_probe), str(obj), str(kernel32)])
                    if exe.read_bytes()[:2] != b"MZ":
                        raise RuntimeError(f"Linker did not produce a Windows executable: {exe}")
                    run([wine, str(exe)], env=env)
                    executions += 1
                    print(f"PASS Windows x64 / Wine: {source.name} -O{optimization}", flush=True)
        finally:
            subprocess.run([wineserver, "-k"], env=env, capture_output=True, timeout=20)
            subprocess.run([wineserver, "-w"], env=env, capture_output=True, timeout=20)
    print(f"Passed {executions} Windows executions across {len(fixtures)} core/alloc/std fixtures.")


if __name__ == "__main__":
    main()
