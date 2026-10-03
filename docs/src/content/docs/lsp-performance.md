---
title: "Editor responsiveness"
description: "Reproduce full-document LSP latency measurements for large bodies, recovery, imports, and projects."
section: "Project"
order: 525
---

The LSP checks synchronously after each change and shares the resulting symbol
index between open files in a checked package. These measurements cover checking,
diagnostic delivery, and editor requests, and guide decisions about caching.

## Reproduce the measurements

Configure LLVM as described in [Building from source](/dodo/building-from-source/),
then build and run from the repository root:

```sh
cargo build --locked --release
python3 scripts/test_bench_lsp.py
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 \
  --workload functions large-function errors generic-imports project \
  --compiler-revision "$(git rev-parse HEAD)" --build-profile release \
  --report target/lsp-performance/suite.json
```

`--size` selects one or more sizes; `--functions` remains an alias. Without it,
each workload uses the sizes below. `--documents` defaults to one open buffer,
or five for `project`. `--samples` defaults to 30.

| Workload | Size means | Default sizes | Fixture |
| --- | --- | --- | --- |
| `functions` | Small functions | 20, 200, 1,000 | Typed parameter, arithmetic return, and a caller |
| `large-function` | Local declaration/assignment pairs in one body | 200, 1,000, 2,000 | One function with twice that many statements, plus initialization and return |
| `errors` | Intended diagnostics | 20, 100, 200 | Alternating syntax errors and unknown bindings in independent functions, plus a valid helper and caller |
| `generic-imports` | Distinct nominal element types | 10, 50, 100 | `Record` types instantiate `fixed_vector.Vector` and its methods; also imports `collections`, `shared_vector`, and `shared_hash_map` and their transitive dependencies |
| `project` | Functions per file | 20, 200 | Required `dodo.toml`, explicit named build target, 20 sibling files on disk, five open overlays, and package checking |

The project manifest has two targets without a default, so successful
initialization requires selecting the named `bench` target. The manifest supplies
compiler configuration; `--check-mode package` selects sibling checking. A
manifest alone does not add siblings to the import graph. `--project-files`
controls the total number of project files, including closed files on disk, and
must be at least `--documents`. Other workloads use unsaved file-mode buffers.

Useful controls for repeated work:

```sh
# Repeat the original comment-only fixtures, including std/math.
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 --edit comment
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 \
  --size 20 --stdlib --edit comment
# A larger individual-body stress case uses 30 timed samples.
python3 scripts/bench_lsp.py target/release/dodo --features --samples 30 \
  --workload large-function --size 5000
# Same statement count while reusing two locals.
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 \
  --workload large-function --size 5000 --body-shape assignments
# Edit one of many independent open buffers.
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 \
  --size 200 --documents 5  --report target/lsp-performance/five-files.json
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 \
  --size 200 --documents 20 --report target/lsp-performance/twenty-files.json
# Same manifest workspace, different checking scope or overlay count.
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 \
  --workload project --size 200 --project-files 20 --documents 5 --check-mode file
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 \
  --workload project --size 200 --project-files 20 --documents 1
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 \
  --workload project --size 200 --project-files 20 --documents 20
# Refresh selected target settings and recheck unchanged buffers.
python3 scripts/bench_lsp.py target/release/dodo --features --samples 100 \
  --workload project --size 200 --edit manifest
```

The script starts a real `dodo lsp` process. After opening the fixture, it warms up
with five changes, then measures full-text body edits alternating a return
constant between zero and one. `--edit comment` appends a changing comment to the
original source instead. Only the first buffer changes; other open overlays and
on-disk dependencies stay fixed. Requests run after every change, including
warmup. Initialization and initial document opens are outside the measured samples.
`--body-shape assignments` reuses two locals instead of declaring a new local
for every pair, isolating growing checker state from statement count.
For `project`, `--edit manifest` alternates the selected target between 32-bit
WebAssembly and 64-bit Linux, writes the manifest, and sends a watched-file change.
Its `manifest_to_diagnostics` interval includes that disk write and configuration
resolution; buffer versions stay fixed. These arithmetic-only projects are valid
for both targets. Ordinary body edits reuse the settings selected at initialization.

Each change ends with an unknown-method request barrier. Because the server
processes messages synchronously, its `MethodNotFound` response follows diagnostic
publication and completion of analysis for all open roots. The measured interval
includes JSON serialization, pipe transport, checking, indexing, and diagnostic
delivery plus that barrier round trip. It ends before benchmark validation.

