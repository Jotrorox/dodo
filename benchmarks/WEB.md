# Web routing and header fast paths: 2026-09-18

The web library now traverses copied headers sequentially, retains application
routing metadata, reuses validated paths and captured parameters, and indexes
dynamic routes and hosted tables above 1,024 entries.

`header_next` advances a caller-owned byte cursor. Hosted serialization and
in-process response lookup use it; response header storage length is available
without enumeration. Ordinal `header_at` remains available.

Application registration owns validated routes and index slots together, so the
table can move without self-references. Requests borrow a router view instead of
rebuilding it. `testing.send_router` also accepts an existing router. `Path` holds
an immutable validation proof, and requests capture the first eight named spans
while matching. Additional parameters remain supported without repeating the
complete match on access.

The routing hash table indexes complete literals and literal prefixes before the
first parameter or wildcard. Lookup probes segment boundaries and verifies
candidate patterns, preserving specificity, HEAD fallback, and 404/405 selection.
Routes sharing a prefix still scan within that group. Both hosted runners use
inline index storage through 1,024 routes and one owned native allocation for
larger tables; request routing does not allocate. Portable routers continue to
use caller-owned storage.

## Measurements

Windows 11 x64 build 26200, Intel Core Ultra 5 125U, native Dodo 0.1.3 / LLVM 23,
`-O 3`. Processes were restricted to logical CPU 0. The microbenchmarks use five
measured batches targeting 0.1 seconds after discarded calibration. Times below
are medians, comparing APIs in the same rebuilt compiler.

| Operation | Existing API | Fast path | Ratio |
| --- | ---: | ---: | ---: |
| Enumerate 32 headers | 5.953 µs | 0.372 µs | 16.0× |
| Read a named parameter | 68.3 ns | 5.7 ns | 12.0× |
| Lookup a validated path | 135.4 ns | 52.2 ns | 2.6× |
| Lookup among 256 dynamic routes | 4.259 µs | 0.349 µs | 12.2× |
| Lookup among 2,048 dynamic routes | 45.018 µs | 0.605 µs | 74.4× |

Header names are `X-Header`, with five-byte values. Parameter reads cycle among
three names. Capture construction and path validation are outside these individual
timers. Dynamic tables use `/group/NNNN/:id`; these results do not predict the
benefit for routes sharing a literal prefix. Checksums and selected route IDs are
verified. Loop, volatile selection, and checksum costs remain included.

The complete application comparison builds identical fixtures with an unchanged
compiler from `ecd7efc2127130dcf66f8d1016ad7bf505ce8ad2` and the updated compiler.
It alternates their execution order, discards one warmup per compiler, and measures
five batches of **1,000,000 requests** each. It includes request construction,
escaped-path and query decoding, routing, capture construction, handler execution,
and an owned response.

| Application request | Before | After |
| --- | ---: | ---: |
| Median | 0.809 µs | 0.582 µs |
| Batch-average range | 0.793–0.853 µs | 0.555–0.593 µs |

That is about **28% less time per request** in this comparison. Shorter runs varied
substantially on this machine, including for unchanged operations; these are local
observations, not timing thresholds or hosted HTTP throughput claims.

```sh
cargo build --locked --bin dodo
python scripts/bench_stdlib.py --suite web --route-sizes 8 64 256 2048 --samples 5 --seconds 0.1 --report benchmark-data/web.json
```

Use a configured native compiler/linker and OS affinity controls for comparable
runs. Raw samples and compiler/source hashes for this run are saved locally in
`benchmark-data/web-after.json`; the alternating application samples and fixture
hashes are in `benchmark-data/web-application-before-after.json`.

## Validation

Nine web integration tests passed across the broad and focused runs, covering
serial/concurrent serving, middleware, shutdown, backpressure, static files,
ownership rejection, and portable routing. Portable fixtures execute at O0/O3
and emit objects for `wasm32-unknown-unknown` and `thumbv6m-none-eabi`.

New checks cover repeated/empty/truncated headers, immutable path proofs, mixed
route precedence, HEAD/405, captured and overflow parameters, invalid request
indices, failed-registration rollback, and a 1,026-entry mixed hosted index.
Index storage is exercised at 0, 1, 1,024, 1,025, and 4,096 routes, including
size overflow and destruction. Formatting, Python compilation, and whitespace
checks pass.

The separate fragmented-request test sometimes receives an empty response after
early rejection on Windows. This also reproduces using the unchanged starting
revision, so the full web suite is not reported as green. That test and the
transport behavior were left unchanged.
