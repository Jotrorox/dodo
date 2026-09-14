---
title: "Memory and foreign calls"
description: "Implemented layouts, C interoperation, pointer intrinsics, and MMIO limits in Dodo 0.1.1."
section: "Language reference"
order: 230
---

This page covers the implemented memory layout and low-level interfaces in
Dodo 0.1.1. The normative allocation, cast, and aliasing contracts are in
[specification section 14](language-spec-0.1.md#14-raw-memory-validity); layout and
ABI rules are in [section 16](language-spec-0.1.md#16-foreign-interfaces-layout-and-target-attributes).
See the [compiler overview](implementation.md#remaining-design-surface)
for planned facilities that are not yet available.

## Struct layout and the C ABI

Struct fields keep declaration order and LLVM target ABI alignment/padding.
`@repr(C)` uses this C-compatible field layout. Native Dodo function ABI is
compiler-internal and unstable. Primitive and raw-pointer `extern "C"`
parameters/results use LLVM's C calling convention; C aggregate-by-value ABI
classification and variadics are not supported. Pass pointers to `@repr(C)`
structs instead. Imported foreign declarations use the declared final name as
the external symbol; Dodo functions use `dodo.<root-package>.<qualified-name>`.

## Enum, Result, and Option layout

Enum storage is a zero-based u32 tag plus separate storage for every variant's
payload, in declaration order. Result storage is a bool error tag (false for ok,
true for err) plus success/error slots; Option storage is a bool presence tag
plus a payload slot. These are the 0.1 storage rules, with target ABI padding
and no niche optimizations; they do not define a C enum ABI. `void` Result
success uses a one-byte placeholder slot. Inactive payloads and padding are not
guaranteed to contain initialized bytes.

## Entry points and linking

Hosted executables add a C `main` wrapper around the source `main`. Object,
assembly, bitcode, and IR emission do not add startup code. A native C linker
driver links executables; the driver is executed directly with argument vectors.
Custom targets require caller-supplied startup and linker configuration.

## Implemented core calls

This table supplies the normative core call signatures and restrictions referenced
by the 14 September edition of the 0.1 specification. Calls with an owner argument
also obey its checked dependency rules. Other library calls are versioned in their
respective references.

| Call | Return | Requirements |
| --- | --- | --- |
| `core.drop(value)` | `void` | Consumes a value; cannot discard a Result. |
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

## Unsafe memory access

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

Element types containing checked references or Results are rejected recursively:
the owner loan cannot reconstruct hidden element provenance or Result handling
obligations. The owner's own checked fields remain tracked. Owner expressions
are evaluated after the pointer (and count for slices), even though their
address is not used by the generated view. Safe container methods establish the
unsafe invariant privately and return ordinary checked views.

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