Validation requires current diagnostic versions for every open buffer, the exact
intended error counts, locations, and message fragments, and no errors in valid
fixtures or imported files. Hover, completion, definition, and signature-help
responses must resolve the valid helper at positions derived from the source.
This avoids timing an empty result after recovery fails. Run without `--features`
for older servers that only support hover. The harness's smoke test covers all
workloads in both edit modes; it skips real-server checks if the binary is absent.
It also covers the assignment-only body and manifest refresh.

JSON lines contain p50, nearest-rank p95, and maximum milliseconds. `--report`
also saves toolchain, platform, command, source revision, binary and benchmark
hashes, and fixture dimensions after each completed run.
`--compiler-revision` and `--build-profile` record build provenance supplied by
the caller; the checkout revision and binary hash are recorded independently.
Compare identical hardware, build profile, fixtures, and samples; these are observations, not CI
latency thresholds.

## Measurements

Measured on **2026-10-03**, macOS 27.0.1 arm64, Apple M5 (10 cores), 16 GiB
RAM, Rust/Cargo 1.99.0, and LLVM 23.1.2. Both binaries used Cargo's ordinary
release profile, locked dependencies, and the default LLVM feature. Builds and
tests finished before the sequential measurements. Python 3.9.6 ran the harness.

The matched comparison is `326003d545364e209faf2515702f2ef2682e9bb4`
(Dodo 0.1.4, before the latest flow-analysis refinements) against
`388f463f6c96adcb3f8f72bd274c121b21dc10d7`. The older build already includes
the earlier flow checker. Compiler sources were unchanged during the runs;
the benchmark and documentation were working-tree changes. The September 14
Linux/Core Ultra 5 measurements are historical and are not a same-machine
baseline for this series.

The [published JSON report](/dodo/benchmarks/lsp-2026-10-03.json) contains every
run, command, binary/benchmark hash, fixture byte count, and all four request
percentiles and maxima. Repository paths are normalized for publication.
Both binaries passed the harness's correctness smoke tests.

Unless indicated below, each fixture has five warmups and **100 timed changes**
without inter-sample delays. All values below are milliseconds. Each row uses
one fresh server process; these percentiles do not measure variation across many
server launches. Sizes exclude the additional valid caller.

| Body-edit fixture | 0.1.4 p50 / p95 | Current p50 / p95 | Current completion p95 |
| --- | --- | --- | --- |
| 20 small functions | 0.20 / 0.22 | 0.21 / 0.24 | 0.07 |
| 200 small functions | 1.90 / 2.03 | 2.10 / 2.25 | 0.54 |
| 1,000 small functions | 23.27 / 23.90 | 26.69 / 30.93 | 3.05 |
| One body, 200 local pairs | 4.78 / 4.90 | 5.47 / 5.73 | 0.06 |
| One body, 1,000 local pairs | 104.30 / 115.24 | 112.92 / 119.46 | 0.11 |
| One body, 2,000 local pairs | 443.46 / 481.95 | 463.32 / 488.26 | 0.15 |
| 20 mixed syntax/semantic errors | 0.19 / 0.21 | 0.20 / 0.23 | 0.04 |
| 100 mixed syntax/semantic errors | 0.72 / 0.77 | 0.73 / 0.75 | 0.06 |
| 200 mixed syntax/semantic errors | 1.61 / 1.68 | 1.62 / 1.67 | 0.08 |
| Imports, 10 distinct container types | 9.05 / 9.39 | 9.50 / 9.75 | 0.07 |
| Imports, 50 distinct container types | 40.27 / 42.62 | 41.25 / 43.59 | 0.12 |
| Imports, 100 distinct container types | 103.16 / 110.71 | 113.27 / 121.75 | 0.17 |
| Manifest package: 20 files × 20 functions, 5 open | 5.76 / 5.98 | 5.92 / 6.17 | 0.26 |
| Manifest package: 20 files × 200 functions, 5 open | 266.21 / 381.00 | 274.59 / 275.60 | 2.35 |

The other controls isolate open-buffer count, checking scope, edit kind, and
local-state size. File mode checks only the open roots here; package mode checks
all 20 project files, including closed siblings. The 5,000-local stress and
2,000-local comment controls use **30** timed changes; all other controls use 100.

