#!/usr/bin/env python3
"""Windows test suites for Dodo, on Linux with Wine or on native Windows (see COMMAND --help)."""
import argparse
import contextlib
import importlib.util
import inspect
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
import time
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MSVC = "x86_64-pc-windows-msvc"
# Freestanding PE link: no CRT unless a suite adds its import libraries.
LINK_PE = ["/nologo", "/nodefaultlib", "/entry:mainCRTStartup", "/subsystem:console", "/machine:x64"]
OPTIMIZATIONS = (0, 3)


def tool(names):
    for name in names:
        found = shutil.which(name)
        if found:
            return found
    raise SystemExit(f"Required tool not found: {' or '.join(names)}")


def run(arguments, *, env=None, timeout=120, cwd=None, allowed=(0,)):
    """Run with output in a log file, not pipes, and return the exit code.

    Wine's background services and leaked native children can inherit
    stdout/stderr; a file avoids waiting for them to close pipes. Fixtures may
    write binary data and read stdin: give them no input and decode their
    output only for the error message.
    """
    arguments = [str(argument) for argument in arguments]
    with tempfile.TemporaryFile() as log:
        with subprocess.Popen(arguments, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                              stdout=log, stderr=log) as process:
            try:
                code = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired as error:
                try:
                    if os.name == "nt":
                        # Kill descendants before the parent so captured children cannot
                        # outlive a hung fixture or keep its temporary files open.
                        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                       timeout=20, check=False)
                finally:
                    process.kill()
                    process.wait(timeout=20)
                log.seek(0)
                raise RuntimeError(f"Command timed out after {timeout}s: {' '.join(arguments)}\n"
                                   f"{log.read().decode('utf-8', errors='replace')}") from error
        log.seek(0)
        output = log.read().decode("utf-8", errors="replace")
    if code not in allowed:
        raise RuntimeError(f"Command failed ({code}): {' '.join(arguments)}\n{output}")
    return code


def write_report(path, records):
    if path:
        path.write_text(json.dumps({"executions": len(records), "fixtures": records}, indent=2) + "\n")



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
        for name in re.findall(r'^\s*import\s+"([^"]+)"', path.read_text(encoding="utf-8"), re.MULTILINE):
            if not name.startswith(("std/", "core/", "alloc/")):
                continue
            if name.endswith("/native"):
                name = name[:-6] + "windows"
            module = ROOT / "stdlib" / (name + ".dodo")
            directory = ROOT / "stdlib" / name
            dependencies = [module] if module.is_file() else sorted(directory.glob("*.dodo"))
            for dependency in dependencies:
                boundary = dependency.parent / "runtime.c"
                if boundary.is_file() and dependency.stem in ("posix", "windows"):
                    runtime.add(boundary)
                visit(dependency)

    visit(source)
    return sorted(runtime)


def llvm_tools():
    return tool(["clang-23", "clang"]), tool(["lld-link-22", "lld-link"])


def add_wine_arguments(parser):
    parser.add_argument("--wine", type=Path, help="Use a specific Wine binary, including a local unpacked installation")
    parser.add_argument("--wineserver", type=Path, help="Matching Wine server for --wine")


def find_wine(args):
    wine = str(args.wine.absolute()) if args.wine else tool(["wine64", "wine", "/usr/lib/wine/wine64"])
    wineserver = str(args.wineserver.absolute()) if args.wineserver else tool(
        ["wineserver", "/usr/lib/wine/wineserver64", "/usr/lib/wine/wineserver"])
    wine_data = Path(wine).parent.parent / "share/wine"
    wine_inf = wine_data / "wine.inf"
    if wine_inf.is_file() and ".winmd" in wine_inf.read_text(errors="replace").lower() and not (wine_data / "winmd").is_dir():
        raise SystemExit(f"Wine data files are missing from {wine_data / 'winmd'}. "
                         "Install the matching Wine data package (wine-common on Fedora) before running a fresh prefix.")
    return wine, wineserver


