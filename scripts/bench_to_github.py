#!/usr/bin/env python3
"""Convert a bench_stdlib.py report for github-action-benchmark.

Writes the `customSmallerIsBetter` format: the median nanoseconds per operation
for in-process results, and nanoseconds per request for loopback HTTP results.
"""

import argparse
import json
from pathlib import Path


def entries(report):
    for result in report["results"]:
        name = f"{result['name']} -O{result['optimization']}"
        if result["kind"] == "in_process":
            timings = result["ns_per_op"]
            yield {"name": name, "unit": "ns/op", "value": timings["median"],
                   "range": f"{timings['min']:.3f} to {timings['max']:.3f}"}
        elif result["kind"] == "loopback_http":
            rates = result["requests_per_second"]
            yield {"name": name, "unit": "ns/request", "value": 1e9 / rates["median"],
                   "range": f"{1e9 / rates['max']:.0f} to {1e9 / rates['min']:.0f}",
                   "extra": f"p95 latency {result['latency_us']['p95']:.1f} us"}
        else:
            raise ValueError(f"Unknown benchmark kind: {result['kind']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    report = json.loads(args.report.read_text(encoding="utf-8"))
    results = list(entries(report))
    if not results:
        parser.error(f"{args.report} contains no results")
    args.output.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {len(results)} benchmarks to {args.output}")


if __name__ == "__main__":
    main()
