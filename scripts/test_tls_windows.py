#!/usr/bin/env python3
"""Run Dodo TLS engines and HTTPS/TCP loopback clients as Windows PE under Wine.

Requires an already built OpenSSL >=3.5 for Windows x64, clang/lld, MinGW
headers/import libraries, Wine, and host openssl for local test credentials.
No dependency download or install is performed by this test runner.
"""
import argparse
import json
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import time
import test_stdlib_windows as harness

ROOT = Path(__file__).resolve().parents[1]
PEER_SPEC = importlib.util.spec_from_file_location("dodo_tls_interop", ROOT / "tests/tls/interop.py")
interop = importlib.util.module_from_spec(PEER_SPEC)
PEER_SPEC.loader.exec_module(interop)


def credentials(work):
    (work / "extensions.cnf").write_text(
        "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1\n"
        "extendedKeyUsage=serverAuth,clientAuth\nbasicConstraints=critical,CA:FALSE\n")
    for args in [
        ["req", "-x509", "-newkey", "rsa:2048", "-noenc", "-keyout", "ca.key", "-out", "ca.pem", "-subj", "/CN=Dodo local test CA", "-days", "3650"],
        ["req", "-new", "-newkey", "rsa:2048", "-noenc", "-keyout", "key.pem", "-out", "leaf.csr", "-subj", "/CN=localhost"],
        ["x509", "-req", "-in", "leaf.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", "cert.pem", "-days", "2", "-extfile", "extensions.cnf"],
    ]:
        harness.run(["openssl", *args], cwd=work)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    parser.add_argument("--openssl-source", type=Path, required=True,
                        help="Configured OpenSSL build tree containing include/, libssl.a and libcrypto.a")
    parser.add_argument("--mingw-include", type=Path, required=True)
    parser.add_argument("--mingw-lib", type=Path, default=Path("/usr/x86_64-w64-mingw32/sys-root/mingw/lib"))
    parser.add_argument("--wine", required=True)
    parser.add_argument("--wineserver", required=True)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--work", type=Path, help="Retain artifacts in this directory")
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    backend = args.openssl_source.resolve()
    clang = harness.tool(["clang-22", "clang"])
    linker = harness.tool(["lld-link-22", "lld-link"])
    temporary = tempfile.TemporaryDirectory(prefix="dodo-tls-windows-")
    work = args.work.resolve() if args.work else Path(temporary.name)
    work.mkdir(parents=True, exist_ok=True)
    credentials(work)
    source = (ROOT / "tests/tls/engine_checks.dodo").read_text()
    for marker, filename in [("CA", "ca.pem"), ("CERT", "cert.pem"), ("KEY", "key.pem")]:
        source = source.replace(f"@@{marker}@@", (work / filename).read_text().replace("\n", "\\n"))
    source = source.replace("const NOW: i64 = 0", f"const NOW: i64 = {int(time.time())}")
    fixture = work / "checks.dodo"
    fixture.write_text(source)
    runtime = work / "startup.c"
    startup_source = harness.RUNTIME
    startup_source = startup_source.replace(
        "void mainCRTStartup(void) { ExitProcess((unsigned int)dodo_main()); }",
        "static void (*exit_callbacks[64])(void);\n"
        "static unsigned int exit_count;\n"
        "int atexit(void (*callback)(void)) { if (exit_count == 64) return 1; "
        "exit_callbacks[exit_count++] = callback; return 0; }\n"
        "void mainCRTStartup(void) { unsigned int code = (unsigned int)dodo_main(); "
        "while (exit_count) exit_callbacks[--exit_count](); ExitProcess(code); }")
    startup = work / "startup.obj"
    probe = work / "probe.obj"
    harness.run([clang, "--target=x86_64-pc-windows-msvc", "-c", str(ROOT / "tests/support/windows_chkstk.S"), "-o", str(probe)])
    alias = work / "probe-alias.S"
    alias.write_text(".text\n.globl ___chkstk_ms\n___chkstk_ms:\n jmp __chkstk\n")
    alias_object = work / "probe-alias.obj"
    harness.run([clang, "--target=x86_64-pc-windows-msvc", "-c", str(alias), "-o", str(alias_object)])
    cflags = [clang, "--target=x86_64-w64-windows-gnu", "-std=c11", "-fno-stack-protector",
              "-isystem", str(args.mingw_include), "-I", str(backend / "include")]
    (work / "wine").mkdir(exist_ok=True)
    env = dict(os.environ, WINEPREFIX=str(work / "wine"), WINEARCH="win64", WINEDEBUG="-all")
    env.pop("DISPLAY", None)
    records = []
    subprocess.run([args.wineserver, "-p"], env=env, check=True, timeout=20)
    try:
        native_objects = {}
        libraries = [args.mingw_lib / f"lib{name}.a" for name in
                     ["kernel32", "msvcrt", "mingwex", "mingw32", "ws2_32", "crypt32", "advapi32", "bcrypt", "user32"]]
        for name, package in [("engine_checks", "tls_checks"), ("loopback_client", "tls_loopback")]:
            original = ROOT / f"tests/tls/{name}.dodo"
            runtime.write_text(startup_source.replace("PACKAGE", package))
            harness.run([clang, "--target=x86_64-pc-windows-msvc", "-O2", "-fno-builtin",
                         "-c", str(runtime), "-o", str(startup)])
            for level in (0, 3):
                peer = interop.LoopbackPeer(work) if name == "loopback_client" else None
                try:
                    if peer:
                        fixture = work / f"loopback-{level}.dodo"
                        fixture.write_text(peer.source(original))
                    obj, exe = work / f"{name}-{level}.obj", work / f"{name}-{level}.exe"
                    boundaries = harness.native_sources(original)
                    for boundary in boundaries:
                        key = (boundary, level)
                        if key not in native_objects:
                            native = work / f"{boundary.parent.name}-native-{level}.obj"
                            harness.run([*cflags, f"-O{level}", "-Wall", "-Wextra", "-Werror", "-c",
                                         str(boundary), "-o", str(native)])
                            native_objects[key] = native
                    harness.run([str(compiler), "build", str(fixture), "--target", "x86_64-pc-windows-msvc",
                                 "--emit", "obj", "-O", str(level), "-o", str(obj)], timeout=600)
                    harness.run([linker, "/nologo", "/nodefaultlib", "/entry:mainCRTStartup", "/subsystem:console",
                                 "/machine:x64", "/stack:8388608", f"/out:{exe}", str(startup), str(probe),
                                 str(alias_object), str(obj),
                                 *[str(native_objects[(boundary, level)]) for boundary in boundaries],
                                 str(backend / "libssl.a"), str(backend / "libcrypto.a"), *map(str, libraries)])
                    def execute():
                        harness.run([args.wine, str(exe)], env=env, cwd=work, timeout=60)
                    if peer:
                        peer.run(execute)
                    else:
                        execute()
                    records.append({"fixture": f"tls/{name}.dodo", "optimization": level,
                                    "target": "x86_64-pc-windows-msvc", "exit_code": 0})
                    print(f"PASS Windows x64 / Wine {name} -O{level}", flush=True)
                finally:
                    if peer:
                        peer.close()
    finally:
        subprocess.run([args.wineserver, "-k"], env=env, capture_output=True, timeout=20)
        subprocess.run([args.wineserver, "-w"], env=env, capture_output=True, timeout=20)
    if args.report:
        args.report.write_text(json.dumps({"executions": len(records), "fixtures": records}, indent=2) + "\n")
    temporary.cleanup()


if __name__ == "__main__":
    main()
