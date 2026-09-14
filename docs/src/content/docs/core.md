---
title: "Core utilities"
description: "Core utilities: a runnable starting point, storage choices, and detailed contracts."
section: "Standard library"
order: 141
---

Use `core/num.checked_add` for size calculations that can fail. The `core/*`
family also covers bytes, slices, ASCII, options, memory and device primitives.
These APIs are portable and need no allocator. Start with checked values and
slices; raw pointers and MMIO are for explicit unsafe integration.

## Quickstart

Save this as `core_start.dodo`:

```dodo test
package core_start
import "core/num"
import "core/ascii"
import "core/slice"

fn main() -> i32 {
    // num handles pointer-sized counts; this array is all the storage needed.
    values := [10i32, 20, 30]
    match num.checked_add(1, 1) {
        ok(index) => {
            match slice.get(&values, index) {
                some(value) => { assert_eq(*value, 30) },
                none => { return 2 },
            }
        },
        err(_) => { return 1 },
    }
    assert(ascii.is_digit(55u8))
    return 0
}
```

```sh
dodo run core_start.dodo
```

Expected output: none; exit status 0 confirms that index 2 contains 30 and byte
55 is an ASCII digit. Exit 1 means size arithmetic failed: reject the requested
size before allocating. Exit 2 means the index is absent: check the length or
choose a fallback, rather than dereferencing it.

Imports expose their final component (`num`, `ascii`, `slice`); assertions and
`core.drop` are built in. See [packages](packages.md) for aliases.
`core/bytes` handles raw byte operations; [binary bytes](bytes.md) adds cursors.
[Slice splitting](slice-splitting.md) covers disjoint mutable views.

`core/mem`, `core/ptr`, and `core/mmio` are compiler intrinsic packages.
Use `mem.size_of::<T>()` for layouts, checked references for ordinary access,
and [memory and FFI](memory-and-ffi.md) for raw-pointer contracts. MMIO requires
a board-specific valid device address and access width; there is no universally
runnable hosted MMIO example. The [GPIO example](https://github.com/Jotrorox/dodo/blob/main/examples/gpio.dodo)
is a hardware template, not a desktop program.

## Portable core utilities

`num.MAX` and `num.MIN` follow the selected target's pointer width. The functions
`checked_add`, `checked_sub`, `checked_mul`, `checked_div`, `checked_rem`,
`checked_shl`, and `checked_shr` return `Result<usize, ArithmeticError>`.
Errors distinguish overflow, division by zero, invalid shifts, and invalid
alignment. `checked_shl` rejects shifted-out bits as overflow. Saturating and
wrapping variants are provided for addition, subtraction, and multiplication.
`min`, `max`, `is_power_of_two`, and `align_up` complete the resource-arithmetic
helpers. This module currently covers `usize`, not every numeric type.

The wrapping helpers use compiler primitives that lower directly to integer
addition, subtraction, and multiplication without overflow checks or loops.
`core.wrapping_add(a, b)`, `core.wrapping_sub(a, b)`, and
`core.wrapping_mul(a, b)` are also available without an import for any integer
type. Both operands and the result have the same type; an optional explicit
type argument, such as `core.wrapping_mul::<u32>(a, b)`, selects that type.
Results wrap modulo 2 to the power of the type's width, including for signed
integers. Ordinary arithmetic operators continue to check overflow.

`bytes` operates on borrowed byte slices. `equal`, `compare`, `starts_with`,
`ends_with`, and `find` do not allocate. `compare` returns -1, 0, or 1 in
lexicographic order. `fill` and `reverse` mutate a slice in place. `copy_from`
requires equal lengths and returns a `BufferError.LengthMismatch` otherwise.

Endian functions are named `read_u16_le`, `write_u16_le`, and so on for 16-, 32-,
and 64-bit unsigned integers, with both `le` and `be` forms. They take a byte
offset, permit unaligned byte positions, and return `BufferError.OutOfBounds`
for invalid ranges, including overflowing offsets. Writes validate the entire
range before changing any byte. They work independently of CPU endianness.

`slice.get`, `get_mut`, `first`, and `last` return optional checked references;
`subslice` and `subslice_mut` return optional views using an exclusive end
index. Empty valid ranges succeed, reversed/out-of-bounds ranges return `none`.
Returned borrows cannot outlive or conflict with their source. `is_empty` works
with any element type.

`slice.split_at_mut(data, mid)` returns `Option<slice.SplitMut<T>>`. Consume
the pair with a struct pattern to obtain simultaneously usable, disjoint mutable
halves. See [mutable slice splitting](slice-splitting.md) for bounds, reborrowing,
source dependencies, and the pair's construction and mutation restrictions.

`ascii` classifies bytes with `is_ascii`, `is_digit`, `is_hex_digit`,
`is_lowercase`, `is_uppercase`, `is_alphabetic`, `is_alphanumeric`,
`is_whitespace`, `is_control`, and `is_graphic`. Whitespace includes ASCII space
and bytes 9 through 13. Case conversion leaves non-ASCII bytes unchanged;
`digit_value` and `hex_value` return `Option<u8>`.

`option.take(&mut value)` leaves `none` and returns the old option;
`option.replace(&mut value, replacement)` installs `some(replacement)` and
returns the old option. They inherit `mem.replace`'s current restriction against
payloads containing checked borrows or Results. `unwrap_or` consumes both its
arguments and returns the payload or fallback; the unused value is destroyed.

## Opaque storage and moving values

`MaybeUninit<T>` is a move-only built-in storage type with the size and alignment
of `T`. It never automatically destroys a `T`, including when it is a struct
field or array element. Its bytes do not constitute a valid initialized value
until the unsafe caller establishes that invariant.

```dodo test
package storage

import "core/mem"
import "core/ptr"

fn main() -> i32 {
    storage := mem.uninit::<i32>()
    pointer := mem.uninit_as_mut_ptr(&mut storage)
    unsafe { ptr.write(pointer, 42) }
    value := unsafe { mem.assume_init(storage) }
    return value - 42
}
```

`mem.init(value)` moves an initialized value into opaque storage. Dropping the
storage does not drop that value. `mem.assume_init(storage)` consumes storage
and restores ordinary ownership and destruction; it is unsafe because every
byte and invariant of `T` must be valid. `mem.uninit_as_ptr` and
`mem.uninit_as_mut_ptr` expose raw pointers without dereferencing them.

`mem.replace(&mut destination, replacement)` returns the old value without
destroying it and installs the replacement. `mem.swap(&mut a, &mut b)` exchanges
two disjoint initialized values without running destructors. Arguments are
evaluated before storage changes; a failed `?` in the replacement leaves the
original value intact.

`mem.offset_of::<Record>("field")` returns a direct struct field's ABI offset.
The field name must be a string literal and satisfy ordinary field visibility;
nested paths and runtime reflection are not supported.

Exchange currently rejects types containing checked borrows or Results because
their source dependencies and handling obligations cannot yet be transferred
through this interface. `mem.init` likewise cannot hide either in opaque
storage. Reading a checked-borrow-bearing `T` back from raw or opaque storage
is unsupported. These restrictions also apply transitively through aggregates.

See [memory and foreign calls](memory-and-ffi.md) for pointer signatures and
unsafe contracts. The compiler may lower memory operations to target toolchain
helpers such as `memcpy`, `memmove`, and `memset`. Freestanding programs must
supply those helpers when referenced, as well as their own startup/linker setup;
none of these helpers requires a heap or an OS.
