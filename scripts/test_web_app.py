#!/usr/bin/env python3
"""Exercise the application object, bounded storage, and shutdown at O0/O3."""
import argparse
from pathlib import Path
import socket
import selectors
import subprocess
import tempfile

from test_http_web import ROOT, connect, port, run
from test_web_reactor import request, response


def cases():
    yield request(), 200, b"<h1>first</h1>", "text/html; charset=utf-8"
    yield request(path="/hello/Dodo"), 200, b"Dodo", "text/plain; charset=utf-8"
    yield request(path="/hello/A%2520B"), 200, b"A%20B", "text/plain; charset=utf-8"
    yield request(), 200, b"<h1>again</h1>", "text/html; charset=utf-8"
    yield request(path="/missing"), 404, b"", None
    yield request(method="PUT"), 405, b"", None
    yield request("POST", "/echo", b"Content-Length: 16\r\n", b"a" * 16), 200, b"a" * 16, None
    yield request("POST", "/echo", b"Content-Length: 17\r\n"), 413, b"", None
    yield request(path="/large"), 500, b"", None
    yield request(path="/headers"), 500, b"", None
    yield request(extra=b"X-Big: " + b"a" * 240 + b"\r\n"), 431, b"", None
    yield request(extra=b"A: a\r\nB: b\r\nC: c\r\n"), 431, b"", None
    yield request(path="/hello/" + "a" * 30), 414, b"", None
    yield request(path="/hello/a?x=1&y=2"), 414, b"", None
    yield request(path="/hello/a?x=" + "a" * 16), 414, b"", None
    yield b"GET / HTTP/1.1\r\nHost: ", 408, b"", None
    yield request("POST", "/echo", b"Content-Length: 4\r\n", b"a"), 408, b"", None


def build(compiler, scratch, optimization, policy, custom, stopping=False):
    number = port()
    exchanges = list(cases())
    source = (ROOT / "tests/http_hosted/application.dodo").read_text()
    values = dict(PORT=number, POLICY=policy, COUNT=(1 if policy == "Serial" else 2) if stopping else len(exchanges),
                  STOP_MS=1500 if stopping else 30000, CANCELLED=str(stopping).lower(),
                  COMPLETED=(0 if policy == "Serial" else 1) if stopping else 5,
                  REJECTED=0 if stopping else len(exchanges) - 5)
    for key, value in values.items():
        source = source.replace(f"@@{key}@@", str(value))
    if stopping:
        source = source.replace("timeouts.header_ms = 500", "timeouts.header_ms = 10000")
    if custom:
        slots = 2 if policy == "Concurrent" else 1
        storage = f"""workspace := [0u8; hosted.WORKSPACE_BYTES * {slots}]
    input := [0u8; 16 * {slots}]
    output := [0u8; 32 * {slots}]
"""
        if policy == "Concurrent":
            storage += "    slots := [reactor.Slot.new(), reactor.Slot.new()]\n    storage := app.Storage.concurrent(&mut workspace, &mut input, &mut output, &mut slots)"
        else:
            storage += "    storage := app.Storage.new(&mut workspace, &mut input, &mut output)"
        source = source.replace("// @@STORAGE@@", storage)
        source = source.replace(f'server.serve_until(b"127.0.0.1:{number}", &cancel)',
                                f'server.serve_with(b"127.0.0.1:{number}", &mut storage, &mut clock, &cancel)')
        # Supplied capacities independently bound larger configured body limits.
        source = source.replace("limits.body_bytes = 16", "limits.body_bytes = 100")
        source = source.replace("limits.response_bytes = 32", "limits.response_bytes = 100")
    path = scratch / f"app-{optimization}-{policy}-{custom}-{stopping}.dodo"
    path.write_text(source)
    binary = path.with_suffix("")
    run([compiler, "build", str(path), "-O", str(optimization), "-o", str(binary)])
    return binary, number, exchanges


def returned(process):
    with selectors.DefaultSelector() as ready:
        ready.register(process.stdout, selectors.EVENT_READ)
        assert ready.select(5), "serving did not return"
    assert process.stdout.readline() == b"stopped\n", "server failed before returning"
    assert process.poll() is None, "shutdown must be checked while the process is alive"


def check_server(binary, number, exchanges, policy, stopping):
    process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        if stopping:
            with connect(number, process) as pending:
                pending.sendall(b"GET / HTTP/1.1\r\nHost: ")
                if policy == "Concurrent":
                    with connect(number, process) as idle:
                        idle.sendall(request(close=False))
                        assert response(idle)[0] == 200
                        returned(process)
                        assert idle.recv(1) == b""
                else:
                    returned(process)
                assert pending.recv(1) == b""
        else:
            for wire, code, body, content_type in exchanges:
                with connect(number, process) as peer:
                    peer.sendall(wire)
                    actual, headers, data = response(peer)
                    assert (actual, data) == (code, body), (wire, actual, data)
                    if content_type:
                        assert headers["Content-Type"] == content_type
            returned(process)
        # The program is blocked on stdin after serving returned: OS process
        # cleanup cannot mask a leaked listener or accepted socket.
        try:
            with socket.create_connection(("127.0.0.1", number), timeout=.2):
                raise AssertionError("listener survived shutdown")
        except ConnectionRefusedError:
            pass
        stdout, stderr = process.communicate(input=b"x", timeout=5)
        assert process.returncode == 0, (binary, process.returncode, stdout, stderr)
    finally:
        if process.poll() is None:
            process.kill()
        process.communicate(timeout=5)


def check_example(compiler, scratch, optimization):
    number = port()
    source = (ROOT / "examples/web_routes.dodo").read_text().replace(":8080", f":{number}")
    source = source.replace("server := app.Server.new(routes)", "server := app.Server.new(routes)\n    server.config.max_connections = 4")
    path = scratch / f"routes-{optimization}.dodo"
    path.write_text(source)
    binary = path.with_suffix("")
    run([compiler, "build", str(path), "-O", str(optimization), "-o", str(binary)])
    process = subprocess.Popen([str(binary)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        for wire, expected in [
            (request(), (200, b'<!doctype html><h1>Dodo</h1><a href="/hello/Dodo">Say hello</a>')),
            (request(path="/hello/Ada"), (200, b"Ada")),
            (request("POST", "/echo", b"Content-Length: 3\r\n", b"abc"), (200, b"abc")),
            (request(path="/missing"), (404, b"")),
        ]:
            with connect(number, process) as peer:
                peer.sendall(wire)
                code, headers, body = response(peer)
                assert (code, body) == expected
                if wire == request():
                    assert headers["Content-Type"] == "text/html; charset=utf-8"
        stdout, stderr = process.communicate(timeout=5)
        assert process.returncode == 0, (stdout, stderr)
    finally:
        if process.poll() is None:
            process.kill()
        process.communicate(timeout=5)
    print(f"PASS multi-route example -O{optimization}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    args = parser.parse_args()
    compiler = str(args.compiler.resolve())
    with tempfile.TemporaryDirectory(prefix="dodo-web-app-") as directory:
        scratch = Path(directory)
        for optimization in (0, 3):
            check_example(compiler, scratch, optimization)
            for policy in ("Serial", "Concurrent"):
                for custom in (False, True):
                    for stopping in (False, True):
                        binary, number, exchanges = build(compiler, scratch, optimization, policy, custom, stopping)
                        check_server(binary, number, exchanges, policy, stopping)
                        print(f"PASS app -O{optimization} {policy} custom={custom} shutdown={stopping}", flush=True)


if __name__ == "__main__":
    main()
