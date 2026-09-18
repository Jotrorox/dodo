# JSON and web benchmarks

The [recorded Windows baseline](RESULTS.md) includes measured results and findings.
The [indexed JSON comparison](JSON-INDEXED.md) measures the implemented key index.
The [sequential array comparison](JSON-ARRAYS.md) measures generated decoding.
The [single-pass struct comparison](JSON-STRUCTS.md) measures field dispatch.
The [ordinary-string comparison](JSON-STRINGS.md) measures string fast paths.
The [web comparison](WEB.md) measures header traversal, retained routing metadata,
validated paths, captures, and dynamic route indexing.
See [optimization opportunities](OPTIMIZATION-NOTES.md) for source-level analysis
and additional decoding/header probes.

Run from the repository root with Python 3.10+ and a working native Dodo/C
toolchain. Rebuild Dodo after changing the standard library: the compiler embeds
the library sources. The compiler's own Cargo profile does not select the
optimization of the measured programs; this runner passes `-O 3` by default.

```sh
cargo build --locked --bin dodo
python scripts/bench_stdlib.py --linker clang
```

The default compiler is `target/debug/dodo` (`dodo.exe` on Windows). Use
`--compiler PATH` to select another build and `--linker PATH` for the C compiler
driver. Native Windows requires Clang and the Visual Studio C++/Windows SDK
libraries; use a configured developer shell if those are not discoverable.
Linux can use `--linker cc`.

```sh
# Select a suite, or compare generated-code optimization levels.
python scripts/bench_stdlib.py --suite json --optimization 0 3 --linker clang
python scripts/bench_stdlib.py --suite json-arrays --optimization 0 3 --linker clang
python scripts/bench_stdlib.py --suite json-structs --optimization 0 3 --linker clang
python scripts/bench_stdlib.py --suite json-strings --optimization 0 3 --linker clang
python scripts/bench_stdlib.py --suite web --samples 9 --seconds 0.5 --linker clang
python scripts/bench_stdlib.py --suite http --http-requests 500 --linker clang

# Short execution/correctness check, not a performance baseline.
python scripts/bench_stdlib.py --samples 1 --seconds 0.002 --http-requests 10 --linker clang
```

The runner prints results and saves raw samples and metadata to
`benchmark-data/stdlib.json` (ignored by Git). Override that path with `--report`.
The report includes the compiler version/hash, Git revision/status, fixture and
runner hashes, OS/CPU/Python information, compilation time, executable size,
iteration counts, and timing distributions. Temporary executables are removed
and local HTTP servers are stopped when the runner finishes or fails.

## Workloads

| Suite | What is measured |
| --- | --- |
| JSON record | Strict parsing, derived decoding, and derived compact encoding of a 55-byte record with an integer, string, boolean, and four integers. |
| JSON escaped | Parsing a document containing repeated escaped quotes, backslashes, newlines, Unicode, and a nested object/array. |
| JSON strings | Parsing, equality, decoded byte length, and copying to a caller buffer for ASCII, literal UTF-8, and escaped strings. Run alone with `--suite json-strings`; also included in `json` and `all`. |
| JSON arrays | Parsing arrays of 256, 4,096, and 16,384 integer elements. |
| JSON fixed-array decoding | Parsing and derived decoding of objects containing 16, 64, and 256 integers. Run alone with `--suite json-arrays`; also included in `json` and `all`. |
| JSON struct decoding | Derived decoding of 16 and 64 integer fields in reverse schema order, with both ordinary and indexed validation. Indexed parsing is measured separately. Run alone with `--suite json-structs`; also included in `json` and `all`. Use `--struct-sizes` to select widths from 1 through 256. |
| JSON wide objects | Parsing 32, 256, and 1,024 distinct keys with both `parse` and `parse_indexed`, including duplicate-name checks. Indexed cases use 5,120 scratch words (1,024 live members); index reset and construction are timed. |
| Web router | Successful GET lookup cycling through every route in tables of 8, 64, 256, and 2,048 literal routes: sorted, indexed, and unsorted. Select sizes with `--route-sizes`. The optional hash index uses twice as many slots as routes. |
| Web dynamic routing | Scanned versus indexed lookup of `/group/NNNN/:id` routes at the same table sizes. |
| Web headers | Enumerating 32 copied headers by ordinal lookup versus a sequential byte cursor. |
| Web parameters | Repeated named lookup with full pattern matching versus captured request offsets. |
| Web validated paths | Router lookup with path validation versus reuse of a borrowed `web.Path`. |
| Web path | Percent-decoding and validating two alternating paths, excluding their query strings. |
| Web application | Three registered routes; a request with an escaped path, path parameter, and query parameter; handler execution and construction of an owned response. |
| HTTP | A `/health` JSON response over IPv4 loopback: serial execution with one client and a new connection per request; concurrent execution with one or eight persistent clients. |

