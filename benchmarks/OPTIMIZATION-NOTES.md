# JSON and web optimization opportunities

Investigation of revision `7baf51dd2ffa41e9ad3f644df4b4b7be939dce8d`,
2026-09-18. These proposals describe the original investigation, before standard
library changes. Baseline numbers are in [RESULTS.md](RESULTS.md). The key-index
proposal has since been implemented; see [measurements and validation](JSON-INDEXED.md).
Sequential array decoding has also been implemented; see its
[before/after measurements](JSON-ARRAYS.md).

The best first targets are JSON duplicate detection and derived decoding, plus
web header traversal and reuse of validated route metadata in in-process
applications. The existing HTTP benchmark does not establish server saturation,
so it cannot assign an end-to-end speedup to a micro-optimization.

## Additional measurements

These probes use the same Windows x64 machine and compiler as the baseline,
native `-O 3`, five samples per case, and calibration targeting 0.1 seconds per
sample. Checksums are checked outside timing. No modified implementation was
measured, so the ratios below describe existing costs, not achieved speedups.

| Probe | Median time |
| --- | ---: |
| Parse object containing 16 integer array elements | 0.260 µs |
| Derived decode of that object into `[16]u32` | 1.691 µs |
| Parse object containing 64 integer array elements | 0.713 µs |
| Derived decode of that object into `[64]u32` | 13.556 µs |
| Parse object containing 256 integer array elements | 2.757 µs |
| Derived decode of that object into `[256]u32` | 234.033 µs |
| Enumerate one web header through `field_at` | 0.009 µs |
| Enumerate eight web headers through `field_at` | 0.368 µs |
| Enumerate 32 web headers through `field_at` | 4.582 µs |
| Hosted Windows `MonotonicClock.now_ms()` | 0.154 µs |

The array documents have one object field, so their decoding slowdown is
separate from wide-object duplicate detection. Decoding does more work than
parsing, but the repeated traversal in the generated decoder is avoidable.
Header probes use `X-Bench` with a 16-byte value and enumerate all indexes in a
rotating order; modulo and checksum overhead are included. They measure header
enumeration, not a full HTTP response. Clock timing includes an error check.

Local probe sources are in `target/perf-investigation/`; raw samples are in
`benchmark-data/optimization-probes.json`. Both directories are ignored by Git.

## JSON

### 1. Index object keys during validation — highest impact for wide objects

In [`scan`](../stdlib/std/encoding/json/value.dodo), each new object key walks
all previous members, reconstructs their string views, compares decoded keys,
and skips their values. A 1,024-field object performs 523,776 prior-key
comparisons. Large earlier values also get skipped repeatedly. The existing
baseline grows from 2.21 ms at 256 fields to 33.3 ms at 1,024 fields.

Add a parsing entry point with caller-provided scratch storage for a key index.
A hash table can provide expected linear work in total key bytes and member
count. Hash decoded key content, retain key offsets, and compare complete keys
on collisions. A sorted offset index is an alternative when deterministic
worst-case behavior is more important than average lookup cost.

Keep the existing constant-extra-memory entry point as a fallback. Define
scratch exhaustion explicitly, and share scratch across nested objects using
scopes instead of reserving a large table at every recursive depth. Preserve
duplicate rejection for escaped equivalents such as `"a"` and `"\u0061"`,
unknown fields, nesting limits, and useful error positions. Hash equality alone
must never decide that keys are equal.

### 2. Generate forward-only array decoding — high impact, separate bottleneck

Implemented with a consuming cursor and primitive decoding loops; see
[measurements and validation](JSON-ARRAYS.md). The analysis below describes
the original implementation.

[`json_derive.rs`](../src/json_derive.rs) generates `take_element(value, 0)`,
`take_element(value, 1)`, and so on. `take_element` calls `Value.at`, which starts
at the beginning on every call. The derive also computes `len()` in a separate
pass and emits code for every element. This creates quadratic traversal and
large generated methods. The 256-element probe spends about 85 times as long
decoding as parsing, although that is not an estimate of the achievable speedup.

Introduce an ownership-preserving consuming array cursor, returning the next
child plus the remaining cursor. Generate sequential extraction; where type
and initialization rules allow it, use a loop rather than a fully unrolled
decoder. Check the exact array length while consuming or with one final end
check. Borrowed strings, nested arrays, partial initialization, and failure
cleanup must retain the existing ownership behavior.

