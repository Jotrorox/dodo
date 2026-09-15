#!/usr/bin/env python3
"""Exercise the buffered web API through independent HTTP peers at O0/O3."""
import argparse
from pathlib import Path
import subprocess
import tempfile

from test_http_web import ROOT, connect, port, run


def request(target, method=b"GET", headers=b"", body=b""):
    return method + b" " + target + b" HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n" + headers + b"\r\n" + body


def cases():
    yield "json", request(b"/json"), 201, b'{"ok":true}', [(b"content-type", b"application/json"), (b"set-cookie", b"a=1"), (b"set-cookie", b"b=2")]
    yield "html", request(b"/html"), 200, b"<h1>Hello</h1>", [(b"content-type", b"text/html; charset=utf-8")]
    yield "bytes", request(b"/bytes"), 200, bytes([0, 255, 128, 13, 10]), [(b"content-type", b"application/octet-stream")]
    for name, target, expected, length in [("head-fallback", b"/html", 200, b"14"), ("head-explicit", b"/json", 200, b"13")]:
        yield name, request(target, b"HEAD"), expected, b"", [(b"content-length", length)]
    for name, code, length in [("empty", 204, None), ("not-modified", 304, b"7"), ("reset", 205, b"0")]:
        yield name, request(b"/" + name.encode()), code, b"", [(b"content-length", length)]
    headers = b"X-Input: first\r\nx-INPUT:  second \t\r\n"
    for name, target, framing, body in [
        ("request-access", b"/inspect/a%252Fb?q=one+two&q=%2526", b"Content-Length: 3\r\n", b"abc"),
        ("absolute-chunked", b"http://localhost/inspect/a%252Fb?q=one+two&q=%2526", b"Transfer-Encoding: chunked\r\n", b"1\r\na\r\n2\r\nbc\r\n0\r\nX-Trailer: secret\r\n\r\n"),
    ]:
        yield name, request(target, b"POST", headers + framing, body), 200, b"abc", [
            (b"x-method", b"POST"), (b"x-path", b"/inspect/a%2Fb"), (b"x-id", b"a%2Fb"),
            (b"x-q-first", b"one two"), (b"x-q-second", b"%26"), (b"x-input-second", b"second"), (b"content-type", None)]
    yield "query-rules", request(b"/query?&=empty&&bare&q=a%26b%3Dc%2B%23&"), 200, b"a&b=c+#", []
    yield "utf8-query", request(b"/query?=empty&bare&q=%C3%A9"), 200, "é".encode(), []
    for target in [b"/html?q=%", b"/html?q=%GG", b"/html?q=%00", b"/html?q=%FF", b"/html?q=%C0%AF", b"/html?q=%0D", b"/html?q=x#fragment", b"/a%2Fb", b"/../html", b"/%FF"]:
        yield f"malformed-{target!r}", request(target), 400, b"", [(b"x-handled", None)]
    for target in [b"/html?" + b"q=x&" * 5, b"/html?q=" + b"x" * 127, b"/" + b"x" * 64]:
        yield f"target-capacity-{len(target)}", request(target), 414, b"", [(b"x-handled", None)]
    yield "request-header-fields", request(b"/html", headers=b"X: a\r\n" * 7), 431, b"", []
    yield "request-header-bytes", request(b"/html", headers=b"X: " + b"x" * 512 + b"\r\n"), 431, b"", []
    yield "request-body-bytes", request(b"/html", headers=b"Content-Length: 129\r\n", body=b"x" * 129), 413, b"", []
    yield "malformed-framing", request(b"/html", headers=b"Content-Length: 1\r\nContent-Length: 2\r\n", body=b"xx"), 400, b"", []
    for name in ["body-full", "fields-full", "headers-full", "framing", "bad-status", "bad-text", "duplicate-type", "bad-header"]:
        yield name, request(b"/" + name.encode()), 500, b"", [(b"x-handled", None)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    args = parser.parse_args()
    compiler = str(args.compiler.resolve())
    checks = list(cases())
    with tempfile.TemporaryDirectory(prefix="dodo-web-response-") as directory:
        scratch = Path(directory)
        for optimization in (0, 3):
            for runner in ("serial", "reactor"):
                number = port()
                source = (ROOT / "tests/http_hosted/request_response.dodo").read_text()
                source = source.replace("@@PORT@@", str(number)).replace("config.max_connections = 1 // @@COUNT@@", f"config.max_connections = {len(checks)}")
                if runner == "reactor":
                    source = source.replace("// @@SERVE@@", "slots := [reactor.Slot.new()]\n    concurrent := reactor.Config.defaults()\n    concurrent.server = config")
                    source = source.replace('match hosted.serve(', 'match reactor.serve(').replace('&mut handler, &mut workspace', '&mut handler, &mut slots, &mut workspace').replace('&mut response, config)', '&mut response, concurrent)')
                path = scratch / "server.dodo"
                executable = scratch / "server"
                path.write_text(source)
                run([compiler, "build", str(path), "-O", str(optimization), "-o", str(executable)])
                process = subprocess.Popen([str(executable)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    for name, wire, status, body, fields in checks:
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
                        print(f"PASS {runner} {name} -O{optimization}", flush=True)
                    stdout, stderr = process.communicate(timeout=10)
                    assert process.returncode == 0, (runner, process.returncode, stdout, stderr)
                finally:
                    if process.poll() is None:
                        process.kill()
                    process.communicate(timeout=5)
    print(f"Passed {len(checks) * 4} buffered request/response checks.")


if __name__ == "__main__":
    main()
