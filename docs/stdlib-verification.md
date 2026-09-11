# Portable std implementation verification

This implementation adds bundled `std/io`, `std/fmt`, `std/bytes`, and `std/text`,
independent allocation adapters, and optional Unicode 16.0.0 whitespace support.
The public contracts and limitations are documented in
[src/content/docs/standard-library.md](src/content/docs/standard-library.md).

Compiler support includes package-local import aliases and distinct resolved
package identities, generic formatting-method instantiation, checked owner-anchored
raw views, UTF-8 string/byte conversions, and preservation of shared/exclusive
borrow dependencies through adapters. Regression tests reject aliased mutable
views, view invalidation, lifetime escape, private access, and hidden Results.

Verified on Linux x86-64 on 2026-09-11:

| Check | Result |
| --- | --- |
| `cargo test --locked --all-targets` | 329 passed, 0 failed, 0 ignored. |
| Expanded `cargo test --locked --test std_fmt` | 3 passed; includes both formatting fixtures on both cross targets at both optimization levels. |
| New native std fixtures | 10 fixtures executed successfully at both `-O0` and `-O3`: 20 executions. |
| Floating formatting oracle | 352 binary64 cases compared with Rust's formatter at both optimization levels. |
| Decimal parsing oracle | 263 decimal/reference cases at both optimization levels. |
| Optional Unicode property | All code points U+0000..U+10FFFF checked at both optimization levels; all 25 whitespace scalars tested. |
| Windows x64 under Wine | 32 actual PE executions passed: 16 core/alloc/std fixtures at both optimization levels, using the release compiler. |
| WebAssembly and Cortex-M0 | 28 new std object-build combinations passed; existing core/alloc tests add 12 object builds. |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed. |
| `cargo fmt --all --check` | Passed. |
| `dodo fmt --check` | All 27 new/changed Dodo package, fixture, and example files passed. |
| `cargo build --locked --release --bin dodo` | Passed. |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | 16 passed. |
| Documentation build | 17 pages built successfully. |
| `python3 scripts/check_docs.py` | 17 pages and 1,050 internal links, assets, and search targets passed. |
| Cargo packaging | All 12 new source modules and both third-party notices verified inside the generated 147-file Cargo archive (`cargo package --locked --allow-dirty --no-verify`). |

The new fixture stems are `std_bytes`, `std_bytes_alloc`, `std_checked_views`,
`std_io`, `std_io_alloc`, `std_fmt`, `std_fmt_alloc`, `std_text`, `std_text_alloc`,
and `std_text_unicode`. The Windows run also executes `core_checks`,
`core_intrinsics`, `alloc_arena`, `alloc_pool`, `alloc_layout`, and `alloc_boxed`.
`io` and `fmt` fixtures cross-compile at both `-O0` and `-O3`; the other std
fixtures cross-compile at `-O3`. Both target triples are
`wasm32-unknown-unknown` and `thumbv6m-none-eabi`.

The host had versioned LLVM support libraries but lacked unversioned development
links for ffi, z, and xml2. Cargo builds used
`LIBRARY_PATH=/tmp/dodo-link-libs`, containing links to the installed libraries.
The system Wine installation lacked `wine-common` metadata; Windows tests used
a complete local Wine 11 installation assembled with the matching data package,
selected through `PATH`. No host libraries, allocator, or C runtime were linked
into the tested Windows programs. The script provides a small startup, compiler
memory helpers, the MSVC floating-point marker, and LLVM's stack probe.

The reproducible Windows invocation is:

```sh
python3 scripts/test_stdlib_windows.py --compiler target/release/dodo
```

The script discovers all standard-library fixtures; `--fixture PATH` can be
repeated to select the exact fixture set above. Each run gets a temporary Wine
prefix and terminates only that prefix's server.

Remaining supported boundaries are deliberate and documented:

- I/O provides memory implementations and structural device contracts. Platform
  file/socket/UART drivers remain separate. Polling reports progress/pending;
  it does not provide a scheduler. Seeking is absolute and bounded.
- Floating formatting provides fixed/scientific output with explicit precision
  0–324, using musl's MIT-licensed exact decimal expansion. There is no shortest
  or dynamic format-string API. Padding width counts bytes.
- Decimal parsing accepts finite binary64 results with at most 768 mantissa
  digits and uses several KiB of fixed stack scratch. Underflow rounds to signed
  zero/subnormals; overflow is recoverable.
- Text operates on Unicode scalars. Unicode 16 whitespace is optional; grapheme
  segmentation, normalization, and full case folding are not implemented.
- Growing buffers exclusively borrow one explicit allocator. Growth temporarily
  needs old and new blocks alive; arenas do not reclaim individual old blocks.
  Live checked views prevent mutation, growth, movement, and destruction.
- Freestanding object generation verifies compilation, not board startup or
  execution. Programs supply any target memory/soft-float helpers requested by
  LLVM and their own startup/linker configuration.
