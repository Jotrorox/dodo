---
title: "Read the API reference"
description: "Find every bundled package and learn to read signatures, receivers, errors, and borrowed-return contracts."
section: "API reference"
order: 0
---

The [complete package directory](api/index.md) lists every source package shipped
with Dodo. Each package page contains its public types, fields, enum variants,
constants, functions, and methods, with exact signatures and links to their
implementation. These pages are generated when the documentation is built, so
new declarations appear automatically.

Use the [standard-library guide](standard-library.md) to choose an API and learn
its behavior. Use this reference when you need an exact name or parameter type.
The guides explain contracts that a signature cannot express: capacity limits,
partial writes, invalidation, numerical accuracy, and operating-system support.

## Find a declaration

Open the [package directory](api/index.md), expand a package family in the sidebar,
or search with **Ctrl+K** (**Cmd+K** on macOS). Search accepts names such as
`split_at_mut`, `Buffer.reserve`, or `std/encoding/json`. A type's methods appear
below its declaration. Public fields and enum alternatives are shown inside the
type declaration; private storage and method bodies are omitted.

Start with these frequently used packages:

| Task | Declarations | Explanation |
| --- | --- | --- |
| Print or read a line | [std/console](api/std/console.md) | [Console](console.md) |
| Work with bytes or UTF-8 | [std/bytes](api/std/bytes.md), [std/text](api/std/text.md) | [Bytes](bytes.md), [text](text.md) |
| Format values | [std/fmt](api/std/fmt.md) | [Formatting](formatting.md) |
| Store a bounded list | [std/collections/fixed_vector](api/std/collections/fixed_vector.md) | [Collections](collections.md) |
| Allocate explicitly | [alloc/shared_arena](api/alloc/shared_arena.md) | [Allocation](allocation.md) |
| Parse or encode JSON | [std/encoding/json](api/std/encoding/json.md) | [JSON](json.md) |
| Read and write files | [std/fs](api/std/fs.md) | [Filesystem](filesystem.md) |
| Serve an HTTP application | [std/web/app](api/std/web/app.md) | [Web](web.md) |

## Read a signature

Consider this declaration from `core/slice`:

```dodo
pub fn get<T>(data: &[T], index: usize) -> Option<&T> from(data)
```

Read it from left to right:

1. `pub` makes the function accessible outside its package.
2. `get<T>` works with an element type `T`, often inferred from `data`.
3. `data: &[T]` borrows a read-only slice; it does not take ownership of the array.
4. `index: usize` accepts a pointer-sized unsigned index.
5. `Option<&T>` returns either `some(reference)` or `none` for an absent element.
6. `from(data)` ties the returned reference to the input's storage. The reference
   cannot outlive that storage or overlap a conflicting write.

A complete use looks like this:

```dodo test
package lookup
import "core/slice"

fn main() {
    values := [10i32, 20, 30]
    match slice.get(&values, 1) {
        some(value) => { assert_eq(*value, 20) },
        none => { assert(false, "index 1 should exist") },
    }
}
```

Save it as `lookup.dodo` and run `dodo run lookup.dodo`. It exits successfully
without printing. The `*` reads the integer behind the reference.

## Recognize the common types

| Notation | Meaning |
| --- | --- |
| `T` | A value; passing an owning type moves it into the function. |
| `&T` | Shared checked reference; read through it while its owner remains valid. |
| `&mut T` | Exclusive checked reference; allows mutation. |
| `[N]T` | An inline array with exactly `N` elements. |
| `&[T]`, `&mut[T]` | Borrowed slice views with a length; they do not own storage. |
| `&str` | Borrowed valid UTF-8; `.len` counts bytes. |
| `Option<T>` | Optional value: `some(value)` or `none`. |
| `T!E` | Result: `ok(value)` or `err(error)`; handling is required. |
| `void!E` | A fallible operation with no success payload: `ok()` or `err(error)`. |
| `*const T`, `*mut T` | Raw pointers; validity and lifetime require explicit care. |
| `MaybeUninit<T>` | Opaque storage that does not automatically destroy a `T`. |

`usize` is commonly used for lengths, indices, and capacities. Its width follows
the compilation target. Numeric `as` conversions use the language's range
checks; integer narrowing does not truncate or wrap. Unsafe raw-pointer casts
do not validate the pointed-to storage or extend its lifetime.

See [types and functions](types-and-functions.md), [ownership](ownership.md), and
[Results and options](patterns-and-results.md) for the language rules behind
these signatures.

## Receivers and constructors

Methods are declared inside their struct. The first parameter determines how a
call uses the receiver:

