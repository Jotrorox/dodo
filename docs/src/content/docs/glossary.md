---
title: "Glossary"
description: "Plain-language definitions for the terms used in Dodo's tutorials and reference."
section: "Language reference"
order: 270
---

Use this page when a guide introduces an unfamiliar term. The links lead to the
full rule or a worked example.

## Programs and tools

| Term | Meaning in Dodo |
| --- | --- |
| Source file | A UTF-8 text file containing Dodo code, conventionally ending in `.dodo`. |
| Compiler | The `dodo` tool that checks source and produces machine code or another [output artifact](command-line.md#choose-a-compiler-output). |
| Ahead-of-time compilation | Compiling a program before executing it. `dodo run` performs both steps for you. |
| Linker / C toolchain | Tools that combine compiled code and its platform dependencies into an executable. See [installation](installation.md). |
| Entry point | The `main` function called when a hosted executable starts. |
| Exit status | A number returned to the launching process. Zero conventionally means success; it is separate from printed output. |
| Standard output / stderr | The process's normal output stream and error-reporting stream. See [console](console.md). |
| Package | A named unit of source loaded by an [import](packages.md). Importing a folder combines its immediate `.dodo` files. |
| Standard library / stdlib | Bundled `core`, `alloc`, and `std` packages. They are embedded in the compiler. |
| Target | The processor, operating system, and ABI selected for generated code. A target triple is a string such as `x86_64-pc-windows-msvc`. |
| ABI | Application binary interface: machine-level rules for layouts, calls, and symbols. See [foreign interfaces](memory-and-ffi.md). |
| LSP | Language Server Protocol. Editors use `dodo lsp` for [diagnostics and navigation](editors.md). |

## Values and control flow

| Term | Meaning in Dodo |
| --- | --- |
| Binding | A name for a value, such as `let count = 3` or mutable `count := 3`. |
| Type | The kind of value and operations it supports: for example `i32`, `bool`, or a struct you define. |
| Type inference | Deducing a local type from an initializer or other immediate context. Function interfaces remain declared. |
| Expression | Code that computes a value, such as `a + b` or a value-producing `if`. |
| Statement | A step in a block, such as a declaration, assignment, or return. |
| Scope | The region where a name is available and its local resources are kept alive. Braces often introduce a scope. |
| Struct | A named type with fields, and optionally methods, for grouping data. |
| Enum / variant | A type with named alternatives; one active variant can carry payload values. |
| Pattern | A shape used to select or unpack a value, for example `some(value)` in a match. |
| Exhaustive match | A match that covers every possible input alternative, perhaps with `_` as a fallback. |
| Generic | Code parameterized by types, such as `fn identity<T>(value: T) -> T`. See [generics](generics.md). |
| Monomorphization | Generating specialized code for the concrete types used at generic calls. |
| Receiver | The object on which a method is called: `self`, `&self`, or `&mut self` describes how the method uses it. |

## Ownership and storage

| Term | Meaning in Dodo |
| --- | --- |
| Owner | The value responsible for a resource or storage. Its destruction releases what the type's cleanup contract owns. |
| Move | Transferring a value's ownership. The old place can no longer be used as if it still contained that value. |
| Copyable | A value category that ordinary use can duplicate. Dodo structs and owned arrays are not automatically copyable. |
| Borrow / reference | Temporary checked access to someone else's value. `&T` permits shared reads; `&mut T` permits exclusive mutation. |
| Reborrow | A temporary reference derived from an existing reference; it retains the original storage dependency. |
| Lifetime / dependency | How long storage must remain valid for a checked view. Dodo tracks return sources with [inference or `from(...)`](ownership.md). |
| Destructor / drop | Cleanup when an owner is destroyed, including explicit `core.drop(value)`. Panics do not unwind or run cleanup. |
| Array | Inline storage for a fixed number of elements: `[4]i32`. |
| Slice | A borrowed view of consecutive elements with a length: `&[i32]` or `&mut[i32]`. |
| Capacity | How much storage a buffer/container can hold. Length is how much it currently contains. |
| Caller-backed | Storage supplied by the program using an API, often an array borrowed by a library object. |
| Allocator | A capability for obtaining and returning storage. Dodo's [allocation APIs](allocation.md) make this explicit. |
| Arena | An allocator that advances through a region of backing storage; individual frees do not generally recover that space. |
| Pool | An allocator that reuses fixed-size slots. |
| Invalidation | A change that makes an earlier view or pointer unusable, such as freeing or relocating its storage. |
| Raw pointer | An address without the full checked-reference lifetime guarantees. Dereferencing it has [unsafe requirements](memory-and-ffi.md). |

## Errors, bytes, and platforms

| Term | Meaning in Dodo |
| --- | --- |
| Option | `Option<T>` represents `some(value)` or `none`: presence or absence. |
| Result | `T!E` represents `ok(value)` or `err(error)`: success or failure. Results must be handled. |
| Propagation | Postfix `?` continues on success or returns the error from the enclosing Result-returning function. |
| Unwrap | Postfix `!` extracts Result success or panics on error. It is not a general Option unwrap. |
| Panic / trap | Program termination after a failed runtime check or assertion. It does not recover like a Result. |
| UTF-8 | The text encoding used by `&str`. One Unicode scalar can occupy several bytes; `.len` counts bytes. |
| Byte string | A literal such as `b"GET"` used as bytes rather than text. See [bytes](bytes.md). |
| Portable | A package whose contracts do not require a specific operating system. Final executable linking can still need target support. |
| Hosted | Functionality that uses supported OS services, such as processes, files, or native sockets. |
| Provider / adapter | An implementation of a contract for a particular platform or execution strategy. `/native` imports select a target provider. |
| FFI | Foreign function interface: calling or exposing code through another language's ABI, usually C. |
| MMIO | Memory-mapped input/output: accessing hardware registers at device addresses. It needs board-specific knowledge. |

Return to the [learning path](index.md#start-here), the
[language reference](implementation-syntax.md), or [API notation](stdlib-api.md#read-a-signature).
