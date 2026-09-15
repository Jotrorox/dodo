# Stdlib web server optimization report

Implemented and measured on 2026-09-15 against the original baseline at
revision `546985e48b6f2b0e5dc530b54088c8304e853775`.

The implementation stays within Dodo's stdlib and its existing native socket
adapter. It adds no HTTP framework, allocator, task runtime, or package dependency.
Raw measurements, generated fixtures, benchmark tooling, binaries, and logs
remain in the Git-ignored `/benchmark-data/` directory.

The final keep-alive check reached **208,026 requests/s versus 44,436 originally
(4.68×)**. In the main workload matrix, header-heavy serial throughput increased
**2.12×**. Fresh-connection throughput at eight clients remained approximately
flat; the report includes CPU overhead and the one-byte parser regression.

## What changed

| Area | Implementation | Effect / contract |
| --- | --- | --- |
| Socket concurrency | New [`std/web/reactor`](../stdlib/std/web/reactor.dodo), backed by a bounded `PollSet` in the native network adapters | One thread progresses up to 255 caller-provided connection slots. Slow socket input/output does not occupy the only execution lane. |
| HTTP reuse | Keep-alive and ordered pipelining in the reactor | Default 100 requests per connection and 15-second idle timeout; unread pipelined bytes survive message transitions. |
| Routing | Sorted literal binary search; optional caller-owned hash index; combined specificity/method scan for dynamic routes | Hosted servers choose a bounded literal index automatically. Exact path comparison resolves hash collisions. Duplicate shapes, specificity, method errors, and HEAD fallback retain their semantics. |
| Parsing | Bounded eight-byte line copying and header-value validation with scalar handling of delimiters and exceptional bytes | Unaligned reads/writes remain entirely inside their slices. There are no sentinel overreads or target-specific SIMD requirements. |
| Response output | Coalesce small bodies with unsent headers; send remaining fixed-length body bytes directly from caller storage | Removes staging copies for larger bodies and reduces write calls. Partial writes, pending writes, chunked framing, and error poisoning retain explicit progress. |
| Socket setup | Remove redundant Linux `fcntl` calls after sockets already opened with nonblocking and close-on-exec flags | Atomic socket flags remain in place; the Windows setup path is preserved. |
| Admission | Default listen backlog raised from 16 to 256 | Reduces queue overflow and retransmission delays during bursts of new connections. The kernel may constrain the requested backlog. |
| Protocol storage | Opaque parser/connection state can be suspended and resumed against caller buffers | Enables bounded peer storage without retaining mutable buffer borrows between reactor turns. |

The existing `hosted.serve` API receives the shared routing, parsing, output, and
socket improvements. It still handles one request per connection serially. Use
the new reactor to obtain concurrent socket progress and connection reuse.

## Using the concurrent server

The runnable example is
[`examples/web_server_concurrent.dodo`](../examples/web_server_concurrent.dodo).
Build it with the compiler containing the updated embedded stdlib:

```sh
cargo build --locked --bin dodo
target/debug/dodo compile examples/web_server_concurrent.dodo -O 3 -o target/web-server
./target/web-server
```

Use the repository's LLVM/native-library environment when building from source.
The example listens on `127.0.0.1:8080`. Its handler uses the same structural
interface as the serial example. Supply slots, workspace, request buffers, and
response buffers to `reactor.serve`; `reactor.serve_with` also accepts a clock and
cancellation provider. See the [web guide](src/content/docs/web.md) for the full
configuration and buffer ownership contracts.

The example has eight slots, 38 KiB protocol workspace per slot, and 4 KiB each of
request and response storage per slot: 368 KiB of byte arrays in total, plus slot
state, a bounded route index, and poll bookkeeping. Request and response arrays
are partitioned equally across slots; their capacities also obey configured body
limits. More slots require proportionally more explicitly supplied memory.

## Measurement method

The original compiler binary was saved before changing stdlib sources. Both that
binary and a rebuilt candidate compile identical generated applications at `-O 3`.
The only serving-model difference is the explicit eight-slot reactor versus the
serial API. Tests use defaults, including the old backlog of 16 and new backlog
of 256. This is a comparison of shipped defaults; the high-concurrency result
must not be attributed entirely to faster request handling.

