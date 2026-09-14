# Changelog

## Unreleased

- Add `dodo test` with recursive discovery, `@test` and `test_` functions,
  companion test files, assertions, filtering, ignored tests, captured output,
  timeouts, and isolated native execution. Report assertion values and source
  locations for checked traps. Execute Markdown fences marked `dodo test` in CI.
- Make self-contained standard-library fixtures and portable examples runnable
  through native discovery, with separate core/math/time checks and executable
  introductory, collection, hashing, allocation, math, clock, and thread docs.
  Run the native suite at both `-O0` and `-O3` in CI.
- Add LSP completion, definition, references, rename, signature help, and canonical
  document formatting. Recover multiple editor diagnostics, honor the configured
  compilation target, and add a reproducible stdio responsiveness benchmark.
- Use `main.dodo` in the current folder for `dodo run`, `dodo check`, and
  `dodo compile`; keep `build` as an alias for `compile`. Explicit project
  folders also select `main.dodo`; default build outputs use the project folder
  name. Shared code lives in ordinary imported
  subfolders, with no manifest, package manager, or special library layout.
- Add portable network addresses/DNS, Linux/Windows TCP/UDP/resolver providers,
  verified OpenSSL TLS, bounded incremental HTTP/1.1, streaming clients/server
  composition, routing/middleware and optional rooted static files. Keep transport,
  TLS, clocks, allocation and execution independently selected.
- Reject external-reference assignments that could retain a callback's borrowed
  request storage. Add deterministic parser mutation/fragmentation tests, independent
  loopback interoperability, local TLS credentials, Wine peers and portable objects.

- Add independently selected Linux/Windows filesystem, process, environment,
  native thread, and synchronization packages, with native strings, explicit
  allocation, owned resources, bounded output collection, and deterministic
  guard/thread destruction. Embed their native ABI boundaries in the compiler.
- Add checked cross-thread transfer/sharing contracts, native callback
  specialization, and integer atomics with validated memory orderings and target
  capabilities. Extend Wine verification with real hosted and child programs.
- Add independently imported portable collections, binary64 mathematics,
  hashing/checksums, and duration/calendar/clock-contract packages, with explicit
  fallible allocation and no required OS, global allocator, or libm dependency.
- Add checked owner-bound storage views and shared arena capabilities; preserve
  source access modes through container borrowing and reject mutable reborrows
  through shared aggregates. Borrow-free generic contract inputs contribute no
  dependencies after specialization.
- Validate native O0/O3 behavior, published hash vectors, MPFR references,
  deterministic container/calendar models, Windows/Wine execution, and
  WebAssembly/Cortex-M0 object generation.

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
