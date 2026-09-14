---
title: "Hosted platform adapters"
description: "Independent target selection, native errors and strings, and explicit native boundaries."
section: "Standard library"
order: 151
---

Use `std/platform.workspace()` when a hosted operation needs reusable native
path storage. It owns 4096 native units (bytes on Linux, UTF-16 on Windows),
including the terminator. This is the recommended starting point instead of
managing two scratch arrays. Import `std/platform/error` to name failures.

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

[Filesystem](filesystem.md), [processes](processes.md),
[environment](environment.md), [threads](threads.md),
[synchronization](synchronization.md), [clocks](time.md), [networking](networking.md),
[TLS](tls.md) and [web static files](web.md#optional-static-files) select their
adapters independently. [Console](console.md) uses the platform stream adapter.
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