@contextlib.contextmanager
def wine_prefix(directory, wineserver, **extra):
    """Yield an environment for a fresh, display-less win64 prefix in directory."""
    directory.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, WINEPREFIX=str(directory), WINEARCH="win64", WINEDEBUG="-all", **extra)
    env.pop("DISPLAY", None)
    # Keep Wine services alive across fixtures; terminate only our own prefix.
    subprocess.run([wineserver, "-p"], env=env, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, check=True, timeout=20)
    try:
        yield env
    finally:
        subprocess.run([wineserver, "-k"], env=env, capture_output=True, timeout=20)
        subprocess.run([wineserver, "-w"], env=env, capture_output=True, timeout=20)


def stack_probes(clang, scratch):
    """LLVM's stack probe handles large Windows frames; GNU-targeted C uses the same x64 probe."""
    probe = scratch / "chkstk.obj"
    run([clang, f"--target={MSVC}", "-c", ROOT / "tests/support/windows_chkstk.S", "-o", probe])
    alias = scratch / "probe-alias.S"
    alias.write_text(".text\n.globl ___chkstk_ms\n___chkstk_ms:\n jmp __chkstk\n")
    alias_object = scratch / "probe-alias.obj"
    run([clang, f"--target={MSVC}", "-c", alias, "-o", alias_object])
    return [probe, alias_object]


def compile_startup(clang, source, scratch):
    path, obj = scratch / "startup.c", scratch / "startup.obj"
    path.write_text(source)
    run([clang, f"--target={MSVC}", "-ffreestanding", "-fno-builtin", "-fno-stack-protector",
         "-O2", "-c", path, "-o", obj])
    return obj


def native_object(cflags, boundary, optimization, scratch, cache):
    """Compile a hosted runtime.c boundary for MinGW once per optimization level."""
    key = (boundary, optimization)
    if key not in cache:
        obj = scratch / f"{boundary.parent.name}-native-O{optimization}.obj"
        run([*cflags, f"-O{optimization}", "-Wall", "-Wextra", "-Werror", "-c", boundary, "-o", obj])
        cache[key] = obj
    return cache[key]



def command_startup(args):
    """Exercise the Wine harness entry wrapper without requiring Windows tools."""
    if not (platform.system() == "Linux" and platform.machine() == "x86_64" and shutil.which("cc")):
        print("SKIP startup: requires Linux x64 and a C compiler")
        return
    # Execute the exact generated wrapper with a local ExitProcess shim.
    # A void callee may leave anything in EAX, even after successful work.
    # Deliberately leave 73 there so reading it as an int cannot pass by luck.
    for result_type in ("", " -> void", " -> i32"):
        for optimization in OPTIMIZATIONS:
            runtime = runtime_source(f"package fixture\nfn main(){result_type} {{}}\n")
            callee = ('int dodo_main(void) { return 73; }' if result_type == " -> i32" else
                      'void dodo_main(void) { __asm__ volatile("movl $73, %%eax" ::: "eax"); }')
            source = ("#define __declspec(x)\n#define __stdcall\n" + runtime +
                      '\n#include <stdlib.h>\n'
                      'void ExitProcess(unsigned int code) { _Exit(code); }\n'
                      '__attribute__((noinline)) ' + callee +
                      '\nint main(void) { mainCRTStartup(); return 99; }\n')
            expected = 73 if result_type == " -> i32" else 0
            with tempfile.TemporaryDirectory(prefix="dodo-startup-") as directory:
                path = Path(directory)
                (path / "startup.c").write_text(source)
                run(["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-fno-builtin", f"-O{optimization}",
                     path / "startup.c", "-o", path / "startup"])
                run([path / "startup"], timeout=10, allowed=(expected,))
            print(f"PASS startup: main(){result_type or ' (implicit void)'} -O{optimization} exits {expected}")



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


