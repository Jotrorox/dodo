---
title: "Binary bytes and buffers"
description: "Binary bytes and buffers: a runnable starting point, storage choices, and detailed contracts."
section: "Standard library"
order: 143
---

Use `std/bytes.Writer.new` and `Reader.new` to build and inspect binary messages
in a fixed byte array. They are portable, allocation-free cursors. Use
[UTF-8 text](text.md) when the contents are human-readable strings.

## Quickstart

Save this as `bytes_start.dodo`:

```dodo test
package bytes_start
import "std/bytes"

fn round_trip() -> void!bytes.RangeError {
    storage := [0u8; 4]
    writer := bytes.Writer.new(&mut storage)
    writer.write_u32_be(42)?
    // Reader borrows the written prefix, so finish writing before reading.
    reader := bytes.Reader.new(writer.written())
    assert_eq(reader.read_u32_be()?, 42u32)
    return ok()
}
fn main() -> i32 {
    match round_trip() {
        ok() => { return 0 },
        err(_) => { return 1 },
    }
}
```

```sh
dodo run bytes_start.dodo
```

Expected output: none; exit 0 confirms the four big-endian bytes decode to 42.
Exit 1 means the requested field did not fit. Check message lengths before
reading, or increase output storage; failed cursor operations preserve position
and contents. `?` propagates the Result to `main`, which chooses the exit status.

Start with the fixed writer above. When a buffer must grow, import
`std/arena_bytes` and construct it with an `alloc/arena`, initial capacity and
maximum length. `std/pool_bytes` is the fixed-block alternative.
`std/bytes_alloc` defines the owned buffer and unsafe custom-allocator entry
point; prefer the concrete safe constructors. [Allocation](allocation.md)
explains their storage lifetime and failure behavior.

## Binary bytes and growing buffers

`std/bytes` imports `core/bytes` under an explicit alias. It operates on ordinary
checked byte slices and never allocates. `check_range(length, offset, count)`
uses subtraction before addition, so overflowing and reversed extents produce
`RangeError.OutOfBounds`. `view` and `view_mut` return source-dependent checked
slices. Valid zero-length views include the end of the input. `find` and `rfind`
search byte subsequences (an empty needle matches the start/end respectively);
`find_byte`, `equal`, `compare`, `starts_with`, and `ends_with` reuse core helpers.
No operation interprets UTF-8 or requires aligned storage.

`Reader.new(data)` has `position`, `remaining`, `rest`, checked absolute `seek`,
`skip`, `take(count)`, `read_u8`, and `read_u16_le`/`read_u16_be` through 32/64-bit
variants. `take` returns a checked borrowed view and advances only on success.
`Writer.new(storage)` exposes `position`, `remaining`, `written`, `seek`, `put`,
`write_u8`, and matching endian methods. Operations validate the complete range
before changing data or position. `written()` is the prefix ending at the
current cursor, including after a seek; it is not a high-water mark. `std/bytes`
also provides offset-based endian free functions with the same names. Fixed
writers never grow or allocate. See `examples/bytes.dodo`.

`std/bytes_alloc.Buffer<A>` is imported separately. Safe constructors
`arena_bytes.new(&mut arena, capacity, limit)` and
`pool_bytes.new(&mut pool, capacity, limit)` use the existing allocation layouts
and allocator contracts. Both capacities are explicit byte counts. The maximum
must fit `alloc/layout.max_size()` and initial capacity must not exceed it.
The buffer starts with length zero. A zero maximum supports empty operations;
appending a byte returns `AllocError.Exhausted`.

`len`, `capacity`, `limit`, `as_slice`, and `as_mut_slice` expose initialized
storage. `reserve(additional)` reserves beyond the current length; `extend` and
`push` append bytes. They return `void!AllocError` and preserve data, length, and
capacity on failure. Growth doubles capacity, clamps it to the specified limit,
and temporarily requires both old and new allocations to be live. No global
allocator or fallback is consulted. Arena growth consumes additional arena space
because individual deallocation does not reclaim it; pools must have another
suitable free block. `truncate(length)` clamps to the existing length and
`clear()` empties the logical contents; neither zeroes storage nor shrinks it.

The buffer owns its block and exclusively borrows its allocator. Destruction
releases the current block exactly once. Each successful growth releases the
old block. `as_slice`/`as_mut_slice` retain a checked borrow of the buffer; a
subsequent use of a view prevents growth, mutation, moving, or destruction in
between. Raw pointers are invalidated by growth, movement of their actual
storage, or destruction. Safe code cannot append a view of the same buffer to
itself. Moving a buffer transfers ownership and its allocator dependency; it
does not clone the allocation. The unsafe generic `bytes_alloc.new` constructor
requires an allocator honoring the documented unique-storage and deallocation
contracts. Its methods must be public for static dispatch.
