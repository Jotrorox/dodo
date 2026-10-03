#!/usr/bin/env python3
"""Measure real stdio LSP edits and queries; no portable latency thresholds."""
import argparse
import datetime
import hashlib
import json
import math
import os
import pathlib
import platform
import queue
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass


DEFAULT_SIZES = {
    "functions": [20, 200, 1000],
    "large-function": [200, 1000, 2000],
    "errors": [20, 100, 200],
    "generic-imports": [10, 50, 100],
    "project": [20, 200],
}
CALLER = "fn main() -> i32 { return value_0(1) }\n"
HELPER = "fn value_0(x: i32) -> i32 { return x + 0 }\n"


@dataclass
class Fixture:
    sources: dict
    disk_files: dict
    initialize: dict
    errors: dict
    edit_expression: str = "return x + 0"


def fixture(workload, size, documents, stdlib, project_files, check_mode, body_shape="locals"):
    """Deterministic ASCII fixtures: sizes count bodies, statements, or types."""
    header = "package bench\n" + ('import "std/math"\n' if stdlib else "")
    errors = []
    disk_files = {}
    initialize = {"capabilities": {}, "initializationOptions": {"checkMode": check_mode}}
    edit_expression = "return x + 0"
    if workload in ("functions", "project"):
        source = header + "".join(
            f"fn value_{i}(x: i32) -> i32 {{ return x + {i} }}\n" for i in range(size)
        ) + CALLER
    elif workload == "large-function":
        edit_expression = "return total + 0"
        source = header + "fn value_0(x: i32) -> i32 {\n    total := x\n"
        if body_shape == "assignments":
            source += "    scratch := 0i32\n" + "".join(
                f"    scratch = total + {i}\n    total = scratch\n" for i in range(size)
            )
        else:
            source += "".join(
                f"    local_{i} := total + {i}\n    total = local_{i}\n" for i in range(size)
            )
        source += "    return total + 0\n}\n" + CALLER
    elif workload == "errors":
        source = header
        for i in range(size):
            line = source.count("\n")
            if i % 2 == 0:
                source += f"fn syntax_{i}() {{ broken := ; }}\n"
                errors.append((line, "expected"))
            else:
                source += f"fn semantic_{i}() -> i32 {{ return missing_{i} }}\n"
                errors.append((line, f"unknown binding `missing_{i}`"))
        source += HELPER + CALLER
    elif workload == "generic-imports":
        source = header + ('import "std/collections"\n'
                           'import "std/collections/fixed_vector"\n'
                           'import "std/collections/shared_vector"\n'
                           'import "std/collections/shared_hash_map"\n')
        # Distinct nominal types force new Vector<T> and method specializations.
        for i in range(size):
            source += (f"struct Record_{i} {{ value: i32 }}\n"
                       f"fn specialize_{i}() -> i32!collections.CapacityError {{\n"
                       f"    slots: [1]Option<Record_{i}> = [none]\n"
                       "    values := fixed_vector.Vector.new(&mut slots)\n"
                       f"    values.push(Record_{i} {{ value: {i} }})?\n"
                       "    match values.pop() {\n"
                       "        some(value) => { return ok(value.value) },\n"
                       "        none => { return ok(0) },\n"
                       "    }\n}\n")
        source += HELPER + CALLER
    else:
        raise ValueError(workload)
    sources = {"main.dodo": source}
    expected = {"main.dodo": errors}
    for document in range(1, project_files if workload == "project" else documents):
        name = f"other{document}.dodo"
        if workload == "project":
            text = header + "".join(
                f"fn file_{document}_value_{i}(x: i32) -> i32 {{ return x + {i} }}\n"
                for i in range(size)
            )
            disk_files[name] = text
        else:
            # File checking keeps identical declarations in separate programs.
            text = source
        if document < documents:
            sources[name] = text
            expected[name] = errors
    if workload == "project":
        disk_files["main.dodo"] = source
        # Multiple targets without a default require explicit target selection.
        disk_files["dodo.toml"] = ("schema = 1\n[targets.bench]\nentry = 'main.dodo'\n"
                                   "[targets.wasm]\nentry = 'main.dodo'\n"
                                   "emit = 'obj'\ntriple = 'wasm32-unknown-unknown'\n")
        initialize["initializationOptions"].update(manifestPath="dodo.toml", buildTarget="bench")
    return Fixture(sources, disk_files, initialize, expected, edit_expression)