def default_stdlib_fixtures():
    # window_events, window_frames, window_multi, window_desktop and window_gpu open real
    # windows, which Wine cannot without a display; tests/window_library.rs drives them.
    # window_desktop also resizes its window and goes fullscreen. The gpu_metal_* fixtures
    # need Metal, which only macOS has.
    return (sorted(path for path in (ROOT / "tests/stdlib").glob("*.dodo")
                   if path.name not in ("window_events.dodo", "window_frames.dodo", "window_multi.dodo",
                                        "window_desktop.dodo", "window_gpu.dodo")
                   and not path.name.startswith("gpu_metal")) +
            sorted(path for path in (ROOT / "tests/os").glob("*.dodo")
                   if "_unix_" not in path.name and path.name not in (
                       "process_checks.dodo", "env_native_checks.dodo", "thread_failures.dodo")))


def command_stdlib(args):
    """Cross-compile portable and hosted std fixtures and execute real PE files in Wine.

    Requires a built host dodo, clang, lld-link, Wine, and MinGW import libraries.
    Hosted fixtures also require MinGW headers. Portable fixtures retain their
    freestanding startup and trap on failed runtime checks; hosted thread/child
    fixtures link the required C runtime.
    """
    compiler = args.compiler.resolve()
    if not compiler.is_file():
        raise SystemExit(f"Build the compiler first: {compiler}")
    clang, linker = llvm_tools()
    wine, wineserver = find_wine(args)
    candidates = [args.kernel32] if args.kernel32 else [
        Path("/usr/x86_64-w64-mingw32/sys-root/mingw/lib/libkernel32.a"),
        Path("/usr/x86_64-w64-mingw32/lib/libkernel32.a"),
    ]
    kernel32 = next((path.resolve() for path in candidates if path and path.is_file()), None)
    if kernel32 is None:
        raise SystemExit("MinGW libkernel32.a not found; provide --kernel32 or install the MinGW development files")
    mingw_lib = kernel32.parent
    fixtures = [path.resolve() for path in args.fixture] if args.fixture else default_stdlib_fixtures()
    if not fixtures:
        raise SystemExit("No standard-library fixtures found")
    records = []
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="dodo-windows-stdlib-") as directory:
        scratch = Path(directory)
        hosted_env = dict(DODO_ENV_TEST="parent  é", DODO_PARENT_ONLY="must not leak",
                          DODO_HOSTED_EMPTY="", DODO_HOSTED_VALUE="hé!!")
        with wine_prefix(scratch / "wine", wineserver, **hosted_env) as env:
            env.pop("DODO_HOSTED_ABSENT", None)
            probes = stack_probes(clang, scratch)
            native_objects = {}
            hosted = any(native_sources(source) for source in fixtures)
            includes = [args.mingw_include] if args.mingw_include else [
                mingw_lib.parent / "include",
                Path("/usr/x86_64-w64-mingw32/include"),
            ]
            headers = next((path for path in includes if path and (path / "windows.h").is_file()), None)
            if hosted and headers is None:
                raise RuntimeError("Hosted Windows fixtures require MinGW target headers; provide --mingw-include")
            cflags = [clang, "--target=x86_64-w64-windows-gnu", "-std=c11", "-O2", "-fno-stack-protector"]
            if headers:
                cflags += ["-isystem", str(headers)]
            for boundary in sorted({path for source in fixtures for path in native_sources(source)}):
                for optimization in OPTIMIZATIONS:
                    native_object(cflags, boundary, optimization, scratch, native_objects)
            child = ROOT / "tests/support/os_child.c"
            if any("process" in source.stem for source in fixtures):
                child_obj, child_start_obj = scratch / "child.obj", scratch / "child-start.obj"
                child_start = scratch / "child-start.c"
                child_start.write_text(CHILD_STARTUP)
                run([*cflags, "-D__USE_MINGW_ANSI_STDIO=0", "-c", child, "-o", child_obj])
                run([*cflags, "-c", child_start, "-o", child_start_obj])
                run([linker, *LINK_PE, f"/out:{scratch / 'os child.exe'}", child_obj, child_start_obj,
                     *probes, kernel32, mingw_lib / "libmsvcrt.a"])
            for source in fixtures:
                boundaries = native_sources(source)
                # Portable fixtures link without a CRT, even with a Windows
                # target triple whose automatic panic strategy uses the CRT.
                panic = "auto" if boundaries else "trap"
                runtime_object = compile_startup(clang, runtime_source(source.read_text()), scratch)
                for optimization in OPTIMIZATIONS:
                    # Run every fixture and report all failures at the end, so one
                    # broken fixture does not hide the ones after it.
                    try:
                        obj = scratch / f"{source.stem}-O{optimization}.obj"
                        exe = obj.with_suffix(".exe")
                        run([compiler, "build", source, "--emit", "obj", "--target", MSVC,
                             "--panic", panic, "-O", str(optimization), "-o", obj])
                        run([linker, *LINK_PE, "/stack:8388608", f"/out:{exe}", runtime_object, *probes, obj, kernel32,
                             *[native_objects[path, optimization] for path in boundaries],
                             *([mingw_lib / "libmsvcrt.a"] if boundaries else []),
                             *([mingw_lib / "libws2_32.a"]
                               if any(path.parent.name == "net" for path in boundaries) else []),
                             *([mingw_lib / "libuser32.a", mingw_lib / "libgdi32.a"]
                               if any(path.parent.name == "window" for path in boundaries) else [])])
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
                        exit_code = 0
                        if source.stem == "console_checks":
                            run_console([wine, str(exe)], env=env, cwd=work)
                        elif source.stem in ("gpu_checks", "gpu_feature_checks", "gpu_surface_checks", "gpu_wgsl_checks"):
                            # Wine has no GPU std/gpu can use: the fixtures must try
                            # every backend, report each failure and exit 2.
                            exit_code = run([wine, exe], env=env, cwd=work, allowed=(0, 2))
                        else:
                            run([wine, exe, *extra_args], env=env, cwd=work)
                        records.append({"fixture": source.name, "optimization": optimization,
                                        "target": MSVC, "exit_code": exit_code})
                        print(f"PASS Windows x64 / Wine: {source.name} -O{optimization}", flush=True)
                    except RuntimeError as error:
                        failures.append(f"{source.name} -O{optimization}")
                        print(f"FAIL Windows x64 / Wine: {source.name} -O{optimization}\n{error}", flush=True)
    write_report(args.report, records)
    print(f"Passed {len(records)} Windows executions across {len(fixtures)} fixtures.")
    if failures:
        raise SystemExit(f"{len(failures)} Windows executions failed: {', '.join(failures)}")



