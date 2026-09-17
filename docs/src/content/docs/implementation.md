---
title: "Compiler support and limits"
description: "What Dodo 0.1.3 implements, where to find reference details, and which design features remain open."
section: "Language reference"
order: 200
---

Dodo 0.1.3 compiles Dodo source ahead of time, checks types and borrowing, and
emits native programs or compiler artifacts. Use this page to distinguish what
you can write today from features outside this release's scope. The
[0.1 language specification](language-spec-0.1.md) defines the retained language
rules and connects them to conformance tests. Its Appendix C records the
implementation-defined choices for this compiler and the excluded features.

Compiler releases and language versions are separate: this compiler is 0.1.3,
and the language specification remains 0.1. The implemented September 2026
ergonomics revision includes name-first declarations, immutable `let` bindings,
final-expression returns, inference, patterns, and canonical formatting.

## Start with the learning path

If you are learning Dodo, read the chapters in this order:

1. [Your first program](first-program.md): compile, run, and handle console errors.
2. [Values, variables, and arrays](language-basics.md): represent data and borrow slices.
3. [Functions, structs, and enums](types-and-functions.md): define interfaces and data types.
4. [Decisions and loops](control-flow.md): branches, block values, and iteration.
5. [Patterns and Results](patterns-and-results.md): select alternatives and handle failures.
6. [Ownership and borrowing](ownership.md): transfers, reference lifetimes, and cleanup.
7. [Generics](generics.md): reuse behavior across concrete types.

The references below cover the same concepts in more detail, including exact
operator precedence, runtime checks, unsafe contracts, and compiler restrictions.

## Choose a reference

| Topic | Read next |
| --- | --- |
| Declarations, literals, operators, inference, and control flow | [Syntax and expressions](implementation-syntax.md) |
| Primitive and compound types, methods, and associated functions | [Functions, structs, and enums](types-and-functions.md) |
| Generic declarations, inference, and method protocols | [Generics](generics.md) |
| Files, imports, and public declarations | [Projects and imports](packages.md) |
| Copying, moving, borrows, and destruction | [Ownership and borrowing](ownership.md) |
| Matching, guards, destructuring, and error handling | [Patterns and Results](patterns-and-results.md) |
| Layout, C calls, raw pointers, and MMIO | [Memory and foreign calls](memory-and-ffi.md) |
| Independent mutable slices | [Mutable slice splitting](slice-splitting.md) |
| Stored references, String mutation, and storage effects | [Container element safety](container-elements.md) |
| Commands, formatting, targets, and output formats | [Command-line guide](command-line.md) |
| Native tests, assertions, and executable documentation | [Testing guide](testing.md) |
| Design requirements and unfinished specification work | [Implementation checklist](spec-requirements.md) |

## Support at a glance

| Area | Implemented | Boundary to keep in mind |
| --- | --- | --- |
| Values | Integers, floats, bool, arrays, checked references/slices, strings, structs, enums, Options, Results, raw pointers. | No tuples, type aliases, trait objects, or general function-value types. |
| Functions | Declared signatures, final-expression returns, methods, associated functions, recursion, generics. | No overloads, inheritance, default/named arguments, closures, or specialization. |
| Control flow | `if`, `match`, conditional patterns, four `for` forms, `break`, `continue`, `return`. | No `while`, `loop`, labels, general iterator protocol, or async/await. |
| Ownership | Moves, shared/exclusive loans, source-based returns, deterministic destruction. | Conservative indexing, joins, loops, and aggregate lifetime tracking. |
| Errors | Mandatory Result handling, explicit matching, `?`, panic-on-error `!`. | No exceptions, automatic error conversion, or panic unwinding. |
| Generic behavior | Concrete specialization and statically checked public method protocols. | No general trait/constraint system or separately compiled generic interfaces. |
| Packages | Single-file and directory imports, aliases, public/private declarations, embedded library. | Local dependencies only; no manifest, package manager, registry, or re-exports. |
| Foreign and hardware code | C primitive/pointer calls, raw storage, volatile MMIO, layout queries. | No C aggregate-by-value calls, C variadics, inline assembly, or interrupt attributes. |
| Hosted library | Files, processes, environment, threads, atomics, synchronization, networking, TLS, HTTP, web routing. | Availability and native dependencies vary by target; see [platform support](platform.md). |

Named omission here means unavailable syntax or behavior, not a suggestion to
emulate it with unchecked memory. Use the documented library protocols and
explicit ownership contracts where they cover your needs.

## Lexing and expressions

[Syntax and expressions](implementation-syntax.md) covers accepted syntax,
evaluation order, contextual inference, arrays and constants, loops, slicing,
checked numeric operations, and compiler resource limits. Historical spellings
remain accepted; `dodo fmt` migrates them to the canonical syntax.

## Packages and visibility

[Projects and imports](packages.md) explains the `main.dodo` project entry,
ordinary imported subfolders, local import resolution, and visibility.
`dodo run`, `dodo check`, and `dodo compile` default to `main.dodo` in the current
folder; `build` is an alias for `compile`. There is no manifest, package manager,
registry, network resolver, or separate library project type.

