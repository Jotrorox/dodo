# Changelog

## Unreleased

- Add `std/web/app.Server` with bounded default storage, grouped limits and
  timeouts, custom buffers, cooperative cancellation, and explicit serial or
  concurrent execution. Add portable named-handler registration without manual
  route IDs; update the server examples and add a three-route HTML/parameter demo.

- Remove `std/text_unicode` and its Unicode data license notice. Text trimming
  uses the ASCII rules in `Text.trim_ascii`.
- Add postfix `!` to unwrap a Result or panic on error, preserving `?` for
  propagation. Shorten hello world to a single console call in `fn main()`.
- Update the Rust toolchain to 1.98.1 and the compiler backend to LLVM 23.1.1
  with llvm-sys 231.0.0. Source builds now use `LLVM_SYS_231_PREFIX`.
- Update Rust and npm package dependencies, including TypeScript 7.
  The VS Code extension now requires VS Code 1.137 or newer.
- Remove the Zed extension and Tree-sitter grammar, including their CI checks
  and setup documentation.

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