def tls_credentials(work):
    (work / "extensions.cnf").write_text(
        "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1\n"
        "extendedKeyUsage=serverAuth,clientAuth\nbasicConstraints=critical,CA:FALSE\n")
    for args in [
        ["req", "-x509", "-newkey", "rsa:2048", "-noenc", "-keyout", "ca.key", "-out", "ca.pem", "-subj", "/CN=Dodo local test CA", "-days", "3650"],
        ["req", "-new", "-newkey", "rsa:2048", "-noenc", "-keyout", "key.pem", "-out", "leaf.csr", "-subj", "/CN=localhost"],
        ["x509", "-req", "-in", "leaf.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", "cert.pem", "-days", "2", "-extfile", "extensions.cnf"],
    ]:
        run(["openssl", *args], cwd=work)


def command_tls(args):
    """Run Dodo TLS engines and HTTPS/TCP loopback clients as Windows PE under Wine.

    Requires an already built OpenSSL >=3.5 for Windows x64, clang/lld, MinGW
    headers/import libraries, Wine, and host openssl for local test credentials.
    No dependency download or install is performed by this test runner.
    """
    spec = importlib.util.spec_from_file_location("dodo_tls_interop", ROOT / "tests/tls/interop.py")
    interop = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(interop)
    compiler = args.compiler.resolve()
    backend = args.openssl_source.resolve()
    clang, linker = llvm_tools()
    wine, wineserver = find_wine(args)
    temporary = tempfile.TemporaryDirectory(prefix="dodo-tls-windows-")
    work = args.work.resolve() if args.work else Path(temporary.name)
    work.mkdir(parents=True, exist_ok=True)
    tls_credentials(work)
    source = (ROOT / "tests/tls/engine_checks.dodo").read_text()
    for marker, filename in [("CA", "ca.pem"), ("CERT", "cert.pem"), ("KEY", "key.pem")]:
        source = source.replace(f"@@{marker}@@", (work / filename).read_text().replace("\n", "\\n"))
    source = source.replace("const NOW: i64 = 0", f"const NOW: i64 = {int(time.time())}")
    fixture = work / "checks.dodo"
    fixture.write_text(source)
    startup_source = RUNTIME.replace(
        "void mainCRTStartup(void) { ExitProcess((unsigned int)dodo_main()); }",
        "static void (*exit_callbacks[64])(void);\n"
        "static unsigned int exit_count;\n"
        "int atexit(void (*callback)(void)) { if (exit_count == 64) return 1; "
        "exit_callbacks[exit_count++] = callback; return 0; }\n"
        "void mainCRTStartup(void) { unsigned int code = (unsigned int)dodo_main(); "
        "while (exit_count) exit_callbacks[--exit_count](); ExitProcess(code); }")
    probes = stack_probes(clang, work)
    cflags = [clang, "--target=x86_64-w64-windows-gnu", "-std=c11", "-fno-stack-protector",
              "-isystem", str(args.mingw_include), "-I", str(backend / "include")]
    records = []
    with wine_prefix(work / "wine", wineserver) as env:
        native_objects = {}
        libraries = [args.mingw_lib / f"lib{name}.a" for name in
                     ["kernel32", "msvcrt", "mingwex", "mingw32", "ws2_32", "crypt32", "advapi32", "bcrypt", "user32"]]
        for name, package in [("engine_checks", "tls_checks"), ("loopback_client", "tls_loopback")]:
            original = ROOT / f"tests/tls/{name}.dodo"
            startup = compile_startup(clang, startup_source.replace("PACKAGE", package), work)
            for level in OPTIMIZATIONS:
                peer = interop.LoopbackPeer(work) if name == "loopback_client" else None
                try:
                    if peer:
                        fixture = work / f"loopback-{level}.dodo"
                        fixture.write_text(peer.source(original))
                    obj, exe = work / f"{name}-{level}.obj", work / f"{name}-{level}.exe"
                    boundaries = [native_object(cflags, boundary, level, work, native_objects)
                                  for boundary in native_sources(original)]
                    run([compiler, "build", fixture, "--target", MSVC, "--emit", "obj", "-O", str(level),
                         "-o", obj], timeout=600)
                    run([linker, *LINK_PE, "/stack:8388608", f"/out:{exe}", startup, *probes, obj, *boundaries,
                         backend / "libssl.a", backend / "libcrypto.a", *libraries])

                    def execute():
                        run([wine, exe], env=env, cwd=work, timeout=60)
                    if peer:
                        peer.run(execute)
                    else:
                        execute()
                    records.append({"fixture": f"tls/{name}.dodo", "optimization": level,
                                    "target": MSVC, "exit_code": 0})
                    print(f"PASS Windows x64 / Wine {name} -O{level}", flush=True)
                finally:
                    if peer:
                        peer.close()
    write_report(args.report, records)
    temporary.cleanup()



