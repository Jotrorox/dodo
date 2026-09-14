---
title: "Environment"
description: "Native argument access, immutable environment snapshots, current directories and explicit child environments."
section: "Standard library"
order: 153
---

Use `std/env.get` to read one environment variable as UTF-8. `env.Workspace`
owns reusable native name/value storage; the returned text lives in your output
array. Import `std/platform` to choose strict conversion and `std/console` to
print the value. This is hosted Linux GNU x86-64 / Windows x64 functionality.

## Quickstart

Save this as `environment_start.dodo`:

```dodo test
package environment_start
import "std/env"
import "std/platform"
import "std/console"

fn main() -> i32 {
    workspace := env.Workspace.new()
    output := [0u8; 128]
    match env.get("DODO_GREETING", &mut output, &mut workspace, platform.TextPolicy.Strict) {
        ok(found) => {
            match found {
                some(value) => {
                    match console.println(value) {
                        ok(_) => { return 0 }, err(_) => { return 2 },
                    }
                },
                none => {
                    match console.println("DODO_GREETING is not set") {
                        ok(_) => { return 0 }, err(_) => { return 2 },
                    }
                },
            }
        },
        err(reason) => {
            errors := console.stderr()
            match errors.println_value(&reason) {
                ok(_) => {}, err(_) => {},
            }
            return 1
        },
    }
}
```

```sh
dodo run environment_start.dodo
```

With no variable set, stdout is `DODO_GREETING is not set` and exit status is 0.
To supply a reproducible child environment on either hosted OS, run:

```sh
python3 -c "import os, subprocess; e = dict(os.environ, DODO_GREETING='Hello, environment!'); subprocess.run(['dodo', 'run', 'environment_start.dodo'], env=e, check=True)"
```

This prints `Hello, environment!` followed by a newline. `none` means absent;
`some("")` is a present, empty value. Choose a default or report a required
configuration variable as missing. Exit 1 means lookup/conversion failed:
increase the 128-byte output for `BufferTooSmall`, or reject malformed text.
Each native workspace has a fixed 4096-unit bound. Exit 2 means printing failed.

`env.Arguments.capture(&mut storage, TextPolicy.Strict)` is the recommended argument API:
`next(&mut output)` and `get(index, &mut output)` return optional strings,
including `argv[0]`. Create `storage` with `platform.list_workspace()`; the
argument view keeps it borrowed until the view is dropped. Its native snapshot is bounded to 32768 units. A failed
conversion does not advance `next`, allowing a larger output-buffer retry.
Use the native snapshot and child-environment APIs below only when you need
full native entries or explicit child configuration.

## API and contracts

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

## UTF-8 storage and conversion

`env.get(name, output, workspace, policy)` copies just one variable, so unrelated
large environment entries do not consume the workspace. `none` leaves output
unchanged. `some("")` succeeds with zero output capacity. Names must be nonempty
and contain neither NUL nor `=`. Linux compares names case sensitively; Windows
uses the OS case-insensitive lookup. A native name or value must fit 4096 units,
including NUL; output needs the UTF-8 byte count, without NUL. Windows surrogate
pairs can expand to four UTF-8 bytes. Native lookup/capacity failures leave output
unchanged; conversion errors can leave a prefix but return no string or valid
prefix count. Do not use output after such an error as a complete value.

`Arguments.get` and `next` use the policy supplied at capture. Strict conversion
rejects malformed native text; replacement emits U+FFFD per malformed Linux byte
or unpaired Windows surrogate. Missing indices return `none`; empty arguments
return `some("")`. `len()` includes the executable at index zero. `rewind()`
resets iteration. Capture copies at most 32768 native units, counting each NUL;
failed capture clears the logical list and returns no argument view. Native
`arguments` remains available for larger snapshots and full native text.

Foreign process-global environment mutation must be synchronized with lookups
as well as snapshots. Dodo never mutates it implicitly. Linux arguments come
from startup `argv`; Windows parses the wide process command line using CRT
backslash/quote rules, with separate executable-name rules.

The complete [environment example](https://github.com/Jotrorox/dodo/blob/main/examples/hosted_environment.dodo)
prints a present value or reports absence. An empty value prints an empty line.
