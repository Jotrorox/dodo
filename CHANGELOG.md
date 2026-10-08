# Changelog

## Unreleased

- Support macOS in the hosted standard library: console, files,
  environment, processes, threads, synchronization, clocks, sockets, TLS
  and HTTP/web, on Apple silicon (`aarch64-apple-darwin`, tested natively)
  and Intel (`x86_64-apple-darwin`, built and linked). macOS uses
  libSystem: `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT`,
  `SO_NOSIGPIPE` sockets, `renamex_np` for no-replace renames, and relative
  condition-variable waits on the monotonic clock. Apple arm64 calls now
  extend narrow integer arguments as Apple's ABI requires, atomics accept
  LLVM's `arm64` spelling, and links pass `-arch` so
  `--target x86_64-apple-darwin` works on Apple silicon. TLS programs find
  Homebrew's OpenSSL 3 automatically. CI builds, tests and packages a
  self-contained macOS compiler that needs only system libraries.
- Support AArch64 Linux (glibc). CI builds, tests and packages the compiler
  on native AArch64 runners.
- Linux and macOS share one hosted adapter, `std/AREA/posix`; the C
  boundary now owns every OS- and architecture-specific detail (`struct
  stat` and `struct dirent` layouts, `O_*` flags, errno numbering and
  access, SIGPIPE handling). This fixes `O_NOFOLLOW`, whose value differs
  on AArch64. `std/AREA/native` is unchanged; `std/AREA/linux` and the new
  `std/AREA/macos` select the POSIX adapter on their own OS.
- `sync.Storage` grows from 256 to 384 bytes so that it holds macOS's
  pthread state; allocated synchronization objects reserve a 512-byte
  header. `EILSEQ` (APFS rejecting a non-UTF-8 file name) maps to
  `InvalidInput`.
- Add firmware builds for microcontroller boards, starting with the Raspberry
  Pi Pico. `board = "pico"` in a `dodo.toml` target (or `--board pico`) selects
  the board's chip: its target and CPU, a startup runtime the compiler builds
  itself (boot stage 2, vector table, reset handler, memory and integer
  helpers), a linker script, and `ld.lld`. Builds write a flashable `.uf2`
  next to the `.elf`, and `dodo run` copies it to a board in BOOTSEL mode.
  `chip = "rp2040"` (or `--chip`) builds for a custom board. Floating point
  does not link in firmware yet.
- Add `import "std/embedded/board"` and `import "std/embedded/chip"`, which resolve to the
  packages of the build's board and chip, so firmware is not tied to one
  board. Every board package provides `take()`, an LED, GPIO pins, a timer,
  and `reboot_to_bootloader()`. Editors follow the manifest's board.
