---
title: "Editor responsiveness"
description: "Reproduce Dodo's stdio LSP latency measurements before introducing incremental analysis."
section: "Project"
order: 525
---

The LSP currently checks full documents synchronously and shares the resulting
symbol index between open files in a checked package. Measure the edit-to-diagnostics
delay and request round trips before introducing incremental analysis.

## Reproduce the measurements

Build the release compiler, then run from the repository root:

```sh
cargo build --locked --release
python3 scripts/bench_lsp.py target/release/dodo --features
python3 scripts/bench_lsp.py target/release/dodo --features --functions 200 --documents 5
python3 scripts/bench_lsp.py target/release/dodo --features --functions 20 --stdlib
```

The script starts a real `dodo lsp` process, opens unsaved buffers, warms up with
five changes, and measures 30 further changes by default. Each timed change ends
with a request barrier after all diagnostics have been published. It includes
serialization, pipe transport, checking all open buffers, and diagnostic delivery.
The script rejects unexpected diagnostics. Request timings measure completion,
definition, signature help, and hover against the refreshed analysis. It prints
JSON lines with p50, p95, and maximum milliseconds. Use `--samples` for longer runs.

Run the same script without `--features` against an older compiler that supports
only diagnostics and hover. Compare the same build profile, hardware, fixtures,
and open-document counts; these are observations, not CI latency thresholds.

## Measurements

Measured on 2026-09-14 on Linux x86-64, an Intel Core Ultra 5 125U, Rust 1.95.0, LLVM
22.1.8, and Cargo's release profile. These runs used `--samples 100`, with builds and tests finished before measuring.
The baseline is commit
`b3aad8cd29725942611ab09a67472eaec66e3c32`, before the additional editor features.
The fixture contains small functions with a typed parameter and an arithmetic
return, plus a caller. The `std/math` case also loads and checks that bundled
dependency. The five-buffer case uses file checking mode.

| Fixture | Baseline change p50 / p95 | Expanded LSP change p50 / p95 | Completion p95 |
| --- | --- | --- | --- |
| 20 functions, 933 bytes | 0.23 / 0.36 ms | 0.48 / 0.59 ms | 0.49 ms |
| 200 functions, 9,233 bytes | 2.09 / 2.20 ms | 3.83 / 4.15 ms | 1.60 ms |
| 1,000 functions, 46,833 bytes | 11.59 / 11.86 ms | 23.69 / 28.07 ms | 8.76 ms |
| Five open files, 200 functions each | 6.17 / 9.78 ms | 13.41 / 17.41 ms | 1.73 ms |
| 20 functions plus `std/math` | 3.07 / 3.20 ms | 4.57 / 5.81 ms | 0.59 ms |

Hover, definition, and signature-help p95 round trips were below 0.5 ms in these
expanded-server runs. Symbol indexing and diagnostic recovery add work, but these
fixtures do not establish a need for incremental analysis. Full-document checking
is retained. Large individual bodies, error-heavy files, larger import graphs,
and more open buffers need separate measurements before drawing conclusions about
those workloads. The benchmark can be rerun as the implementation evolves.
