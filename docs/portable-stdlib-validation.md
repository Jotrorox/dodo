# Portable standard-library implementation and validation

This report records historical verification on 2026-09-11 in
`/home/johannes/Projects/dodo-portable-std`, branch `feat/portable-std`, based on
committed core/alloc revision `80a0b2d` and delivered in revision `6a6a6fd`.
The counts below describe that branch before its integration with main revision
`54ddcd3`; they are not verification results for the merged tree.

The integration preserves main's complete byte I/O, formatting, binary-buffer,
and UTF-8 packages, including explicit import aliases. Time's decimal parser
and padded integer writer extend those existing text/formatting packages.
The merged-tree verification below includes both sets of packages.

## Merged-tree verification

Verified on 2026-09-11 in `/home/johannes/Projects/dodo` while merging
`6a6a6fd` into main revision `54ddcd3`. The merge preserves import aliases,
string/byte conversion intrinsics, checked disjoint field borrows, and the
complete I/O/text APIs. The decimal helpers now reuse the existing text parser
and formatter. One rejection test was updated to the unified ownership
diagnostic; the rejected program remains rejected.

| Check | Result |
| --- | --- |
| `cargo test --locked --all-targets` | 354 passed, 0 failed, 0 ignored across 25 suites |
| Native portable fixtures | All 27 fixtures execute at O0 and O3 through Cargo integration tests |
| Windows x64 under Wine | 54 real PE executions passed: 27 fixtures at O0 and O3 |
| WebAssembly and Cortex-M0 | 108 objects passed: 27 fixtures × 2 targets × O0/O3; correct object magic checked |
| Compiler integration review | Seven focused probes passed; accepted generic mutable methods and disjoint field borrows also execute at O0/O3 |
| Clippy | `cargo clippy --locked --all-targets -- -D warnings` passed |
| Formatting | Rust formatting, Dodo formatting, and staged/unstaged whitespace checks passed |
| Release build | `cargo build --locked --release` passed |
| Relocated release compiler | Four copied portable examples built and ran at O0/O3: 8 executions passed |
| Release linkage | No shared LLVM dependency; compiler-host support libraries remain dynamically linked |
| Release/linkage script tests | 6 release-script tests and 10 linkage-script tests passed |
| Documentation | Build passed: 21 pages; 1,316 internal links, assets and search targets checked |

The commands and host setup listed below also apply to this merged validation.
The Windows and cross-target runners used report paths
`/tmp/dodo-merged-windows.json` and `/tmp/dodo-merged-cross.json`, respectively.

## Portable packages delivered before integration

- 19 independently imported collection modules: borrowed algorithms, fixed
  vectors/rings/maps/sets, allocated vectors/rings/hash maps/sets/ordered
  maps/sets/heaps, and safe shared-arena adapters.
- Portable checked integer and binary64 mathematics; independently imported
  full-range trigonometry and its attributed Payne–Hanek reduction table.
- Incremental FNV-1a-64/SipHash-2-4, explicit fallible caller key sources,
  reusable field-wise encoding policies, separate CRC-32 and Adler-32.
- Allocation-free checked duration/timestamp/Gregorian date/time values,
  independent monotonic clock identities, static clock/timer contracts,
  deterministic fakes, and exact UTC byte parsing/formatting.
- Checked owner-bound pointer views, shared allocation capabilities, preserved
  allocator/policy dependencies, rejection of mutable access through shared
  aggregate reference paths, generic borrow-source specialization, and direct
  initialization of large repeated local arrays.
- Package documentation, attribution, four practical examples, focused compiler
  regressions, model-based fixtures, and reusable native/cross-target runners.

## Historical results before integration

| Check | Result |
| --- | --- |
| `cargo test --locked --all-targets` | 339 passed, 0 failed, 0 ignored across 21 suites |
| Portable native fixtures | All 17 fixtures execute at O0 and O3 through Cargo integration tests |
| Windows x64 under Wine | 34 real PE executions passed: 17 fixtures at O0 and O3 |
| WebAssembly and Cortex-M0 | 68 objects passed: 17 fixtures × 2 targets × O0/O3; correct object magic checked |
| MPFR reference comparisons | 8,767 deterministic binary64 cases at 256-bit reference precision, plus 79 special-value/boundary checks and integer models |
| Hash vectors | All 64 official SipHash-2-4 vectors; FNV, CRC32 and Adler32 vectors; every split in a 513-byte message; explicit key-source failure |
| Calendar models | Full 146,097-day Gregorian cycle; every second in a day; 134 independent Python datetime vectors; duration and epoch-boundary models |
| Clippy | `cargo clippy --locked --all-targets -- -D warnings` passed |
| Formatting | `cargo fmt --all -- --check`, `dodo fmt --check .`, and `git diff --check` passed |
| Release build | `cargo build --locked --release` passed |
| Relocated release compiler | All four copied examples built and ran at O0/O3 from an unrelated directory: 8 executions passed |
| Release linkage | No shared LLVM dependency; ordinary compiler-host support libraries remain dynamically linked |
| Release-script tests | 6 passed |
| Linkage-script tests | 10 passed |
| Documentation | Build passed: 21 pages; 1,281 internal links, assets and search targets checked |

