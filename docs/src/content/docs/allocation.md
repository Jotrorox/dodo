---
title: "Allocation and boxes"
description: "Allocation and boxes: a runnable starting point, storage choices, and detailed contracts."
section: "Standard library"
order: 142
---

Use `alloc/arena.Arena.new` plus the safe `alloc/arena_box.new` constructor to
own one value in an explicit byte array. Allocation is portable and fallible.
Start with fixed storage for ordinary text and collections; use an allocator
when you need an owning box or growth.

## Quickstart

Save this as `allocation_start.dodo`:

```dodo test
package allocation_start
import "alloc/arena"
import "alloc/arena_box"

fn main() -> i32 {
    // 64 caller-owned bytes back this allocator; there is no global heap.
    storage := [0u8; 64]
    allocator := arena.Arena.new(&mut storage)
    match arena_box.new(&mut allocator, 42i32) {
        ok(value) => {
            assert_eq(value.into_inner(), 42)
            return 0
        },
        err(_) => { return 1 },
    }
}
```

```sh
dodo run allocation_start.dodo
```

Expected output: none; exit 0 confirms the box returned 42. Exit 1 means
allocation failed. Increase the backing array for `Exhausted`; reject invalid or
oversized layouts instead of retrying indefinitely. There is no implicit heap
fallback. `into_inner` consumes the box and moves out its value.

`arena` manages the bytes and `arena_box` supplies the safe concrete constructor.
For several simultaneously live owners, use `alloc/shared_arena` with
`alloc/shared_box` or the [shared collection constructors](collections.md).
Choose `alloc/pool` and `alloc/pool_box` only when individual fixed-size blocks
must be reusable. The generic `alloc/boxed` constructor and `alloc/block` are
implementation building blocks for custom allocators, not the starting path.
`alloc/error` names failures and `alloc/layout` validates requested layouts.

## Allocation layouts and failures

`layout.Layout.new(size, align)` returns a validated layout. Alignment must be a
nonzero power of two, and size rounded up to alignment must fit in `isize` on
the selected target. Fields are private so safe code cannot violate this rule.
Zero-sized allocations are supported.

Methods include `size`, `align`, `clone`, `padding`, `pad_to_align`, `align_to`,
`repeat`, and `extend`. `extend` returns an `ExtendedLayout` with `offset()`
for the appended field and `layout()` for the combined layout. Use
`pad_to_align` before treating a composed record as an array element.
`layout.of::<T>()` and `layout.array::<T>(count)` use the target's ABI.
Arithmetic failures return an error without wrapping or trapping.

`AllocError` distinguishes `InvalidAlignment`, `SizeOverflow`, `Exhausted`,
and `UnsupportedLayout`. There is no implicit process termination on allocation
failure and no global allocator fallback.

## Arenas, pools, and raw blocks

`arena.Arena.new(&mut bytes)` retains an exclusive borrow of caller-owned
storage. `allocate(&layout)` returns `Block!AllocError`; `allocate_zeroed`
also zeroes every requested byte. Alignment is calculated from the actual
backing address, so an unaligned subslice works. Failed requests leave the arena
unchanged. `capacity`, `used`, and `remaining` report byte counts including
consumed alignment padding.

Arena deallocation consumes the descriptor but reclaims no individual space.
`reset` reclaims everything. Both operations are unsafe: destroy stored values
first and stop using all pointers affected by the operation.

`pool.Pool.new(&mut bytes, block_size, block_align)` builds a fixed-block free
list in the unused storage. Blocks are padded to hold an aligned `usize` link;
`block_size()` and `block_align()` report the effective size/alignment.
Allocation and deallocation take constant time. `capacity()` and `available()`
count blocks, not bytes. Oversized or over-aligned requests return
`UnsupportedLayout`; a depleted free list returns `Exhausted`. Deallocation
reuses a block, and unsafe `reset` invalidates every outstanding allocation.