NATIVE_FIXTURES = tuple(f"{name}.dodo" for name in (
    "os/fs_checks", "os/fs_modes", "os/fs_path_checks", "os/fs_windows_checks", "os/hosted_fs",
    "os/process_windows_checks", "os/hosted_process", "os/console_windows_checks", "os/thread_checks",
    "os/sync_checks", "net/native_checks", "net/backpressure", "os/process_handles_windows_checks"))


def command_native(args):
    """Build and execute hosted platform fixtures on native Windows x64.

    Requires a Windows Dodo compiler, Clang and the Visual Studio C++/Windows SDK
    libraries. Unlike the stdlib suite, this uses the normal executable build
    path, including the embedded adapters and the Windows CRT startup.
    """
    if os.name != "nt":
        raise SystemExit("this suite requires native Windows; use `test_windows.py stdlib` for Wine")
    compiler = args.compiler.resolve()
    if not compiler.is_file():
        raise SystemExit(f"build the Windows compiler first: {compiler}")
    linker = shutil.which(args.linker)
    if linker is None:
        raise SystemExit(f"Windows C compiler driver not found: {args.linker}")
    fixtures = ([path.resolve() for path in args.fixture] if args.fixture else
                [ROOT / "tests" / name for name in NATIVE_FIXTURES])
    for fixture in fixtures:
        if not fixture.is_file():
            raise SystemExit(f"fixture not found: {fixture}")
    env = dict(os.environ, DODO_PARENT_ONLY="must not leak")
    executions = 0
    with tempfile.TemporaryDirectory(prefix="dodo native Windows ") as directory:
        scratch = Path(directory)
        child = scratch / "os child.exe"
        run([linker, "-std=c11", "-O2", ROOT / "tests/support/os_child.c", "-o", child], cwd=scratch, env=env)
        for index, source in enumerate(fixtures):
            for optimization in OPTIMIZATIONS:
                work = scratch / f"{index}-{source.stem}-O{optimization}"
                work.mkdir()
                shutil.copy2(child, work / child.name)
                child_work = work / "child cwd é"
                child_work.mkdir()
                shutil.copy2(child, child_work / child.name)
                (child_work / "cwd-marker").write_bytes(b"fixture")
                exe = work / f"{source.stem}.exe"
                print(f"RUN native Windows x64: {source.name} -O{optimization}", flush=True)
                run([compiler, "build", source, "--target", MSVC, "-O", str(optimization), "-o", exe,
                     "--linker", linker, "--link-arg", "-Wl,/stack:8388608"], cwd=work, env=env)
                run([exe], cwd=work, env=env)
                executions += 1
                print(f"PASS native Windows x64: {source.name} -O{optimization}", flush=True)
    print(f"Passed {executions} native Windows executions across {len(fixtures)} fixtures.")



