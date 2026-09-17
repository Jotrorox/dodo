---
title: "Binary bytes and buffers"
description: "Read and write binary records with checked cursors, byte order, fixed storage, and explicitly allocated buffers."
section: "Standard library"
order: 143
---

Use `std/bytes.Writer.new` and `Reader.new` to build and inspect binary messages
in a fixed byte array. They are portable, allocation-free cursors. Use
[UTF-8 text](text.md) when the contents are human-readable strings.

A byte slice (`&[u8]`) is a borrowed view into existing storage. A reader or
writer adds a position that advances after each successful field. An owned
buffer manages an allocation and can grow. These are separate choices: most
binary records need only an array and a cursor.

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

### Choose a byte API

| Task | API | Storage |
| --- | --- | --- |
| Inspect a field at a known offset | `bytes.read_u32_be(data, offset)` | Borrowed input |
| Read consecutive fields | `bytes.Reader.new(data)` | Borrowed input and a cursor |
| Write a fixed-size message | `bytes.Writer.new(&mut storage)` | Caller-owned mutable bytes |
| Select a checked range | `bytes.view(data, offset, count)` | Borrowed input; no copy |
| Append an unknown amount up to a limit | `arena_bytes.new(&mut arena, capacity, limit)` | Explicit allocator |
| Transfer bytes from a file or socket | `io.read_exact`, `io.write_all` | See [byte I/O](io.md) |

`bytes.view` takes **offset and count**. This differs from `data[start..end]`
and `core/slice.subslice`, whose second endpoint is exclusive. For example,
offset 2 and count 3 select indices 2, 3, and 4.

### Views, searching, and byte order

`std/bytes` imports `core/bytes` under an explicit alias. It operates on ordinary
checked byte slices and never allocates. `check_range(length, offset, count)`
uses subtraction before addition, so overflowing and reversed extents produce
`RangeError.OutOfBounds`. `view` and `view_mut` return source-dependent checked
slices. Valid zero-length views include the end of the input. `find` and `rfind`
search byte subsequences (an empty needle matches the start/end respectively);
`find_byte`, `equal`, `compare`, `starts_with`, and `ends_with` reuse core helpers.
No operation interprets UTF-8 or requires aligned storage. Suffix `le` means
little-endian (least significant byte first); `be` means big-endian (most
significant byte first). A protocol's byte order determines which to use,
regardless of the computer running the program.

### Reader and writer cursors

`Reader.new(data)` has `position`, `remaining`, `rest`, checked absolute `seek`,
`skip`, `take(count)`, `read_u8`, and `read_u16_le`/`read_u16_be` through 32/64-bit
variants. `take` returns a checked borrowed view and advances only on success.
`Writer.new(storage)` exposes `position`, `remaining`, `written`, `seek`, `put`,
`write_u8`, and matching endian methods. Operations validate the complete range
before changing data or position. `written()` is the prefix ending at the
current cursor, including after a seek; it is not a high-water mark. `std/bytes`
also provides offset-based endian free functions with the same names. Fixed
writers never grow or allocate. See `examples/bytes.dodo`.

### Encode a record with several fields

This record contains a one-byte version, a two-byte big-endian payload length,
and the payload. Each `?` stops at the first failed field. A successful `take`
returns a view, so finish using that view before advancing the reader again.

```dodo test
package binary_record
import "std/bytes"

fn example() -> void!bytes.RangeError {
    storage := [0u8; 7]
    writer := bytes.Writer.new(&mut storage)
    writer.write_u8(1)?
    writer.write_u16_be(4)?
    writer.put(b"Dodo")?
    assert_eq(writer.position(), 7usize)

    reader := bytes.Reader.new(writer.written())
    assert_eq(reader.read_u8()?, 1u8)
    length := reader.read_u16_be()?
    payload := reader.take(length as usize)?
    assert(bytes.equal(payload, b"Dodo"))
    assert_eq(reader.remaining(), 0usize)
    return ok()
}

fn main() -> i32 {
    match example() {
        ok() => { return 0 },
        err(_) => { return 1 },
    }
}
```

Save as `binary_record.dodo` and run `dodo run binary_record.dodo`. The program
exits with zero and prints nothing. Seven bytes are sufficient: 1 + 2 + 4.
Each failed operation preserves its own position and destination, but earlier
successful operations remain committed. Building a whole message is not one
transaction: discard a partially built record if a later field fails.

### Owned buffers and capacity limits

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

The logical maximum is not a promise that the allocator has enough backing
space. Budget for alignment and temporary old/new allocations too. Consult
[allocation](allocation.md) before choosing arena and pool sizes, and use the
[API reference](stdlib-api.md) for the full byte and buffer signatures.
