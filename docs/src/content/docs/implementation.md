---
title: "Compiler support and limits"
description: "What Dodo 0.1.1 implements, where to find reference details, and which design features remain open."
section: "Language reference"
order: 200
---

Dodo 0.1.1 implements a hosted compiler, local packages, checked borrowing,
and target object emission. This guide describes compiler behavior. The
[0.1 language specification](language-spec-0.1.md) describes the wider design;
its open questions remain open.

Compiler releases and language versions are separate: this compiler is 0.1.1,
and the language specification remains 0.1. The implemented September 2026
ergonomics revision includes name-first declarations, immutable `let` bindings,
final-expression returns, inference, patterns, and canonical formatting.

## Choose a reference

| Topic | Read next |
| --- | --- |
| Declarations, literals, operators, inference, and control flow | [Syntax and expressions](implementation-syntax.md) |
| Files, imports, and public declarations | [Packages and imports](packages.md) |
| Copying, moving, borrows, and destruction | [Ownership and borrowing](ownership.md) |
| Matching, guards, destructuring, and error handling | [Patterns and Results](patterns-and-results.md) |
| Layout, C calls, raw pointers, and MMIO | [Memory and foreign calls](memory-and-ffi.md) |
| Commands, formatting, targets, and output formats | [Command-line guide](command-line.md) |
| Design requirements and unfinished specification work | [Implementation checklist](spec-requirements.md) |

## Lexing and expressions

[Syntax and expressions](implementation-syntax.md) covers accepted syntax,
evaluation order, contextual inference, arrays and constants, loops, slicing,
checked numeric operations, and compiler resource limits. Historical spellings
remain accepted; `dodo fmt` migrates them to the canonical syntax.

## Packages and visibility

[Packages and imports](packages.md) explains file and directory inputs, local
import resolution, and visibility. Dependencies are local files; this release
has no package registry or network resolver.

## Ownership and generics

[Ownership and borrowing](ownership.md) documents copy and move categories,
borrowed-return contracts, and cleanup. The checker conservatively rejects some
valid programs. [Patterns and Results](patterns-and-results.md) explains matching
and mandatory error handling; [generic inference](implementation-syntax.md#generic-syntax-and-inference)
covers monomorphization and omitted type arguments.

## Layout, entry points, and foreign calls

[Memory and foreign calls](memory-and-ffi.md) describes layout and entry points.
The native function ABI and enum layout are unstable. The supported C ABI accepts
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
and strings. `std/text_unicode` supplies optional Unicode 16 whitespace rules.
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

The broad design's disjoint mutable slice splitting, trait-based checked allocator
interfaces, DMA/interrupt-safe abstractions, general target barriers, inline
assembly, section/alignment/export/interrupt attributes, and custom panic-handler
integration are not implemented. Unsupported syntax and unknown intrinsics
produce diagnostics. The trap implementation is `llvm.trap`; it is not a
platform reset driver.

This compiler provides a usable hosted core and target object emission. It is an
initial implementation, with a conservative borrow checker and explicit platform
limits, not the entire embedded ecosystem described by the design. Tests cover
many accepted/rejected cases and native behavior; they are not a soundness proof.
