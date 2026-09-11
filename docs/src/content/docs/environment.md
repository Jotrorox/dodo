---
title: "Environment"
description: "Native argument access, immutable environment snapshots, current directories and explicit child environments."
section: "Using Dodo"
order: 146
---

`std/env` observes process arguments, environment entries, and the current
directory on x86-64 Linux/glibc and Windows x64. Its independently selected
`std/env/native` adapter does not import filesystem, processes, threads, or
synchronization. Portable foundation packages never read the environment
implicitly.

## Caller-owned native storage

`env.snapshot(bytes, wide)` copies the current environment into caller storage
and returns an immutable `Snapshot`. `env.arguments(bytes, wide)` similarly
copies arguments including `argv[0]`. `env.current_dir(bytes, wide)` returns a
borrowed `std/platform/native.NativeString` containing the current directory.
All return checked Results with native errors or `BufferTooSmall`. On failure,
the supplied storage may contain a partial copy; no snapshot is returned.

Portable calls take both `&mut[u8]` and `&mut[u16]` scratch. Linux writes only the
byte scratch; Windows writes only the wide scratch. The unused scratch can be
empty. Returned borrows keep both supplied owners alive under the explicit
`from(bytes, wide)` contract. This keeps allocation a caller decision: stack
arrays, arena storage, or explicitly allocated typed buffers can all be used.
No global Dodo allocator is required. Windows obtains and releases an OS-owned
environment snapshot with `GetEnvironmentStringsW` while copying it.

Native Unix bytes are preserved even when invalid UTF-8. Windows UTF-16 code
units are preserved even when they contain unpaired surrogates. No locale or
lossy conversion happens implicitly. Use `native.from_utf8(text, bytes, wide)`
for strict text-to-native conversion and `native.to_utf8(value, output, lossy)`
with an explicit replacement policy for native-to-text conversion.

Linux's hosted C entry wrapper records `argc` and `argv` before calling Dodo
`main`. Object-only users supplying their own startup must call
`dodo_env_init_args(int32_t, char **)` first. Windows argument capture parses
`GetCommandLineW` using C-runtime quote/backslash conventions and preserves
empty arguments and native code units. Arguments from executables with a
custom startup parser can differ from that convention.

## Reading snapshots

A snapshot contains a copy of all entries; later process-global changes do not
alter it or invalidate its references. `len()` counts entries, and `at(index)`
returns `Option<NativeString>` borrowed from the snapshot. For environment
snapshots, each entry is the complete `name=value` string. For argument
snapshots, it is one argument.

`get(name)` returns a native value, excluding `name=`. Absence is `none`; an
existing variable with an empty value is `some` with zero units. Unix compares
names byte-for-byte. Windows uses `CompareStringOrdinal` case-insensitively,
including Windows' special leading-equals drive-directory entries when reading
snapshots. `entries()` exposes the checked `NativeList` for explicit child
process construction.

## Building a child environment

`env.child_environment(bytes, wide)` creates an empty caller-backed `Builder`.
Its `set(name, value)` replaces a matching name or appends a new entry;
`remove(name)` returns whether an entry existed. `clear()` empties the builder.
Names supplied to `set` must be nonempty and cannot contain `=`; values may be
empty. These operations never modify the process environment.

`copy_from(snapshot)` starts from an inherited environment explicitly. Capacity
failure preserves the previous builder content, as does a failed `set`.
`snapshot()` returns an immutable view that borrows the builder: the compiler
rejects changes to or destruction of the builder while the view is live.
Pass `snapshot.entries()` to `process.spawn` with
`Options.inherit_environment = false`. Windows process construction sorts the
native environment block before passing it to the OS.

## Global-state and concurrency rules

There is no safe process-global `setenv`, `unsetenv`, or `chdir` API. Environment
and current-directory mutation changes assumptions in every native thread and
can invalidate C library environment storage. Use per-child environment and
working-directory options instead. An unsafe foreign caller performing global
mutation must exclude simultaneous observations and mutations across all
threads and foreign libraries. The snapshot API's copy does not make concurrent
foreign mutation safe during capture.

Immutable snapshots can be read according to Dodo's checked borrowing and
cross-thread sharing rules. A snapshot does not smuggle unchecked global
pointers into a safe reference, and builder views do not extend the backing
storage lifetime. Aborting traps do not unwind Dodo values; these observations
retain no OS resources after a successful return.

The compiler automatically links the native environment boundary for hosted
executables. Object-only consumers must compile and link
`stdlib/std/env/runtime.c` for their target. Unsupported targets fail during
adapter selection, while `core`, `alloc`, `std/io`, `std/text`, and `std/time`
remain independently cross-compilable for freestanding targets.