### 3. Decode struct members in one traversal — high impact for larger schemas

Implemented with a consuming object cursor, a hash decision tree, and optional
raw field views that track presence; see [measurements and validation](JSON-STRUCTS.md).
The analysis below describes the original implementation.

Generated struct decoders call `take_field` or `take_optional_field` for every
field. In [`ownership.dodo`](../stdlib/std/encoding/json/ownership.dodo), `rest`
continues to view the complete object; it is not a cursor. Each lookup therefore
restarts through `Value.get`. Strict unknown-field checking adds another
member-by-schema-name scan.

Generate one object traversal with a field-name dispatch table and a seen-field
bitset. A generated hash lookup or decision tree can dispatch to the field
without a linear scan of every schema name. A simple chain of all field-name
comparisons would retain quadratic comparison work. Combine unknown-field,
missing-field, and optional-field handling with this pass. Preserve arbitrary
input field order, renamed/escaped names, and borrowed output lifetimes.

This improves conversion after validation; it does not by itself remove the
parser's duplicate-detection cost described above.

### 4. Add fast paths for ordinary strings — smaller, broadly useful change

Implemented with byte comparison/copying and direct ASCII scanning; see
[measurements and validation](JSON-STRINGS.md). The analysis below describes
the original implementation.

In [`value.dodo`](../stdlib/std/encoding/json/value.dodo), `String.equal`
decodes both strings scalar by scalar even if neither has escapes. `string_end`
also sends ordinary ASCII through scalar decoding. `String.decode` first
decodes the whole string to calculate its size and then decodes it again to
write the destination.

For two already validated, unescaped strings, compare lengths and raw bytes.
For unescaped decoding, use the byte length and a checked copy. Scan runs of
ordinary ASCII directly, switching to the existing decoder at an escape or
non-ASCII byte. Retain Unicode validation, surrogate handling, and the promise
that a capacity error does not partially write the string destination.

These paths should help key comparison and derived field lookup as well as
string-heavy documents. Their speedup remains to be measured.

### 5. Simplify encoder bookkeeping and avoid repeated validation

[`Encoder`](../stdlib/std/encoding/json/encoder.dodo) stores 128 `usize` element
counts, but uses them only to distinguish empty from nonempty containers. A
boolean or a state bit can express that distinction, reducing the depth-state
footprint and initialization work. Benchmark generated code first: LLVM may
already remove some initialization in small cases.

Its private `quoted` method validates UTF-8 before scanning for escapes, even
when reached through `&str` or a validated `json.String`. Separate the trusted
string path from raw-byte validation and retain escaping. For streaming to an
expensive writer, document/use the existing `io.BufferedWriter` and explicitly
flush it; the encoder emits many small fragments. The current `to_slice`
benchmark uses memory and does not measure system-call savings from buffering.

## Web

### 1. Add a forward header iterator — concrete hosted and in-process improvement

[`field_at`](../stdlib/std/web.dodo) starts scanning the packed header buffer
from byte zero for each index. [`response_head`](../stdlib/std/web/hosted.dodo)
calls `response.header_at(i)` for every outgoing header. The in-process test
helper uses the same pattern to calculate header storage usage. Enumeration is
therefore quadratic in header count; the probe measured 4.58 µs for 32 headers.

Add a borrowed iterator or retain header offsets when appending, then serialize
headers in one pass. An iterator needs no additional allocation. Preserve
insertion order, duplicate `Set-Cookie` fields, limits, and partial-write
semantics. This primarily helps responses with many headers; the one-header
health endpoint does not represent that workload.

### 2. Reuse validated routing metadata for application requests

`Application.request_with` delegates to [`testing.send`](../stdlib/std/web/testing.dodo),
which constructs `Router.new(routes)` every time. This validates every method
and pattern and checks route ambiguity on every in-process request, despite
registration already performing those checks.

Introduce a built/frozen route table and a request path that reuses its
validation/index metadata. The table is currently publicly accessible, so a
cache needs invalidation or an API that prevents mutation after building.
Keep the owned-response test API, with an optional reusable workspace API for
callers who can accept a borrowed response. The current helper creates a 4 KiB
request-header buffer and an 8 KiB owned response, plus path/query buffers.
The optimizer may remove some work; profile the emitted code before assigning
a cost to those buffers.

This targets the 0.757 µs application benchmark. Hosted servers already create
and retain their router outside the request loop, so do not count this as a
hosted-server optimization.

### 3. Avoid repeated path validation and parameter matching