Environment: Intel Core Ultra 5 125U, Fedora Linux, LLVM 23.1.1, Rust 1.98.1.
The server is pinned to CPU 0 and the C load generator to CPU 2, on separate
physical performance cores. Each case gets a 0.5-second warmup and three
3-second admission windows, with all outstanding requests drained afterward.
Version/repetition order is shuffled within each workload. Compilation, focused
microbenchmarks, instrumentation, and test suites run outside timed load windows.

The client validates HTTP/1.1 status, Content-Length, and exact body bytes.
Fresh-connection measurements include connection establishment and EOF. Reuse
measurements issue one outstanding request per socket, validate each complete
response, and reconnect when the server closes. Baseline and updated serial
servers advertise close even when the client requests reuse. Pipeline correctness
is tested separately. This is closed-loop loopback load, not an open-loop arrival
rate test or a remote deployment capacity guarantee.

Report values are medians of three runs; per-run latency percentiles are not
pooled. CPU time comes from `/proc` process accounting and includes kernel time
charged to the server. One hundred percent CPU represents one logical core.
Memory, thread count, and open descriptors are sampled every 50 ms. Call counting
uses a separate instrumented run and supplies counts, not syscall timing.

## HTTP results

All **81 measured runs** completed with **11,133,393 validated responses and zero failures**.

The strongest throughput gains are **4.81× for keep-alive** (reactor) and
**2.12× for header-heavy requests** (updated serial server). Small fresh-connection
throughput at eight clients is effectively unchanged, despite lower serial CPU.

`c` is the number of concurrent client sockets. All rows except the reuse row
open a fresh connection for every request. The reactor has eight active slots,
including in the c128 experiment.

| Workload | Original requests/s | Updated serial requests/s | Reactor requests/s |
| --- | ---: | ---: | ---: |
| Small response, c1 | 26,080 | 31,626 | 33,288 |
| Small response, c8 | 47,672 | 47,676 | 47,932 |
| Small response, c128 | 17,314 | 49,321 | 49,588 |
| Small response, c8, reuse requested | 47,268 | 46,792 | 227,324 |
| 1,000 sorted literal routes, c8 | 30,790 | 47,384 | 47,023 |
| 1,000 shuffled literal routes, c8 | 42,208 | 47,595 | 46,539 |
| 95 extra headers, c8 | 21,031 | 44,652 | 39,695 |
| 64 KiB response, c8 | 23,315 | 23,654 | 23,377 |
| 64 KiB upload, c8 | 33,843 | 34,367 | 33,997 |

For the reuse row, run ranges were 41,921–47,411 (original), 44,847–49,776
(updated serial), and 223,955–232,394 (reactor) requests/s. Header-heavy ranges
were 20,824–21,106, 43,415–44,856, and 39,654–40,282 respectively. These ranges
are observed variation over three short runs, not confidence intervals.

| Workload | Original CPU µs/request | Updated serial CPU µs/request | Reactor CPU µs/request |
| --- | ---: | ---: | ---: |
| Small response, c1 | 10.43 | 8.33 | 7.70 |
| Small response, c8 | 7.46 | 6.05 | 8.07 |
| Small response, c128 | 7.33 | 5.07 | 5.30 |
| Small response, c8, reuse requested | 7.40 | 6.20 | 3.14 |
| 1,000 sorted literal routes, c8 | 25.33 | 5.91 | 7.94 |
| 1,000 shuffled literal routes, c8 | 17.57 | 5.89 | 8.06 |
| 95 extra headers, c8 | 39.79 | 18.66 | 20.82 |
| 64 KiB response, c8 | 18.72 | 12.48 | 14.60 |
| 64 KiB upload, c8 | 17.45 | 15.32 | 19.32 |

The updated serial server uses about 19% less CPU for small c8 requests, 53%
less for header-heavy requests, 77% less for sorted 1,000-route requests, and
33% less for 64 KiB responses. The reactor has bookkeeping costs: with fresh
connections it uses about 8% more CPU than the original on small c8 requests,
and 11% more on 64 KiB uploads. It earns its largest throughput gain through reuse.

| Case | Version | p99 latency, µs | Median run maximum, ms |
| --- | --- | ---: | ---: |
| Small response, c8, reuse requested | baseline | 188 | 3.05 |
| Small response, c8, reuse requested | serial | 233 | 1.34 |
| Small response, c8, reuse requested | reactor | 59 | 1.66 |
| Small response, c128 | baseline | 658 | 7,637.34 |
| Small response, c128 | serial | 2,292 | 3.19 |
| Small response, c128 | reactor | 2,313 | 3.27 |

