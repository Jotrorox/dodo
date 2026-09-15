#!/usr/bin/env python3
"""Check fluent HTTPS, bounded credentials, startup errors, and cancellation."""
import argparse
from pathlib import Path
import ssl
import subprocess
import tempfile

from test_hosted_http import credentials
from test_http_web import ROOT, connect, port, run
from test_web_developer import cases
from test_web_reactor import request, response


def build(compiler, work, name, source, optimization):
    path = work / f"{name}-{optimization}.dodo"
    path.write_text(source)
    binary = path.with_suffix("")
    run([str(compiler), "build", str(path), "-O", str(optimization), "-o", str(binary)])
    return binary


def startup(compiler, work, optimization):
    source = (ROOT / "examples/https_server.dodo").read_text().replace("127.0.0.1:8443", "invalid address")
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
    rejected(b"concurrent HTTPS is not supported; use serial execution")
    duplicate = source.replace('.get(b"/",', '.get(b"/", web.text(b"duplicate")).get(b"/",')
    binary = build(compiler, work, "duplicate", duplicate, optimization)
    # Registration errors win even when credential files are missing.
    (scratch / "cert.pem").unlink()
    rejected(b"web route #2 (GET /): web: AmbiguousRoute")
    print(f"PASS HTTPS startup diagnostics and credential bounds -O{optimization}", flush=True)


def serving(compiler, work, optimization):
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
    for name, value in dict(PORT=number, COUNT=len(exchanges), POLICY="Serial").items():
        source = source.replace(f"@@{name}@@", str(value))
    binary = build(compiler, work, "routes", source, optimization)
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
    print(f"PASS fluent HTTPS routes, middleware, and rejections -O{optimization}", flush=True)


def connection_failure(compiler, work, optimization):
    number = port()
    source = (ROOT / "examples/https_server.dodo").read_text().replace(":8443", f":{number}")
    source = source.replace(".run_with(", ".max_connections(1).run_with(")
    binary = build(compiler, work, "failed-peer", source, optimization)
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
            serving(compiler, work, optimization)
            connection_failure(compiler, work, optimization)
            source = (ROOT / "tests/http_hosted/https_app.dodo").read_text()
            binary = build(compiler, work, "config", source, optimization)
            run([str(binary)], cwd=work)
            print(f"PASS HTTPS custom storage, config, and cancellation -O{optimization}", flush=True)


if __name__ == "__main__":
    main()
