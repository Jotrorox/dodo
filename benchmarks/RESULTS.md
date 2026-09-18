# JSON and web baseline: 2026-09-18

Measured at revision `7baf51dd2ffa41e9ad3f644df4b4b7be939dce8d` on Windows 11 x64
(build 26200), Intel Core Ultra 5 125U (12 cores / 14 logical processors),
with Dodo 0.1.3 (LLVM 23, BSD-2-Clause) generating native `-O 3` code and Python 3.14.7.
The compiler was rebuilt from the checkout before measurement. No standard
library implementation was changed for this baseline.

Seven measured batches per case; in-process batches target 0.2 seconds after
calibration. HTTP uses 200 requests per client per sample after ten warmups.
See [the benchmark guide](README.md) for workload definitions, timing boundaries,
and limitations. Raw local samples and build metadata are in
`benchmark-data/stdlib.json`; rerunning the runner regenerates that ignored file.

## JSON

Times are median batch averages; ranges are the minimum and maximum batch
averages, not individual-operation percentiles. Throughput counts input bytes.

| Operation | Input bytes | Median µs/op | Range µs/op | MiB/s |
| --- | ---: | ---: | ---: | ---: |
| json.parse.record | 55 | 0.433 | 0.417–0.463 | 121.19 |
| json.decode.derived | 55 | 1.218 | 1.206–1.237 | 43.07 |
| json.encode.derived | 55 | 0.673 | 0.654–0.685 | 78.00 |
| json.parse.escaped | 2,011 | 10.475 | 10.340–10.606 | 183.09 |
| json.parse.array_256 | 739 | 2.518 | 2.467–2.599 | 279.90 |
| json.parse.array_4096 | 11,879 | 39.974 | 39.041–42.594 | 283.40 |
| json.parse.array_16384 | 47,513 | 172.311 | 163.111–189.960 | 262.97 |
| json.parse.object_32 | 339 | 40.516 | 39.534–42.061 | 7.98 |
| json.parse.object_256 | 2,958 | 2212.489 | 2135.582–2352.160 | 1.28 |
| json.parse.object_1024 | 12,197 | 33322.760 | 32332.220–39862.300 | 0.35 |

The largest performance limitation in these JSON cases is wide-object parsing.
Increasing distinct fields from 256 to 1,024 multiplies parsing time by about
15× for 4× as many fields. The parser explicitly uses quadratic duplicate-name
checking with constant auxiliary storage: each new key is compared with earlier
keys ([implementation](../stdlib/std/encoding/json/value.dodo)). This is consistent
with the observed growth. Arrays remain around 263–283 MiB/s in this run.

## Web in process

Router cases cycle through every registered literal path. Construction and index
building are excluded; path/method validation and lookup are included.

| Operation | Median ns/op | Range ns/op | Million ops/s |
| --- | ---: | ---: | ---: |
| web.router.sorted.8 | 50.8 | 47.9–56.2 | 19.70 |
| web.router.indexed.8 | 41.9 | 41.4–42.8 | 23.89 |
| web.router.unsorted.8 | 161.9 | 161.0–163.6 | 6.18 |
| web.decode_path | 37.2 | 37.0–38.0 | 26.87 |
| web.application | 756.9 | 748.5–772.8 | 1.32 |
| web.router.sorted.64 | 63.5 | 62.2–67.2 | 15.74 |
| web.router.indexed.64 | 57.0 | 55.3–57.7 | 17.54 |
| web.router.unsorted.64 | 907.9 | 884.8–933.5 | 1.10 |
| web.router.sorted.256 | 90.8 | 86.5–92.7 | 11.01 |
| web.router.indexed.256 | 66.4 | 65.4–71.1 | 15.07 |
| web.router.unsorted.256 | 3652.3 | 3559.6–3718.5 | 0.27 |

At 256 routes, indexing is about 55× faster than an unsorted literal table for
this workload; a sorted table is about 40× faster. Sorted literal routing uses
binary search, optional indexing uses a caller-owned hash table, and the
unsorted case scans the route table ([implementation](../stdlib/std/web.dodo)).
The application case includes parameter matching, query decoding, response
storage, and handler execution; its rate should not be treated as HTTP throughput.

## HTTP over loopback

All responses are HTTP 200 with `Content-Type: application/json` and the 11-byte
body `{"ok":true}`. Rates are medians of seven whole-sample rates; latency
percentiles combine all measured requests for each case.

| Execution | Clients | Connection policy | Requests/s | Rate range | p50 latency µs | p95 latency µs | p99 latency µs |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| serial | 1 | New connection per request | 127 | 100–139 | 1872.2 | 18045.0 | 23282.6 |
| concurrent | 1 | Keep-alive | 5827 | 4839–7504 | 133.9 | 292.2 | 437.0 |
| concurrent | 8 | Keep-alive | 7025 | 6513–7590 | 947.0 | 2519.5 | 3740.2 |

These are local, closed-loop Python-client measurements. The serial run includes
connection establishment on every request, so its difference from concurrent
mode is not a controlled comparison of execution policy alone. Both client and
server share the machine; Python client work, scheduling, and Windows socket
behavior affect the rates. The concurrency-eight result does not establish
server saturation or maximum capacity. No cause for the serial tail latency was
isolated by this benchmark.

## Verification

All 24 cases completed with checked results at O3. An O0 smoke run also exercises
all cases with a single short sample; its timings are not a comparison baseline.
Dodo fixtures pass the formatter check, and the Python runner compiles. Runtime
checks verify every batch checksum and every HTTP response. No performance
thresholds are imposed.