| Control | 0.1.4 p50 / p95 | Current p50 / p95 | Current completion p95 |
| --- | --- | --- | --- |
| 5 independent open files, 200 functions each | 10.37 / 12.95 | 9.81 / 10.00 | 0.49 |
| 20 independent open files, 200 functions each | 38.66 / 40.39 | 38.78 / 39.61 | 0.52 |
| Manifest, 20 files × 200 functions, 5 open, file mode (5 checked) | 9.86 / 10.66 | 10.11 / 10.57 | 0.54 |
| Manifest, 20 files × 200 functions, 20 open, file mode (20 checked) | 39.81 / 45.75 | 39.96 / 40.71 | 0.51 |
| Manifest, 20 files × 200 functions, 1 open, package mode | 201.00 / 351.69 | 209.35 / 210.89 | 2.30 |
| Manifest, 20 files × 200 functions, 20 open, package mode | 576.28 / 835.48 | 513.89 / 1309.45 | 7.48 |
| Manifest target change, 20 files × 200 functions, 5 open | 242.13 / 249.15 | 649.93 / 908.44 | 8.78 |
| One body, 5,000 local pairs (30 samples) | 2694.92 / 2921.84 | 3049.68 / 3153.57 | 0.31 |
| One body, 5,000 assignment pairs reusing two locals | 273.14 / 308.04 | 273.32 / 294.12 | 0.27 |
| Comment edit, one body with 2,000 local pairs (30 samples) | 485.74 / 752.81 | 468.20 / 510.15 | 0.15 |
| Comment edit, imports with 100 container types | 108.04 / 127.35 | 107.33 / 114.17 | 0.17 |

The original small-function comment fixtures now have current p95 values of
0.22, 2.07, and 26.14 ms for 20, 200, and 1,000 functions; 20 functions plus
`std/math` reach 2.77 ms. They still represent only a small part of the workload
range. The errors fixture verifies all intended errors while retaining navigation
to the valid helper; its 200-error p95 remains 1.67 ms.

Hover, definition, and signature help are generally below 0.2 ms after analysis;
the initial wide-package and manifest-refresh runs have tails above that. Requests
arriving while the synchronous server is checking wait for that check to finish.
The post-barrier request measurements describe querying already refreshed analysis.

### Repeated wide-project runs

The first current 20-overlay package run has a 513.89 ms median, 1,309.45 ms p95,
and 1,416.64 ms maximum. Its target-refresh control also has a long tail. The host
had substantial swap allocated during the series, and a later query reported
44% memory free; neither observation establishes the cause of the tails.
Repeats after both main series are reported alongside the first runs:

| Repeat (100 samples) | Change p50 / p95 | Change max | Completion p95 |
| --- | --- | --- | --- |
| Current, 20 files / 20 open, package mode | 520.69 / 542.03 | 600.25 | 2.58 |
| 0.1.4, 20 files / 20 open, package mode | 482.60 / 531.79 | 598.84 | 2.53 |
| Current, manifest target change, 5 open | 274.79 / 303.69 | 394.62 | 2.67 |

The repeated current target-refresh median (274.79 ms) is close to the ordinary
five-overlay body-edit median (274.59 ms). Configuration parsing is not shown to
dominate refresh, and ordinary buffer edits already reuse the selected settings.

The median scaling across bodies and package sizes is sufficient to identify
expensive work. Individual tail differences between these sequential runs should
not be attributed solely to the recent compiler changes.

## Caching decision

**A cache of unchanged independent root analyses is worth prototyping.** Editing
one of twenty independent buffers repeats about 39 ms of work, compared with
about 2 ms for one buffer. Only one root changes in this fixture. The server
already retains each document's analysis, so a prototype can reuse those results
and diagnostics when the root and all its dependencies remain valid. The roughly
37 ms difference is an opportunity estimate, not a demonstrated cache speedup.

**Reduce work inside the changed root before building a general incremental
checker.** The 5,000-local body reaches a 3.15-second p95; the same statement count
with two reused locals is 294 ms. `Checker::block` clones recovery state before
every statement, including successful statements, and `HoverIndex::name_span`
scans the document's token stream. The latter also runs while visiting every
function for each open document in a checked package. These code paths are
specific candidates for smaller recovery checkpoints, bounded token lookups,
and skipping declarations outside the document. Caching unchanged imports or
independent files cannot eliminate the work in the edited large function.

**Investigate reuse within import graphs and packages after those fixes.** The
100-type import case reaches 122 ms p95, and checking a 4,000-function package
reaches hundreds of milliseconds even with one open overlay. Comment edits
remain similarly expensive. This justifies profiling reusable parsing,
specialization, body checking, and indexing separately. The round trips alone
do not establish how much an imported-parse-tree cache would save, and a whole-root
cache misses when any member of the checked package changes.

Any prototype must invalidate for imported buffer edits, disk dependency and
package-membership changes, and manifest target/pointer-width changes. Re-run
these fixtures and correctness guards against the prototype, including recovery
and manifest refresh, before retaining it. Full synchronous checking remains the
current implementation; the broader fixtures identify costs that the original
small-function measurements could not expose.