WINDOWS_LIBRARIES = {
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "dbghelp.dll",
    "kernel32.dll", "msvcrt.dll", "ntdll.dll", "ole32.dll", "oleaut32.dll",
    "psapi.dll", "rpcrt4.dll", "secur32.dll", "shell32.dll", "ucrtbase.dll",
    "user32.dll", "userenv.dll", "version.dll", "ws2_32.dll",
}


def command_release(args):
    """Extract a Windows release ZIP and test its compiler natively or with Wine."""
    match = re.fullmatch(r"dodo-(.+)-x86_64-pc-windows-msvc.zip", args.archive.name)
    if not match:
        raise SystemExit("expected a dodo-VERSION-x86_64-pc-windows-msvc.zip archive")
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
                     "licenses/zlib-LICENSE", "licenses/zstd-LICENSE", "licenses/zstd-COPYING",
                     "licenses/rust/COPYRIGHT-library.html"):
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

        def output(*command, tools=False):
            env = dict(os.environ)
            env.pop("LLVM_SYS_231_PREFIX", None)
            if os.name == "nt" and not tools:
                # Windows os.environ keys are uppercase after copying to a dict.
                env["PATH"] = str(Path(env["SYSTEMROOT"]) / "System32")
            result = subprocess.run([*runner, *map(str, command)], cwd=scratch, env=env,
                                    capture_output=True, text=True, timeout=120)
            if result.returncode:
                raise RuntimeError(f"Failed: {command}\n{result.stdout}\n{result.stderr}")
            return result.stdout.strip()

        def check(condition, message):
            # Not `assert`: these checks must also run under `python -O`.
            if not condition:
                raise RuntimeError(message)

        version_line = output(compiler, "--version")
        check(version_line == f"dodo {version} (LLVM 23, BSD-2-Clause)", f"Unexpected version: {version_line!r}")
        # Test sources come from the checkout and stay outside the extracted bundle.
        for name in ("hello.dodo", "io.dodo", "bytes.dodo"):
            shutil.copy2(ROOT / "examples" / name, scratch / name)
        hello = "hello.dodo"
        portable = "io.dodo"
        output(compiler, "check", hello)
        output(compiler, "check", "bytes.dodo")
        print("PASS extracted compiler: version, source checks, embedded standard library", flush=True)

        link = ["--linker", args.linker]
        for argument in args.link_arg:
            link.extend(["--link-arg", argument])
        for optimization in ("0", "3"):
            for emit, filename in (("llvm-ir", "hello.ll"), ("bitcode", "hello.bc"),
                                   ("asm", "hello.s"), ("obj", "hello.obj")):
                output(compiler, "build", hello, "--emit", emit, "-O", optimization, "-o", filename)
                check((scratch / filename).stat().st_size > 0, f"empty {filename}")
            check((scratch / "hello.obj").read_bytes().startswith(b"\x64\x86"), "expected x64 COFF")
            output(compiler, "build", portable, "--emit", "obj", "--target", "wasm32-unknown-unknown",
                   "-O", optimization, "-o", "hello.wasm")
            check((scratch / "hello.wasm").read_bytes().startswith(b"\x00asm"), "expected a WebAssembly object")
            output(compiler, "build", hello, "-O", optimization, "-o", "hello.exe", *link, tools=True)
            check(output(scratch / "hello.exe") == "Hello, world!", "hello.exe printed the wrong output")
            check(output(compiler, "run", hello, "-O", optimization, *link, tools=True) == "Hello, world!",
                  "dodo run printed the wrong output")
            print(f"PASS -O{optimization}: IR, bitcode, assembly, COFF, WebAssembly, build and run", flush=True)

    print(f"Verified Windows release: {args.archive}")



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True, metavar="COMMAND")

    def command(name, function):
        doc = inspect.cleandoc(function.__doc__)
        sub = commands.add_parser(name, help=doc.splitlines()[0], description=doc,
                                  formatter_class=argparse.RawDescriptionHelpFormatter)
        sub.set_defaults(function=function)
        return sub

    command("startup", command_startup)

    stdlib = command("stdlib", command_stdlib)
    stdlib.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    stdlib.add_argument("--kernel32", type=Path, help="Path to the MinGW libkernel32.a import library")
    stdlib.add_argument("--mingw-include", type=Path, help="Target MinGW headers (needed for hosted native boundaries)")
    stdlib.add_argument("--fixture", type=Path, action="append", help="Run only this fixture (repeatable)")
    add_wine_arguments(stdlib)
    stdlib.add_argument("--report", type=Path, help="Write a JSON record of successful fixture executions")

    tls = command("tls", command_tls)
    tls.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    tls.add_argument("--openssl-source", type=Path, required=True,
                     help="Configured OpenSSL build tree containing include/, libssl.a and libcrypto.a")
    tls.add_argument("--mingw-include", type=Path, required=True)
    tls.add_argument("--mingw-lib", type=Path, default=Path("/usr/x86_64-w64-mingw32/sys-root/mingw/lib"))
    add_wine_arguments(tls)
    tls.add_argument("--report", type=Path, help="Write a JSON record of successful fixture executions")
    tls.add_argument("--work", type=Path, help="Retain artifacts in this directory")

    native = command("native", command_native)
    native.add_argument("--compiler", type=Path, default=ROOT / f"target/{MSVC}/release/dodo.exe")
    native.add_argument("--linker", default="clang", help="Windows C compiler driver")
    native.add_argument("--fixture", type=Path, action="append", help="Run only this fixture (repeatable)")

    release = command("release", command_release)
    release.add_argument("archive", type=Path)
    release.add_argument("--runner", help="Wine executable when testing on Linux")
    release.add_argument("--linker", default="clang", help="Windows C compiler driver")
    release.add_argument("--link-arg", action="append", default=[], help="Extra linker driver argument")
    release.add_argument("--llvm-readobj", default="llvm-readobj", help="Host LLVM binary inspection tool")

    args = parser.parse_args()
    args.function(args)


if __name__ == "__main__":
    main()