The Windows fixtures link with `/nodefaultlib`, a minimal startup, memory
helpers, stack probing and the COFF `_fltused` marker, without libc or libm.
This validates generated Windows programs, not a Windows build of the compiler.
Cross-target object emission does not execute WebAssembly or Cortex-M0 hardware.

The host has Fedora Wine 11.0 but lacks its installed data package. Tests used
an existing complete, locally unpacked matching Wine installation, without a
system installation change. The host's missing development-library symlinks
were supplied by `LIBRARY_PATH=/tmp/dodo-contracts-link` for Cargo linking.

```sh
LIBRARY_PATH=/tmp/dodo-contracts-link cargo test --locked --all-targets
LIBRARY_PATH=/tmp/dodo-contracts-link cargo clippy --locked --all-targets -- -D warnings
LIBRARY_PATH=/tmp/dodo-contracts-link cargo build --locked --release
python3 scripts/test_stdlib_windows.py \
  --wine /tmp/dodo-wine-complete-06nr3fy3/usr/bin/wine64 \
  --wineserver /tmp/dodo-wine-complete-06nr3fy3/usr/bin/wineserver64 \
  --report /tmp/dodo-portable-windows.json
python3 scripts/test_portable_stdlib.py --report /tmp/dodo-portable-cross.json
cargo fmt --all -- --check
target/debug/dodo fmt --check .
python3 scripts/test_build_release.py
python3 scripts/test_check_linkage.py
python3 scripts/check-linkage.py target/release/dodo
# In docs/: npm run build
python3 scripts/check_docs.py
```

Use the standard system development packages and matching Wine/data packages
instead of these machine-specific temporary paths on another host.

## Historical Rust suite counts before integration

| Suite | Passed | Failed |
| --- | ---: | ---: |
| `unittests src/lib.rs` | 146 | 0 |
| `unittests src/main.rs` | 4 | 0 |
| `tests/alloc_library.rs` | 14 | 0 |
| `tests/array_annotations.rs` | 5 | 0 |
| `tests/array_storage.rs` | 2 | 0 |
| `tests/collections_library.rs` | 8 | 0 |
| `tests/compiler.rs` | 68 | 0 |
| `tests/core_intrinsics.rs` | 7 | 0 |
| `tests/core_library.rs` | 2 | 0 |
| `tests/diagnostics.rs` | 7 | 0 |
| `tests/format_cli.rs` | 5 | 0 |
| `tests/hash_library.rs` | 1 | 0 |
| `tests/lsp.rs` | 13 | 0 |
| `tests/math_library.rs` | 1 | 0 |
| `tests/newline_continuation.rs` | 7 | 0 |
| `tests/owned_storage.rs` | 10 | 0 |
| `tests/reference_iteration.rs` | 10 | 0 |
| `tests/regressions.rs` | 11 | 0 |
| `tests/stdlib_packages.rs` | 10 | 0 |
| `tests/stdlib_safety.rs` | 6 | 0 |
| `tests/time_library.rs` | 2 | 0 |

## Remaining restrictions and boundaries

- Opaque container elements containing checked references or Results are
  explicitly rejected, recursively. Their internal provenance/handling cannot
  yet be transferred through raw storage. Read-only slice algorithms can work
  with reference-bearing elements; allocator and policy dependencies remain
  checked. No borrowing rule or mandatory Result handling was disabled.
- SharedArena is single-threaded and bump-allocated. Individual frees do not
  reclaim bytes; drop all capabilities before resetting. Custom allocators are
  supported through explicit unsafe capability contracts. Existing exclusive
  arena/pool Box APIs retain their original exclusive allocator borrowing.
  The integrated Box API keeps both `as_ref`/`as_mut` and `get`/`get_mut`
  checked accessors for compatibility.
- Ordered maps use sorted vectors, with O(n) insertion/removal. Containers do
  not shrink. Invalid allocated-vector insertion indices currently use
  `AllocError.UnsupportedLayout`. Hash removal can be quadratic with adversarial
  collisions; keyed SipHash policies and unpredictable caller keys are available.
- Math currently exposes binary64 functions. Reported accuracy bounds are
  reproducible test expectations, not universal correctly-rounded guarantees.
  IEEE round-to-nearest and gradual underflow are required. There is no optional
  hardware backend yet, and no mandatory libm dependency.
- Time uses a UTC ISO 8601 subset, years 0000–9999, nanosecond precision and no
  leap seconds. Callers assign distinct clock-domain IDs. OS clock access,
  blocking waits, scheduling, timezone databases and entropy acquisition remain
  separate adapter responsibilities. No cryptographic digest/MAC API is provided.
- The ownership checker remains conservative, including chained assignment
  through a returned mutable view; bind that view to a local first. Large array
  initialization is optimized for repeated local declarations; other aggregate
  expression forms retain their existing lowering.

See the [package overview](src/content/docs/standard-library.md),
[collections](src/content/docs/collections.md), [math](src/content/docs/math.md),
[hash](src/content/docs/hash.md), and [time](src/content/docs/time.md) for complete
API semantics, complexity, allocation behavior and numerical contracts.