`decode_path` validates the completed path. `Router.find` validates it again,
and `Router.parameter` reruns full pattern matching before walking segments to
locate a parameter. Multiple `request.param(...)` calls repeat this work.

Represent validated methods/paths with internal types and provide a trusted
lookup path for the HTTP/application runners. Keep validation in public APIs
that accept arbitrary bytes. Capture parameter offsets during matching, using
caller-owned storage or an optional match workspace, and reuse them in the
handler. Preserve single percent-decoding, path traversal rejection, UTF-8
checks, route precedence, HEAD fallback, and 404/405 behavior.

### 4. Extend routing indexes where the existing fast paths stop

Sorted literal lookup and caller-owned literal hashing already exist. The
baseline measured 66 ns indexed versus 3.65 µs unsorted at 256 literal routes.
That is an existing implementation choice, not a new promised speedup.

The serial server automatically indexes 17–1,024 routes; the concurrent server
indexes up to 1,024. Above that, they fall back to `Router.new`, whose lookup
depends on ordering and route shape. Parameterized/wildcard routes still use a
specificity scan even with the literal hash table.

Allow caller-sized index storage for larger hosted tables. For applications
with many dynamic routes, build a segment/radix tree with literal, parameter,
and final-wildcard branches, plus method dispatch. Save parameter offsets
during traversal. This is a larger change and needs new mixed-route benchmarks;
the current successful literal lookups cannot estimate its benefit.

### 5. Reduce deadline and reactor bookkeeping — secondary, profile first

The default network clock delegates to `std/time/hosted`. On Windows,
[`runtime.c`](../stdlib/std/time/runtime.c) queries the performance-counter
frequency and performs a 30-step fractional-nanosecond conversion for each
call, even when HTTP only asks for milliseconds. The measured complete
`now_ms()` call is about 154 ns.

Cache the frequency with thread-safe initialization, add a checked direct
millisecond conversion, and reuse a timestamp during bounded reactor work where
doing so does not weaken deadlines. Refresh after user handlers or potentially
long work. Preserve monotonic behavior, overflow handling, and failure handling.
This is a small per-call opportunity, not an explanation of the measured
133.9 µs median HTTP latency.

The reactor also rebuilds readiness registrations and scans all slots on each
turn. Persistent registrations, an active-slot list, or platform event queues
may help at higher connection counts. Profile with a native load generator and
many connections before undertaking a backend rewrite; eight Python clients
do not establish this bottleneck.

### 6. Reuse connections and prepare immutable replies

The serial runner deliberately serves one request and writes `Connection:
close`; the concurrent runner supports persistent connections. Using the
existing concurrent mode is an immediate application-level option. An optional
serial keep-alive mode could amortize connection setup, but it needs bounded
idle time and request counts so one idle connection cannot monopolize the
serial server. Compare execution modes with the same connection policy before
attributing any gain to scheduling.

`web.Reply.handle` revalidates immutable response text as UTF-8 and copies it
into response storage on each request. A prepared reply could validate once
and cache safe response metadata. A borrowed-body path is a more invasive
option because current responses deliberately retain no handler borrows.
Preserve middleware behavior, HEAD/bodyless-status rules, header policy, and
lifetimes across pending I/O. Large static responses should be benchmarked;
the current 11-byte body is too small to assess copying costs.

Small response headers and bodies are already coalesced by `append_body`, and
larger fixed-length bodies already have a direct-write path through `poll_body`.
Adding those same mechanisms again would not address a missing optimization.

## Suggested implementation order

1. JSON forward-only array decoding and plain-string fast paths; header iteration
   on the web side. These have clear local changes and focused correctness tests.
2. JSON scratch-assisted duplicate checking and one-pass struct decoding; reuse
   built routing metadata for in-process applications.
3. Validated-path lookup and captured parameters, followed by larger/mixed-route
   indexing if workload measurements justify it.
4. Streaming/encoder and immutable-reply work, then clock/reactor tuning with a
   stronger network load generator.

Validate JSON changes against escaped-equivalent duplicates, unknown/missing
fields, arbitrary field order, numeric limits, exact array lengths, nested
borrowed values, malformed UTF-8, capacity errors, and writer failures. Validate
web changes against route precedence, HEAD/405, duplicate headers, partial I/O,
timeouts, cancellation, connection reuse, and handler lifetimes. Keep the
existing strict behavior while measuring the changed workloads at O0 and O3.