| Receiver | Call behavior |
| --- | --- |
| No receiver | A type-level function such as `text.Builder.new(...)`. |
| `&self` | Reads the existing object through a shared borrow. |
| `&mut self` | Exclusively borrows the object and can change it. |
| `self` | Consumes the object; fluent builders often return its replacement. |

A `new` function is an ordinary named function, not special syntax. It may
return a value directly, an Option, or a Result. Check its return type before
adding `!` or `?`. A `new` name does not imply a hidden heap allocation.

Returned views often borrow the receiver. Finish using those views before
calling a method that mutates, grows, clears, moves, or destroys their owner.
The [container guide](container-elements.md) explains additional restrictions
for reference-bearing elements.

## Errors, unsafe calls, and compiler-expanded functions

For `T!E`, use `match` to recover, `?` in a function with a compatible Result
return type to propagate, or postfix `!` to unwrap success and panic on error.
Read the guide's failure contract before retrying: I/O can fail after a prefix
was transferred, and failed insertion can consume its input.

An `unsafe fn` needs an `unsafe` call context. Its source comments and guide
define obligations such as initialization, alignment, aliasing, and allocator
validity. Merely obtaining a raw pointer is different from dereferencing one.
See [memory and foreign calls](memory-and-ffi.md).

Some declarations use `@compiler(print)`, `@compiler(println)`, or
`@compiler(printf)`. The compiler expands these calls. In particular, `printf`
accepts a literal format followed by heterogeneous arguments even though its
source declaration only names the format parameter. The
[formatting guide](formatting.md) documents accepted values and format syntax.
These attributes identify bundled compiler hooks; they are not general user
extension points.

## Compiler intrinsics

The following functionality lives in the compiler, so it has no `.dodo` source
package for the generator to extract. It remains part of the documented API:

| Entry point | Reference |
| --- | --- |
| Built-in `assert`, `assert_eq`, `assert_ne` and their `core.` forms | [Assertions](testing.md#assert-behavior) |
| `core.drop(value)` | [Destruction and ownership](ownership.md), [core signatures](memory-and-ffi.md#implemented-core-calls) |
| `core.wrapping_add`, `core.wrapping_sub`, `core.wrapping_mul` | [Numeric behavior](implementation-syntax.md#numeric-behavior) |
| `core/mem` | [Layout, initialization, string views, and exchange](memory-and-ffi.md#implemented-core-calls) |
| `core/ptr` | [Pointers and unsafe memory access](memory-and-ffi.md#implemented-core-calls) |
| `core/mmio` | [Volatile hardware access](memory-and-ffi.md#mmio-and-volatile-access) |
| Typed storage intrinsics | [Container elements](container-elements.md), [typed allocated storage](memory-and-ffi.md#typed-allocated-storage) |
| Atomic compiler operations used by `std/sync/atomic` | [Atomics](synchronization.md#atomics) |

Import `core/mem`, `core/ptr`, or `core/mmio` before using its short package name.
`core.drop`, wrapping arithmetic, and assertions need no import. Prefer safe
library abstractions over the storage intrinsics they use internally.

## Virtual imports and target providers

Imports ending in `/native` select the implementation for the target. Some are
virtual package names rather than independent source files:

| Import | Contract and supported targets |
| --- | --- |
| `std/platform/native` | [Native platform values and handles](platform.md) |
| `std/fs/native` | [Filesystem providers](filesystem.md) |
| `std/env/native` | [Native environment strings](environment.md) |
| `std/process/native` | [Process providers](processes.md) |
| `std/thread/native` | [Native threads](threads.md) |
| `std/sync/native` | [Blocking synchronization](synchronization.md) |
| `std/net/native` | [Native sockets](networking.md) |
| `std/time/native` | [Hosted clocks](time.md) |
| `std/tls/native` | [TLS providers](tls.md) |
| `std/web/native` | [Static-file providers](web.md) |

`std/net/native` is the socket entry point; many other native packages implement
the higher-level facade shown in their guide. Explicit `/linux` and `/windows`
pages document provider boundaries. Their presence in the index does not mean
both can be used on every target. C runtime support files are implementation
dependencies, not importable Dodo packages.

## Versions and source links

These pages describe the source checkout used to build this website. The library
is embedded in each compiler binary. A released `dodo` can therefore expose
fewer methods than the latest documentation on `main`; check `dodo --version`
and the [release notes](https://github.com/Jotrorox/dodo/blob/main/CHANGELOG.md).

The generator preserves source spellings, including a few accepted historical
type-first fields. Application examples use current canonical syntax. Source
links point to `main` and show the declaration's line in the documentation build;
line numbers can shift after later commits. For a reproducible comparison,
inspect the same file at the tag or commit used to build your compiler.
