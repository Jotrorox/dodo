---
title: "Mutable slice splitting"
description: "Checked disjoint mutable views with source lifetimes and ordinary move semantics."
section: "Language reference"
order: 235
---

Use `slice.split_at_mut` when you need to update two separate regions of one
array or slice at the same time. Ordinary index or slice syntax does not prove
that two mutable views are disjoint, even when their indices look different.
The splitter provides that proof through its return type.

Read [ownership and borrowing](ownership.md#fields-indices-and-disjoint-views)
first if mutable references are new to you. No unsafe code is needed to use this
API. Import `core/slice`:

```text
pub fn split_at_mut<T>(data: &mut[T], mid: usize)
    -> Option<slice.SplitMut<T>> from(data)
```

`mid <= data.len` returns `some` with the ranges `[0, mid)` and
`[mid, data.len)`. A larger midpoint returns `none`, like the existing checked
`subslice_mut` helper. The midpoint is a `usize`. Both boundary splits succeed;
splitting an empty slice at zero produces two empty slices. The operation takes
constant time, allocates nothing, and does not move or destroy any elements.

Use ordinary owned struct patterns to obtain the two mutable views:

```dodo test
package main

import "core/slice"

fn update(data: &mut[i32]) {
    let some(parts) = slice.split_at_mut(data, 2) else { return }
    let slice.SplitMut{left, right} = parts
    if left.len > 0 && right.len > 0 {
        a := &mut left[0]
        b := &mut right[0]
        *a += 1
        *b += 2
    }
    // Both views have reached their last use.
    data[0] += 3
}

fn main() {
    data := [10i32, 20, 30, 40]
    update(&mut data)
    core.assert_eq(data[0], 14i32)
    core.assert_eq(data[2], 32i32)
}
```

The first `let` unwraps the successful Option or returns early. The second
destructures and consumes the pair, giving `left` and `right` separate mutable
views. After their last uses, the original `data` becomes available again.
An immutable `let` binding can still modify elements through its stored mutable
slice; it cannot replace the slice binding itself.

The combined pattern `let some(slice.SplitMut{left, right}) = ... else { ... }`
also works. Fields may be renamed or discarded in the pattern. No tuple syntax,
traits, or lifetime parameters are needed.

## Handle every boundary

For an input of length `n`:

| Midpoint | Left length | Right length | Result |
| --- | --- | --- | --- |
| `0` | `0` | `n` | `some(parts)` |
| `n` | `n` | `0` | `some(parts)` |
| Between `0` and `n` | `mid` | `n - mid` | `some(parts)` |
| Greater than `n` | — | — | `none` |

A successful split does not imply both halves are nonempty. Check `.len` before
indexing, as the example does. If you need a checked shared or exclusive single
subrange instead of two disjoint views, use `slice.subslice` or
`slice.subslice_mut`; see [core utilities](core.md).

## Ownership and reborrowing

`SplitMut<T>` has public `left` and `right` fields of type `&mut[T]`. It is a
compiler-recognized struct: construct it with `split_at_mut`, and consume it to
get independent views. Struct literals and replacement of an individual slice
field are rejected. Whole-pair moves and replacement are allowed. This protects
the invariant that its two slices always cover disjoint parts of one input.
Element mutation through either field is permitted.

The pair and its mutable views move. Destructuring consumes the pair once;
borrowing `&parts` or `&mut parts` retains the existing conservative aggregate
rules. Dropping a pair or view only releases its borrow; the original owner
still owns and eventually destroys each element exactly once. A custom
destructor that holds a view keeps that view live until destruction.

Each half can be passed as a reborrow, indexed, subsliced, or split again while
the other half remains usable. Conflicting accesses through a parent slice or
the original owner stay suspended until the derived views' last uses. Moving,
destroying, or reallocating the owner while a view remains live is rejected.
Nested views similarly suspend their parent half. Zero-length views retain
source lifetimes under the same conservative rules.

Functions may return the pair, an `Option` containing it, or a derived view with
the ordinary `from(data)` contract. Moves, wrappers, and control-flow joins keep
the complete source dependencies. A contract cannot make local storage outlive
its scope. A returned pair can be consumed by its caller to recover independent
views; an ordinary user-defined aggregate of borrowed fields still follows the
ordinary aggregate rules.

## Why the views are disjoint

The details in this section are useful when implementing low-level containers
or understanding a compiler rejection. They are not additional steps a caller
must perform.

The library uses the safe, narrowly typed intrinsic
`mem.split_at_mut::<slice.SplitMut<T>>(data, mid)`. It checks `mid <= len` itself
and traps on failure, like direct slice syntax; the public helper checks first
and returns `none`. The intrinsic emits two existing slice descriptors:
`(pointer, mid)` and `(pointer + mid elements, len - mid)`. Bounds are checked
before pointer arithmetic. It accepts a checked exclusive slice, not raw
pointers. There is no additional runtime ownership object or new AST syntax.

Consuming a pair introduces a fresh partition identity into the checker's
storage loans. The two sides of that identity are disjoint; a parent loan,
which lacks the identity, still overlaps both. Nested splits retain their
ancestor identities. Copies of loan facts through reborrows, moves, returns,
and joins retain the identities. Joins retain every possible dependency rather
than choosing one branch's identity.

The checker never partitions allocator, policy, or stored-reference lifetime
dependencies. Their original exclusivity and source lifetimes remain intact.
Merely constructing two owner-bound raw-pointer views supplies no partition
identity and does not grant independent access.

Zero-sized elements need no distinct byte addresses: the two ranges contain
different logical element indices and cannot access any common element bytes.
The backend uses the same non-inbounds element offset operation as ordinary
slicing and adds no pointer inequality or `noalias` assumptions. Neither slice
owns elements, including zero-sized elements with destructors.

## Conservative limits

- Indices and independently written subslices of the same collection still
  overlap. Two borrows of the same split half also overlap.
- Only consuming a `SplitMut` (optionally inside `some`) introduces partition
  identities. Borrowed patterns and arbitrary nested aggregate patterns do not
  gain independent borrowed-field lifetimes. Bind an inner pair and consume it
  separately when needed.
- Shared element and allocator dependencies remain shared. Exclusive element
  dependencies remain exclusive and may still prevent simultaneous use of both
  halves; splitting does not add independent lifetimes for those dependencies.
- A split opened inside a loop may be used within that iteration but cannot be
  stored into an outer binding across iterations. This prevents a static split
  identity from proving disjointness between different dynamic executions.
  Returning a view from the function remains subject to normal return checks.
- Joins, argument evaluation, custom destruction, partial moves, and mutation
  of borrowed fields keep their existing conservative restrictions.
