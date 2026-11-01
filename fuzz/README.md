# Generated compiler safety fuzzing

`compiler_safety` turns bytes into small Dodo programs with initialization,
whole-value moves, reassignment, branches, conditional/range/forever loops,
break/continue/return edges, nesting, and shadowed scalar/owned bindings.
Every input is syntactically valid and well typed.
Generation has a node budget of 20 and a nesting limit of two; missing bytes
decode as zero, and bytes beyond the generated program are ignored.

The shared oracle in `tests/support/compiler_safety.rs` executes the small
language directly. It enumerates concrete slot states and both branch choices,
and explores loop iterations until every reachable slot state has been visited.
This is exhaustive for the generated model, rather than an iteration cutoff.
Like the conservative compiler policy, range loops include zero iterations,
even for literal bounds, and repeated tests of `b` are independent choices.
Repeating generated loops read outer owned bindings before modifying them, so losing
an outer owned value on a back edge produces an actual invalid read on the next
iteration. This avoids treating stronger loop restrictions as a semantic
safety oracle.

Recovering and fail-fast checking are both compared with the oracle. The flow
report must analyze every generated body, and every expected use/declaration
position, binding name, and presence of move provenance is checked.
Hand-specified fixtures validate the oracle's branch joins, loop exits,
reinitialization, and shadowing. These tests cover initialization and
whole-local ownership; loans, Result obligations, and destruction order are
covered by `tests/flow_ownership`.

The regular frontend CI automatically runs 1,024 deterministic seeds at both
target widths, short/repeated byte inputs, and single-bit mutations:

```sh
cargo test --locked --no-default-features --test compiler_safety_generated
```

A longer campaign needs only the normal Rust toolchain and no LLVM backend:

```sh
DODO_SAFETY_SEED=1024 DODO_SAFETY_CASES=10000 \
  cargo test --locked --no-default-features --test compiler_safety_generated \
  replay_or_fuzz_generated_safety -- --ignored --nocapture
```

Failures print their seed, target width, and complete generated source. Replay
one seed by setting `DODO_SAFETY_SEED` to it and `DODO_SAFETY_CASES=1`.