`json_arrays.dodo` is a source template: the runner substitutes each fixed array
length before compiling it. Its checksum uses the first and last values, and
every decoded element is checked outside the timed region.

`json_strings.dodo` parses a single JSON string or operates on two pre-parsed
strings selected through a volatile index. Equality alternates equal content
and a final-byte mismatch against a decoded reference. Decode checks the byte
count and last byte in the timer and verifies the complete final output after
it. Parse changes the final content byte on each iteration. Length and copy
timings exclude parsing and reference construction; the fastest operations
include measurable selection, loop, and checksum overhead.

`json_structs.dodo` is a source template for each schema width. Every field is
checked outside the timer. Indexed decoding includes scratch reset, validation,
and conversion; it helps expose field-dispatch costs without quadratic duplicate
detection dominating the result. Ordinary decoding retains that validation cost.

`web.dodo` is a source template: the runner fills its route table before
compiling it. Its router construction, registration validation, and optional
index construction happen outside the timed region.
`web_dynamic.dodo` similarly fills route patterns and matching concrete paths.
`web_operations.dodo` compares ordinal and sequential header access, parameter
access, and validated-path lookup. Its capture and path-proof construction are
outside timing; the application benchmark includes request construction.

## Measurement boundaries

In-process cases use `std/time/hosted.MonotonicClock` inside the compiled Dodo
program. Calibration passes are discarded. Each of seven measured batches aims
for 0.2 seconds by default (at least one operation), and the report summarizes
batch-average nanoseconds per operation. Process startup, input/output, fixture
construction, and compilation are excluded. Byte throughput uses the full input
length; derived encoding emits the same number of bytes as its input fixture.

Inputs change each iteration using a volatile read of the iteration counter.
JSON input comes from stdin; parsing changes one digit, encoding changes the
record ID, and web cases vary the requested path. Observable checksums are
verified by Python after every calibration and measured batch. The encoder's
last output is decoded and checked outside the timer. Web application status is
checked in the loop, with a full reference body check afterward. Loop, checksum,
and volatile-read costs are included, so the fastest microbenchmarks have a
nonzero harness cost.

HTTP uses Python `http.client` and one Python thread per client, checks every
status/body/content type, and discards ten warmup requests per client per sample.
A barrier releases clients together. Request throughput includes client work,
socket I/O, and worker scheduling; latency percentiles are per request, measured
at the client. This is a closed-loop local workload, with no TLS or pipelining.
The serial and concurrent modes have different connection lifetimes, so their
rates do not isolate execution-policy costs. Python and the shared machine can
limit throughput; these results do not establish the server's maximum capacity.

The router cases use successful literal and parameterized matches. Dynamic
tables have distinct literal prefixes; routes sharing a prefix still scan within
their group. The application case exercises parameter matching and query decoding.
This suite does not characterize errors, large bodies, streaming, TLS, or remote
network behavior. Keep the same machine, inputs, optimization, sample settings,
and connection policy for before/after comparisons. There are no performance
thresholds or claims of statistical significance.
