---
title: "Memory and foreign calls"
description: "Implemented layouts, C interoperation, pointer intrinsics, and MMIO limits in Dodo 0.1.3."
section: "Language reference"
order: 230
---

Use this reference when writing a C interface, hardware driver, allocator, or
other low-level library. Ordinary application code can usually stay with checked
references, [slices](language-basics.md#slices-borrow-a-region), and the
[standard library](standard-library.md).

The compiler checks types and ordinary borrowing inside unsafe code, but cannot
prove that a raw address identifies live initialized memory. An `unsafe` block
marks the place where your code supplies that guarantee. An `unsafe fn` makes
its caller responsible for documented preconditions; its body still needs
explicit unsafe blocks for unchecked operations.

The normative allocation, cast, and aliasing contracts are in
[specification section 14](language-spec-0.1.md#14-raw-memory-validity); layout and
ABI rules are in [section 16](language-spec-0.1.md#16-foreign-interfaces-layout-and-target-attributes).
See the [compiler overview](implementation.md#remaining-design-surface)
for planned facilities that are not yet available.

## Start with an owned value

```dodo test
package main

import "core/ptr"

fn main() {
    value := 40i32
    pointer := ptr.from_mut(&mut value)
    unsafe {
        // value is live, initialized, aligned, and not borrowed elsewhere.
        previous := ptr.read(pointer)
        ptr.write(pointer, previous + 2)
    }
    core.assert_eq(value, 42i32)
}
```

Obtaining the pointer is safe. Reading or writing it requires `unsafe`.
The pointer does not keep `value` alive or extend a checked borrow. For an owned
non-copy payload, `ptr.read` transfers the value without clearing its old bytes;
the caller must prevent a second read or destruction from creating two owners.
This example uses an integer to avoid that additional ownership obligation.

Use checked references whenever they express the operation. A raw pointer is
useful for a foreign boundary or storage implementation, not a way to avoid a
borrow-checking error.

## Struct layout and the C ABI

Struct fields keep declaration order and LLVM target ABI alignment/padding.
`@repr(C)` uses this C-compatible field layout. Native Dodo function ABI is
compiler-internal and unstable. Primitive and raw-pointer `extern "C"`
parameters/results use LLVM's C calling convention; C aggregate-by-value ABI
classification and variadics are not supported. Pass pointers to `@repr(C)`
structs instead. Imported foreign declarations use the declared final name as
the external symbol; C definitions do the same. Native Dodo functions use
`dodo.<root-package>.<qualified-name>`.

Declare a foreign function without a body, then call it in an unsafe block:

```dodo
package main

unsafe extern "C" fn abs(value: i32) -> i32

fn main() {
    unsafe {
        // C abs is defined for this input; the signed minimum is excluded.
        core.assert_eq(abs(-42), 42i32)
    }
}
```

This example requires a hosted C runtime that supplies `abs`. Dodo declarations
must match the foreign library's actual argument widths, signedness, ABI, and
symbol names. An external declaration does not automatically link its library;
use the [linker options](command-line.md) when extra libraries or objects are
needed. Dodo can also define a function with a body using `extern "C" fn`.

`&str` is not a C string. Its bytes have no guaranteed trailing NUL, and its
representation includes a length. C interfaces that expect a string usually
need an explicitly prepared byte buffer and raw pointer.

## Enum, Result, and Option layout

Enum storage is a zero-based u32 tag plus one shared payload area. Each variant's
fields retain declaration order, and the area accommodates the largest payload
and strictest alignment. Result storage is a bool error tag (false for ok, true
for err) plus shared storage for success or error; a `void` alternative needs no
payload bytes. Option storage is a bool presence tag plus a payload slot. These
rules include target ABI padding and no niche optimizations; they do not define
a C enum or union ABI. Only the active alternative owns resources. Inactive
bytes and padding are not guaranteed to be initialized. Rebuild native objects
together when migrating from the earlier separate payload slots.

## Query layout without guessing

```dodo test
package main

import "core/mem"

@repr(C)
struct Header {
    tag: u8
    length: u32
}

fn main() {
    core.assert_eq(mem.size_of::<u32>(), 4usize)
    core.assert_eq(mem.offset_of::<Header>("tag"), 0usize)
    core.assert(mem.size_of::<Header>() >= 5usize)
    core.assert(mem.align_of::<Header>() >= mem.align_of::<u32>())
}
```

`size_of`, `align_of`, and `offset_of` use the selected compilation target. Padding
means a struct's size need not equal the sum of its field sizes. `offset_of`
requires a literal name of one directly accessible field; it does not accept a
nested field path. None of these queries reads a value or allocates storage.

Do not serialize a struct by assuming that padding or inactive enum payload
bytes are initialized. Use explicit byte encoding such as [bytes](bytes.md) or
[JSON](json.md) for portable data formats.

## Entry points and linking

Hosted executables add a C `main` wrapper around the source `main`. Object,
assembly, bitcode, and IR emission do not add startup code. A native C linker
driver links executables; the driver is executed directly with argument vectors.
Custom targets require caller-supplied startup and linker configuration.

The hosted source entry is a root non-C `main` function with no
parameters, returning `void` or `i32`. `fn main()` means `void` and exits with
status zero; an `i32` return supplies the process exit status. Put fallible
startup in a helper returning a Result and handle it in `main`. Import
`std/env` to access process arguments rather than adding parameters to `main`.

## Implemented core calls

This table supplies the normative core call signatures and restrictions referenced
by the 14 September edition of the 0.1 specification. Calls with an owner argument
also obey its checked dependency rules. Other library calls are versioned in their
respective references.

| Call | Return | Requirements |
| --- | --- | --- |
| `core.drop(value)` | `void` | Consumes a value; cannot discard a Result. |
| `core.assert(condition)` | `void` | `condition: bool`; a false condition panics. |
| `core.assert_eq(left, right)`, `core.assert_ne(left, right)` | `void` | Checked comparison assertions; see [testing](testing.md). |
| `core.wrapping_add(a, b)`, `core.wrapping_sub(a, b)`, `core.wrapping_mul(a, b)` | Same integer type | Safe explicit modulo arithmetic; optional `::<T>`; not constant expressions. |
| `mem.size_of::<T>()` | `usize` | Selected-target ABI size; `void` is zero. |
| `mem.align_of::<T>()` | `usize` | Selected-target ABI alignment; `void` is one. |
| `mem.offset_of::<T>("field")` | `usize` | Direct accessible field of a struct; selected-target ABI offset. |
| `mem.replace(&mut value, replacement)` | Old value | Moves without dropping; disallows checked-borrow/Result-containing values. |
| `mem.swap(&mut a, &mut b)` | `void` | Disjoint initialized values; same type restrictions as `replace`. |
| `mem.uninit::<T>()` | `MaybeUninit<T>` | Opaque storage with T's size/alignment; never implicitly drops T. |
| `mem.init(value)` | `MaybeUninit<T>` | Moves T into opaque storage; cannot hide checked borrows or Results. |
| `mem.assume_init(storage)` | `T` | Unsafe; consumes fully initialized storage; checked-borrow T unsupported. |
| `mem.uninit_as_ptr(&storage)` | `*const T` | Obtains a raw pointer without reading or initializing T. |
| `mem.uninit_as_mut_ptr(&mut storage)` | `*mut T` | Requires an exclusive reference to the storage. |
| `mmio.read8/16/32/64(address)` | Corresponding unsigned integer | Unsafe; `address: usize`. |
| `mmio.write8/16/32/64(address, value)` | `void` | Unsafe; width-matched value. |
| `ptr.read(pointer)` | Pointee value | Unsafe; raw pointer. |
| `ptr.write(pointer, value)` | `void` | Unsafe; mutable raw pointer; overwrites without dropping. |
| `ptr.read_unaligned(pointer)` | Pointee value | Unsafe; load alignment one. |
| `ptr.write_unaligned(pointer, value)` | `void` | Unsafe; store alignment one. |
| `ptr.read_volatile(pointer)` | Pointee value | Unsafe; volatile load. |
| `ptr.write_volatile(pointer, value)` | `void` | Unsafe; volatile store. |
| `ptr.offset(pointer, offset)` | Same pointer type | Unsafe; `offset: isize`, in elements. |
| `ptr.from_ref(&value)` / `ptr.from_mut(&mut value)` | `*const T` / `*mut T` | Safe pointer conversion; dereferencing remains unsafe. |
| `ptr.as_ptr(slice)` / `ptr.as_mut_ptr(slice)` | `*const T` / `*mut T` | Borrowed array or slice; mutable form requires exclusive access. |
| `mem.str_bytes(text)` | `&[u8]` | Safe; preserves the string's checked dependencies. |
| `mem.str_from_utf8(bytes)` | `&str` | Unsafe; bytes must be valid UTF-8, dependencies preserved. |
| `ptr.is_null(pointer)` | `bool` | Safe; does not access storage. |
| `ptr.borrow(pointer, owner)` | `&T` | Unsafe; raw `*const T` or `*mut T`, checked owner reference or slice. |
| `ptr.borrow_mut(pointer, owner)` | `&mut T` | Unsafe; mutable raw pointer and exclusive checked owner. |
| `ptr.borrow_slice(pointer, count, owner)` | `&[T]` | Unsafe; initialized range, count in elements, checked owner. |
| `ptr.borrow_slice_mut(pointer, count, owner)` | `&mut[T]` | Unsafe; uniquely accessible initialized range and exclusive owner. |
| `ptr.copy(source, destination, count)` | `void` | Unsafe; copies `count` T elements; overlapping ranges allowed. |
| `ptr.copy_nonoverlapping(source, destination, count)` | `void` | Unsafe; copied ranges must not overlap. |
| `ptr.write_bytes(destination, byte, count)` | `void` | Unsafe; fills `count * size_of<T>()` bytes with a `u8` pattern. |
| `ptr.drop_in_place(pointer)` | `void` | Unsafe; destroys one initialized T, including its fields; does not free storage. |

The following compiler calls support checked library implementations:

| Call | Return | Requirements |
| --- | --- | --- |
| `mem.storage_type::<T>()` | `void` | Rejects unsupported typed-storage element types; see [container elements](container-elements.md). |
| `ptr.store(pointer, value, &mut owner)` | `void` | Unsafe; matching typed witness, valid destination, and checked dependency deposition. |
| `ptr.take(pointer, &mut owner)` | `T` | Unsafe; removes one initialized value, transferring stored sources. |
| `ptr.relocate(source, destination, count, &mut owner)` | `void` | Unsafe; moves within one owner, allowing overlap; preserves source dependencies. |
| `ptr.view(pointer, &owner)` | `&T` | Unsafe; checked shared typed-storage view. |
| `ptr.view_slice(pointer, count, &owner)` | `&[T]` | Unsafe; checked shared typed-storage slice. |
| `mem.split_at_mut::<slice.SplitMut<T>>(data, mid)` | `slice.SplitMut<T>` | Safe narrow intrinsic; traps if `mid > data.len`; prefer `slice.split_at_mut` for an Option result. |
| `mem.assert_send::<T>()`, `mem.assert_sync::<T>()` | `void` | Compile-time thread capability check; see [threads](threads.md). |
| `mem.callback::<Types...>(function_name)` | `*const u8` | Unsafe; address of a statically selected `unsafe fn(*mut u8) -> void` specialization, for a matching native trampoline. |

Atomic raw-storage intrinsics are described [below](#atomic-storage-intrinsics).
There is no user-defined intrinsic mechanism. Each package must directly import
`core/mem`, `core/ptr`, or `core/mmio` for the corresponding calls.

## Initialize opaque storage

`MaybeUninit<T>` lets a library reserve storage without pretending it contains a
valid `T`. It has `T`'s size and alignment but does not implicitly destroy a `T`:

```dodo test
package main

import "core/mem"
import "core/ptr"

fn main() {
    storage := mem.uninit::<i32>()
    unsafe {
        // Write a complete, valid i32 before transferring it out of storage.
        ptr.write(mem.uninit_as_mut_ptr(&mut storage), 42i32)
    }
    value := unsafe { mem.assume_init(storage) }
    core.assert_eq(value, 42i32)
}
```

`mem.init(value)` creates already initialized opaque storage. `mem.assume_init`
consumes the storage and transfers its contained value, but does not check the
bytes at runtime. Calling it before complete initialization violates its unsafe
contract. Destroying a still-initialized `MaybeUninit<T>` does not destroy its
hidden payload; low-level owners must arrange exactly one transfer or explicit
destruction. These operations cannot be used to hide checked borrows or pending
Result obligations.

## Unsafe memory access

### Raw casts and pointer arithmetic

| Conversion or operation | Rule |
| --- | --- |
| `&T` to `*const T` | Safe; same pointee type, no lifetime extension. |
| `&mut T` to `*mut T` or `*const T` | Safe; later accesses must honor the original permissions. |
| Raw pointer to another raw-pointer type | Unsafe; does not validate the resulting pointee or alignment. |
| Integer to pointer, or pointer to integer | Unsafe; an address alone does not prove usable storage. |
| Raw pointer to checked reference | Rejected; use an owner-bound `ptr.borrow*` primitive. |
| `ptr.offset(pointer, offset)` | Unsafe; `offset: isize` counts elements, not bytes. |

On the supported integral-address profile, integer/pointer casts preserve low
bits, discard excess high bits, and zero-extend when widening. These are different
from checked numeric casts. Zero produces a null pointer, and `ptr.is_null`
checks it without dereferencing. Pointer equality compares addresses, not proof
of common allocation identity.

For allocation-derived pointers, an offset must stay within the same allocation
or one past its end, its signed byte displacement must fit `isize`, and address
calculation must not wrap. A one-past pointer cannot be used for a nonempty read
or write. Moving, freeing, or reallocating its source may invalidate the pointer;
reusing the same numerical address does not revive the old storage lifetime.

### Access and owner-bound views

Aliases above require their matching `core/...` import. Raw memory operations
remain subject to the specification's validity and aliasing obligations.
Reading/writing checked-borrow-carrying values through raw pointer intrinsics and
raw-pointer-to-checked-reference casts are rejected in this release. Unsafe
functions still need explicit unsafe blocks for unchecked operations.

Raw copies and byte fills operate on byte ranges with alignment one and do not
establish initialization or ownership by themselves. Their element count times
element size must fit in `usize`, and nonempty source/destination ranges must be
valid for the requested reads/writes. Zero-byte operations accept null pointers,
including nonzero counts of zero-sized elements. A bitwise copy of an owning
value must not result in two live owners being destroyed. `drop_in_place`
requires a valid aligned pointer to an initialized value, and that value must
not subsequently be dropped again without reinitialization.

Raw-pointer conversions do not extend storage lifetimes. The four explicit
unsafe `ptr.borrow*` operations create checked views tied to a supplied owner
reference or slice. The caller must establish that the owner keeps the complete
storage alive, nonnull, correctly aligned, initialized, and valid for the
returned lifetime. The byte extent must fit in `isize`, stay within one live
allocation, and respect all existing aliases. Empty and zero-sized views still
require nonnull aligned pointers. The pointer need not point inside the owner
itself, which permits a buffer or box to anchor a view into its allocation.

The pointer/owner correspondence is an unsafe invariant; the compiler cannot
establish it from their bytes. It does check that returned views retain the
owner's complete dependencies and cannot outlive, move, destroy, or mutably
alias their owner. Mutable views require a mutable raw pointer and exclusive
owner access. Borrowing a container exclusively retains its inherited shared
allocator dependencies as shared. Raw casts still cannot fabricate checked
references, and unsafe code must not invalidate an allocation while a checked
view remains live.

For `ptr.borrow*`, element types containing checked references or Results are
rejected recursively:
the owner loan cannot reconstruct hidden element provenance or Result handling
obligations. The owner's own checked fields remain tracked. Owner expressions
are evaluated after the pointer (and count for slices), even though their
address is not used by the generated view. Safe container methods establish the
unsafe invariant privately and return ordinary checked views.

These are two independent restrictions. A stored reference needs its referent
dependencies transferred on insertion and removal; an owner loan alone only
protects storage. A stored Result needs its pending handling obligation
transferred as well, including on failed insertion and destruction. Matching a
value before replacing it does not handle the replacement. Ordinary assignment
of Result-containing values through fields, indices, or references is rejected
for the same missing handling-state transfer. See the
[container element guide](container-elements.md) for the implemented typed-storage
primitives and their checked mutation/return contracts. Existing raw pointer and
opaque storage primitives retain their restrictions.

`mem.str_from_utf8` requires the complete byte slice to be valid UTF-8. Prefer
safe `text.Text.new(bytes)?.as_str()` after binding the text view. It creates no
allocation and does not extend the source lifetime.

Raw access must respect all live checked borrows. Allocator and storage examples
are in [standard library](standard-library.md).

## MMIO and volatile access

MMIO uses LLVM volatile loads/stores directly, without fabricating checked
references. The compiler rejects widths above the target pointer width; actual
device access legality remains a target/board obligation. Volatile operations
are not atomics or CPU barriers.

LLVM lowering follows the [LLVM Language Reference](https://llvm.org/docs/LangRef.html),
including explicit overflow intrinsics, volatile instructions, and x86 SysV
C-ABI integer extension attributes on declarations and call sites.

## Atomic storage intrinsics

Prefer the checked `std/sync/atomic` API documented in
[synchronization](synchronization.md). Its implementation uses these unsafe
`core/mem` operations on aligned raw integer pointers:

| Operation | Returned value |
| --- | --- |
| `mem.atomic_load(pointer, order)` | Loaded integer. |
| `mem.atomic_store(pointer, value, order)` | `void`. |
| `mem.atomic_exchange(pointer, value, order)` | Previous integer. |
| `mem.atomic_fetch_add(pointer, amount, order)` | Previous integer; addition wraps at the integer width. |
| `mem.atomic_compare_exchange(pointer, expected, replacement, success, failure)` | Observed old integer; writes only when it equals `expected`. |

All operations require `unsafe`. Modification requires a mutable raw pointer;
load accepts a const pointer. Supported integer widths cannot exceed the target's
supported lock-free width, which this compiler bounds by pointer width. An
optional explicit type argument must match the pointer's integer element type.

Orders are compile-time integers: 0 Relaxed, 1 Acquire, 2 Release, 3 AcqRel,
4 SeqCst. Loads allow 0, 1, 4; stores allow 0, 2, 4. Compare-exchange failure
cannot release or be stronger than success: success 0/2 allows failure 0,
success 1/3 allows failure 0/1, and success 4 allows failure 0/1/4. Invalid orders
are compile errors. The caller must provide valid initialized storage and obey
the atomic storage and concurrency contract; these calls are not a general
exception allowing arbitrary writes through shared references.


## Typed allocated storage

`mem.storage_type::<T>()` checks the supported element category. The unsafe
`ptr.store`, `ptr.take`, `ptr.relocate`, `ptr.view`, and `ptr.view_slice` primitives
require a matching zero-length typed witness in their checked owner. They track
element dependencies separately from storage borrows; they cannot store owned
Results or exclusive-reference elements. Pointer validity, initialized extents,
and exactly-once ownership transfer remain unsafe obligations. See the complete
[contracts and restrictions](container-elements.md#checked-region-invariant).
