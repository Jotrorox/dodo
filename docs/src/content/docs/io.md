---
title: "Byte I/O"
description: "Byte I/O: a runnable starting point, storage choices, and detailed contracts."
section: "Standard library"
order: 144
---

Use `std/io.read_exact` when a task needs a known number of bytes, and
`io.write_all` to deliver a complete slice. `std/io` is portable: start with
`MemoryReader` and `MemoryWriter`, then use the same helpers with [files](filesystem.md).
For standard streams and bounded `read_line`, see [console I/O](console.md).
For portable printing to any writer, see [formatting](formatting.md).

## Quickstart

Save this as `io_start.dodo`:

```dodo test
package io_start
import "std/io"
import "core/bytes"

fn transfer() -> void!io.Error {
    source := io.MemoryReader.new(b"hello")
    storage := [0u8; 5]
    count := io.read_exact(&mut source, &mut storage)?
    assert_eq(count, 5usize)
    assert(bytes.equal(&storage, b"hello"))
    return ok()
}
fn main() -> i32 {
    match transfer() {
        ok() => { return 0 },
        err(reason) => {
            if reason.kind == io.ErrorKind.UnexpectedEof { return 2 }
            return 1
        },
    }
}
```

```sh
dodo run io_start.dodo
```

Expected output: none; exit 0 confirms five bytes were read and equal `hello`.
`core/bytes` is imported only to compare the result. The five-byte array is the
entire destination; no allocator is needed. To try the failure path, shorten
the input to `hell`: exit 2 reports incomplete input. Reject that record or ask
for more input. Other I/O failures exit 1. Inspect `reason.transferred` before
retrying: a failure can still have consumed or written a prefix.

Use `read_up_to` when a shorter file is acceptable, and bounded `copy` for
streaming. Add caller-backed buffering only when needed. `std/io_alloc` adds
append-to-owned-buffer operations; start with fixed scratch and an explicit
transfer limit, then choose [allocated bytes](bytes.md) if the result must grow.

## Portable byte I/O (`std/io`)

`std/io` imports only `core/bytes`, `core/num`, and `core/mem`. It operates on bytes and checked
borrowed slices, with no OS, libc, heap, locale, scheduler, global initialization,
or device discovery. Files, sockets, UARTs, and other platform adapters implement
the same small method contracts in independently imported packages. This package
provides memory implementations; it does not open files or configure hardware.

| Contract | Required public method |
| --- | --- |
| Blocking reader | `read(&mut self, destination: &mut[u8]) -> usize!io.Error` |
| Blocking writer | `write(&mut self, source: &[u8]) -> usize!io.Error` |
| Polling reader | `poll_read(&mut self, destination: &mut[u8]) -> io.Poll!io.Error` |
| Polling writer | `poll_write(&mut self, source: &[u8]) -> io.Poll!io.Error` |
| Optional absolute seek | `seek(&mut self, position: usize) -> usize!io.Error` |

These are structural contracts checked when generic helpers are instantiated.
Dodo monomorphizes calls to the concrete public methods; no trait objects,
reflection, closures, or scheduler are required. An adapter type exposing public
methods must itself be public. See `examples/io.dodo`
for memory-to-memory copying through caller scratch space.

A successful primitive operation returns its processed prefix length. A reader
may initialize only that prefix; a writer accepts only that prefix. Counts must
never exceed the supplied slice length. A nonempty successful read returning zero
means EOF. A nonempty successful write returning zero becomes `WriteZero` in
completion helpers. Empty requests succeed with zero and helpers never invoke the
underlying device. `MemoryWriter` and `Cursor` report `BufferFull` when full.

An `io.Error` contains `kind`, `transferred`, and an adapter-defined `code: i32`.
`transferred` records prefix progress even when the same operation fails. The
portable `io.failure(kind, transferred)` constructor sets `code` to zero. Error
kinds are `UnexpectedEof`, `WriteZero`, `Interrupted`, `BufferFull`, `OutOfBounds`,
`InvalidInput`, `InvalidProgress`, `Other`, `WouldBlock`, `TimedOut`, `Cancelled`,
`Closed`, `ConnectionReset`, `ConnectionRefused`, `BrokenPipe`, and `PermissionDenied`. A provider is responsible for
classifying recoverable device errors and preserving useful platform codes.
Helpers validate returned counts before forming their next checked subslice;
`InvalidProgress` means the provider violated its contract. This validation does
not repair an unsafe provider that already wrote outside its supplied slice.

`read_once` and `write_once` perform at most one call. `read_up_to` fills the
provided destination or successfully returns a shorter prefix at EOF.
`read_exact` returns `UnexpectedEof` for a short input. `write_all` accepts the
entire source or returns an error. Completion helpers retry `Interrupted` after
accounting for any prefix already processed; a persistent interruption can block
indefinitely. They return total progress for the entire helper invocation,
preserving a terminal device error and code even if its prefix finished the
requested byte count. None rolls back bytes already read or written.

`MemoryReader.new(&bytes)` retains a shared borrow and exposes `position`,
`remaining`, `read`, `poll_read`, and absolute `seek`. `MemoryWriter.new(&mut
bytes)` retains an exclusive borrow and exposes `position`, remaining capacity,
`written`, `write`, and `poll_write`. `Cursor.new(&mut bytes)` reads and overwrites
initialized fixed-size storage, with `bytes`, `position`, and `seek`. It never
grows. Seeking permits positions from zero through the end, returns the new
position, and leaves the old position unchanged on failure. Generic `io.seek`
requires only the optional seek method; read/write helpers never require it.

