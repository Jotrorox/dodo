#!/usr/bin/env python3
"""Run Win64 HTTP examples against independent Python peers at O0 and O3.

Uses the same clang/lld, MinGW libraries, isolated Wine prefix and startup shim
as test_stdlib_windows.py. All listeners bind controlled IPv4 loopback ports.
The server fixture consumes streaming request data; the client receives a
fragmented informational response, chunked body and trailers.
"""
import argparse
import http.client
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import threading
import time

from test_stdlib_windows import ROOT, RUNTIME, native_sources, run, tool


def available_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    parser.add_argument("--wine", type=Path)
    parser.add_argument("--wineserver", type=Path)
    parser.add_argument("--mingw-include", type=Path)
    parser.add_argument("--kernel32", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    clang = tool(["clang-22", "clang"])
    linker = tool(["lld-link-22", "lld-link"])
    wine = str(args.wine.absolute()) if args.wine else tool(["wine64", "wine"])
    wineserver = str(args.wineserver.absolute()) if args.wineserver else tool(["wineserver64", "wineserver"])
    candidates = [args.kernel32] if args.kernel32 else [
        Path("/usr/x86_64-w64-mingw32/sys-root/mingw/lib/libkernel32.a"),
        Path("/usr/x86_64-w64-mingw32/lib/libkernel32.a"),
    ]
    kernel32 = next((path.resolve() for path in candidates if path and path.is_file()), None)
    if kernel32 is None:
        raise SystemExit("MinGW import libraries required; provide --kernel32")
    candidates = [args.mingw_include] if args.mingw_include else [
        kernel32.parent.parent / "include", Path("/usr/x86_64-w64-mingw32/include")]
    headers = next((path for path in candidates if path and (path / "winsock2.h").is_file()), None)
    if headers is None:
        raise SystemExit("MinGW headers required; provide --mingw-include")
    compiler = str(args.compiler.resolve())
    records = []
    with tempfile.TemporaryDirectory(prefix="dodo-web-windows-") as directory:
        scratch = Path(directory)
        prefix = scratch / "wine"
        prefix.mkdir()
        env = dict(os.environ, WINEPREFIX=str(prefix), WINEARCH="win64", WINEDEBUG="-all")
        env.pop("DISPLAY", None)
        subprocess.run([wineserver, "-p"], env=env, check=True, timeout=20)
        try:
            stack = scratch / "chkstk.obj"
            run([clang, "--target=x86_64-pc-windows-msvc", "-c",
                 str(ROOT / "tests/support/windows_chkstk.S"), "-o", str(stack)])
            alias = scratch / "alias.S"
            alias.write_text(".text\n.globl ___chkstk_ms\n___chkstk_ms:\n jmp __chkstk\n")
            alias_obj = scratch / "alias.obj"
            run([clang, "--target=x86_64-pc-windows-msvc", "-c", str(alias), "-o", str(alias_obj)])
            cache = {}

            def build(name, optimization, port):
                source = scratch / f"{name}-{optimization}.dodo"
                source.write_text((ROOT / "examples" / f"{name}.dodo").read_text().replace("8080", str(port)).replace("config.max_connections = 0", "config.max_connections = 1"))
                package = re.search(r"^package\s+(\w+)", source.read_text()).group(1)
                startup = scratch / f"startup-{name}.c"
                startup.write_text(RUNTIME.replace("PACKAGE", package))
                startup_obj = startup.with_suffix(".obj")
                run([clang, "--target=x86_64-pc-windows-msvc", "-ffreestanding", "-fno-builtin",
                     "-fno-stack-protector", "-O2", "-c", str(startup), "-o", str(startup_obj)])
                obj = source.with_suffix(".obj")
                run([compiler, "build", str(source), "--emit", "obj", "--target",
                     "x86_64-pc-windows-msvc", "-O", str(optimization), "-o", str(obj)])
                native = []
                for boundary in native_sources(source):
                    key = boundary, optimization
                    if key not in cache:
                        native_obj = scratch / f"{boundary.parent.name}-native-{optimization}.obj"
                        run([clang, "--target=x86_64-w64-windows-gnu", "-std=c11", f"-O{optimization}",
                             "-fno-stack-protector", "-Wall", "-Wextra", "-Werror", "-isystem", str(headers),
                             "-c", str(boundary), "-o", str(native_obj)])
                        cache[key] = native_obj
                    native.append(str(cache[key]))
                exe = obj.with_suffix(".exe")
                run([linker, "/nologo", "/nodefaultlib", "/entry:mainCRTStartup", "/subsystem:console",
                     "/machine:x64", "/stack:8388608", f"/out:{exe}", str(startup_obj), str(stack),
                     str(alias_obj), str(obj), *native, str(kernel32),
                     str(kernel32.parent / "libmsvcrt.a"), str(kernel32.parent / "libws2_32.a")])
                if exe.read_bytes()[:2] != b"MZ":
                    raise RuntimeError("Expected Windows PE executable")
                return exe

            for optimization in (0, 3):
                port = available_port()
                server = build("web_server", optimization, port)
                with tempfile.TemporaryFile(mode="w+") as log:
                    process = subprocess.Popen([wine, str(server)], cwd=scratch, env=env, stdout=log, stderr=log)
                    try:
                        deadline = time.monotonic() + 45
                        connection = None
                        while connection is None:
                            if process.poll() is not None:
                                raise RuntimeError(f"Wine web server exited {process.returncode} before accepting")
                            try:
                                connection = socket.create_connection(("127.0.0.1", port), timeout=1)
                            except OSError:
                                if time.monotonic() >= deadline:
                                    raise RuntimeError("Wine server did not listen within 45 seconds")
                                time.sleep(0.05)
                        with connection:
                            connection.settimeout(10)
                            # A fragmented chunked request exercises streaming request consumption.
                            request = (b"GET / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n"
                                       b"Connection: close\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n")
                            for offset in range(0, len(request), 7):
                                connection.sendall(request[offset:offset + 7])
                            response = http.client.HTTPResponse(connection)
                            response.begin()
                            if response.status != 200 or response.read() != b"Hello, Dodo!\n":
                                raise RuntimeError("Unexpected Wine HTTP server response")
                        if process.wait(timeout=15) != 0:
                            raise RuntimeError(f"Wine web server exited {process.returncode}")
                    except Exception:
                        process.kill()
                        process.wait(timeout=10)
                        log.seek(0)
                        print(log.read())
                        raise
                records.append({"fixture": "web_server.dodo", "optimization": optimization,
                                "peer": "Python HTTP/1.1 client", "exit_code": 0})
                print(f"PASS Wine web_server.dodo -O{optimization} / Python client", flush=True)
                with socket.socket() as listener:
                    listener.bind(("127.0.0.1", 0))
                    listener.listen(1)
                    listener.settimeout(15)
                    port = listener.getsockname()[1]
                    client = build("http_client", optimization, port)
                    errors = []

                    def respond():
                        try:
                            connection, _ = listener.accept()
                            with connection:
                                connection.settimeout(10)
                                request = b""
                                while b"\r\n\r\n" not in request:
                                    part = connection.recv(1024)
                                    if not part:
                                        raise RuntimeError("Client disconnected before request headers")
                                    request += part
                                    if len(request) > 8192:
                                        raise RuntimeError("Client request exceeded test bound")
                                if not request.startswith(b"GET / HTTP/1.1\r\n"):
                                    raise RuntimeError(f"Unexpected request: {request!r}")
                                response = (b"HTTP/1.1 103 Early Hints\r\n\r\nHTTP/1.1 200 OK\r\n"
                                            b"Transfer-Encoding: chunked\r\nTrailer: X-Finished\r\nConnection: close\r\n\r\n"
                                            b"7\r\nHello, \r\n6\r\nDodo!\n\r\n0\r\nX-Finished: yes\r\n\r\n")
                                for offset in range(0, len(response), 3):
                                    connection.sendall(response[offset:offset + 3])
                                    time.sleep(0.001)
                                connection.shutdown(socket.SHUT_WR)
                        except Exception as error:
                            errors.append(error)

                    peer = threading.Thread(target=respond, daemon=True)
                    peer.start()
                    run([wine, str(client)], env=env, cwd=scratch, timeout=30)
                    peer.join(timeout=20)
                    if peer.is_alive() or errors:
                        raise RuntimeError(f"Python server failed: {errors}")
                records.append({"fixture": "http_client.dodo", "optimization": optimization,
                                "peer": "Python informational/chunked/trailers server", "exit_code": 0})
                print(f"PASS Wine http_client.dodo -O{optimization} / Python server", flush=True)
        finally:
            subprocess.run([wineserver, "-k"], env=env, capture_output=True, timeout=20)
            subprocess.run([wineserver, "-w"], env=env, capture_output=True, timeout=20)
    if args.report:
        args.report.write_text(json.dumps({"executions": len(records), "fixtures": records}, indent=2) + "\n")
    print(f"Passed {len(records)} Windows HTTP example interoperability executions.")


if __name__ == "__main__":
    main()
