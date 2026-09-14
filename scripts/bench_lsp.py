#!/usr/bin/env python3
"""Measure stdio round trips, including checking and diagnostic publication.

Run against the same build profile and machine before/after editor changes.
No latency thresholds: timings are observations, not portable correctness tests.
"""
import argparse
import json
import math
import pathlib
import queue
import statistics
import subprocess
import tempfile
import threading
import time


def measure(binary, functions, samples, features, documents, stdlib):
    with tempfile.TemporaryDirectory(prefix="dodo-lsp-bench-") as directory:
        uri = (pathlib.Path(directory) / "main.dodo").as_uri()
        source = "package bench\n" + ('import "std/math"\n' if stdlib else "") + "".join(
            f"fn value_{i}(x: i32) -> i32 {{ return x + {i} }}\n"
            for i in range(functions)
        ) + "fn main() -> i32 { return value_0(1) }\n"
        process = subprocess.Popen(
            [binary, "lsp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        messages = queue.Queue()

        def read():
            while header := process.stdout.readline():
                length = int(header.split(b":", 1)[1])
                assert process.stdout.readline() == b"\r\n"
                messages.put(json.loads(process.stdout.read(length)))

        threading.Thread(target=read, daemon=True).start()
        request_id = 0

        def send(method, params, identifier=None):
            message = {"jsonrpc": "2.0", "method": method, "params": params}
            if identifier is not None:
                message["id"] = identifier
            body = json.dumps(message).encode()
            process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
            process.stdin.flush()

        def request(method, params):
            nonlocal request_id
            request_id += 1
            send(method, params, request_id)
            while True:
                response = messages.get(timeout=60)
                if response.get("method") == "textDocument/publishDiagnostics":
                    assert not response["params"]["diagnostics"], response
                if response.get("id") == request_id:
                    if method != "bench/barrier":
                        assert "error" not in response, response
                    return response

        try:
            request("initialize", {"capabilities": {}})
            for document in range(documents):
                document_uri = uri if document == 0 else (pathlib.Path(directory) / f"other{document}.dodo").as_uri()
                send("textDocument/didOpen", {"textDocument": {
                    "uri": document_uri, "languageId": "dodo", "version": 1, "text": source,
                }})
            request("bench/barrier", None)
            timings = {"change_to_diagnostics": [], "hover": []}
            if features:
                timings.update({"completion": [], "definition": [], "signatureHelp": []})
            for sample in range(samples + 5):
                start = time.perf_counter_ns()
                send("textDocument/didChange", {
                    "textDocument": {"uri": uri, "version": sample + 2},
                    "contentChanges": [{"text": source + f"// edit {sample}\n"}],
                })
                request("bench/barrier", None)
                if sample >= 5:
                    timings["change_to_diagnostics"].append((time.perf_counter_ns() - start) / 1e6)
                for method in sorted(timings.keys() - {"change_to_diagnostics"}):
                    start = time.perf_counter_ns()
                    request("textDocument/" + method, {
                        "textDocument": {"uri": uri},
                        "position": {"line": functions + 1 + int(stdlib), "character": 34 if method == "signatureHelp" else 26},
                    })
                    if sample >= 5:
                        timings[method].append((time.perf_counter_ns() - start) / 1e6)
            request("shutdown", None)
            send("exit", None)
            assert process.wait(timeout=10) == 0
            assert not process.stderr.read()
            return {"functions": functions, "bytes_per_document": len(source.encode()), "documents": documents, "stdlib_math": stdlib, "samples": samples,
                    "milliseconds": {method: {
                        "p50": round(statistics.median(values), 3),
                        "p95": round(sorted(values)[math.ceil(len(values) * .95) - 1], 3),
                        "max": round(max(values), 3),
                    } for method, values in timings.items()}}
        finally:
            if process.poll() is None:
                process.kill()
            process.communicate()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=str)
    parser.add_argument("--functions", type=int, nargs="+", default=[20, 200, 1000])
    parser.add_argument("--samples", type=int, default=30)
    parser.add_argument("--features", action="store_true", help="also measure new editor requests")
    parser.add_argument("--documents", type=int, default=1, help="number of open file-mode buffers")
    parser.add_argument("--stdlib", action="store_true", help="include the bundled std/math dependency")
    args = parser.parse_args()
    if args.samples < 1 or args.documents < 1 or any(n < 1 for n in args.functions):
        parser.error("samples and function counts must be positive")
    for size in args.functions:
        print(json.dumps(measure(args.binary, size, args.samples, args.features, args.documents, args.stdlib)), flush=True)
