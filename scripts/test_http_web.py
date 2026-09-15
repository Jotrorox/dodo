#!/usr/bin/env python3
"""Compile the runnable examples and check HTTP interoperability on loopback."""
import argparse
import http.client
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def run(command, **kwargs):
    result = subprocess.run(command, capture_output=True, timeout=120, **kwargs)
    if result.returncode:
        raise RuntimeError(f"{command}: {result.returncode}\n{result.stdout!r}\n{result.stderr!r}")


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def connect(number, process):
    until = time.monotonic() + 10
    while time.monotonic() < until:
        if process.poll() is not None:
            raise RuntimeError(f"Server exited early: {process.returncode}")
        try:
            connection = socket.create_connection(("127.0.0.1", number), timeout=1)
            connection.settimeout(5)
            return connection
        except ConnectionRefusedError:
            time.sleep(0.01)
    raise RuntimeError("Server did not listen")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug/dodo")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    compiler = str(args.compiler.resolve())
    records = []
    with tempfile.TemporaryDirectory(prefix="dodo-http-web-") as directory:
        scratch = Path(directory)
        for optimization in (0, 3):
            number = port()
            programs = {}
            for example in ("http_client", "web_server", "http_memory"):
                source = scratch / f"{example}.dodo"
                source.write_text((ROOT / f"examples/{example}.dodo").read_text().replace(":8080", f":{number}").replace("config.max_connections = 0", "config.max_connections = 1").replace(".run(b", ".max_connections(1).run(b"))
                executable = scratch / f"{example}-{optimization}"
                run([compiler, "build", str(source), "-O", str(optimization), "-o", str(executable)])
                programs[example] = executable
            run([str(programs["http_memory"])])
            records.append({"case": "memory", "optimization": optimization})
            for case in ("dodo_client", "python_get", "python_head", "missing", "chunked", "malformed", "disconnect"):
                if case != "dodo_client":
                    number = port()
                    source = scratch / "web_server.dodo"
                    source.write_text((ROOT / "examples/web_server.dodo").read_text().replace(":8080", f":{number}").replace("config.max_connections = 0", "config.max_connections = 1").replace(".run(b", ".max_connections(1).run(b"))
                    run([compiler, "build", str(source), "-O", str(optimization), "-o", str(programs["web_server"])])
                process = subprocess.Popen([str(programs["web_server"])], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    if case == "dodo_client":
                        # No probe connection: the example admits exactly one.
                        until = time.monotonic() + 5
                        while True:
                            result = subprocess.run([str(programs["http_client"])], capture_output=True, timeout=10)
                            if result.returncode == 0:
                                break
                            if process.poll() is not None or time.monotonic() >= until:
                                raise RuntimeError(f"Dodo client failed: {result.returncode}")
                            time.sleep(0.01)
                    else:
                        with connect(number, process) as stream:
                            if case in ("python_get", "python_head", "missing"):
                                method = "HEAD" if case == "python_head" else "GET"
                                path = "/absent" if case == "missing" else "/"
                                stream.sendall(f"{method} {path} HTTP/1.1\r\nHost: localhost\r\n\r\n".encode())
                                response = http.client.HTTPResponse(stream, method=method)
                                response.begin()
                                assert response.status == (404 if case == "missing" else 200)
                                assert response.read() == (b"Hello, Dodo!\n" if case == "python_get" else b"")
                            elif case == "chunked":
                                request = b"GET / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\nX-Checksum: yes\r\n\r\n"
                                for value in request:
                                    stream.sendall(bytes([value]))
                                response = http.client.HTTPResponse(stream)
                                response.begin()
                                assert response.status == 200 and response.read() == b"Hello, Dodo!\n"
                            elif case == "malformed":
                                stream.sendall(b"GET / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nxx")
                                response = http.client.HTTPResponse(stream)
                                response.begin()
                                assert response.status == 400 and response.read() == b""
                            else:
                                stream.sendall(b"GET / HTTP/1.1\r\nHost:")
                                stream.shutdown(socket.SHUT_WR)
                                response = http.client.HTTPResponse(stream)
                                response.begin()
                                assert response.status == 400 and response.read() == b""
                    stdout, stderr = process.communicate(timeout=10)
                    expected = 0
                    if process.returncode != expected:
                        raise RuntimeError(f"{case} server: {process.returncode}, expected {expected}: {stdout!r} {stderr!r}")
                finally:
                    if process.poll() is None:
                        process.kill()
                    process.communicate(timeout=5)
                records.append({"case": case, "optimization": optimization})
                print(f"PASS HTTP/web {case} -O{optimization}", flush=True)
    if args.report:
        args.report.write_text(json.dumps({"checks": len(records), "records": records}, indent=2) + "\n")
    print(f"Passed {len(records)} HTTP/web example and interoperability checks.")


if __name__ == "__main__":
    main()