`LimitReader.new(&mut reader, limit)` yields a synthetic EOF after consuming at
most `limit` bytes, including progress reported with errors. `remaining` reports
its unused budget. `skip(&mut reader, count, &mut scratch)` discards exactly
`count` bytes or returns `UnexpectedEof` with total consumed progress. Empty
scratch is an `InvalidInput` unless the requested count is zero.

`copy(&mut reader, &mut writer, &mut scratch, limit)` copies at most `limit` bytes.
`CopyReport` contains `read`, `written`, and `eof`; `eof` is false when the limit
was reached without an additional read. A nonzero limit requires nonempty
scratch. A `CopyError` keeps total `read`, total `written`, and the terminal
`cause`. Thus `read - written` tells the caller how many bytes were consumed but
not delivered. Progress from a failed read is offered to the writer before the
read error is returned. If that write also fails, the write error takes
precedence. Scratch retains the most recently read chunk, including its unwritten
suffix. The explicit limit bounds every count and prevents count overflow; pass
`core/num.MAX` when that target-sized limit is suitable.

`BufferedReader.new(&mut reader, &mut storage)` and
`BufferedWriter.new(&mut writer, &mut storage)` reject empty storage and retain
exclusive checked borrows of both arguments. The reader exposes its current
`buffered()` view, drains read-ahead before fetching more, and preserves an error
that arrived with data until its final buffered byte is delivered. Dropping a
buffered reader discards unread read-ahead, so callers must consume it before
resuming directly from the underlying reader when every byte matters.

The writer exposes `pending()` and `capacity()`. Writes accept source bytes into
caller storage and flush when that storage becomes full and another write needs
space. `flush()` returns physical device progress and preserves the unwritten
suffix after failure so it can be retried. A write that fails while flushing old
bytes reports zero accepted bytes from its current input. There is deliberately
no I/O in destruction: dropping a writer discards pending bytes. Call `flush`
and handle its `Result` explicitly before releasing the device. Neither adapter
implicitly calls an optional platform flush operation such as a disk sync.

`Poll` contains `transferred` and `PollState.Ready`, `Pending`, or `Eof`. Pending
may accompany positive progress. Callers consume that prefix and decide when to
retry. `poll_read_once` and `poll_write_once` issue exactly one attempt and never
spin, wait, register a waker, or allocate. For a nonempty read, ready with zero
progress is invalid; EOF is explicit. For a nonempty write, ready with zero
progress is `WriteZero`, and EOF is invalid. Empty polling requests return ready
with zero progress without touching the provider. Interrupted polling errors are
returned directly rather than retried. Blocking and polling methods are separate
capabilities; neither is implicitly converted into the other.

## Allocation-dependent I/O (`std/io_alloc`)

`io_alloc.BufferWriter<A>.new(&mut bytes_alloc.Buffer<A>)` appends to an explicitly
allocated, capacity-limited buffer. The adapter holds an exclusive checked borrow
of the buffer; its allocator and backing storage remain live. Each `write` either
appends the entire source or returns `BufferFull` with zero progress and leaves
existing contents unchanged. Error codes preserve the `AllocError` category:
1 = `InvalidAlignment`, 2 = `SizeOverflow`, 3 = `Exhausted`, and
4 = `UnsupportedLayout`. No global allocator is consulted.

`io_alloc.append_from(&mut reader, &mut buffer, &mut scratch, limit)` combines
this append writer with bounded `io.copy`. Earlier successful chunks stay owned
by the buffer if a later read or allocation fails; `CopyError` preserves consumed
and appended counts. Both the transfer limit and the buffer's independent growth
limit apply. Construct buffers using `std/arena_bytes`, `std/pool_bytes`, or the
unsafe custom-allocator constructor in `std/bytes_alloc`; merely importing
`std/io` does not import any allocation package.

Checked views returned by memory or buffered adapters borrow the adapter, so it
cannot be mutated, moved, or destroyed while that view is still in use. Growing a
buffer likewise requires ending its outstanding views. Destruction releases
borrow dependencies deterministically and never hides a pending `Result`.



## Bounded and line-oriented input

`io.read_bounded(reader, output)` returns `{ read, eof }`. It uses a one-byte
probe after filling the destination: an exact fit has `eof=true`; overflow has
`eof=false` and discards that probe byte. Use it for a whole bounded input, not
for resumable chunk streaming. Errors retain `output[..transferred]`.

`io.read_line(&mut reader, &mut output)` returns `io.Line { count, end }`,
retaining LF and any preceding CR in `count`. Its `LineEnd` is `Newline`, `Eof`,
or `Full`. `Eof` with zero length is end-of-input; with
positive length it is a final unterminated line. `Full` consumes no lookahead:
process that fragment and call again to resume. Empty storage returns `Full`
without reading. Errors preserve the initialized prefix, even if it ends in LF.
Only `Interrupted` is retried. Neither helper decodes UTF-8 or allocates.
See [bounded console input](console.md#read-a-bounded-line) for standard streams.