At c128, the original had a wide throughput range (10,068–30,442 requests/s)
because a small number of queued connection attempts waited through
retransmissions. A low p99 hides those slowest requests. The new default backlog
largely removes these drain tails; the serial server benefits as much as the
reactor. This result measures admission behavior as well as request execution.

### Final reactor verification

After the main matrix, review added a guard preventing an error head from being
appended after a partially written informational response. The final embedded
stdlib was rebuilt; its reactor behavior and Windows-object checks passed again.
The following small-response comparison rechecked that build, with the same
client, pinning, warmup, and three repetitions per configuration.

| Eight client sockets | Original requests/s | Final reactor requests/s |
| --- | ---: | ---: |
| Fresh connection per request | 44,903 | 44,029 |
| Reuse requested | 44,436 | 208,026 |

These 12 runs validated **3,057,561 responses with zero failures**.
Keep-alive throughput was **4.68×** the original. Absolute rates varied from the
main matrix; the final comparison still shows a large reuse gain and no
fresh-connection throughput gain. Evidence and final compiler/executable hashes
are preserved separately in `benchmark-data/web-optimization-2026-09-15/final-verification/`.

## Focused CPU measurements

Each microbenchmark alternates routes or consumes a complete parser message and
checks a checksum. Fast lookup cases use five million iterations to avoid the
coarse one-millisecond timer dominating the result. These synthetic hot-cache
numbers describe these paths and tables, not arbitrary application requests.

| Workload | Original ns/operation | Updated ns/operation |
| --- | ---: | ---: |
| 1 literal route | 26.2 | 14.8 |
| 100 sorted literal routes | 1,070.0 | 33.2 |
| 1,000 sorted literal routes | 8,180.0 | 43.0 |
| 40-byte request, full input | 161.0 | 93.0 |
| 8,020-byte request, 95 extra headers | 23,370.0 | 6,630.0 |
| 40-byte request, one-byte fragments | 215.0 | 237.0 |
| 1,000 shuffled literals, explicit index for updated router | 7,310.0 | 16.0 |

Large-header parsing is approximately 3.5× faster in isolation. Eight-byte
scanning retains exact validation while reducing per-byte state-machine work.
The one-byte fragmentation case increases from 215 to 237 ns, about **10% slower**;
it cannot use the span path and still pays additional dispatch/state-layout costs.
Those costs are inferred from the implementation; no sampling profile isolates
their individual contribution. This regression is retained in the evidence.

## Diagnostics and sustained load

Each call-count case sends 2,000 validated fresh-connection requests and lets
the server exit normally so counters are flushed.

| Response | Original sends/request | Updated serial sends/request | Reactor sends/request |
| --- | ---: | ---: | ---: |
| 13 bytes | 2 | 1 | 1 |
| 64 KiB | 5 | 2 | 2 |

The original performs 6,003 `fcntl` calls in each 2,000-connection case: three
for the listener and three per accepted connection. Both new servers perform
zero. A small response is now one 110-byte send. A 64 KiB response is one
16,384-byte send containing the head and initial body, followed by a direct
49,252-byte send. Total delivered bytes match the original.

With a peer paused midway through headers or body input, healthy requests
finished in 0.102–0.586 ms across six probes. The paused requests then also
completed successfully when their remaining bytes arrived. This directly
demonstrates independent socket progress. The historical serial baseline
waited for the stalled peer, including almost ten seconds in its timeout case.

A separate **30-second, eight-client keep-alive run** validated **7,024,646**
responses with **zero failures**, averaging **234,155 requests/s**. Sampled
RSS stayed at **1,956 KiB** throughout, thread count stayed at one, and open
descriptors peaked at 12 and returned to 4 after clients disconnected, matching
the pre-run count. This provides bounded-run evidence, not proof against every
possible resource leak.

The plain reactor executable lists only `libc.so.6` as a dynamic dependency.
The diagnostic launcher initially attempted to reuse a recently closed test
port and failed before serving requests; that log is retained. Diagnostic
fixtures were regenerated with fresh ephemeral ports for the successful runs.

## Correctness and portability

- `cargo fmt --all --check` and Clippy with `-D warnings` passed.
- `cargo test --locked --all-targets -- --test-threads=1` passed:
  **576 Rust tests** across 47 test binaries, including HTTP, TLS, networking,
  web behavior, and portable/cross-target compilation.
