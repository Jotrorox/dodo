#!/usr/bin/env python3
"""Check serial/concurrent HTTPS, bounded reuse, credentials, and cancellation."""
import argparse
import contextlib
from pathlib import Path
import ssl
import socket
import time
import subprocess
import tempfile

from test_hosted_http import credentials
from test_http_web import ROOT, connect, port, run
from test_web_developer import cases
from test_web_reactor import checks, request, response


def build(compiler, work, name, source, optimization):
    path = work / f"{name}-{optimization}.dodo"
    path.write_text(source)
    binary = path.with_suffix("")
    run([str(compiler), "build", str(path), "-O", str(optimization), "-o", str(binary)])
    return binary


def startup(compiler, work, optimization):
    source = (ROOT / "examples/https_server.dodo").read_text().replace("127.0.0.1:8443", "invalid address").replace(".concurrent()", "")
    binary = build(compiler, work, "startup", source, optimization)
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
    binary = build(compiler, work, "concurrent", concurrent, optimization)
    rejected(b"http hosted: InvalidInput")
    duplicate = source.replace('.get(b"/",', '.get(b"/", web.text(b"duplicate")).get(b"/",')
    binary = build(compiler, work, "duplicate", duplicate, optimization)
    # Registration errors win even when credential files are missing.
    (scratch / "cert.pem").unlink()
    rejected(b"web route #2 (GET /): web: AmbiguousRoute")
    print(f"PASS HTTPS startup diagnostics and credential bounds -O{optimization}", flush=True)


