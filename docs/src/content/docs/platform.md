---
title: "Hosted platform adapters"
description: "Independent target selection, native errors and strings, and explicit native boundaries."
section: "Using Dodo"
order: 144
---

[Filesystem](filesystem.md), [processes](processes.md),
[environment](environment.md), [threads](threads.md), and
[synchronization](synchronization.md) select their adapters independently.
`std/AREA/native` resolves to `std/AREA/linux` or `std/AREA/windows` using the
compilation target, including `dodo check --target ...`. Explicitly importing an
incompatible adapter produces a diagnostic.

Supported hosted ABIs are x86-64 Linux GNU and Windows x64 MSVC/GNU. Linux x32,
musl, AArch64, Darwin, freestanding targets, and other ABIs receive hosted adapter
diagnostics. This restriction concerns these OS packages; portable core, alloc,
I/O, text, time, and the other portable packages still cross-compile independently
for WebAssembly and Cortex-M0.

`std/platform/error` defines recoverable error kinds plus the native error code:
Linux errno or Windows GetLastError. Synthetic library errors use code zero.
Thread-start failures retain their native thread error domain. Synchronization
has a separate typed error enum; it does not expose native implementation error
numbers as portable values.

`std/platform/native` centralizes checked native strings and owned I/O handles.
Unix strings use bytes; Windows uses UTF-16 code units and preserves unpaired
surrogates. Constructors validate termination/interior NUL. Text conversion
explicitly chooses strict or lossy behavior and uses caller storage. Native
handles close once through deterministic destruction; explicit fallible close
allows error handling. Unsafe raw construction must establish unique ownership
and the correct native resource kind.

## Linking and dependencies

The compiler embeds both Dodo sources and the small C boundaries required by
imported adapters. Executable builds materialize only those boundaries in a
temporary directory and compile/link them through the selected C driver at the
requested optimization level. Complex layouts come from target headers with
static size/alignment checks. Simple handle/filesystem declarations use explicit
target-specific Dodo layouts. Portable imports do not link hosted boundaries.
A relocated compiler retains all boundary sources.

Hosted executable linking requires a C toolchain and target headers. Linux links
pthread; its process spawn file actions require glibc 2.34 or newer. Windows
uses system import libraries and the C runtime required by the thread adapter.
Cross-linking requires a target C toolchain: an LLVM target selection alone does
not install headers or libraries. Select a C driver with `--linker` and pass
its target options/libraries with `--link-arg`.

Object, IR, and assembly output retain external native-boundary references.
Custom linking must compile the relevant `stdlib/std/AREA/runtime.c` files for
the target and supply system libraries. Linux custom startup using `env.arguments`
must call `dodo_env_init_args(argc, argv)` before the Dodo entry point. Hosted
Dodo executable startup does this automatically. Windows argument access reads
the native command line. The Windows harness supplies custom PE startup and
links and executes real child programs.

No package installs a global allocator, scheduler, environment cache, or hidden
startup worker. OS/C-library calls may allocate internally; Dodo-managed dynamic
payloads require explicit allocation APIs. Foundation packages have no mandatory
OS dependency.
