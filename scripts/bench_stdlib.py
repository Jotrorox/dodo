#!/usr/bin/env python3
"""Benchmark compiled Dodo JSON, web routing/application, and loopback HTTP.

Only Python's standard library is required. Build Dodo from the current checkout
first: its stdlib is embedded. Timings are observations, never pass/fail limits.
"""

import argparse
import concurrent.futures
import datetime
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import platform
import shutil
import socket
import statistics
import struct
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "benchmarks"


def run(command, **kwargs):
    result = subprocess.run(command, capture_output=True, timeout=180, **kwargs)
    if result.returncode:
        raise RuntimeError(f"Failed: {command}\n{result.stdout.decode(errors='replace')}"
                           f"\n{result.stderr.decode(errors='replace')}")
    return result.stdout


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)]


def stats(values):
    return {"median": statistics.median(values), "min": min(values),
            "max": max(values), "p90": percentile(values, 0.9)}


def digits(iterations):
    groups, rest = divmod(iterations, 10)
    return groups * 45 + rest * (rest - 1) // 2


def route_checksum(iterations, size, unsorted):
    groups, rest = divmod(iterations, size)
    total = groups * size * (size - 1) // 2 + rest * (rest - 1) // 2
    # The unsorted table exchanges the first and last entries.
    return total + (size - 1 if unsorted and rest else 0)


def measure(binary, mode, payload, iterations, expected):
    data = struct.pack("<QB7x", iterations, mode) + payload
    output = run([str(binary)], input=data).decode().split()
    if len(output) != 2:
        raise RuntimeError(f"Malformed benchmark output: {output}")
    nanos, checksum = map(int, output)
    if nanos <= 0 or checksum != expected(iterations):
        raise RuntimeError(f"Invalid result: ns={nanos}, checksum={checksum}, "
                           f"expected={expected(iterations)}")
    return nanos


def benchmark(binary, name, mode, payload, expected, args, **metadata):
    iterations = 1
    # Discard calibration passes; reach the requested duration without timing
    # process startup, compilation, stdin, stdout, or fixture construction.
    for _ in range(8):
        nanos = measure(binary, mode, payload, iterations, expected)
        if args.seconds * 1e9 * 0.8 <= nanos <= args.seconds * 1e9 * 1.5:
            break
        adjusted = min(1_000_000_000, max(1, int(iterations * min(100, args.seconds * 1e9 / nanos))))
        if adjusted == iterations:
            break
        iterations = adjusted
    samples = [measure(binary, mode, payload, iterations, expected)
               for _ in range(args.samples)]
    timings = stats([value / iterations for value in samples])
    record = {"name": name, "kind": "in_process", "iterations": iterations,
              "elapsed_ns": samples, "ns_per_op": timings,
              "ops_per_second": 1e9 / timings["median"], **metadata}
    if payload:
        record["input_bytes"] = len(payload)
        record["mib_per_second"] = len(payload) * record["ops_per_second"] / 1048576
    print(f"  {name:32s} {timings['median'] / 1000:10.3f} us/op  "
          f"{record['ops_per_second']:12,.0f} ops/s", flush=True)
    return record


def compile_fixture(compiler, linker, scratch, name, optimization, source=None):
    path = FIXTURES / f"{name}.dodo"
    if source is not None:
        path = scratch / f"{name}.dodo"
        path.write_text(source, encoding="utf-8")
    binary = scratch / (name + (".exe" if os.name == "nt" else ""))
    command = [compiler, "compile", str(path), "-O", str(optimization), "-o", str(binary)]
    if linker:
        command += ["--linker", linker]
    start = time.perf_counter()
    run(command)
    return binary, {"fixture": name, "optimization": optimization,
                    "seconds": time.perf_counter() - start,
                    "binary_bytes": binary.stat().st_size, "command": command}


