---
title: "Hosted platform adapters"
description: "Independent target selection, native errors and strings, and explicit native boundaries."
section: "Standard library"
order: 151
---

Hosted packages call operating-system services such as files, processes, and
standard streams. `std/platform` provides the native string storage those calls
share; `std/platform/error` provides their common error type. Most programs
start with a higher-level package such as `std/fs` or `std/process` and use this
page when they need to understand its path, storage, or target requirements.

A **native unit** is one byte on Linux and one UTF-16 code unit on Windows.
`platform.workspace()` owns 4096 such units, including the terminating NUL.
That number is a storage limit, not a character limit or a promise that the OS
will accept every path of that size.

## Choose the right layer

| Task | Starting API | When to use the lower-level native API |
| --- | --- | --- |
| Print or read a line | [std/console](console.md) | Compose a borrowed standard-stream handle. |
| Open a UTF-8 file path | [fs.File.open_utf8](filesystem.md) | Preserve non-UTF-8 Unix names or unpaired Windows surrogates. |
| Look up configuration | [env.get](environment.md) | Capture all native environment entries. |
| Run a program | [process.Command](processes.md) | Pass native argument/environment lists. |
| Convert a native name for display | `platform.decode` | Supply larger byte/wide scratch explicitly. |

These are independent adapters. A program that prints does not automatically
import the process manager or network stack. Portable libraries such as
`std/io`, `std/text`, and `std/time` work without an OS provider.

## Quickstart

Save this as `native_start.dodo`:

```dodo test
package native_start
import "std/platform"
import "std/platform/error"

fn convert() -> void!error.Error {
    workspace := platform.workspace()
    workspace.set("report.txt")?
    name := workspace.get()?
    utf8 := [0u8; 32]
    policy := platform.TextPolicy.Strict
    decoded := platform.decode(&name, &mut utf8, &policy)?
    assert_eq(decoded, "report.txt")
    return ok()
}
fn main() -> i32 {
    match convert() {
        ok() => { return 0 },
        err(_) => { return 1 },
    }
}
```

```sh
dodo run native_start.dodo
```

Expected output: none; exit 0 confirms `report.txt` survived native conversion.
No file is created. Exit 1 means conversion failed: increase the output array
for `BufferTooSmall`, or reject an invalid name. Workspace capacity is fixed;
use explicit native scratch APIs below when a larger name is required.
`TextPolicy.Strict` rejects malformed native text; choose `Replace` explicitly
if replacement characters are acceptable. A failed decode returns no string,
though it may have written a prefix.

`platform.list_workspace()` provides 32768 native units for argument lists.
Prefer `process.Command` and `env.get` for their respective tasks. Native
workspace views borrow the workspace; finish using one before calling `set`.

Hosted adapters support **x86-64 Linux GNU and Windows x64 MSVC/GNU**. They
reject Linux musl/x32, AArch64 Linux, macOS and freestanding ABIs. Portable
fixtures are checked for `wasm32-unknown-unknown` and `thumbv6m-none-eabi`;
that verifies object emission, not execution or board startup.

## API and contracts

The basic native-string workflow is:

1. Own storage, for example `workspace := platform.workspace()`.
2. Copy UTF-8 into it with `workspace.set("report.txt")?`.
3. Obtain a borrowed native view with `workspace.get()?`.
4. Use that view in a native operation, then let the view's borrow end before
   changing the workspace.

The workspace stores a name; it does not open the resource named by it. A
successful `fs.File.open` returns a separate owner, and does not retain the path
buffer. By contrast, a `NativeString` continues to borrow its backing storage.