- `dodo test` and `dodo test -O 3` each passed **121 tests** with two intentionally
  ignored tests and no discovery errors.
- The concurrent example checks successfully. The changed native C adapter builds
  with `-std=c11 -Wall -Wextra -Werror`.
- The docs build, specification consistency check, and internal-link check passed
  (43 pages and 3,621 links/assets/search targets).

The first parallel Rust run hit `WouldBlock` in the existing console test's
nonblocking-pipe read. The full suite passed with tests serialized; no console
implementation or test was changed. Both attempt logs are preserved.

Portable HTTP/router checks compile for WebAssembly and Cortex-M0 at `-O 0` and
`-O 3`. The reactor compiles to Windows objects at both levels. Windows socket
execution and its C adapter were not exercised on this Linux host.

New checks cover all 256 header byte values at 16 positions, each across five
fragment sizes; resumption between parser events and partial body consumption;
partial/pending output without byte replay; index ambiguity and borrow lifetime;
keep-alive limits, pipelining, HEAD/204/304 framing, malformed pipelined successors,
chunked requests/trailers, 100 Continue, body/header bounds, cancellation, and
independent progress with unfinished requests. Tests use an independent Python
HTTP decoder and exercise both `-O 0` and `-O 3`.

## Remaining limits and next measurements

- Handlers execute synchronously on one thread. CPU-intensive or blocking
  application callbacks still delay other clients; concurrency here advances
  sockets. A worker design would require explicit handler state and ownership
  contracts plus multicore measurements.
- The reactor serves plain HTTP. Existing hosted TLS remains available and is
  covered by regression tests, but a concurrent TLS reactor and TLS performance
  have not been measured.
- Full request and response buffering remains bounded and explicit. Very large
  payloads benefit from the existing streaming application/connection interfaces;
  removing these handler-level buffers would require a different handler API.
- The batch poll backend scans up to 256 registrations. Epoll/IOCP and larger
  connection sets need workload evidence beyond these eight-slot tests.
- Dynamic parameter/wildcard matches still scan route shapes. Sorted literal
  tables use binary search; the explicit literal index uses expected linear
  construction and constant-time lookup, with bounded collision probing. Dynamic
  ambiguity checks remain pairwise. Automatic hosted indexing is limited to
  1,024 routes; larger tables can supply their own index through lower-level APIs.
- Connection-heavy loopback throughput is often constrained by the load generator
  and kernel networking. Lower server CPU does not always increase requests/s.
  Test on separate machines and with representative handlers before assigning a
  production capacity. No claim of a global maximum throughput is warranted.
- One-byte parser fragments retain a small measured regression. Header scanning,
  larger buffers, reuse, and routing gains do not imply every workload is faster.

## Local evidence and reproduction

Main comparison artifacts are under
`benchmark-data/web-optimization-2026-09-15/final/`; earlier candidate runs are
preserved in its parent directory. The original baseline evidence remains under
`benchmark-data/web-2026-09-15/`.

The main comparison folder contains `environment.json` with compiler hashes and machine
information, `configurations.json` with executable hashes, `comparison.jsonl`,
raw latency CSVs, sampled resources, request/expected-body fixtures, generated
Dodo sources, and logs. `micro-baseline/`, `micro-optimized/`, and
`micro-indexed.json` contain focused measurements. `counts.json`, `slow-peer.json`,
and `soak.json` contain the separate diagnostics.

The local, ignored scripts reproduce the comparison after building the compiler:

```sh
export DODO_BENCH_OUT="$PWD/benchmark-data/web-optimization-2026-09-15/final"
python3 benchmark-data/web-optimization-2026-09-15/micro_compare.py
python3 benchmark-data/web-optimization-2026-09-15/compare.py
python3 benchmark-data/web-optimization-2026-09-15/probes.py
python3 benchmark-data/web-optimization-2026-09-15/verify_final.py
```

These commands replace results in the selected directory; preserve an evidence
copy before rerunning. The comparison requires the saved `dodo-baseline` compiler
in that directory. Benchmark scripts/data are local artifacts and are intentionally
excluded from Git. The regression checks intended for version control are
[`scripts/test_web_reactor.py`](../scripts/test_web_reactor.py) and the new
[test fixtures](../tests/stdlib/http_fast_checks.dodo).
