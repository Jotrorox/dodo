#!/usr/bin/env python3
"""Check std/web servers against independent Python HTTP/TLS peers at O0 and O3.

Run one suite per invocation, e.g. `python3 scripts/test_web.py reactor`.
"""
import argparse
import concurrent.futures
import contextlib
import http.client
import json
import os
from pathlib import Path
import queue
import socket
import ssl
import subprocess
import tempfile
import threading
import time

from test_hosted_http import credentials

ROOT = Path(__file__).resolve().parents[1]
HELLO = b"Hello, Dodo!\n"
HTML = {"Content-Type": "text/html; charset=utf-8"}
TEXT = {"Content-Type": "text/plain; charset=utf-8"}
RECORDS = []


def run(command, **kwargs):
    command = [str(part) for part in command]
    result = subprocess.run(command, capture_output=True, timeout=120, **kwargs)
    if result.returncode:
        raise RuntimeError(f"{command}: {result.returncode}\n{result.stdout!r}\n{result.stderr!r}")


def build(compiler, path, source, optimization, *extra):
    path.write_text(source)
    binary = path.with_suffix("")
    run([compiler, "build", path, "-O", optimization, "-o", binary, *extra])
    return binary


def fill(template, **values):
    for key, value in values.items():
        template = template.replace(f"@@{key}@@", str(value))
    return template


def passed(case, optimization, prefix=""):
    RECORDS.append({"case": case, "optimization": optimization})
    print(f"PASS {prefix}{case} -O{optimization}", flush=True)


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def connect(number, process):
    until = time.monotonic() + 10
    while True:
        try:
            # A refused Windows loopback connect can consume a full second,
            # longer than the cancellation fixture's lifetime. Retry promptly.
            connection = socket.create_connection(("127.0.0.1", number), timeout=.1)
            connection.settimeout(5)
            return connection
        except (ConnectionRefusedError, TimeoutError, socket.timeout):  # Distinct before Python 3.10.
            if process.poll() is not None:
                raise RuntimeError(f"Server exited early: {process.returncode}") from None
            if time.monotonic() >= until:
                raise RuntimeError("Server did not listen") from None
            time.sleep(.01)


def request(method="GET", path="/", extra=b"", body=b"", close=True):
    return (f"{method} {path} HTTP/1.1\r\nHost: localhost\r\n".encode()
            + (b"Connection: close\r\n" if close else b"") + extra + b"\r\n" + body)


def response(sock, method="GET"):
    decoded = http.client.HTTPResponse(sock, method=method)
    decoded.begin()
    result = decoded.status, dict(decoded.getheaders()), decoded.read()
    decoded.close()
    return result