[Filesystem](filesystem.md), [processes](processes.md),
[environment](environment.md), [threads](threads.md),
[synchronization](synchronization.md), [clocks](time.md), [networking](networking.md),
[TLS](tls.md) and [web static files](web.md#optional-static-files) select their
adapters independently. [Console](console.md) uses the platform stream adapter.
`std/AREA/native` resolves to `std/AREA/linux` or `std/AREA/windows` using the
compilation target, including `dodo check --target ...`. Explicitly importing an
incompatible adapter produces a diagnostic.

The compilation target chooses the adapter, even when the compiler runs on a
different operating system. Target selection validates the supported ABI; it
does not install that target's C headers, linker, or native libraries. See the
linking requirements below before building a hosted executable for another OS.

`std/platform/error` defines recoverable error kinds plus the native error code:
Linux errno or Windows GetLastError. Synthetic library errors use code zero.
Thread-start failures retain their native thread error domain. Synchronization
has a separate typed error enum; it does not expose native implementation error
numbers as portable values.

Use `kind` for portable control flow and `code` for diagnostics. For example,
handle `NotFound` as a missing path rather than comparing a Linux errno with a
Windows error number. `BufferTooSmall` describes a caller-selected storage bound;
`Unsupported` describes an operation or platform capability that a larger
buffer will not fix.

`std/platform/native` centralizes checked native strings and owned I/O handles.
Unix strings use bytes; Windows uses UTF-16 code units and preserves unpaired
surrogates. Constructors validate termination/interior NUL. Text conversion
explicitly chooses strict or lossy behavior and uses caller storage. Native
handles close once through deterministic destruction; explicit fallible close
allows error handling. Unsafe raw construction must establish unique ownership
and the correct native resource kind.

Borrowed process standard streams are the exception to owning-handle cleanup:
`native.stdin()`, `stdout()`, and `stderr()` return `BorrowedHandle`, which has
no close operation or destructor. Prefer [std/console](console.md) for safe text
printing and line input. Native-to-I/O conversion retains recoverable kinds,
including `WouldBlock`, `Closed`, `PermissionDenied`, and `BrokenPipe`, as well
as the native code.

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

## Complete hosted examples and storage

The examples use safe public constructors and explicit caller storage. Compile
from the repository root; run file/process examples in a disposable working
directory. Compile `examples/hosted_child.dodo -o child.exe` there before running
the command example. That suffix works on both supported operating systems.

| Example | Source lines | Linux x64 stripped executable bytes (`-O 3`) |
| --- | ---: | ---: |
| [Read and write a file](https://github.com/Jotrorox/dodo/blob/main/examples/hosted_files.dodo) | 34 | 15080 |
| [Environment lookup](https://github.com/Jotrorox/dodo/blob/main/examples/hosted_environment.dodo) | 36 | 19192 |
| [Command output capture](https://github.com/Jotrorox/dodo/blob/main/examples/hosted_command.dodo) | 41 | 23376 |
| [Elapsed nanoseconds](https://github.com/Jotrorox/dodo/blob/main/examples/hosted_elapsed.dodo) | 32 | 15056 |
| [Controlled child](https://github.com/Jotrorox/dodo/blob/main/examples/hosted_child.dodo) | 11 | 10952 |

These measurements use the development compiler, LLVM 22, the local Linux C
linker, and `strip`; they exclude shared system libraries and are not Windows
size claims. The elapsed example prints nanoseconds between consecutive reads;
put the operation being measured between them. File contents and process output
remain bytes until explicitly decoded.

Native workspaces store only the selected representation, packed into aligned
inline storage. `Workspace` holds 4096 units plus a counter (4104 bytes on Linux,
8200 on Windows). `ListWorkspace` holds 32768 units plus a counter (32776/65544
bytes). `env.Workspace` contains two text workspaces. `CommandStorage` contains
two text workspaces and a list workspace; `Command` and `Arguments` borrow their
storage. Storage never grows implicitly. Keep large workspace owners out of
small stacks, or choose explicit allocator/native-buffer APIs for another bound.
Underlying byte and UTF-16 constructors and caller-slice APIs remain available.

`cargo test --test hosted_library --test std_io` covers the convenience layer at
`-O0` and `-O3`, controlled temporary files/local children, native text failures,
exact boundaries, retained output, timeout cleanup, and injected clock failures.
The hosted fixtures also emit Windows objects; execute them with
`scripts/test_stdlib_windows.py` when Clang, MinGW headers/libraries and Wine are
available. That does not substitute for native Windows execution. The portable
`io_bounded`, time, and lexical-path fixtures continue to emit WebAssembly and
Cortex-M0 objects without hosted providers.

## Complete API reference

For every public type, field, constant, and function signature, see [std/platform](api/std/platform.md), [std/platform/error](api/std/platform/error.md), [std/platform/linux](api/std/platform/linux.md), [std/platform/windows](api/std/platform/windows.md).
