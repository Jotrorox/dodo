# Generated compiler safety fuzzing

`compiler_safety` turns bytes into small Dodo programs with initialization,
whole-value moves, reassignment, branches, conditional/range/forever loops,
break/continue/return edges, nesting, and shadowed scalar/owned bindings.
Every input is syntactically valid and remains in the supported flow subset.
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
iteration. This avoids treating the AST checker's stronger loop restrictions
as a semantic safety oracle.

AST-only, flow-only, combined, and production fail-fast results are each checked
against the oracle. Both flow reports must analyze every generated body; missing
coverage and fallback fail the test. Every expected use/declaration position,
binding name, and presence of move provenance is checked. Hand-specified fixtures
validate the oracle's branch joins, loop exits, reinitialization, and shadowing.
These tests cover initialization and whole-local ownership; they do not certify
borrowing, Result obligations, types outside this subset, or native cleanup.

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
