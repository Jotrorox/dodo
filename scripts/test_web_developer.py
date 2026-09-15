#!/usr/bin/env python3
"""Exercise fluent applications and explicit HTTP rejections with real peers."""
import argparse
from pathlib import Path
import subprocess
import tempfile

from test_http_web import ROOT, connect, port, run
from test_web_reactor import request, response


def cases():
    yield request(), 200, b"<h1>Home</h1>", {"Content-Type": "text/html; charset=utf-8"}
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


def check_startup(compiler, scratch, optimization):
    source = scratch / f"startup-{optimization}.dodo"
    source.write_text('''package startup
import "std/web"
import "std/web/app"
fn main() -> i32 {
    return app.new().get(b"/:first", web.text(b"one"))
        .get(b"/:second", web.text(b"two"))
        .run(b"invalid address")
}
''')
    binary = source.with_suffix("")
    run([str(compiler), "build", str(source), "-O", str(optimization), "-o", str(binary)])
    result = subprocess.run([str(binary)], capture_output=True, timeout=5)
    assert result.returncode == 1, result
    assert b"web route #2 (GET /:second): web: AmbiguousRoute" in result.stderr, result.stderr
    assert b"already registered" in result.stderr, result.stderr
    print(f"PASS startup diagnostics -O{optimization}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="dodo-web-developer-") as directory:
        scratch = Path(directory)
        for optimization in (0, 3):
            check_startup(args.compiler.resolve(), scratch, optimization)
            for policy in ("Serial", "Concurrent"):
                number = port()
                exchanges = list(cases())
                source = (ROOT / "tests/http_hosted/developer.dodo").read_text()
                for key, value in {"PORT": number, "COUNT": len(exchanges), "POLICY": policy}.items():
                    source = source.replace(f"@@{key}@@", str(value))
                path = scratch / f"developer-{optimization}-{policy}.dodo"
                path.write_text(source)
                binary = path.with_suffix("")
                run([str(args.compiler.resolve()), "build", str(path), "-O", str(optimization), "-o", str(binary)])
                process = subprocess.Popen([str(binary)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    for wire, expected_status, expected_body, expected_headers in exchanges:
                        with connect(number, process) as peer:
                            peer.sendall(wire)
                            code, headers, body = response(peer)
                            assert (code, body) == (expected_status, expected_body), (wire, code, body)
                            for key, value in expected_headers.items():
                                assert headers.get(key) == value, (key, headers)
                    stdout, stderr = process.communicate(timeout=5)
                    assert process.returncode == 0, (process.returncode, stdout, stderr)
                finally:
                    if process.poll() is None:
                        process.kill()
                    process.communicate(timeout=5)
                print(f"PASS fluent app -O{optimization} {policy}", flush=True)


if __name__ == "__main__":
    main()