For coverage-guided fuzzing, install [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
and a nightly Rust toolchain, then run from the repository root:

```sh
cargo +nightly fuzz run compiler_safety -- -max_len=128 -max_total_time=60
```

The fuzz package disables the compiler's LLVM feature and has its own lockfile.
Its seed corpus is tracked; generated crash artifacts, coverage, and builds are
ignored. The nightly `fuzz.yml` workflow fuzzes for 30 minutes, keeps the
minimized generated corpus in the Actions cache, and uploads crash artifacts
when it fails. libFuzzer minimizes failures with:

```sh
cargo +nightly fuzz tmin compiler_safety fuzz/artifacts/compiler_safety/crash-<hash>
```

Replay a saved artifact without nightly or cargo-fuzz:

```sh
DODO_SAFETY_INPUT=fuzz/artifacts/compiler_safety/crash-<hash> \
  cargo test --locked --no-default-features --test compiler_safety_generated \
  replay_or_fuzz_generated_safety -- --ignored --nocapture
```

Keep minimized failures as regression fixtures or tracked corpus inputs.

# Image library fuzzing

`fuzz/image` fuzzes the pure-Dodo image library (`stdlib/std/image`). The
contract under test (see `stdlib/std/image/README.md`): untrusted input never
traps, hangs or asks for scratch out of proportion to the image; every error
is a `pixels.Error`. A trap is any runtime check failure (bounds, overflow,
checked cast) or a failed assertion of the harness's oracles.

`fuzz/image/targets` holds one target per decoding stage, each taking one
input byte buffer, with all buffers static and scratch sized by the codec's
`scratch_size` (capped at 64 MiB; larger requests are skipped and reported):

| Target | Exercises |
| --- | --- |
| `png` | `info`, `metadata` (EXIF orientation, zlib payloads), `scratch_size`, `decode` into a contract-checking sink and a `ViewSink`, undersized scratch, every APNG frame; then all of it again with every chunk CRC repaired so damage reaches the decoder |
| `bmp`, `qoi`, `ppm` | the same stages for each codec |
| `jxl_container` | ISOBMFF box iteration (top level and children), metadata boxes, `info` |
| `jxl_headers` | `info`, `scratch_size`, `frame_info` of every frame, `icc` |
| `jxl_decode` | full decode of every displayed frame: entropy coding, modular, VarDCT, rendering |
| `image` | `std/image` detect/info/metadata/icc, decode into nine target layouts (8/16-bit, F32, F16, premultiplied, gray), `draw` onto a clipped canvas, `std/graphics/image`, every frame |
| `roundtrip_png`, `roundtrip_qoi`, `roundtrip_bmp`, `roundtrip_ppm`, `roundtrip_jxl` | the input is a layout, size, encoder options and samples (format in `targets/roundtrip.dodo`); the encoder must not trap (NaN and infinite floats included), and its output must decode to the same size and, for 8/16-bit straight-alpha sources, to exactly the source converted to the decoded layout |

The checking sink asserts that `begin()` comes once with the header `info()`
reported, every rect lies inside the image in the native layout, and that a
successful decode delivered every pixel.

## Reproduce an input

```sh
cargo build                                   # the stdlib is embedded in dodo
target/debug/dodo build fuzz/image/harness.dodo -o build/image-harness
build/image-harness --list
build/image-harness png crash.png other.png   # files
build/image-harness jxl_decode - < crash.jxl  # standard input
build/image-harness --verbose png crash.png   # print each stage's outcome
```

The harness prints `@<index>` before each input and `done <count>` at the
end; a trap aborts with the runtime's message and source location. Add
`-O 1` to the build for speed, or `-g` to debug.

## Deterministic campaign (regular CI)

`tests/image_fuzz.rs` builds the harness once (`-O 1`) and runs two tests:
the tracked seeds plus regression inputs, and a seeded campaign. For every
target the campaign runs the corpora (`fuzz/corpus/image_<target>` and the
fixtures in `tests/image/codecs/fixtures`, `tests/graphics/png`,
`tests/image/jxl`, `tests/image/jxl_encode/golden`), truncations of each at
every chunk/box/header boundary, structure-aware generated files (PNG and
APNG with valid CRCs and stored zlib streams, BMP header variants with RLE
streams, QOI op streams, PNM/PAM/PFM headers, round-trip parameters with
float specials) and mutations: bit flips, interesting bytes and 16/32-bit
values, length-field corruption of PNG chunks and ISOBMFF boxes, chunk/box
duplication and removal, payload damage with CRC fix-up, insertions,
deletions and splicing. Inputs go to the harness in length-prefixed batch
files, one process per batch; a trap or an input making no progress for 30
seconds fails the test, and so does a scratch request above 32 MiB plus 1 KiB
per image pixel (JPEG XL frames may be larger than the image, so only the
cap applies to them). The default run takes about a minute:

```sh
cargo test --test image_fuzz
```

Failures are grouped by target and cause; each prints its seed, case number
and origin (fixture plus mutations) and the path of the input it wrote under
`target/tmp/image-fuzz-failures/`, ready for `build/image-harness`.

Longer campaigns and replay use environment variables:

| Variable | Meaning |
| --- | --- |
| `IMAGE_FUZZ_SEED` | campaign seed (default 0; seed 0 also runs the boundary truncations) |
| `IMAGE_FUZZ_CASES` | generated inputs per target (default 8,000 to 80,000 by target) |
| `IMAGE_FUZZ_SECONDS` | run each target for this long instead |
| `IMAGE_FUZZ_TARGET` | comma-separated targets (default all) |
| `IMAGE_FUZZ_INPUT` | replay a file, or every file in a directory, through the selected targets |
| `IMAGE_FUZZ_OPT` | harness optimization level (default 1) |
| `IMAGE_FUZZ_TIMEOUT` | seconds one input may take (default 30) |
| `IMAGE_FUZZ_JOBS` | harness processes at once (default: up to 4) |
| `IMAGE_FUZZ_DUMP` | write the campaign's inputs to this directory instead of running them |

```sh
IMAGE_FUZZ_SEED=7 IMAGE_FUZZ_SECONDS=300 cargo test --test image_fuzz image_fuzz_campaign -- --nocapture
IMAGE_FUZZ_TARGET=jxl_decode IMAGE_FUZZ_INPUT=crash.jxl cargo test --test image_fuzz image_fuzz_campaign
```

## Coverage-guided fuzzing with libFuzzer

The `image` feature of the fuzz package adds one cargo-fuzz target per Dodo
target, `image_<target>` (`fuzz/fuzz_targets/image_*.rs`). Its build script
compiles `fuzz/image/libfuzzer.dodo`, which exports
`dodo_image_fuzz(target, data, length)`, to LLVM bitcode with the dodo
compiler, and instruments it for libFuzzer with clang
(`-fsanitize=fuzzer-no-link`), so coverage and comparison feedback come from
the Dodo code. The object needs only libc (`write`, `abort`, `memcpy`,
`memmove`, `memset`, `bzero`). Build dodo first; the clang must read the
bitcode of the LLVM dodo links (LLVM 23):

```sh
cargo build                                   # or cargo build --release
cargo +nightly fuzz run --features image image_jxl_decode \
  fuzz/corpus-cache/image_jxl_decode fuzz/corpus/image_jxl_decode -- \
  -max_total_time=600 -max_len=65536 -timeout=20 -rss_limit_mb=3072
```

The build script looks for dodo in `DODO`, then `target/release/dodo`, then
`target/debug/dodo`, and for clang in `DODO_FUZZ_CLANG`,
`LLVM_SYS_231_PREFIX/bin`, `llvm-config --bindir`, the Homebrew and Debian
LLVM 23 locations, then `PATH`. `DODO_FUZZ_OPT` sets dodo's optimization
level (default 1); `DODO_FUZZ_NO_COVERAGE=1` links dodo's own object file
without instrumentation (libFuzzer is then blind to the Dodo code). Without
`--features image` the fuzz package builds as before and needs neither dodo
nor clang. A libFuzzer artifact replays with the harness as well:

```sh
build/image-harness jxl_decode fuzz/artifacts/image_jxl_decode/crash-<hash>
cargo +nightly fuzz tmin --features image image_jxl_decode fuzz/artifacts/image_jxl_decode/crash-<hash>
```

Keep minimized failures as `regress-*` files in `fuzz/corpus/image_<target>`
and list them in `REGRESSIONS` in `tests/image_fuzz.rs`.