## Ownership and generics

[Ownership and borrowing](ownership.md) documents copy and move categories,
borrowed-return contracts, and cleanup. The checker conservatively rejects some
valid programs. [Patterns and Results](patterns-and-results.md) explains matching
and mandatory error handling; [generic inference](implementation-syntax.md#generic-syntax-and-inference)
covers monomorphization and omitted type arguments.

## Layout, entry points, and foreign calls

[Memory and foreign calls](memory-and-ffi.md) describes layout and entry points.
The native function ABI is implementation-defined. Enum storage uses the explicit
tag and shared payload storage specified for 0.1, with target ABI padding. Result
success and error alternatives also share storage. The supported C ABI accepts
primitive and raw-pointer arguments and results; aggregate-by-value calls and
variadics are not supported.

## Implemented core calls

The [core call reference](memory-and-ffi.md#implemented-core-calls) lists early
destruction, layout queries, raw pointer operations, and MMIO. Unsafe functions
still require explicit unsafe blocks for unchecked operations.

## Remaining design surface

The bundled [standard library](standard-library.md) provides opaque
`MaybeUninit` storage, memory exchange, byte/slice utilities, layouts, caller-backed
arenas and pools, and owned boxes with explicit allocator lifetimes. Portable
`std/io`, `std/fmt`, `std/bytes`, and `std/text` add byte I/O contracts, formatting,
binary views, UTF-8, numeric parsing, and separately imported growing buffers
and strings.
Checked views into allocated storage retain their owner's borrow. Import aliases
separate packages with the same final path component, and generic formatting
methods are statically dispatched.

Hosted [platform packages](platform.md) add filesystem/process/environment APIs,
native threads with checked move tasks, explicit unsafe transfer/sharing
contracts, integer atomics, and guarded synchronization. Native callbacks use
statically checked function specialization; closures remain unavailable.

[Networking](networking.md), [TLS](tls.md), [HTTP/1.1](http.md), and [web routing](web.md)
compose existing generic methods and checked buffers. Borrow-carrying assignments
through external mutable references are rejected as well as local reborrows;
callbacks cannot retain request storage by assigning it into their receiver.

Checked [mutable slice splitting](slice-splitting.md) is supported through
`core/slice.split_at_mut`, with disjoint views and retained source lifetimes.

### Features outside this release

The broad design's trait-based checked allocator interfaces,
DMA/interrupt-safe abstractions, general target barriers, inline
assembly, and section/alignment/export/interrupt attributes are not implemented.
Unsupported syntax and unknown intrinsics produce diagnostics. Hosted runtime
failures report their check and source location before aborting. Freestanding
builds default to the target-dependent `llvm.trap`, which may lower to C `abort`.
`--panic-hook` selects a non-returning C ABI board handler for checked operations
and ordinary assertions, with no fallback trap or hosted reporting dependency.
Board code supplies fault reporting, halt, or reset behavior; returning from the
handler is undefined behavior. `-g` emits source and variable/type debug
information. See [debugging and failure configuration](command-line.md).

## Compile-time errors, Results, and traps

These three outcomes have different meanings:

| Outcome | When it happens | How to respond |
| --- | --- | --- |
| Compiler diagnostic | Invalid syntax, a type mismatch, an unhandled Result, an invalid borrow, or a compiler resource limit. | Fix the program before execution; `dodo check` reports these without linking. |
| Returned Result | An operation explicitly reports a recoverable failure, such as capacity, I/O, or parsing failure. | Handle both variants, propagate with `?`, or deliberately unwrap with `!`. |
| Runtime trap or panic | A failed bounds/overflow check, assertion, or Result unwrap. | Execution aborts without unwinding or guaranteed cleanup. |

Bounds, arithmetic, and checked conversions remain checked at every optimization
level. Turning up optimization does not enable C-style unchecked signed overflow.
Use the documented [wrapping operations](implementation-syntax.md#numeric-behavior)
when modulo arithmetic is required.

`dodo check` cannot prove raw pointer validity or satisfy an unsafe external
contract for you. The [memory reference](memory-and-ffi.md) identifies the
obligations that remain with the caller.

## Keeping documentation and programs aligned

Complete examples marked `dodo test` are executable documentation. Run them with
`dodo test docs --doc`; the [testing guide](testing.md) explains discovery and
linker setup. Other fences are declaration fragments, multi-file examples, or
explicitly rejected code. Check their surrounding explanation before treating
them as standalone programs.

Use `dodo fmt` to migrate accepted historical syntax to the canonical name-first
spelling. Source compatibility does not imply a stable native object ABI; rebuild
related objects when compiler layout or ABI behavior changes.

This compiler provides a usable hosted core and target object emission. It is an
initial implementation, with a conservative borrow checker and explicit platform
limits, not the entire embedded ecosystem described by the design. Tests cover
many accepted/rejected cases and native behavior; they are not a soundness proof.