def json_cases():
    record = {"id": 0, "name": "Dodo", "active": True, "scores": [1, 2, 3, 4]}
    compact = lambda value: json.dumps(value, separators=(",", ":")).encode()
    payload = compact(record)
    yield "json.parse.record", 0, payload, lambda n: n * len(payload) + digits(n)
    yield "json.decode.derived", 1, payload, lambda n: n * 4 + digits(n)
    yield "json.encode.derived", 2, payload, lambda n: n * (len(payload) + 48) + digits(n)
    escaped = compact({"id": 0, "text": 'quotes " and backslash \\ and newline\n ä 😀 ' * 32,
                       "nested": {"values": [True, None, -17, 1.25]}})
    yield "json.parse.escaped", 0, escaped, lambda n: n * len(escaped) + digits(n)
    for size in (256, 4096, 16384):
        data = compact([0] + [i % 100 for i in range(1, size)])
        yield f"json.parse.array_{size}", 0, data, lambda n, length=len(data): n * length + digits(n)
    for size in (32, 256, 1024):
        data = compact({"id": 0, **{f"k{i:04}": i for i in range(size - 1)}})
        yield f"json.parse.object_{size}", 0, data, lambda n, length=len(data): n * length + digits(n)
        yield f"json.parse_indexed.object_{size}", 3, data, lambda n, length=len(data): n * length + digits(n)


def connect_ready(port, process):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"HTTP server exited with {process.returncode}")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return
        except OSError:
            time.sleep(0.02)
    raise RuntimeError("HTTP server did not start within 15 seconds")


def http_sample(port, clients, requests, keep_alive):
    start = 0

    def begin():
        nonlocal start
        start = time.perf_counter_ns()

    barrier = threading.Barrier(clients + 1, action=begin, timeout=30)

    def worker():
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        latencies = []

        def request():
            connection.request("GET", "/health", headers={
                "Connection": "keep-alive" if keep_alive else "close"})
            response = connection.getresponse()
            body = response.read()
            if (response.status != 200 or body != b'{"ok":true}'
                    or response.getheader("Content-Type") != "application/json"):
                raise RuntimeError(f"Bad HTTP response: {response.status}, {body!r}")
            if keep_alive and response.will_close:
                raise RuntimeError("Concurrent benchmark unexpectedly closed keep-alive connection")

        try:
            for _ in range(10):
                request()
            barrier.wait()
            for _ in range(requests):
                start = time.perf_counter_ns()
                request()
                latencies.append(time.perf_counter_ns() - start)
            return latencies
        finally:
            connection.close()

    with concurrent.futures.ThreadPoolExecutor(max_workers=clients) as pool:
        futures = [pool.submit(worker) for _ in range(clients)]
        # The barrier action timestamps immediately before releasing workers.
        barrier.wait()
        latencies = [value for future in futures for value in future.result()]
        elapsed = time.perf_counter_ns() - start
    return elapsed, latencies


