# Routing with shared prefixes: 2026-10-04

The literal-prefix index does not reduce the candidate set for tenant-scoped
routes whose distinguishing resource segment follows a parameter. The expanded
benchmark measures that case alongside the distinct-prefix control from
[the earlier comparison](WEB.md). The router implementation is unchanged.

## Workloads and method

All tables contain exactly 8, 64, 256, or 2,048 routes. The layouts are:

| Layout | Patterns | Prefix groups |
| --- | --- | --- |
| Distinct | `/group/NNNN/:id` | One route per group |
| Shared | `/api/tenants/:tenant/resourceNNNN/:id` | One group containing the entire table |
| Nested | `/api/tenants/:tenant/projects/:project/resourceNNNN/:id` | One group containing the entire table |
| Wildcard | `/api/tenants/:tenant/resourceNNNN/*rest` | One group containing the entire table |
| Mixed | GET/POST `.../resourceNNNN/:id`, GET `.../resourceNNNN/new`, GET `.../resourceNNNN/*rest` | One group containing the entire table; four entries per resource |

These controlled synthetic layouts model tenant-scoped APIs. The 2,048-route
tables are stress cases, and uniform requests do not model a production traffic
distribution. Hits visit every route; concrete tenant and ID values vary. Four
request classes are measured separately: successful hits, 404s within the indexed
prefix, HEAD fallback, and DELETE requests producing 405. Distinct-prefix 404s
add an unmatched trailing segment; shared-prefix 404s use an absent resource
name, which also avoids matching a terminal wildcard. Mixed hits test literal
precedence over parameters and parameter precedence over wildcards.

Native Dodo 0.1.4 / LLVM 23.1.2, `-O 3`, macOS 27.0.1 arm64, Apple M5. Processes
were scheduled normally, without CPU affinity. Each case discards calibration
and one warmup, then measures five batches targeting 0.1 seconds. Scan/index
execution order alternates per batch. The tables below report medians of
batch-average lookup time in microseconds, including method/path validation,
volatile selection, result assertions, and checksum accumulation. They exclude
construction, ambiguity checks, index building, the full untimed request
preflight, process startup, I/O, and compilation.

The portable Dodo router runs unchanged on this host. Benchmark-only C calls
provide stdin/stdout and the existing `std/time/runtime.c` monotonic clock at
timer boundaries; the lookup loop has no C calls. These results are a new local
comparison, not a before/after comparison with the earlier Windows run.

## Results

Successful hits. Each cell is **scan / indexed**, in microseconds.

| Layout | 8 routes | 64 routes | 256 routes | 2,048 routes |
| --- | ---: | ---: | ---: | ---: |
| Distinct | 0.109 / 0.088 | 0.632 / 0.189 | 2.462 / 0.231 | 18.120 / 0.215 |
| Shared | 0.292 / 0.472 | 1.939 / 2.944 | 7.365 / 12.385 | 59.858 / 77.213 |
| Nested | 0.432 / 0.738 | 3.738 / 4.113 | 11.408 / 17.247 | 84.777 / 109.153 |
| Wildcard | 0.309 / 0.575 | 1.941 / 3.214 | 7.461 / 13.128 | 57.126 / 81.571 |
| Mixed | 0.435 / 0.699 | 2.092 / 3.140 | 7.741 / 13.079 | 57.942 / 78.487 |

HEAD and error requests at 2,048 routes, also **scan / indexed** in microseconds.

| Layout | HEAD | 404 | 405 |
| --- | ---: | ---: | ---: |
| Distinct | 18.051 / 0.213 | 18.179 / 0.278 | 18.144 / 0.217 |
| Shared | 57.564 / 78.031 | 52.499 / 73.572 | 65.355 / 85.435 |
| Nested | 85.668 / 110.728 | 83.389 / 103.650 | 86.245 / 108.872 |
| Wildcard | 55.857 / 80.907 | 51.633 / 71.699 | 56.574 / 80.851 |
| Mixed | 56.479 / 78.100 | 51.370 / 71.342 | 57.109 / 79.056 |

At 2,048 routes, indexed hits are 29%–43% slower than scanning for the shared
layouts. The distinct-prefix control is 84× faster indexed. Increasing the
shared table from 256 to 2,048 routes grows scan time from 7.365 to 59.858 µs
and indexed time from 12.385 to 77.213 µs. HEAD and errors retain the same
group-size cost.

This follows the implementation: every route in each shared layout hashes to
the same pre-parameter prefix. Lookup still checks that group, recomputes
candidate prefixes, and verifies complete patterns; open-addressing probes
can also walk the cluster for other path boundaries. More table capacity
does not separate routes with the same key. Later literal resource segments
must participate in an index to reduce this candidate set.

## Index decision

The shared-prefix cost warrants evaluating a segment index for large dynamic
groups. A trie must use hashed or sorted literal children: a linear list of
resource children would retain the same scaling problem. It should traverse
later literal segments after parameter edges, retain terminal wildcard edges,
and dispatch methods only after selecting the most specific matching shape.
Traversal must allow a less-specific branch when a literal branch cannot
complete the path; it must also preserve 405 when the best shape lacks the
requested method, even if a less-specific shape offers it. HEAD must prefer an
explicit HEAD handler, then GET on that same winning shape.

Use prefix-group size when evaluating an adaptive strategy: a large table of
distinct prefixes already has a small candidate set. The report records group
counts and maximum group size to distinguish these cases. Keep the current
literal fast path and small-table option while evaluating that design.
A segment index requires more storage than the current two words
per route in some layouts, explicit caller-owned sizing for portable use, and
owned storage for hosted use. Measure construction time and retained bytes
alongside lookup time before selecting a threshold or changing hosted defaults.
This benchmark establishes the current cost; it does not measure or promise a
trie speedup or an end-to-end HTTP throughput improvement.

## Reproduction and validation

```sh
cargo build --locked --bin dodo
python scripts/test_bench_stdlib.py
python scripts/bench_stdlib.py --suite web-dynamic --route-sizes 8 64 256 2048 --samples 5 --seconds 0.1 --linker clang --report benchmark-data/web-shared-prefix.json
```

Use `--route-shapes shared nested mixed` to restrict layouts. Every generated
request is validated before timing, even when a batch contains fewer iterations
than routes. Successful IDs and HEAD flags, exact 404/405 errors, and Python
checksums are verified in calibration, warmup, and measured batches. The smoke
test covers O0/O3, all layouts and request classes, both lookup modes, partial
checksum cycles, and a seven-entry mixed table with an incomplete final group.
The 2,048-route measurement preflights the entire large fixture in both modes.

Raw samples, iteration counts, generated fixture hashes, linked C source hashes,
compiler hash/version, revision, and working-tree state are saved locally in
`benchmark-data/web-shared-prefix.json` (ignored by Git). Compiler SHA-256:
`deeaec5c950f440f723670e2c16890bcbf8a08293602bf4ffa288bc65eacd897`.
Runner SHA-256:
`8f08ae3e57f47f41a3abbe432566a9e259b7398ef6c5d01b36dadac5c98b8eb5`.

Three benchmark correctness tests, Dodo fixture formatting, Python compilation,
C warning checks, and whitespace checks pass. Timing variability is retained in
the raw report; there are no performance thresholds or significance claims.