@contextlib.contextmanager
def running(binary, **kwargs):
    process = subprocess.Popen([str(binary)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs)
    try:
        yield process
    finally:
        if process.poll() is None:
            process.kill()
        process.communicate(timeout=5)


def finished(process, expected=0, timeout=5, data=None):
    stdout, stderr = process.communicate(input=data, timeout=timeout)
    assert process.returncode == expected, (process.returncode, stdout, stderr)
    return stderr


def serve_cases(number, process, exchanges, connector=connect):
    for wire, status, body, headers in exchanges:
        with connector(number, process) as peer:
            peer.sendall(wire)
            actual, fields, data = response(peer)
            assert (actual, data) == (status, body), (wire, actual, data)
            for key, value in headers.items():
                assert fields.get(key) == value, (key, fields)


# http: the runnable examples and HTTP interoperability on loopback.

HTTP_CASES = {
    "python_get": (request(close=False), 200, HELLO),
    "python_head": (request("HEAD", close=False), 200, b""),
    "missing": (request(path="/absent", close=False), 404, b""),
    "chunked": (request(extra=b"Transfer-Encoding: chunked\r\n", body=b"3\r\nabc\r\n0\r\nX-Checksum: yes\r\n\r\n", close=False), 200, HELLO),
    "malformed": (request(extra=b"Content-Length: 1\r\nContent-Length: 2\r\n", body=b"xx", close=False), 400, b""),
    "disconnect": (b"GET / HTTP/1.1\r\nHost:", 400, b""),
}


def example(name, number):
    source = (ROOT / f"examples/{name}.dodo").read_text().replace(":8080", f":{number}")
    return source.replace("config.max_connections = 0", "config.max_connections = 1").replace(".run(b", ".max_connections(1).run(b")


def http_suite(compiler, scratch, optimization):
    number = port()
    binary = {name: build(compiler, scratch / f"{name}-{optimization}.dodo", example(name, number), optimization)
              for name in ("http_client", "web_server", "http_memory")}
    run([binary["http_memory"]])
    passed("memory", optimization, "HTTP/web ")
    for case in ("dodo_client", *HTTP_CASES):
        if case != "dodo_client":
            number = port()
            build(compiler, scratch / f"web_server-{optimization}.dodo", example("web_server", number), optimization)
        with running(binary["web_server"]) as process:
            if case == "dodo_client":
                # No probe connection: the example admits exactly one.
                until = time.monotonic() + 5
                while True:
                    result = subprocess.run([str(binary["http_client"])], capture_output=True, timeout=10)
                    if result.returncode == 0:
                        break
                    if process.poll() is not None or time.monotonic() >= until:
                        raise RuntimeError(f"Dodo client failed: {result.returncode}")
                    time.sleep(.01)
            else:
                wire, status, body = HTTP_CASES[case]
                with connect(number, process) as stream:
                    for piece in ([bytes([value]) for value in wire] if case == "chunked" else [wire]):
                        stream.sendall(piece)
                    if case == "disconnect":
                        stream.shutdown(socket.SHUT_WR)
                    actual = response(stream, "HEAD" if case == "python_head" else "GET")
                    assert (actual[0], actual[2]) == (status, body), (case, actual)
            finished(process, timeout=10)
        passed(case, optimization, "HTTP/web ")


# reactor: bounded concurrent HTTP/1.1 serving with Python's independent decoder.

def checks(number, process, connector=connect):
    completed = []

    def exchange(data, method="GET"):
        with connector(number, process) as sock:
            sock.sendall(data)
            result = response(sock, method)
            assert sock.recv(1) == b""
            return result

    for method, path, expected, body in [
        ("GET", "/", 200, HELLO), ("HEAD", "/", 200, b""),
        ("GET", "/absent", 404, b""), ("PUT", "/", 405, b""),
        ("GET", "/fail", 500, b""), ("GET", "/empty", 204, b""),
        ("GET", "/not-modified", 304, b""),
        ("GET", "/%2e%2e/", 400, b""),
    ]:
        status, _, actual = exchange(request(method, path), method)
        assert (status, actual) == (expected, body)
        completed.append(f"{method} {path}")

    with connector(number, process) as sock:
        for i in range(3):
            sock.sendall(request(close=False))
            status, headers, body = response(sock)
            assert (status, body) == (200, HELLO)
            assert headers["Connection"] == ("close" if i == 2 else "keep-alive")
        assert sock.recv(1) == b""
    completed.append("keep-alive request limit")

    with connector(number, process) as sock:
        sock.sendall(request(path="/large", close=False))
        status, headers, body = response(sock)
        assert (status, body) == (200, b"x" * 65536)
        assert int(headers["Content-Length"]) == 65536
        sock.sendall(request())
        assert response(sock)[2] == HELLO
        assert sock.recv(1) == b""
    completed.append("direct large response followed by another request")

    # Collect the complete pipeline before splitting frames, preserving read-ahead.
    with connector(number, process) as sock:
        sock.sendall(request(close=False) + request("HEAD", close=False) + request(close=False))
        wire = bytearray()
        while part := sock.recv(4096):
            wire += part
        parts = bytes(wire).split(b"HTTP/1.1 ")[1:]
        assert len(parts) == 3
        assert all(part.startswith(b"200 ") for part in parts)
        assert [part.split(b"\r\n\r\n", 1)[1] for part in parts] == [HELLO, b"", HELLO]
        assert b"Connection: close\r\n" in parts[-1]
    completed.append("pipelining and HEAD framing")

    with connector(number, process) as sock:
        bad = request("POST", "/echo", b"Content-Length: 1\r\nContent-Length: 2\r\n", b"xx")
        sock.sendall(request(close=False) + bad)
        wire = bytearray()
        while part := sock.recv(4096):
            wire += part
        parts = bytes(wire).split(b"HTTP/1.1 ")[1:]
        assert len(parts) == 2 and parts[0].startswith(b"200 ") and parts[1].startswith(b"400 ")
    completed.append("malformed pipelined successor gets one final error")

    with connector(number, process) as sock:
        sock.sendall(request(close=False))
        assert response(sock)[2] == HELLO
        assert sock.recv(1) == b""  # bounded idle expiration
    completed.append("idle expiration")

    for phase in ("headers", "body"):
        with connector(number, process) as slow:
            prefix = b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nX-Slow: "
            suffix = b"done\r\n\r\n"
            if phase == "body":
                prefix = request("POST", "/echo", b"Content-Length: 4\r\n", b"a")
                suffix = b"bcd"
            slow.sendall(prefix)
            time.sleep(.02)
            assert exchange(request())[2] == HELLO
            # Success on the unfinished first request proves the second made
            # progress before the first expired, without an RPS/latency threshold.
            slow.sendall(suffix)
            assert response(slow)[0] == 200
            assert slow.recv(1) == b""
        completed.append(f"independent progress during partial {phase}")

    for phase in ("headers", "body"):
        with connector(number, process) as sock:
            prefix = b"GET / HTTP/1.1\r\nHost: "
            if phase == "body":
                prefix = request("POST", "/echo", b"Content-Length: 4\r\n", b"a")
            sock.sendall(prefix)
            assert response(sock)[0] == 408
            assert sock.recv(1) == b""
        completed.append(f"{phase} timeout")

    payload = b"x" * 4000
    assert exchange(request("POST", "/echo", b"Content-Length: 4000\r\n", payload))[2] == payload
    completed.append("buffered body and direct response")
    chunked = request("POST", "/echo", b"Transfer-Encoding: chunked\r\n", b"3\r\nabc\r\n2\r\nde\r\n0\r\nX-Checksum: yes\r\n\r\n")
    assert exchange(chunked)[2] == b"abcde"
    completed.append("chunked request and trailers")

    with connector(number, process) as sock:
        sock.sendall(request("POST", "/echo", b"Content-Length: 4\r\nExpect: 100-continue\r\n"))
        interim = bytearray()
        while not interim.endswith(b"\r\n\r\n"):
            interim += sock.recv(1)
        assert bytes(interim) == b"HTTP/1.1 100 Continue\r\n\r\n"
        sock.sendall(b"abcd")
        assert response(sock)[2] == b"abcd"
        assert sock.recv(1) == b""
    completed.append("100 Continue")

    with connector(number, process) as sock:
        sock.sendall(request("POST", "/echo", b"Transfer-Encoding: chunked\r\nExpect: 100-continue\r\n"))
        interim = bytearray()
        while not interim.endswith(b"\r\n\r\n"):
            interim += sock.recv(1)
        assert bytes(interim) == b"HTTP/1.1 100 Continue\r\n\r\n"
        sock.sendall(b"z\r\n")
        assert response(sock)[0] == 400
        assert sock.recv(1) == b""
    completed.append("malformed body after complete informational response")

    for data, expected in [
        (request("POST", "/echo", b"Content-Length: 5000\r\n"), 413),
        (request(extra=b"X-Large: " + b"a" * 17000 + b"\r\n"), 431),
        (request("POST", "/echo", b"Content-Length: 1\r\nContent-Length: 2\r\n", b"xx"), 400),
        (request("POST", "/echo", b"Transfer-Encoding: chunked\r\n", b"z\r\n"), 400),
    ]:
        with connector(number, process) as sock:
            sock.sendall(data)
            assert response(sock)[0] == expected
            # Closing a rejected request can reset unread client input.
    completed.append("header/body limits and malformed framing")

    def batch(_):
        assert exchange(request())[2] == HELLO
    with concurrent.futures.ThreadPoolExecutor(max_workers=24) as pool:
        list(pool.map(batch, range(120)))
    completed.append("requests beyond active slot capacity")
    assert process.poll() is None
    return completed


def reactor_suite(compiler, scratch, optimization):
    template = (ROOT / "tests/http_hosted/reactor.dodo").read_text()
    # The bounded eight-slot fixture needs an 8 MiB stack on Windows.
    link = ["--link-arg", "-Wl,/stack:8388608"] if os.name == "nt" else []
    for stopping in (False, True):
        number = port()
        source = fill(template, PORT=number).replace("STOP_MS: u64 = 30000", f"STOP_MS: u64 = {500 if stopping else 30000}")
        if stopping:
            source = source.replace("idle_timeout_ms = 100", "idle_timeout_ms = 5000").replace("header_timeout_ms = 300", "header_timeout_ms = 5000")
        binary = build(compiler, scratch / f"reactor-{optimization}-{stopping}.dodo", source, optimization, *link)
        with running(binary) as process:
            if not stopping:
                for name in checks(number, process):
                    passed(name, optimization, "reactor ")
                continue
            with connect(number, process) as sock:
                sock.sendall(request(close=False))
                assert response(sock)[0] == 200
                # Fill another slot with an incomplete request.
                with connect(number, process) as pending:
                    pending.sendall(b"GET / HTTP/1.1\r\nHost: ")
                    finished(process, timeout=3)
                    assert pending.recv(1) == b""
                assert sock.recv(1) == b""
            passed("cancellation closes all slots", optimization, "reactor ")


# request-response: the buffered web API through peers that fragment every request.

def response_cases():
    yield "json", request(path="/json"), 201, b'{"ok":true}', [(b"content-type", b"application/json"), (b"set-cookie", b"a=1"), (b"set-cookie", b"b=2")]
    yield "html", request(path="/html"), 200, b"<h1>Hello</h1>", [(b"content-type", b"text/html; charset=utf-8")]
    yield "bytes", request(path="/bytes"), 200, bytes([0, 255, 128, 13, 10]), [(b"content-type", b"application/octet-stream")]
    for name, target, expected, length in [("head-fallback", "/html", 200, b"14"), ("head-explicit", "/json", 200, b"13")]:
        yield name, request("HEAD", target), expected, b"", [(b"content-length", length)]
    for name, code, length in [("empty", 204, None), ("not-modified", 304, b"7"), ("reset", 205, b"0")]:
        yield name, request(path="/" + name), code, b"", [(b"content-length", length)]
    headers = b"X-Input: first\r\nx-INPUT:  second \t\r\n"
    for name, target, framing, body in [
        ("request-access", "/inspect/a%252Fb?q=one+two&q=%2526", b"Content-Length: 3\r\n", b"abc"),
        ("absolute-chunked", "http://localhost/inspect/a%252Fb?q=one+two&q=%2526", b"Transfer-Encoding: chunked\r\n", b"1\r\na\r\n2\r\nbc\r\n0\r\nX-Trailer: secret\r\n\r\n"),
    ]:
        yield name, request("POST", target, headers + framing, body), 200, b"abc", [
            (b"x-method", b"POST"), (b"x-path", b"/inspect/a%2Fb"), (b"x-id", b"a%2Fb"),
            (b"x-q-first", b"one two"), (b"x-q-second", b"%26"), (b"x-input-second", b"second"), (b"content-type", None)]
    yield "query-rules", request(path="/query?&=empty&&bare&q=a%26b%3Dc%2B%23&"), 200, b"a&b=c+#", []
    yield "utf8-query", request(path="/query?=empty&bare&q=%C3%A9"), 200, "é".encode(), []
    for target in ["/html?q=%", "/html?q=%GG", "/html?q=%00", "/html?q=%FF", "/html?q=%C0%AF", "/html?q=%0D", "/html?q=x#fragment", "/a%2Fb", "/../html", "/%FF"]:
        yield f"malformed-{target.encode()!r}", request(path=target), 400, b"", [(b"x-handled", None)]
    for target in ["/html?" + "q=x&" * 5, "/html?q=" + "x" * 127, "/" + "x" * 64]:
        yield f"target-capacity-{len(target)}", request(path=target), 414, b"", [(b"x-handled", None)]
    yield "request-header-fields", request(path="/html", extra=b"X: a\r\n" * 7), 431, b"", []
    yield "request-header-bytes", request(path="/html", extra=b"X: " + b"x" * 512 + b"\r\n"), 431, b"", []
    yield "request-body-bytes", request(path="/html", extra=b"Content-Length: 129\r\n", body=b"x" * 129), 413, b"", []
    yield "malformed-framing", request(path="/html", extra=b"Content-Length: 1\r\nContent-Length: 2\r\n", body=b"xx"), 400, b"", []
    for name in ["body-full", "fields-full", "headers-full", "framing", "bad-status", "bad-text", "duplicate-type", "bad-header"]:
        yield name, request(path="/" + name), 500, b"", [(b"x-handled", None)]


def request_response_suite(compiler, scratch, optimization):
    exchanges = list(response_cases())
    for runner in ("serial", "reactor"):
        number = port()
        source = fill((ROOT / "tests/http_hosted/request_response.dodo").read_text(), PORT=number)
        source = source.replace("config.max_connections = 1 // @@COUNT@@", f"config.max_connections = {len(exchanges)}")
        if runner == "reactor":
            source = source.replace("// @@SERVE@@", "slots := [reactor.Slot.new()]\n    concurrent := reactor.Config.defaults()\n    concurrent.server = config")
            source = source.replace('match hosted.serve(', 'match reactor.serve(').replace('&mut handler, &mut workspace', '&mut handler, &mut slots, &mut workspace').replace('&mut response, config)', '&mut response, concurrent)')
        binary = build(compiler, scratch / f"server-{optimization}-{runner}.dodo", source, optimization)
        with running(binary) as process:
            for name, wire, status, body, fields in exchanges:
                with connect(number, process) as stream:
                    # Deliberately fragment heads, fields, and chunk boundaries.
                    for offset in range(0, len(wire), 7):
                        try:
                            stream.sendall(wire[offset:offset + 7])
                        except (BrokenPipeError, ConnectionResetError):
                            assert status >= 400, name
                            break
                    output = b""
                    while True:
                        try:
                            chunk = stream.recv(8192)
                        except ConnectionResetError:
                            assert status >= 400, name
                            # A rejected start line can leave unread request
                            # bytes; still require a complete error response below.
                            break
                        if not chunk:
                            break
                        output += chunk
                assert b"\r\n\r\n" in output, (runner, name, output)
                head, actual = output.split(b"\r\n\r\n", 1)
                lines = head.split(b"\r\n")
                assert int(lines[0].split()[1]) == status, (runner, name, output)
                assert actual == body, (runner, name, actual, body)
                parsed = [tuple(part.strip() for part in line.split(b":", 1)) for line in lines[1:]]
                parsed = [(key.lower(), value) for key, value in parsed]
                for key, value in fields:
                    if value is None:
                        assert not any(k == key for k, _ in parsed), (name, key, parsed)
                    else:
                        assert (key, value) in parsed, (name, key, value, parsed)
                if name == "json":
                    assert [value for key, value in parsed if key == b"set-cookie"] == [b"a=1", b"b=2"]
                passed(f"{runner} {name}", optimization)
            finished(process, timeout=10)


# app: the application object, bounded storage, and shutdown.

def app_cases():
    yield request(), 200, b"<h1>first</h1>", HTML
    yield request(path="/hello/Dodo"), 200, b"Dodo", TEXT
    yield request(path="/hello/A%2520B"), 200, b"A%20B", TEXT
    yield request(), 200, b"<h1>again</h1>", HTML
    yield request(path="/missing"), 404, b"", {}
    yield request(method="PUT"), 405, b"", {}
    yield request("POST", "/echo", b"Content-Length: 16\r\n", b"a" * 16), 200, b"a" * 16, {}
    yield request("POST", "/echo", b"Content-Length: 17\r\n"), 413, b"", {}
    yield request(path="/large"), 500, b"", {}
    yield request(path="/headers"), 500, b"", {}
    yield request(extra=b"X-Big: " + b"a" * 240 + b"\r\n"), 431, b"", {}
    yield request(extra=b"A: a\r\nB: b\r\nC: c\r\n"), 431, b"", {}
    yield request(path="/hello/" + "a" * 30), 414, b"", {}
    yield request(path="/hello/a?x=1&y=2"), 414, b"", {}
    yield request(path="/hello/a?x=" + "a" * 16), 414, b"", {}
    yield b"GET / HTTP/1.1\r\nHost: ", 408, b"", {}
    yield request("POST", "/echo", b"Content-Length: 4\r\n", b"a"), 408, b"", {}


def app_build(compiler, scratch, optimization, policy, custom, stopping):
    number = port()
    exchanges = list(app_cases())
    source = fill((ROOT / "tests/http_hosted/application.dodo").read_text(), PORT=number, POLICY=policy,
                  COUNT=(1 if policy == "Serial" else 2) if stopping else len(exchanges),
                  STOP_MS=1500 if stopping else 30000, CANCELLED=str(stopping).lower(),
                  COMPLETED=(0 if policy == "Serial" else 1) if stopping else 5,
                  REJECTED=0 if stopping else len(exchanges) - 5)
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
    return build(compiler, path, source, optimization), number, exchanges


def returned(process):
    # Windows select() accepts sockets only, so wait for the pipe on a reader
    # thread. Keep the timeout and the check that serving returns before exit.
    lines = queue.Queue()
    threading.Thread(target=lambda: lines.put(process.stdout.readline()), daemon=True).start()
    try:
        line = lines.get(timeout=5)
    except queue.Empty:
        raise AssertionError("serving did not return") from None
    assert line == b"stopped\n", "server failed before returning"
    assert process.poll() is None, "shutdown must be checked while the process is alive"


def app_check(binary, number, exchanges, policy, stopping):
    with running(binary, stdin=subprocess.PIPE) as process:
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
            serve_cases(number, process, exchanges)
            returned(process)
        # The program is blocked on stdin after serving returned: OS process
        # cleanup cannot mask a leaked listener or accepted socket.
        try:
            # Windows can take about a second to report a refused connection.
            with socket.create_connection(("127.0.0.1", number), timeout=3):
                raise AssertionError("listener survived shutdown")
        except ConnectionRefusedError:
            pass
        finished(process, data=b"x")


def app_suite(compiler, scratch, optimization):
    number = port()
    source = (ROOT / "examples/web_routes.dodo").read_text().replace(":8080", f":{number}")
    source = source.replace(".run(b", ".max_connections(4).run(b")
    binary = build(compiler, scratch / f"routes-{optimization}.dodo", source, optimization)
    with running(binary) as process:
        serve_cases(number, process, [
            (request(), 200, b'<!doctype html><h1>Dodo</h1><a href="/hello/Dodo">Say hello</a>', HTML),
            (request(path="/hello/Ada"), 200, b"Ada", {}),
            (request("POST", "/echo", b"Content-Length: 3\r\n", b"abc"), 200, b"abc", {}),
            (request(path="/missing"), 404, b"", {}),
        ])
        finished(process)
    passed("multi-route example", optimization)
    for policy in ("Serial", "Concurrent"):
        for custom in (False, True):
            for stopping in (False, True):
                app_check(*app_build(compiler, scratch, optimization, policy, custom, stopping), policy, stopping)
                passed(f"app {policy} custom={custom} shutdown={stopping}", optimization)


# developer: fluent applications and explicit HTTP rejections with real peers.

def developer_cases():
    yield request(), 200, b"<h1>Home</h1>", HTML
    yield request(path="/numbers/42"), 200, b'{"id":42}', {"Content-Type": "application/json"}
    yield request(path="/numbers/42?limit=10&limit=invalid"), 200, b'{"id":42}', {}
    yield request(path="/numbers/42?limit="), 400, b"", {}
    yield request(path="/numbers/42?limit=-1"), 400, b"", {}
    yield request(path="/numbers/42?limit=9"), 422, b"", {}
    yield request(path="/numbers/18446744073709551616"), 400, b"", {}
    yield request(path="/numbers/nope"), 400, b"", {}
    yield request(path="/numbers/43"), 404, b"", {}
    yield request(path="/old"), 303, b"", {"Location": "/"}
    yield request("POST", "/echo?q=a%2Bb", b"Authorization: test\r\nContent-Length: 3\r\n", b"abc"), 200, b"abc", {"X-Query": "a+b"}
    yield request("POST", "/echo?q=ok"), 401, b"", {}
    yield request("POST", "/echo", b"Authorization: test\r\n"), 400, b"", {}
    yield request(path="/missing"), 404, b"", {}
    yield request("PUT", "/"), 405, b"", {}


def developer_suite(compiler, scratch, optimization):
    binary = build(compiler, scratch / f"startup-{optimization}.dodo", '''package startup
import "std/web"
import "std/web/app"
fn main() -> i32 {
    return app.new().get(b"/:first", web.text(b"one"))
        .get(b"/:second", web.text(b"two"))
        .run(b"invalid address")
}
''', optimization)
    result = subprocess.run([str(binary)], capture_output=True, timeout=5)
    assert result.returncode == 1, result
    assert b"web route #2 (GET /:second): web: AmbiguousRoute" in result.stderr, result.stderr
    assert b"already registered" in result.stderr, result.stderr
    passed("startup diagnostics", optimization)
    for policy in ("Serial", "Concurrent"):
        number = port()
        exchanges = list(developer_cases())
        source = fill((ROOT / "tests/http_hosted/developer.dodo").read_text(), PORT=number, COUNT=len(exchanges), POLICY=policy)
        binary = build(compiler, scratch / f"developer-{optimization}-{policy}.dodo", source, optimization)
        with running(binary) as process:
            serve_cases(number, process, exchanges)
            finished(process)
        passed(f"fluent app {policy}", optimization)


# https: serial/concurrent HTTPS, bounded reuse, credentials, and cancellation.

def encrypted(context, **options):
    def connector(number, process):
        raw = connect(number, process)
        try:
            peer = context.wrap_socket(raw, server_hostname="localhost", **options)
            assert peer.selected_alpn_protocol() == "http/1.1"
            return peer
        except BaseException:
            raw.close()
            raise
    return connector


def https_startup(compiler, work, optimization):
    source = (ROOT / "examples/https_server.dodo").read_text().replace("127.0.0.1:8443", "invalid address").replace(".concurrent()", "")
    binary = build(compiler, work / f"startup-{optimization}.dodo", source, optimization)
    scratch = work / f"credentials-{optimization}"
    scratch.mkdir()
    certificate = (work / "cert.pem").read_bytes()
    key = (work / "key.pem").read_bytes()

    def rejected(expected):
        result = subprocess.run([str(binary)], cwd=scratch, capture_output=True, timeout=5)
        assert result.returncode == 1, result
        assert expected in result.stderr, result.stderr

    rejected(b"cannot read credential file: cert.pem; platform/error: NotFound")
    (scratch / "cert.pem").write_bytes(certificate)
    rejected(b"cannot read credential file: key.pem; platform/error: NotFound")
    (scratch / "key.pem").write_bytes(b"x" * 8193)
    rejected(b"credential file exceeds 8192 bytes: key.pem")
    (scratch / "key.pem").write_bytes(key)
    (scratch / "cert.pem").write_bytes(b"x" * 8193)
    rejected(b"credential file exceeds 8192 bytes: cert.pem")
    for invalid in (b"", b"not a certificate"):
        (scratch / "cert.pem").write_bytes(invalid)
        rejected(b"http hosted: Tls")
    (scratch / "cert.pem").write_bytes(certificate)
    (scratch / "key.pem").write_bytes(b"not a private key")
    rejected(b"http hosted: Tls")
    (scratch / "key.pem").write_bytes((work / "ca.key").read_bytes())
    rejected(b"http hosted: Tls")

    # Exact-capacity credentials pass the EOF probe and reach address validation.
    (scratch / "cert.pem").write_bytes(certificate.ljust(8192, b"\n"))
    (scratch / "key.pem").write_bytes(key.ljust(8192, b"\n"))
    rejected(b"http hosted: InvalidInput")

    concurrent = source.replace(".run_with(", ".concurrent().run_with(")
    binary = build(compiler, work / f"concurrent-{optimization}.dodo", concurrent, optimization)
    rejected(b"http hosted: InvalidInput")
    duplicate = source.replace('.get(b"/",', '.get(b"/", web.text(b"duplicate")).get(b"/",')
    binary = build(compiler, work / f"duplicate-{optimization}.dodo", duplicate, optimization)
    # Registration errors win even when credential files are missing.
    (scratch / "cert.pem").unlink()
    rejected(b"web route #2 (GET /): web: AmbiguousRoute")
    passed("HTTPS startup diagnostics and credential bounds", optimization)


def https_serving(compiler, work, optimization, context, policy):
    number = port()
    exchanges = list(developer_cases())
    source = (ROOT / "tests/http_hosted/developer.dodo").read_text()
    source = source.replace('import "std/web/app"', 'import "std/web/app"\nimport "std/web/https"')
    source = source.replace('builder.run(b"127.0.0.1:@@PORT@@")',
                            'builder.run_with(b"127.0.0.1:@@PORT@@", https.files("cert.pem", "key.pem"))')
    # Exercise all eight registration slots alongside middleware and TLS.
    extra = ""
    for index in range(4):
        extra += f'.get(b"/extra/{index}", web.text(b"extra"))\n        '
        exchanges.append((request(path=f"/extra/{index}"), 200, b"extra", {}))
    source = fill(source.replace(".max_connections(", extra + ".max_connections("), PORT=number, COUNT=len(exchanges), POLICY=policy)
    binary = build(compiler, work / f"routes-{policy}-{optimization}.dodo", source, optimization)
    with running(binary, cwd=work) as process:
        serve_cases(number, process, exchanges, encrypted(context))
        finished(process)
    passed(f"fluent {policy} HTTPS routes, middleware, and rejections", optimization)


def https_connection_failure(compiler, work, optimization, concurrent):
    number = port()
    source = (ROOT / "examples/https_server.dodo").read_text().replace(":8443", f":{number}").replace(".concurrent()", "")
    source = source.replace(".run_with(", ".max_connections(1).run_with(")
    if concurrent:
        source = source.replace(".run_with(", ".concurrent().run_with(")
    binary = build(compiler, work / f"failed-peer-{concurrent}-{optimization}.dodo", source, optimization)
    with running(binary, cwd=work) as process:
        # A plaintext peer fails its TLS handshake and is reported as exit 2.
        with connect(number, process) as peer:
            peer.sendall(request())
        stderr = finished(process, 2)
        assert b"http hosted: Tls" in stderr, stderr
    passed("HTTPS connection failure reporting", optimization)


def https_concurrent_serving(compiler, work, optimization, context):
    tls = encrypted(context, suppress_ragged_eofs=False)
    template = (ROOT / "tests/http_hosted/https_reactor.dodo").read_text()
    # A one-event turn stresses all suspend/resume boundaries and buffered TLS
    # records; the regular budget also exercises multiple events per resume.
    for events in (1, 16):
        number = port()
        binary = build(compiler, work / f"reactor-{events}-{optimization}.dodo", fill(template, PORT=number, EVENTS=events), optimization)
        with running(binary, cwd=work) as process:
            for name in checks(number, process, tls):
                passed(f"HTTPS reactor {name}, events={events}", optimization)
            # An accepted socket may delay its ClientHello without blocking a
            # second handshake or request. Complete both before either expires.
            with connect(number, process) as slow:
                with tls(number, process) as fast:
                    fast.sendall(request())
                    assert response(fast)[2] == HELLO
                    assert fast.recv(1) == b""
                with context.wrap_socket(slow, server_hostname="localhost") as peer:
                    peer.sendall(request())
                    assert response(peer)[0] == 200
            # A partial TLS record times out with no plaintext HTTP response.
            incoming, outgoing = ssl.MemoryBIO(), ssl.MemoryBIO()
            client = context.wrap_bio(incoming, outgoing, server_hostname="localhost")
            try:
                client.do_handshake()
            except ssl.SSLWantReadError:
                pass
            hello = outgoing.read()
            with connect(number, process) as slow:
                slow.sendall(hello[:8])
                with tls(number, process) as fast:
                    fast.sendall(request())
                    assert response(fast)[0] == 200
                assert slow.recv(1) == b""
            passed(f"HTTPS independent and bounded handshakes, events={events}", optimization)

    # Queue enough encrypted output to exceed socket/TLS staging while the
    # first client pauses reading; another connection must still complete.
    number = port()
    source = fill(template, PORT=number, EVENTS=16)
    source = source.replace("requests_per_connection = 3", "requests_per_connection = 100")
    source = source.replace("builder.config.timeouts.write_ms = 1000", "builder.config.timeouts.write_ms = 10000")
    source = source.replace("server := builder.build()!", "builder.config.max_connections = 2\n    server := builder.build()!")
    source = source.replace("assert(report.cancelled)",
                            "assert(!report.cancelled && report.accepted == 2 && report.completed == 101 && report.failed == 0 && report.rejected == 0)")
    binary = build(compiler, work / f"reactor-output-{optimization}.dodo", source, optimization)
    with running(binary, cwd=work) as process:
        with tls(number, process) as slow:
            slow.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 65536)
            slow.sendall(request(path="/large", close=False) * 100)
            time.sleep(.05)
            with tls(number, process) as fast:
                fast.sendall(request())
                assert response(fast)[2] == HELLO
                assert fast.recv(1) == b""
            wire = bytearray()
            while part := slow.recv(65536):
                wire += part
            frames = bytes(wire).split(b"HTTP/1.1 ")[1:]
            assert len(frames) == 100, len(frames)
            assert all(frame.startswith(b"200 ") and frame.split(b"\r\n\r\n", 1)[1] == b"x" * 65536 for frame in frames)
            assert b"Connection: close\r\n" in frames[-1]
        finished(process)
    passed("HTTPS slow reader, ciphertext flushing, and completion counts", optimization)

    # Reuse the same slot for successful dispatches, an HTTP rejection, and a
    # TLS protocol failure. Report counters distinguish requests/connections.
    number = port()
    source = fill(template, PORT=number, EVENTS=1)
    source = source.replace("server := builder.build()!", "builder.config.max_connections = 3\n    server := builder.build()!")
    source = source.replace("assert(report.cancelled)",
                            "assert(!report.cancelled && report.accepted == 3 && report.completed == 3 && report.rejected == 1 && report.failed == 1)")
    binary = build(compiler, work / f"reactor-report-{optimization}.dodo", source, optimization)
    with running(binary, cwd=work) as process:
        with tls(number, process) as peer:
            for _ in range(3):
                peer.sendall(request(close=False))
                assert response(peer)[0] == 200
            assert peer.recv(1) == b""
        with tls(number, process) as peer:
            peer.sendall(request(path="/absent"))
            assert response(peer)[0] == 404
            assert peer.recv(1) == b""
        with connect(number, process) as peer:
            peer.sendall(request())
            try:
                assert peer.recv(1) == b""
            except ConnectionResetError:
                pass
        finished(process)
    passed("HTTPS connection/request accounting and slot cleanup", optimization)

    number = port()
    source = fill(template, PORT=number, EVENTS=1).replace("STOP_MS: u64 = 30000", "STOP_MS: u64 = 1000")
    source = source.replace("idle_ms = 100", "idle_ms = 5000").replace("header_ms = 300", "header_ms = 5000")
    binary = build(compiler, work / f"reactor-cancel-{optimization}.dodo", source, optimization)
    with running(binary, cwd=work) as process:
        with tls(number, process) as idle:
            idle.sendall(request(close=False))
            assert response(idle)[0] == 200
            with tls(number, process) as pending, connect(number, process) as handshake:
                pending.sendall(b"GET / HTTP/1.1\r\nHost: ")
                finished(process, timeout=3)
                assert handshake.recv(1) == b""
                # Cancellation aborts TLS immediately instead of waiting for
                # close_notify. Python reports the intentional abrupt EOF.
                for peer in (idle, pending):
                    try:
                        assert peer.recv(1) == b""
                    except ssl.SSLEOFError:
                        pass
    passed("HTTPS cancellation closes idle, request, and handshake slots", optimization)