- Add `std/embedded/chip/rp2040` (clocks from a configurable crystal, 125 MHz system
  PLL, GPIO pins implementing the `std/embedded/hal` pin protocols, the microsecond
  timer, and the boot ROM's USB bootloader) and `std/embedded/board/pico`.
- Add the Raspberry Pi Pico 2: `board = "pico2"` on the new `rp2350` chip
  (Arm Cortex-M33 cores, `thumbv8m.main-none-eabi`). The compiler's RP2350
  runtime provides the vector table, the `IMAGE_DEF` block the boot ROM
  requires, and a reset handler that enables the FPU; images use the
  `rp2350-arm-s` UF2 family and flash to the `RP2350` drive.
  `std/embedded/chip/rp2350` runs the system clock at 150 MHz and provides
  GPIO pins, the TIMER0 microsecond timer, and the boot ROM's USB bootloader;
  `std/embedded/board/pico2` follows the board contract, so `examples/blink`
  builds unchanged with `--board pico2`. The memory and integer helpers are
  now shared by both Arm chip runtimes.
- Add the Raspberry Pi Pico 2 W: `board = "pico2_w"` on the `rp2350` chip.
  Its LED hangs off the CYW43439 wireless chip, so the new
  `std/embedded/wireless/cyw43` driver powers the chip up, bit-bangs its
  half-duplex gSPI bus, starts its backplane clock, and drives its GPIO pins
  through ChipCommon and the GCI pin multiplexer, without loading wireless
  firmware.
  `std/embedded/board/pico2_w` follows the board contract, so
  `examples/blink` builds unchanged; its LED also reads back the pin level
  and senses USB power. RP2040 and RP2350 pins gain `set_as_output` and
  `set_as_input` for lines that change direction. Wi-Fi and Bluetooth are
  not supported yet.
- Add the `examples/blink` project and a hardware self-test,
  `scripts/test_pico.py`, that checks startup, clocks, timing, ownership, and
  the runtime helpers on a connected Pico (`--board pico2` for a Pico 2,
  `--board pico2_w` for a Pico 2 W, which also checks the wireless chip's
  LED and USB-power GPIOs).
- Add `std/embedded/hal`, a portable hardware abstraction layer: typed volatile
  registers (`hal.Reg<T>`) with mask-constant fields, interrupt-masking
  `hal.Critical` sections, one shared `hal.Error`, statically checked pin,
  delay, I2C, and SPI protocols with register and chip-select helpers, and
  serial ports through the existing `std/io` protocols.
- Add `std/embedded/hal/fake` in-memory pins, delays, and I2C/SPI buses so drivers run
  under `dodo test` on a desktop.
- Add the `core/cpu` intrinsics `fence`, `disable_interrupts`,
  `restore_interrupts`, and `wait_for_interrupt` (spec CORE-CPU). They lower to
  PRIMASK on bare-metal Cortex-M and `mstatus.MIE` on bare-metal RISC-V, are
  no-ops on hosted targets, and are rejected on other targets only when
  reachable. `cpu` is now a reserved package name.
- Rework the GPIO example into RP2040 chip support built on `std/embedded/hal`, add a
  desktop-testable I2C sensor driver example, and add the
  [hardware and embedded guide](https://jotrorox.github.io/dodo/hardware/).
- Publish GitHub Releases from `v*` tags. The release workflow checks the tag
  against `Cargo.toml`, the VS Code extension version, and `CHANGELOG.md`, then
  attaches the tested x86-64 and AArch64 Linux, Apple silicon macOS, and x86-64
  Windows archives, the `.vsix`, a `SHA256SUMS`
  file, and build provenance attestations.
- Shrink the release archives: the Linux and macOS archives are `.tar.xz`, release
  binaries no longer carry a symbol table, the Windows ZIP uses maximum
  compression, and the Linux archives include only the shared license texts
  its notices reference.
- Fix the HTTPS client failing an already complete response when the server
  closes right after responding: a reset while sending the client's
  `close_notify` is now ignored. This made the hosted HTTPS integration test
  flaky on macOS.

## 0.1.4 — 2026-09-20

Dodo 0.1.4 adds optional project manifests, typed JSON codecs, editor inlay
hints and quick fixes, and a complete standard-library API reference. It also
improves compiler correctness and JSON and web performance. The language
specification remains version 0.1.

- Add optional `dodo.toml` manifests with named targets, profiles, saved run
  arguments, hosted test settings, and configuration inspection. Preserve
  standalone source compilation and the `compile` alias.
- Add an owned TOML 1.0 parser and shared CLI option definitions without adding
  dependencies. Improve command help, option syntax, and diagnostics; support
  `-b`, standalone `--release`, test debug information, and quiet/verbose output.
- Add non-overwriting project initialization and shell completion generation.
  Let the language server and VS Code select manifest targets and reload their
  platform settings when the manifest changes.
- Send build/check status to stderr and return exit 2 for CLI usage errors.
  Build/configuration/test failures retain exit 1; run preserves the child status.
- Add portable `std/encoding/json` codecs with `@derive(Json)` for structs,
  checked field conversion, field renaming, optional unknown-field rejection,
  borrowed values and strings, caller-owned output, and streaming I/O helpers.
- Optimize JSON validation, escaped strings, indexed object lookup, and derived
  array and struct decoding. Add repeatable benchmarks and recorded results.
- Reuse web routing metadata and validated paths, index dynamic route prefixes,
  preserve captured parameters, and scan HTTP headers sequentially. Expand
  routing regression tests and loopback HTTP performance measurements.
- Add inferred local-type inlay hints and diagnostic quick fixes for mutable
  bindings and missing package imports. Use structured diagnostic data for
  fixes, and reject editor renames that would introduce name collisions.
- Fix definite initialization at unconditional loop exits, ownership checks,
  and short-circuit expression state. Extend control-flow analysis and use it
  for supported function bodies while retaining checks for other bodies.
- Make constant integer-to-`f32` conversions match runtime conversions and fix
  floating-point cast edge cases. Correct Windows child-process exit statuses
  and filesystem path discovery edge cases.
- Fix overlapping borrows when moving fixed-vector elements between slots,
  restoring checked vector insertion, removal, and collection mutation.
- Rework documentation into beginner guides and a separate reference. Add a
  generated API reference covering every bundled standard-library package,
  runnable examples, project-manifest guidance, and stronger documentation
  link and search checks.

The compiler still implements a subset of the language design. See the
[implementation status and limits](https://jotrorox.github.io/dodo/implementation/)
for supported behavior and remaining work.

## 0.1.3 — 2026-09-16

Dodo 0.1.3 adds simpler printing and web application APIs, concurrent HTTPS,
checked mutable slice splitting, reference collections, and improved editor
navigation. The language specification remains version 0.1.

- Add postfix `!` to unwrap a Result or panic on error, preserving `?` for
  propagation. Shorten hello world to a single console call in `fn main()`.
- Add safe hosted console input/output and compiler-checked `print`, `println`,
  and `printf` for primitives and custom printable values. Check literal format
  strings and heterogeneous arguments at compile time.
- Add bounded UTF-8 console and file helpers, command output capture,
  environment conveniences, native clocks, and runnable standard-library
  onboarding guides.
- Add checked `core/slice.split_at_mut` with disjoint mutable views, nested
  splitting, and source lifetimes preserved through moves and returns.
- Support checked shared-reference collection elements and shared-arena owned
  strings. Add scoped `try_update` mutation for vector elements and hash-map
  values, retaining ownership and allocator dependencies on success or error.
  Result-bearing and exclusive-reference elements remain restricted.
- Fix ownership tracking across `break` and `continue`, including loop-carried
  borrows, moved values, and reachable exits.
- Add bounded hosted HTTP/HTTPS clients and fluent `app.new()` and portable
  `application.builder()` APIs. Provide method shortcuts, named handlers,
  middleware, route diagnostics, text/HTML/JSON/redirect responses, request
  accessors, explicit rejections, and owned in-process test responses.
- Add `std/web/app.Server` with bounded default storage, grouped limits and
  timeouts, custom buffers, cooperative cancellation, and explicit serial or
  concurrent execution. The bounded HTTP reactor supports connection reuse and
  ordered pipelined requests.
- Add `std/web/https` with bounded PEM loading, file-specific startup diagnostics,
  and `run_with(address, https.files(certificate, key))`. Support concurrent TLS
  handshakes and requests, HTTPS keep-alive, and graceful connection shutdown;
  TLS remains an explicit import.
- Keep large internal function arguments and results in caller-owned storage
  and compact aggregate copies before LLVM lowering, reducing memory use for
  composed web applications. Exported and C signatures stay stable. Allow up
  to 4,096 generic specializations with a separate nesting limit of 64.
- Open bundled standard-library definitions as read-only sources in VS Code,
  with hover and navigation inside them. Refresh diagnostics and navigation
  when imported files change, preserving unsaved buffers. Add printing
  completions, signature help, and snippets.
- Make frontend tests and Clippy runnable without LLVM through
  `--no-default-features`. Expand native Windows filesystem, process, console,
  thread, synchronization, and networking coverage at `-O0` and `-O3`. Give
  refused-connection checks a fresh deadline and actionable error diagnostics;
  separate HTTP protocol-error checks from short timeout-test deadlines.
- Migrate the backend from Inkwell to llvm-sys 231.0.0 and LLVM 23.1.1; source
  builds now use `LLVM_SYS_231_PREFIX`. Update Rust to 1.98.1, refresh Rust/npm
  dependencies, and adopt TypeScript 7. The VS Code extension requires
  VS Code 1.137 or newer.
- Slim compiler archives to the executable, installation instructions, and
  license notices; documentation and examples remain available online and in
  the source repository. Fix Windows LLVM support-library and linker handling.
- Remove `std/text_unicode` and its Unicode data license notice; text trimming
  uses `Text.trim_ascii`. Remove the Zed extension and Tree-sitter grammar.

The compiler still implements a subset of the language design. See the
[implementation status and limits](https://jotrorox.github.io/dodo/implementation/)
for supported behavior and remaining work.

## 0.1.2 — 2026-09-14

Dodo 0.1.2 adds a portable and hosted standard library, native testing, source
debugging, richer editor support, and Windows compiler downloads. The language
specification remains version 0.1.

- Add portable core and explicit allocation packages: uninitialized storage,
  memory and slice utilities, caller-backed arenas and pools, and owned boxes.
- Add byte I/O, formatting, binary views, UTF-8 text, numeric parsing, and
  separately selected growing buffers and strings. Keep allocation explicit.
- Add portable collections, binary64 mathematics, hashing/checksums, and
  duration/calendar/clock contracts without requiring an OS, global allocator,
  or libm. Generate mathematical constants independently and use integer-based
  floating-point formatting.
- Add independently selected Linux/Windows filesystem, process, environment,
  native thread, and synchronization packages, with owned resources, bounded
  output collection, and deterministic guard/thread destruction.
- Add checked cross-thread transfer/sharing contracts, native callback
  specialization, and integer atomics with validated memory orderings and
  target capabilities.
- Add portable network addresses and DNS, Linux/Windows TCP/UDP/resolver
  providers, verified OpenSSL TLS, bounded incremental HTTP/1.1, streaming
  clients and servers, routing/middleware, and optional rooted static files.
  Transport, TLS, clocks, allocation, and execution remain independently selected.
- Add checked owner-bound storage views and shared arena capabilities. Preserve
  access modes through container borrowing, reject mutable reborrows through
  shared aggregates, and prevent callbacks from retaining borrowed request
  storage through external-reference assignments.
- Add `dodo test` with recursive discovery, `@test` and `test_` functions,
  companion test files, assertions, filtering, ignored tests, captured output,
  timeouts, and isolated native execution. Report assertion values and source
  locations, and execute Markdown fences marked `dodo test`.
- Add source debugging with `-g`, source breakpoints, and local variables.
  Report hosted runtime failures with their check and source location, and
  support non-returning board failure handlers through `--panic-hook`.
- Add LSP completion, definition, references, rename, signature help, canonical
  document formatting, multiple diagnostics, configured compilation targets,
  and a reproducible stdio responsiveness benchmark.
- Publish the Dodo VS Code extension with syntax highlighting, snippets, LSP
  integration, configuration, and restart/output commands as an installable VSIX.
- Use `main.dodo` in the current folder for `dodo run`, `dodo check`, and
  `dodo compile`; keep `build` as an alias for `compile`. Explicit project
  folders select `main.dodo`; default outputs use the project folder name.
  Shared code uses ordinary imported subfolders, without a manifest or package
  manager. Update existing scripts that relied on checking an entire directory
  as one CLI package.
- Share payload storage across enum and `Result` alternatives and initialize
  only the active payload. Compile explicit wrapping arithmetic directly,
  remove unreachable functions, and emit separate function/data sections.
- Specify implemented language contracts and implementation-defined choices,
  with expanded conformance tests and updated reference documentation.
- Replace external JSON, LSP protocol, and file-URI dependencies with internal
  implementations. Rename the Cargo package to `dodo`; the library remains `dodoc`.
- Publish x86-64 Linux and Windows compiler archives with embedded LLVM,
  examples, documentation, and dependency notices, including the embedded
  standard library's Unicode notice. The Windows build uses static runtimes.
  Remove the separate checksum manifest.
- Use the official LLVM 22.1.8 Linux archive without a Z3 dependency. Cache
  native build dependencies, retry downloads, and improve Windows extraction
  and release smoke tests.
- Expand verification with native tests and executable documentation at `-O0`
  and `-O3`, debugger and VS Code sessions, Windows/Wine programs, published hash
  vectors, MPFR references, deterministic models, parser mutation/fragmentation,
  independent network/TLS peers, and WebAssembly/Cortex-M0 object generation.

The compiler still implements a subset of the language design. See the
[implementation status and limits](https://jotrorox.github.io/dodo/implementation/)
for supported behavior and remaining work.

## 0.1.1 — 2026-09-11

Dodo 0.1.1 improves language ergonomics, editor support, compiler distribution,
and documentation. The language specification remains version 0.1.

- Organize the documentation into getting started, usage, language reference,
  and project sections, with focused installation, command-line, editor, and
  source-build guides.
- Simplify the first-program tutorial and add a copyright and license footer
  throughout the documentation website.
- Add `dodo fmt` with automatic migration to name-first declarations, bracket
  array literals, and explicit `function::<Type>()` generic calls.
- Support inferred and annotated array literals, copy-only array repetition,
  immutable `let` bindings, value-producing blocks, integer ranges, checked
  subslices, and recursive patterns with guards and conditional bindings.
- Add explicit copy patterns in shared collection iteration and leading-dot
  continuation for field and method chains.
- Add `dodo lsp` / `dodo --lsp` with live diagnostics, ownership and type hovers,
  unsaved import checking, and directory-package checking.
- Improve labeled ownership diagnostics for borrow origins, conflicting
  accesses, moves, live uses, and borrowed-return contracts.
- Embed LLVM 22 and all code generation targets in the compiler; provide Linux
  release recipes with bundled support libraries and a fully static profile.
- Fix fully static Ubuntu builds when LLVM reports absolute support-library
  paths, and provide the required static Z3 archive through a pinned build.
- Publish the searchable documentation website with light and dark themes,
  keyboard navigation, and generated PDF and plain-text specification downloads.
- Read the CLI help version from Cargo metadata so help, version output, and
  the language server identify the same compiler release.

The compiler still implements a subset of the language design. See the
[implementation status and limits](https://jotrorox.github.io/dodo/implementation/)
for supported behavior and remaining work.

## 0.1.0 — 2026-09-10

Initial Dodo compiler release, with ahead-of-time LLVM code generation, checked
ownership and borrowing, local packages, native execution, and a published
language specification.