def validate_diagnostics(publications, expected, versions):
    """Require every open buffer's current version and only the intended errors."""
    if set(publications) != set(expected):
        raise AssertionError(f"diagnostic URIs: expected {set(expected)}, got {set(publications)}")
    for uri, errors in expected.items():
        publication = publications[uri]
        if publication.get("version") != versions[uri]:
            raise AssertionError(f"stale diagnostics for {uri}: {publication}")
        diagnostics = publication["diagnostics"]
        actual = sorted(diagnostics, key=lambda d: d["range"]["start"]["line"])
        if len(actual) != len(errors):
            raise AssertionError(f"expected {len(errors)} diagnostics for {uri}, got {diagnostics}")
        for diagnostic, (line, message) in zip(actual, errors):
            if (diagnostic.get("severity") != 1
                    or diagnostic["range"]["start"]["line"] != line
                    or message not in diagnostic["message"]):
                raise AssertionError(f"expected error on line {line} containing {message!r}: {diagnostic}")


def query_position(source, method):
    offset = source.index("value_0(1)") + (len("value_0(") if method == "signatureHelp" else 3)
    preceding = source[:offset]
    return {"line": preceding.count("\n"), "character": len(preceding.rsplit("\n", 1)[-1])}


def validate_query(method, response, uri):
    result = response.get("result")
    if method == "completion":
        valid = result and any(item["label"] == "value_0" for item in result["items"])
    elif method == "definition":
        valid = result and result.get("uri") == uri
    elif method == "signatureHelp":
        valid = result and any("value_0" in s["label"] for s in result["signatures"])
    else:
        valid = result and "value_0" in json.dumps(result["contents"])
    if not valid:
        raise AssertionError(f"empty or incorrect {method} response: {response}")


def percentiles(values):
    return {"p50": round(statistics.median(values), 3),
            "p95": round(sorted(values)[math.ceil(len(values) * .95) - 1], 3),
            "max": round(max(values), 3)}