def https_suite(compiler, work, optimization):
    if not (work / "ca.pem").exists():
        credentials(work)
    context = ssl.create_default_context(cafile=str(work / "ca.pem"))
    context.set_alpn_protocols(["http/1.1"])
    https_startup(compiler, work, optimization)
    for policy in ("Serial", "Concurrent"):
        https_serving(compiler, work, optimization, context, policy)
    for concurrent in (False, True):
        https_connection_failure(compiler, work, optimization, concurrent)
    https_concurrent_serving(compiler, work, optimization, context)
    binary = build(compiler, work / f"config-{optimization}.dodo", (ROOT / "tests/http_hosted/https_app.dodo").read_text(), optimization)
    run([binary], cwd=work)
    passed("HTTPS custom storage, config, and cancellation", optimization)


SUITES = {
    "http": (http_suite, "Compile the runnable examples and check HTTP interoperability on loopback."),
    "reactor": (reactor_suite, "Exercise bounded concurrent HTTP/1.1 serving with Python's independent decoder."),
    "request-response": (request_response_suite, "Exercise the buffered web API through independent HTTP peers."),
    "app": (app_suite, "Exercise the application object, bounded storage, and shutdown."),
    "developer": (developer_suite, "Exercise fluent applications and explicit HTTP rejections with real peers."),
    "https": (https_suite, "Check serial/concurrent HTTPS, bounded reuse, credentials, and cancellation."),
}


def main():
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    common.add_argument("--report", type=Path, help="write the passed checks as JSON")
    parser = argparse.ArgumentParser(description=__doc__)
    suites = parser.add_subparsers(dest="suite", required=True)
    for name, (_, description) in SUITES.items():
        suites.add_parser(name, parents=[common], help=description, description=description)
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    with tempfile.TemporaryDirectory(prefix=f"dodo-web-{args.suite}-") as directory:
        for optimization in (0, 3):
            SUITES[args.suite][0](compiler, Path(directory), optimization)
    if args.report:
        args.report.write_text(json.dumps({"checks": len(RECORDS), "records": RECORDS}, indent=2) + "\n")
    print(f"Passed {len(RECORDS)} {args.suite} checks.")


if __name__ == "__main__":
    main()