Zero-sized allocations consume no arena bytes or pool blocks. Their pointers
are nonnull and aligned but may dangle; they must not be accessed as nonzero
storage. Raw `Block` descriptors do not keep an allocator alive and do not free
or destroy anything when dropped. Their pointer access is unsafe. Deallocation
must use the exact originating allocator, with a live descriptor, at most once.
`Block.from_raw_parts` is unsafe and requires a valid, exclusive raw allocation
matching its layout.

## Owning a value

`arena_box.new` and `pool_box.new` allocate and initialize one value. Their
adapter packages depend on the generic `alloc/boxed` package; using arena boxes
does not import the pool implementation.
The resulting `Box<T, A>` holds an exclusive checked borrow of its allocator,
keeping the allocator and its backing storage live until the box is destroyed
or consumed. This initial API permits one live box per borrowed allocator.

```dodo test
package ownership

import "alloc/arena"
import "alloc/arena_box"

fn main() -> i32 {
    storage := [0u8; 64]
    allocator := arena.Arena.new(&mut storage)
    match arena_box.new(&mut allocator, 42i32) {
        ok(value) => { return value.into_inner() - 42 },
        err(_) => { return 1 },
    }
}
```

Save as `ownership.dodo` and run `dodo run ownership.dodo`; success exits with
status zero and prints nothing. Dropping a box destroys its value exactly once,
then deallocates the block. `into_inner` moves the value out and still releases
the allocation. `replace(value)` installs a new value and returns the old one
without destroying it. Allocation failure destroys the supplied value exactly once.
Arena space is only reclaimed on reset; pool blocks are immediately reusable.

`as_ptr` and `as_mut_ptr` expose raw access. `as_ref` / `as_mut` and
`get` / `get_mut` are equivalent checked `&T` / `&mut T` accessors, retained
for compatibility. A live view prevents conflicting access, replacement,
extraction, moving or destroying the box, and releasing its allocation.
`T` cannot contain checked borrows or
Results, including through nested aggregates; a box cannot hide an error that
the program must handle.
The generic `boxed.new::<T, A>` entry point is unsafe: a custom allocator must
honor the documented allocation validity, exclusivity, lifetime, and
deallocation contracts. Concrete arena/pool constructors establish those
contracts for the caller. There are no traits, vtables, or implicit allocation.

## Sharing an explicit allocator

`shared_arena.SharedArena.new(&mut bytes)` reserves an aligned `usize` cursor
inside caller memory. Construction fails with `Exhausted` if that metadata does
not fit. `capacity`, `used`, and `remaining` exclude this reserved prefix; used
space includes allocation alignment padding. The arena never exposes the
metadata as a checked reference. Its shared operations access it through private
raw pointers, without casting any shared reference to an exclusive reference.
There are no callbacks during allocation and no concurrent access guarantee.

`handle()` produces an owned capability that retains a shared loan of the arena
and all its backing dependencies. Different handles can supply allocations to
unrelated containers or boxes concurrently in one thread. Mutating a container
borrows its own storage exclusively and retains the allocator's shared dependency.
It does not upgrade that dependency to an exclusive allocator loan.

`Handle.allocate(&layout)` uses O(1) bump allocation; failure leaves its cursor
unchanged. Zero-sized allocation consumes no additional space. Unsafe
`Handle.deallocate(block)` consumes the unique descriptor without reclaiming
space. Unsafe `SharedArena.reset(&mut self)` reclaims all allocations and requires
that every initialized value be destroyed and every raw pointer invalidated.
The exclusive receiver statically prevents reset while checked handles remain
live. `Handle.clone` is a checked reborrow of that handle; obtaining separate
handles directly from the arena avoids tying their lifetimes to each other.

`shared_box.new(arena.handle(), value)` is a safe constructor with checked
`get`, `get_mut`, moving `replace`/`into_inner`, and deterministic destruction.
Each `std/collections/shared_*` adapter uses the same capability. The original
arena/pool Box interfaces retain their exclusive borrowing behavior.