def measure(binary, workload, size, samples, features, documents, stdlib,
            project_files, check_mode, edit, warmup=5, body_shape="locals"):
    data = fixture(workload, size, documents, stdlib, project_files, check_mode, body_shape)
    with tempfile.TemporaryDirectory(prefix="dodo-lsp-bench-") as directory:
        # Canonicalize macOS /var -> /private/var to match the compiler's URI keys.
        root = pathlib.Path(directory).resolve()
        for name, text in data.disk_files.items():
            (root / name).write_text(text, encoding="utf-8")
        uris = {name: (root / name).as_uri() for name in data.sources}
        uri = uris["main.dodo"]
        source = data.sources["main.dodo"]
        expected = {uris[name]: errors for name, errors in data.errors.items()}
        versions = dict.fromkeys(uris.values(), 1)
        initialize = data.initialize
        if workload == "project":
            initialize["rootUri"] = root.as_uri()
        process = subprocess.Popen([binary, "lsp"], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        messages = queue.Queue()

        def read():
            try:
                while header := process.stdout.readline():
                    length = int(header.split(b":", 1)[1])
                    if process.stdout.readline() != b"\r\n":
                        raise ValueError("invalid LSP frame")
                    messages.put(json.loads(process.stdout.read(length)))
                messages.put(EOFError("LSP exited before the response"))
            except Exception as error:
                messages.put(error)

        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        request_id = 0
        publications = {}

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
                if isinstance(response, Exception):
                    raise response
                if response.get("method") == "textDocument/publishDiagnostics":
                    params = response["params"]
                    publications[params["uri"]] = params
                elif "method" in response:
                    raise AssertionError(f"unexpected server message: {response}")
                if response.get("id") == request_id:
                    if method == "bench/barrier":
                        if response.get("error", {}).get("code") != -32601:
                            raise AssertionError(f"expected MethodNotFound barrier: {response}")
                    elif "error" in response:
                        raise AssertionError(response)
                    return response

        try:
            request("initialize", initialize)
            send("initialized", {})
            for name, text in data.sources.items():
                send("textDocument/didOpen", {"textDocument": {
                    "uri": uris[name], "languageId": "dodo", "version": 1, "text": text,
                }})
            request("bench/barrier", None)
            validate_diagnostics(publications, expected, versions)
            publications.clear()
            change_metric = "manifest_to_diagnostics" if edit == "manifest" else "change_to_diagnostics"
            timings = {change_metric: [], "hover": []}
            if features:
                timings.update({"completion": [], "definition": [], "signatureHelp": []})
            positions = {method: query_position(source, method)
                         for method in timings if method != change_metric}
            for sample in range(samples + warmup):
                if edit == "comment":
                    changed = source + f"// edit {sample}\n"
                else:
                    changed = source.replace(data.edit_expression,
                                             data.edit_expression[:-1] + str(sample % 2), 1)
                start = time.perf_counter_ns()
                if edit == "manifest":
                    manifest = root / "dodo.toml"
                    triple = "wasm32-unknown-unknown" if sample % 2 else "x86_64-unknown-linux-gnu"
                    text = data.disk_files["dodo.toml"].replace(
                        "[targets.bench]\n", f"[targets.bench]\ntriple = '{triple}'\n")
                    manifest.write_text(text, encoding="utf-8")
                    send("workspace/didChangeWatchedFiles", {
                        "changes": [{"uri": manifest.as_uri(), "type": 2}],
                    })
                else:
                    versions[uri] = sample + 2
                    send("textDocument/didChange", {
                        "textDocument": {"uri": uri, "version": versions[uri]},
                        "contentChanges": [{"text": changed}],
                    })
                request("bench/barrier", None)
                elapsed = (time.perf_counter_ns() - start) / 1e6
                validate_diagnostics(publications, expected, versions)
                publications.clear()
                if sample >= warmup:
                    timings[change_metric].append(elapsed)
                for method in sorted(positions):
                    start = time.perf_counter_ns()
                    response = request("textDocument/" + method, {
                        "textDocument": {"uri": uri}, "position": positions[method],
                    })
                    elapsed = (time.perf_counter_ns() - start) / 1e6
                    validate_query(method, response, uri)
                    if sample >= warmup:
                        timings[method].append(elapsed)
            request("shutdown", None)
            send("exit", None)
            if process.wait(timeout=10) != 0:
                raise AssertionError("LSP exited unsuccessfully")
            stderr = process.stderr.read()
            if stderr:
                raise AssertionError(stderr.decode(errors="replace"))
            return {"workload": workload, "size": size, "edit": edit,
                    "body_shape": body_shape if workload == "large-function" else None,
                    "functions": size if workload in ("functions", "project") else None,
                    "bytes_per_document": len(source.encode()),
                    "document_bytes": {name: len(text.encode()) for name, text in data.sources.items()},
                    "documents": documents, "project_files": project_files if workload == "project" else None,
                    "check_mode": check_mode, "stdlib_math": stdlib,
                    "expected_errors_per_document": len(data.errors["main.dodo"]),
                    "samples": samples, "warmup": warmup,
                    "milliseconds": {method: percentiles(values) for method, values in timings.items()}}
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()
            reader.join(timeout=1)
            for pipe in (process.stdin, process.stdout, process.stderr):
                pipe.close()


def output(command):
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=30, check=True)
        return result.stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return None


