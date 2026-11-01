---
title: "Shared libraries"
description: "Open a shared library at run time with std/dylib and call its functions through extern \"C\" fn pointers."
section: "Standard library"
order: 160.8
---

`std/dylib` loads a shared library while the program runs and looks up its
symbols. A program that uses an optional system library this way still
starts when the library is missing, and can say so instead of failing to
launch; [std/gpu/vulkan](vulkan.md) opens the Vulkan loader like this.

| System | Calls | Linked |
| --- | --- | --- |
| Linux (x86-64, AArch64) | `dlopen`, `dlsym`, `dlclose` | libc; libdl on glibc before 2.34 |
| macOS | `dlopen`, `dlsym`, `dlclose` | libSystem |
| Windows (x64) | `LoadLibraryA`, `GetProcAddress`, `FreeLibrary` | kernel32 |

Names and symbols are NUL-terminated byte strings. A bare name such as
`b"libz.so.1\0"` uses the system's search path; a name containing a slash (a
backslash on Windows) is a file path.

```dodo
package main

import "core/ptr"
import "std/dylib"

type Strlen = extern "C" fn(*const u8) -> usize

fn main() {
    library := dylib.open(b"libc.so.6\0")!
    address := library.symbol(b"strlen\0")!
    if ptr.is_null(address) {
        return
    }
    strlen := unsafe { address as Strlen }
    core.assert_eq(unsafe { strlen(ptr.as_ptr(b"dodo\0")) }, 4usize)
}
```

`symbol` returns a null address when the library has no such symbol. Turning
the address into an [`extern "C" fn` pointer](memory-and-ffi.md#c-function-pointers)
is unsafe because nothing checks that the declared signature matches the
library's.

| Call | Result |
| --- | --- |
| `dylib.open(name)` | `dylib.Library!dylib.Error` |
| `library.symbol(name)` | `*const u8!dylib.Error`; null if absent |
| `unsafe { library.close() }` | Unloads it; no address from it may be used afterwards |
| `dylib.error_name(error)` | `"InvalidName"` or `"NotFound"` |

`InvalidName` means the name is empty, lacks its final NUL, or contains
another NUL. `NotFound` means the system found no loadable library by that
name. Dropping a `Library` without `close()` keeps it loaded for the rest of
the process, because addresses looked up in it may still be in use.