def benchmark_http(binary, args, optimization):
    records = []
    for execution, clients in (("serial", 1), ("concurrent", 1), ("concurrent", 8)):
        with socket.socket() as reserved:
            reserved.bind(("127.0.0.1", 0))
            port = reserved.getsockname()[1]
        with tempfile.TemporaryFile() as log:
            process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=log, stderr=log)
            try:
                process.stdin.write(bytes([execution == "concurrent"]) + f"127.0.0.1:{port}".encode())
                process.stdin.close()
                connect_ready(port, process)
                samples, latencies = [], []
                for _ in range(args.samples):
                    elapsed, timings = http_sample(port, clients, args.http_requests, execution == "concurrent")
                    samples.append(elapsed)
                    latencies.extend(timings)
                rates = [clients * args.http_requests * 1e9 / elapsed for elapsed in samples]
                name = f"http.{execution}.c{clients}"
                record = {"name": name, "kind": "loopback_http", "optimization": optimization,
                          "clients": clients, "keep_alive": execution == "concurrent",
                          "requests_per_sample": clients * args.http_requests,
                          "elapsed_ns": samples, "requests_per_second": stats(rates),
                          "latency_us": {"p50": statistics.median(latencies) / 1000,
                                         "p95": percentile(latencies, 0.95) / 1000,
                                         "p99": percentile(latencies, 0.99) / 1000}}
                records.append(record)
                print(f"  {name:32s} {statistics.median(rates):10,.0f} req/s  "
                      f"p95 {record['latency_us']['p95']:,.1f} us", flush=True)
            except Exception:
                log.seek(0)
                print(log.read().decode(errors="replace"), flush=True)
                raise
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=10)
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=ROOT / "target/debug" / ("dodo.exe" if os.name == "nt" else "dodo"))
    parser.add_argument("--linker", help="C compiler driver, e.g. clang or an absolute path")
    parser.add_argument("--optimization", type=int, choices=range(4), nargs="+", default=[3])
    parser.add_argument("--suite", choices=("json", "json-arrays", "json-structs", "json-strings", "web", "http", "all"), default="all")
    parser.add_argument("--struct-sizes", type=int, nargs="+", default=[16, 64],
                        help="field counts for json-structs (1 through 256)")
    parser.add_argument("--samples", type=int, default=7)
    parser.add_argument("--seconds", type=float, default=0.2, help="target seconds per in-process sample")
    parser.add_argument("--http-requests", type=int, default=200, help="requests per client per sample")
    parser.add_argument("--report", type=Path, default=ROOT / "benchmark-data/stdlib.json")
    args = parser.parse_args()
    if args.samples < 1 or not math.isfinite(args.seconds) or args.seconds <= 0 or args.http_requests < 1:
        parser.error("samples, seconds, and http-requests must be positive")
    if any(size < 1 or size > 256 for size in args.struct_sizes):
        parser.error("struct-sizes must be between 1 and 256")
    compiler = str(args.compiler.resolve())
    if not Path(compiler).is_file():
        parser.error(f"Compiler not found: {compiler}; build Dodo first")
    linker = shutil.which(args.linker) if args.linker else None
    if args.linker and not linker:
        parser.error(f"Linker not found: {args.linker}")
    report = {"timestamp_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "platform": platform.platform(), "machine": platform.machine(),
              "processor": platform.processor(), "logical_cpus": os.cpu_count(),
              "python": platform.python_version(), "compiler": compiler,
              "compiler_version": run([compiler, "--version"]).decode().strip(),
              "compiler_sha256": hashlib.sha256(Path(compiler).read_bytes()).hexdigest(),
              "git_commit": run(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
              "git_status": run(["git", "status", "--short"], cwd=ROOT).decode().strip(),
              "source_sha256": {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                                for path in [Path(__file__).resolve(), *sorted(FIXTURES.glob("*.dodo"))]},
              "samples": args.samples, "target_sample_seconds": args.seconds,
              "builds": [], "results": []}
    args.report.parent.mkdir(parents=True, exist_ok=True)

    def save():
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    for optimization in args.optimization:
        print(f"Dodo -O{optimization} on {report['platform']}", flush=True)
        with tempfile.TemporaryDirectory(prefix="dodo-stdlib-bench-") as directory:
            scratch = Path(directory)
            if args.suite in ("json", "all"):
                binary, build = compile_fixture(compiler, linker, scratch, "json", optimization)
                report["builds"].append(build)
                for name, mode, payload, expected in json_cases():
                    report["results"].append(benchmark(binary, name, mode, payload, expected, args,
                                                       optimization=optimization))
                    save()
            if args.suite in ("json", "json-strings", "all"):
                binary, build = compile_fixture(compiler, linker, scratch, "json_strings", optimization)
                report["builds"].append(build)
                for kind, value, ascii_only in (("ascii", "ordinary ASCII text / " * 16 + "0", False),
                                                ("utf8", "Aß€😀 / " * 16 + "0", False),
                                                ("escaped", 'Aß€😀 "\\\n / ' * 16 + "0", True)):
                    payload = json.dumps(value, ensure_ascii=ascii_only).encode()
                    length = len(value.encode())
                    for mode, operation in enumerate(("parse", "equal", "decoded_len", "decode")):
                        if mode == 0:
                            expected = lambda n, base=len(payload): n * base + n // 2
                        elif mode == 1:
                            expected = lambda n: n - n // 2
                        elif mode == 2:
                            expected = lambda n, base=length: n * base
                        else:
                            expected = lambda n, base=length + 48: n * base + n // 2
                        report["results"].append(benchmark(binary, f"json.string.{operation}.{kind}", mode,
                                                           payload, expected, args, optimization=optimization,
                                                           decoded_bytes=length))
                        save()
            if args.suite in ("json", "json-arrays", "all"):
                for size in (16, 64, 256):
                    source = (FIXTURES / "json_arrays.dodo").read_text(encoding="utf-8")
                    source = source.replace("values: [16]u32", f"values: [{size}]u32")
                    binary, build = compile_fixture(compiler, linker, scratch, f"json_arrays_{size}", optimization, source)
                    report["builds"].append(build)
                    payload = json.dumps({"values": list(range(size))}, separators=(",", ":")).encode()
                    for mode, kind in ((0, "parse"), (1, "decode")):
                        base = len(payload) if mode == 0 else size - 1
                        expected = lambda n, base=base: n * base + digits(n)
                        report["results"].append(benchmark(binary, f"json.{kind}.fixed_array_{size}", mode,
                                                           payload, expected, args, optimization=optimization))
                        save()
            if args.suite in ("json", "json-structs", "all"):
                for size in args.struct_sizes:
                    source = (FIXTURES / "json_structs.dodo").read_text(encoding="utf-8")
                    fields = "\n".join(f"    f{i:03}: u32" for i in range(size))
                    checks = "\n".join(f"    assert_eq(record.f{i:03}, {i}u32)" for i in range(1, size))
                    source = source.replace("// FIELDS: generated by bench_stdlib.py for the requested schema size.", fields)
                    source = source.replace("// CHECKS: generated by bench_stdlib.py for all other fields.", checks)
                    source = source.replace("LAST_FIELD", f"f{size - 1:03}")
                    binary, build = compile_fixture(compiler, linker, scratch, f"json_structs_{size}", optimization, source)
                    report["builds"].append(build)
                    payload = json.dumps({f"f{i:03}": i for i in reversed(range(size))}, separators=(",", ":")).encode()
                    for mode, kind in ((0, "parse_indexed"), (1, "decode_indexed"), (2, "decode")):
                        base = len(payload) if mode == 0 else size - 1
                        digit_count = 2 if size == 1 and mode != 0 else 1
                        expected = lambda n, base=base, digit_count=digit_count: n * base + digit_count * digits(n)
                        report["results"].append(benchmark(binary, f"json.{kind}.struct_{size}", mode,
                                                           payload, expected, args, optimization=optimization))
                        save()
            if args.suite in ("web", "all"):
                for size in (8, 64, 256):
                    source = (FIXTURES / "web.dodo").read_text(encoding="utf-8")
                    routes = ",\n".join(f'web.Route {{ method: b"GET", pattern: b"/route/{i:04}", id: {i} }}'
                                         for i in range(size))
                    source = source.replace("// ROUTES: generated by bench_stdlib.py for the requested table size.", routes)
                    source = source.replace("/route/LAST", f"/route/{size - 1:04}")
                    binary, build = compile_fixture(compiler, linker, scratch, f"web_{size}", optimization, source)
                    report["builds"].append(build)
                    for kind, mode in (("sorted", 0), ("indexed", 3), ("unsorted", 4)):
                        expected = lambda n, size=size, kind=kind: route_checksum(n, size, kind == "unsorted")
                        report["results"].append(benchmark(binary, f"web.router.{kind}.{size}", mode, b"", expected,
                                                           args, optimization=optimization, routes=size))
                    if size == 8:
                        for name, mode, base in (("web.decode_path", 1, 75), ("web.application", 2, 82)):
                            expected = lambda n, base=base: n * base + n // 2
                            report["results"].append(benchmark(binary, name, mode, b"", expected,
                                                               args, optimization=optimization))
                    save()
            if args.suite in ("http", "all"):
                binary, build = compile_fixture(compiler, linker, scratch, "http", optimization)
                report["builds"].append(build)
                report["results"].extend(benchmark_http(binary, args, optimization))
                save()
    print(f"Report: {args.report.resolve()}", flush=True)


if __name__ == "__main__":
    main()