def provenance(binary, compiler_revision, build_profile):
    prefix = os.environ.get("LLVM_SYS_231_PREFIX")
    return {"timestamp_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "platform": platform.platform(), "machine": platform.machine(),
            "processor": output(["sysctl", "-n", "machdep.cpu.brand_string"]) or platform.processor(),
            "logical_cpus": os.cpu_count(), "python": platform.python_version(),
            "rustc": output(["rustc", "-Vv"]), "cargo": output(["cargo", "-V"]),
            "llvm": output([str(pathlib.Path(prefix) / "bin/llvm-config") if prefix else "llvm-config", "--version"]),
            "compiler": binary, "compiler_version": output([binary, "--version"]),
            "compiler_revision": compiler_revision, "build_profile": build_profile,
            "compiler_sha256": hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest(),
            "benchmark_sha256": hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
            "git_commit": output(["git", "rev-parse", "HEAD"]),
            "git_status": output(["git", "status", "--short"]), "command": sys.argv}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("--workload", choices=DEFAULT_SIZES, nargs="+", default=["functions"])
    parser.add_argument("--size", "--functions", dest="sizes", type=int, nargs="+",
                        help="function count, local pairs, errors, or generic types; defaults vary by workload")
    parser.add_argument("--samples", type=int, default=30)
    parser.add_argument("--features", action="store_true", help="also measure completion, definition, signature help")
    parser.add_argument("--documents", type=int, help="open buffers (default: 5 for project, 1 otherwise)")
    parser.add_argument("--project-files", type=int, default=20, help="total project files, including closed disk files")
    parser.add_argument("--check-mode", choices=["file", "package"], help="default: package for project, file otherwise")
    parser.add_argument("--stdlib", action="store_true", help="also import bundled std/math")
    parser.add_argument("--edit", choices=["body", "comment", "manifest"], default="body")
    parser.add_argument("--body-shape", choices=["locals", "assignments"], default="locals",
                        help="large-function: grow local state or reuse two locals at the same statement count")
    parser.add_argument("--report", type=pathlib.Path, help="save environment and results as JSON as each run completes")
    parser.add_argument("--compiler-revision", help="record the source commit used to build the compiler")
    parser.add_argument("--build-profile", help="record the compiler's Cargo build profile")
    args = parser.parse_args()
    if (args.samples < 1 or args.project_files < 1
            or (args.documents is not None and args.documents < 1)
            or (args.sizes is not None and any(n < 1 for n in args.sizes))):
        parser.error("samples, sizes, documents, and project-files must be positive")
    for workload in args.workload:
        if args.edit == "manifest" and (workload != "project" or args.stdlib):
            parser.error("manifest edits require the project workload without platform-specific stdlib imports")
        documents = args.documents if args.documents is not None else (5 if workload == "project" else 1)
        if workload == "project" and documents > args.project_files:
            parser.error("project-files must be at least the number of open documents")
        mode = args.check_mode or ("package" if workload == "project" else "file")
        if workload != "project" and mode == "package" and documents > 1:
            parser.error("multi-buffer package checking requires the project workload's unique declarations")
    binary = str(args.binary.resolve())
    if not pathlib.Path(binary).is_file():
        parser.error(f"compiler not found: {binary}; build Dodo first")
    report = {"environment": provenance(binary, args.compiler_revision, args.build_profile),
              "results": []} if args.report else None
    for workload in args.workload:
        documents = args.documents if args.documents is not None else (5 if workload == "project" else 1)
        mode = args.check_mode or ("package" if workload == "project" else "file")
        for size in args.sizes or DEFAULT_SIZES[workload]:
            result = measure(binary, workload, size, args.samples, args.features, documents,
                             args.stdlib, args.project_files, mode, args.edit, body_shape=args.body_shape)
            print(json.dumps(result), flush=True)
            if report is not None:
                report["results"].append(result)
                args.report.parent.mkdir(parents=True, exist_ok=True)
                args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
