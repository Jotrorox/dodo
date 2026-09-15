#!/usr/bin/env python3
"""Exercise bounded concurrent HTTP/1.1 serving with Python's independent decoder."""
import argparse
import concurrent.futures
import contextlib
import http.client
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
HELLO = b"Hello, Dodo!\n"


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def connect(number, process):
    until = time.monotonic() + 5
    while True:
        try:
            return socket.create_connection(("127.0.0.1", number), timeout=3)
        except ConnectionRefusedError:
            if process.poll() is not None or time.monotonic() >= until:
                raise
            time.sleep(.01)


def response(sock, method="GET"):
    decoded = http.client.HTTPResponse(sock, method=method)
    decoded.begin()
    result = decoded.status, dict(decoded.getheaders()), decoded.read()
    decoded.close()
    return result


def request(method="GET", path="/", extra=b"", body=b"", close=True):
    return (f"{method} {path} HTTP/1.1\r\nHost: localhost\r\n".encode()
            + (b"Connection: close\r\n" if close else b"") + extra + b"\r\n" + body)


@contextlib.contextmanager
def running(binary):
    process = subprocess.Popen([str(binary)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        yield process
    finally:
        if process.poll() is None:
            process.terminate()
        process.communicate(timeout=5)


def checks(number, process):
    completed = []

    def exchange(data, method="GET"):
        with connect(number, process) as sock:
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

    with connect(number, process) as sock:
        for i in range(3):
            sock.sendall(request(close=False))
            status, headers, body = response(sock)
            assert (status, body) == (200, HELLO)
            assert headers["Connection"] == ("close" if i == 2 else "keep-alive")
        assert sock.recv(1) == b""
    completed.append("keep-alive request limit")

    with connect(number, process) as sock:
        sock.sendall(request(path="/large", close=False))
        status, headers, body = response(sock)
        assert (status, body) == (200, b"x" * 65536)
        assert int(headers["Content-Length"]) == 65536
        sock.sendall(request())
        assert response(sock)[2] == HELLO
        assert sock.recv(1) == b""
    completed.append("direct large response followed by another request")

    # Collect the complete pipeline before splitting frames, preserving read-ahead.
    with connect(number, process) as sock:
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

    with connect(number, process) as sock:
        bad = request("POST", "/echo", b"Content-Length: 1\r\nContent-Length: 2\r\n", b"xx")
        sock.sendall(request(close=False) + bad)
        wire = bytearray()
        while part := sock.recv(4096):
            wire += part
        parts = bytes(wire).split(b"HTTP/1.1 ")[1:]
        assert len(parts) == 2 and parts[0].startswith(b"200 ") and parts[1].startswith(b"400 ")
    completed.append("malformed pipelined successor gets one final error")

    with connect(number, process) as sock:
        sock.sendall(request(close=False))
        assert response(sock)[2] == HELLO
        assert sock.recv(1) == b""  # bounded idle expiration
    completed.append("idle expiration")

    for phase in ("headers", "body"):
        with connect(number, process) as slow:
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
        with connect(number, process) as sock:
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

    with connect(number, process) as sock:
        sock.sendall(request("POST", "/echo", b"Content-Length: 4\r\nExpect: 100-continue\r\n"))
        interim = bytearray()
        while not interim.endswith(b"\r\n\r\n"):
            interim += sock.recv(1)
        assert bytes(interim) == b"HTTP/1.1 100 Continue\r\n\r\n"
        sock.sendall(b"abcd")
        assert response(sock)[2] == b"abcd"
        assert sock.recv(1) == b""
    completed.append("100 Continue")

    with connect(number, process) as sock:
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
        with connect(number, process) as sock:
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    records = []
    with tempfile.TemporaryDirectory(prefix="dodo-reactor-") as directory:
        scratch = Path(directory)
        template = (ROOT / "tests/http_hosted/reactor.dodo").read_text()
        for optimization in (0, 3):
            for stopping in (False, True):
                number = port()
                source = scratch / f"reactor-{optimization}-{stopping}.dodo"
                content = template.replace("@@PORT@@", str(number)).replace("STOP_MS: u64 = 30000", f"STOP_MS: u64 = {500 if stopping else 30000}")
                if stopping:
                    content = content.replace("idle_timeout_ms = 100", "idle_timeout_ms = 5000").replace("header_timeout_ms = 300", "header_timeout_ms = 5000")
                source.write_text(content)
                binary = source.with_suffix("")
                built = subprocess.run([str(args.compiler.resolve()), "compile", str(source), "-O", str(optimization), "-o", str(binary)], capture_output=True, text=True)
                assert built.returncode == 0, built.stderr
                with running(binary) as process:
                    if stopping:
                        with connect(number, process) as sock:
                            sock.sendall(request(close=False))
                            assert response(sock)[0] == 200
                            # Fill another slot with an incomplete request.
                            with connect(number, process) as pending:
                                pending.sendall(b"GET / HTTP/1.1\r\nHost: ")
                                stdout, stderr = process.communicate(timeout=3)
                                assert process.returncode == 0, (stdout, stderr)
                                assert pending.recv(1) == b""
                            assert sock.recv(1) == b""
                        names = ["cancellation closes all slots"]
                    else:
                        names = checks(number, process)
                    for name in names:
                        records.append({"optimization": optimization, "case": name})
                        print(f"PASS reactor {name} -O{optimization}", flush=True)
    if args.report:
        args.report.write_text(json.dumps({"checks": len(records), "records": records}, indent=2) + "\n")
    print(f"Passed {len(records)} reactor checks.")


if __name__ == "__main__":
    main()
