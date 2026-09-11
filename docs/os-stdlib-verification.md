# Hosted standard-library verification

Verified in `/home/johannes/Projects/dodo` on Linux x86-64, 2026-09-11.
The implementation adds `std/fs`, `std/process`, `std/env`, `std/thread`,
`std/sync`, their independent adapters, explicit allocation extensions,
native strings/errors/handles, and compiler concurrency support.

## Results

| Check | Exact result |
| --- | --- |
| `cargo test --locked --all-targets` | **374 passed, 0 failed, 0 ignored**, across 31 test binaries. |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed. |
| `cargo fmt --all --check` | Passed. |
| `dodo fmt --check stdlib` and `dodo fmt --check tests/os` | Passed; includes all 39 new Dodo library/fixture files. |
| `git diff --check` | Passed. |
| `cargo build --locked --release --bin dodo` | Passed. |
| Windows x64 / Wine, release compiler | **70 actual PE executions passed**: 27 existing portable fixtures and 8 applicable hosted fixtures, each at O0 and O3. Native C boundaries also compile at the corresponding optimization level with `-Wall -Wextra -Werror`. |
| Final condition-notification/cancellation regression | Synchronization passed again at O0/O3 on Linux and Windows; **2 additional PE executions passed** after adding deterministic `notify_one` and cancellation wakeup coverage. |
| Portable freestanding runner | **108 object compilations passed**: 27 portable fixtures × WebAssembly/Cortex-M0 × O0/O3, with object magic checked. |
| New native OS integration runs | **22 executions passed** at O0/O3, including injected thread failures and cross-filesystem rename. |
| Relocated release compiler | **6 executions passed**: copied fs/sync/thread fixtures built and executed at O0/O3 from unrelated temporary directories using only the copied release binary and host C toolchain. |
| Cargo package | `cargo package --locked --allow-dirty --no-verify` passed; the **266-file** archive contains all **6 native C boundaries**. |
| Python script tests | **16 passed**. |
| Documentation build | **27 pages built**. |
| Documentation links/assets | **1,750 targets checked**, all passed. |

New integration binaries contain 19 tests: filesystem 6, environment 3,
process 2, thread 4, sync 2, and platform selection 2. A compiler unit regression
also covers generic methods on specialized generic owners. The full suite
includes every previous test, not only these additions.

Native fixtures cover spaces, Unicode and non-UTF-8 paths/arguments/environment,
strict/lossy native-string conversion, links, permissions, missing files,
all 64 open-option masks on existing and missing files (128 combinations per
run), directory iteration, copy failure and cleanup, and non-destructive
cross-device rename failure. Windows adds sharing denial and malformed UTF-16
preservation. Unix pipe writes report EPIPE without changing global SIGPIPE
disposition.

Process fixtures use a controlled C child with quoted/empty arguments, inherited
and replaced environments, an explicit child working directory, owned stdin,
262,144 bytes on each output stream, explicit allocated output, allocation/size
limits, ordinary exit, signal termination, timeout, cancellation, and drop cleanup.
The Wine harness compiles, links, and runs the real Windows child executable.

Concurrency fixtures use bounded waits and handshakes, 40,000 contended primitive
atomic increments and 10,000 guarded/shared-atomic increments per corresponding
fixture run, 20,000 bounded-channel transfers, shared read/write locks, concurrent
Once initialization, failed sends/unread-message destruction, cancellation,
barrier breakage, and both detach/completion races. Thread startup and mapping
failures are injected. Delayed workers verify joins/destruction on return,
propagated Result error, break, continue, explicit drop, and ordinary scope exit.
Rejection tests cover references, raw pointers, borrowed allocators, opaque/result
storage, thread-affine resources, escaped guards, live views during unlock/wait,
storage lifetime escape/reuse, invalid callback signatures, invalid orderings,
unsupported ABIs, and unsupported atomic targets.

## Reproduction

The host supplies versioned support libraries through existing temporary linker
symlinks, so Cargo commands used `LIBRARY_PATH=/tmp/dodo-link-libs`. No repository
configuration depends on this machine-specific workaround.

```sh
LIBRARY_PATH=/tmp/dodo-link-libs cargo test --locked --all-targets
LIBRARY_PATH=/tmp/dodo-link-libs cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all --check
target/debug/dodo fmt --check stdlib
target/debug/dodo fmt --check tests/os
LIBRARY_PATH=/tmp/dodo-link-libs cargo build --locked --release --bin dodo
python3 -m unittest discover -s scripts -p 'test_*.py'
npm --prefix docs run build
python3 scripts/check_docs.py
python3 scripts/test_portable_stdlib.py --compiler target/debug/dodo \
  --report /tmp/dodo-os-portable.json
python3 scripts/test_stdlib_windows.py --compiler target/release/dodo \
  --wine /tmp/dodo-wine-complete-06nr3fy3/usr/bin/wine64 \
  --wineserver /tmp/dodo-wine-complete-06nr3fy3/usr/bin/wineserver64 \
  --mingw-include /tmp/dodo-mingw-headers/root/usr/x86_64-w64-mingw32/sys-root/mingw/include \
  --report /tmp/dodo-os-windows-final.json
```

The Wine installation includes the matching data package; each run creates a
temporary prefix and stops only its own wineserver. MinGW 13 target headers were
extracted from Fedora's `mingw64-headers` package without modifying system files.
The harness uses target headers, system import libraries, a minimal entry point,
memory helpers and LLVM's stack probe. Hosted thread and child fixtures link
the required C runtime. Portable fixtures retain their freestanding startup.

## Supported boundaries

Hosted ABIs currently cover x86-64 Linux GNU and Windows x64. Other hosted
targets receive diagnostics. Portable foundations remain independent; compile
tests do not claim execution on WebAssembly or Cortex-M0 hardware. Native atomic
code generation additionally supports AArch64, which was not hardware-tested.

Borrowed/scoped task captures, language-level TLS, asynchronous thread
cancellation, and Result-containing task/output storage are deliberately
rejected or unavailable. Results must be handled before entering opaque thread
storage. Channels are bounded, with explicit closure and fixed capacity; allocated
channels do not grow implicitly. Capture drains stdout/stderr together but does
not simultaneously stream unbounded stdin. Child cleanup controls the immediate
child, not a whole process tree. Global environment/cwd mutation is not exposed
as a safe operation. These limits and platform-specific filesystem semantics
are documented in the linked package guides under
[hosted platform adapters](src/content/docs/platform.md).