def serving(compiler, work, optimization, policy):
    number = port()
    exchanges = list(cases())
    source = (ROOT / "tests/http_hosted/developer.dodo").read_text()
    source = source.replace('import "std/web/app"', 'import "std/web/app"\nimport "std/web/https"')
    source = source.replace('builder.run(b"127.0.0.1:@@PORT@@")',
                            'builder.run_with(b"127.0.0.1:@@PORT@@", https.files("cert.pem", "key.pem"))')
    # Exercise all eight registration slots alongside middleware and TLS.
    extra = ""
    for index in range(4):
        extra += f'.get(b"/extra/{index}", web.text(b"extra"))\n        '
        exchanges.append((request(path=f"/extra/{index}"), 200, b"extra", {}))
    source = source.replace(".max_connections(", extra + ".max_connections(")
    for name, value in dict(PORT=number, COUNT=len(exchanges), POLICY=policy).items():
        source = source.replace(f"@@{name}@@", str(value))
    binary = build(compiler, work, f"routes-{policy}", source, optimization)
    context = ssl.create_default_context(cafile=str(work / "ca.pem"))
    context.set_alpn_protocols(["http/1.1"])
    process = subprocess.Popen([str(binary)], cwd=work, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        for wire, expected_status, expected_body, expected_headers in exchanges:
            with connect(number, process) as socket:
                with context.wrap_socket(socket, server_hostname="localhost") as peer:
                    assert peer.selected_alpn_protocol() == "http/1.1"
                    peer.sendall(wire)
                    code, headers, body = response(peer)
                    assert (code, body) == (expected_status, expected_body), (wire, code, body)
                    for name, value in expected_headers.items():
                        assert headers.get(name) == value, (name, headers)
        stdout, stderr = process.communicate(timeout=5)
        assert process.returncode == 0, (process.returncode, stdout, stderr)
    finally:
        if process.poll() is None:
            process.kill()
        process.communicate(timeout=5)
    print(f"PASS fluent {policy} HTTPS routes, middleware, and rejections -O{optimization}", flush=True)


def connection_failure(compiler, work, optimization, concurrent=False):
    number = port()
    source = (ROOT / "examples/https_server.dodo").read_text().replace(":8443", f":{number}").replace(".concurrent()", "")
    source = source.replace(".run_with(", ".max_connections(1).run_with(")
    if concurrent:
        source = source.replace(".run_with(", ".concurrent().run_with(")
    binary = build(compiler, work, f"failed-peer-{concurrent}", source, optimization)
    process = subprocess.Popen([str(binary)], cwd=work, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        # A plaintext peer fails its TLS handshake and is reported as exit 2.
        with connect(number, process) as peer:
            peer.sendall(request())
        stdout, stderr = process.communicate(timeout=5)
        assert process.returncode == 2, (process.returncode, stdout, stderr)
        assert b"http hosted: Tls" in stderr, stderr
    finally:
        if process.poll() is None:
            process.kill()
        process.communicate(timeout=5)
    print(f"PASS HTTPS connection failure reporting -O{optimization}", flush=True)


@contextlib.contextmanager
def running(binary, work):
    process = subprocess.Popen([str(binary)], cwd=work, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        yield process
    finally:
        if process.poll() is None:
            process.kill()
        process.communicate(timeout=5)


def concurrent_serving(compiler, work, optimization):
    context = ssl.create_default_context(cafile=str(work / "ca.pem"))
    context.set_alpn_protocols(["http/1.1"])

    def encrypted(number, process):
        raw = connect(number, process)
        try:
            peer = context.wrap_socket(raw, server_hostname="localhost", suppress_ragged_eofs=False)
            assert peer.selected_alpn_protocol() == "http/1.1"
            return peer
        except BaseException:
            raw.close()
            raise

    template = (ROOT / "tests/http_hosted/https_reactor.dodo").read_text()
    # A one-event turn stresses all suspend/resume boundaries and buffered TLS
    # records; the regular budget also exercises multiple events per resume.
    for events in (1, 16):
        number = port()
        source = template.replace("@@PORT@@", str(number)).replace("@@EVENTS@@", str(events))
        binary = build(compiler, work, f"reactor-{events}", source, optimization)
        with running(binary, work) as process:
            for name in checks(number, process, encrypted):
                print(f"PASS HTTPS reactor {name}, events={events} -O{optimization}", flush=True)
            # An accepted socket may delay its ClientHello without blocking a
            # second handshake or request. Complete both before either expires.
            with connect(number, process) as slow:
                with encrypted(number, process) as fast:
                    fast.sendall(request())
                    assert response(fast)[2] == b"Hello, Dodo!\n"
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
                with encrypted(number, process) as fast:
                    fast.sendall(request())
                    assert response(fast)[0] == 200
                assert slow.recv(1) == b""
            print(f"PASS HTTPS independent and bounded handshakes, events={events} -O{optimization}", flush=True)

    # Queue enough encrypted output to exceed socket/TLS staging while the
    # first client pauses reading; another connection must still complete.
    number = port()
    source = template.replace("@@PORT@@", str(number)).replace("@@EVENTS@@", "16")
    source = source.replace("requests_per_connection = 3", "requests_per_connection = 100")
    source = source.replace("builder.config.timeouts.write_ms = 1000", "builder.config.timeouts.write_ms = 10000")
    source = source.replace("server := builder.build()!", "builder.config.max_connections = 2\n    server := builder.build()!")
    source = source.replace("assert(report.cancelled)",
                            "assert(!report.cancelled && report.accepted == 2 && report.completed == 101 && report.failed == 0 && report.rejected == 0)")
    binary = build(compiler, work, "reactor-output", source, optimization)
    with running(binary, work) as process:
        with encrypted(number, process) as slow:
            slow.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 65536)
            slow.sendall(request(path="/large", close=False) * 100)
            time.sleep(.05)
            with encrypted(number, process) as fast:
                fast.sendall(request())
                assert response(fast)[2] == b"Hello, Dodo!\n"
                assert fast.recv(1) == b""
            wire = bytearray()
            while part := slow.recv(65536):
                wire += part
            frames = bytes(wire).split(b"HTTP/1.1 ")[1:]
            assert len(frames) == 100, len(frames)
            assert all(frame.startswith(b"200 ") and frame.split(b"\r\n\r\n", 1)[1] == b"x" * 65536 for frame in frames)
            assert b"Connection: close\r\n" in frames[-1]
        stdout, stderr = process.communicate(timeout=5)
        assert process.returncode == 0, (stdout, stderr)
    print(f"PASS HTTPS slow reader, ciphertext flushing, and completion counts -O{optimization}", flush=True)

    # Reuse the same slot for successful dispatches, an HTTP rejection, and a
    # TLS protocol failure. Report counters distinguish requests/connections.
    number = port()
    source = template.replace("@@PORT@@", str(number)).replace("@@EVENTS@@", "1")
    source = source.replace("server := builder.build()!", "builder.config.max_connections = 3\n    server := builder.build()!")
    source = source.replace("assert(report.cancelled)",
                            "assert(!report.cancelled && report.accepted == 3 && report.completed == 3 && report.rejected == 1 && report.failed == 1)")
    binary = build(compiler, work, "reactor-report", source, optimization)
    with running(binary, work) as process:
        with encrypted(number, process) as peer:
            for _ in range(3):
                peer.sendall(request(close=False))
                assert response(peer)[0] == 200
            assert peer.recv(1) == b""
        with encrypted(number, process) as peer:
            peer.sendall(request(path="/absent"))
            assert response(peer)[0] == 404
            assert peer.recv(1) == b""
        with connect(number, process) as peer:
            peer.sendall(request())
            try:
                assert peer.recv(1) == b""
            except ConnectionResetError:
                pass
        stdout, stderr = process.communicate(timeout=5)
        assert process.returncode == 0, (stdout, stderr)
    print(f"PASS HTTPS connection/request accounting and slot cleanup -O{optimization}", flush=True)

    number = port()
    source = template.replace("@@PORT@@", str(number)).replace("@@EVENTS@@", "1")
    source = source.replace("STOP_MS: u64 = 30000", "STOP_MS: u64 = 1000")
    source = source.replace("idle_ms = 100", "idle_ms = 5000").replace("header_ms = 300", "header_ms = 5000")
    binary = build(compiler, work, "reactor-cancel", source, optimization)
    with running(binary, work) as process:
        with encrypted(number, process) as idle:
            idle.sendall(request(close=False))
            assert response(idle)[0] == 200
            with encrypted(number, process) as pending, connect(number, process) as handshake:
                pending.sendall(b"GET / HTTP/1.1\r\nHost: ")
                stdout, stderr = process.communicate(timeout=3)
                assert process.returncode == 0, (stdout, stderr)
                assert handshake.recv(1) == b""
                # Cancellation aborts TLS immediately instead of waiting for
                # close_notify. Python reports the intentional abrupt EOF.
                for peer in (idle, pending):
                    try:
                        assert peer.recv(1) == b""
                    except ssl.SSLEOFError:
                        pass
    print(f"PASS HTTPS cancellation closes idle, request, and handshake slots -O{optimization}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    with tempfile.TemporaryDirectory(prefix="dodo-web-https-") as directory:
        work = Path(directory)
        credentials(work)
        for optimization in (0, 3):
            startup(compiler, work, optimization)
            for policy in ("Serial", "Concurrent"):
                serving(compiler, work, optimization, policy)
            for concurrent in (False, True):
                connection_failure(compiler, work, optimization, concurrent)
            concurrent_serving(compiler, work, optimization)
            source = (ROOT / "tests/http_hosted/https_app.dodo").read_text()
            binary = build(compiler, work, "config", source, optimization)
            run([str(binary)], cwd=work)
            print(f"PASS HTTPS custom storage, config, and cancellation -O{optimization}", flush=True)


if __name__ == "__main__":
    main()
