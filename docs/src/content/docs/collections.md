---
title: "Collections"
description: "Borrowed algorithms, caller-backed containers, and explicit fallible ownership."
section: "Using Dodo"
order: 141
---

`std/collections` supplies algorithms over checked borrowed slices. Its child
packages provide independently imported containers. Nothing starts a runtime,
requests entropy, or selects a global allocator.

| Import | Storage and ordering |
| --- | --- |
| `std/collections` | Allocation-free algorithms and `I32` / `U64` comparison policies. |
| `std/collections/fixed_vector` | Caller-backed contiguous logical sequence of `Option<T>` slots. |
| `std/collections/fixed_deque` | Caller-backed ring with constant-time operations at both ends. |
| `std/collections/fixed_map`, `fixed_set` | Bounded linear map/set, insertion order. |
| `std/collections/vector` | Explicitly allocated contiguous vector. |
| `std/collections/deque` | Explicitly allocated ring buffer. |
| `std/collections/hash_map`, `hash_set` | Open addressing, linear probing, unspecified bucket order. |
| `std/collections/ordered_map`, `ordered_set` | Sorted contiguous entries, ascending key order. |
| `std/collections/heap` | Maximum binary heap; heap order is not sorted order. |

Each owned container has an independent `std/collections/shared_*` adapter:
`shared_vector`, `shared_deque`, `shared_hash_map`, `shared_hash_set`,
`shared_ordered_map`, `shared_ordered_set`, and `shared_heap`. These safe
constructors accept an `alloc/shared_arena.Handle`. Several containers can use
handles from the same arena simultaneously; the checked ownership model keeps
the allocator and its backing storage alive without exclusively lending the
entire allocator to one container.

## Algorithms and customization

`find`, `equal`, and lexicographic `compare` take a policy value by checked shared
reference. `find` returns the first matching index. Length breaks equal-prefix
lexicographic ties. Comparison results are normalized to -1, 0, or 1.

A comparison policy implements `compare(&self, a: &T, b: &T) -> i32`; only its
sign matters. An equality policy implements `equal(&self, a: &T, b: &T) -> bool`.
A partition predicate implements `test(&self, value: &T) -> bool`. These are
ordinary generic methods resolved statically during compilation. There are no
traits, closures, reflection, vtables, or const generics involved. Comparisons
must define a consistent total order; equality must be an equivalence relation.

`sort` is unstable in-place heapsort: O(n log n) worst-case time and O(1)
auxiliary storage. `stable_sort` is stable insertion sort: O(n²) worst-case,
O(n) on already sorted input, O(1) storage. `is_sorted` is O(n).

`lower_bound` returns the first position not less than the key, `upper_bound`
the first greater position, and `binary_search` the first equal index or `none`.
All three take O(log n) comparisons and require input sorted under the same
policy. Empty input has insertion position zero. `partition` runs in O(n),
places true predicate values before false values, and returns their boundary;
relative order is unspecified. `swap` checks both indices and returns false on
out-of-bounds input; it never destroys either exchanged value.

## Capacity, ownership, and failure

Caller-backed constructors take `&mut[Option<T>]`, destroy any preexisting slot
payloads, and start empty. The backing array length fixes capacity. Construction,
insertion, removal, and destruction never allocate. Destruction empties all
occupied slots so the caller can reuse the backing array. Array repetition
requires copyable elements in Dodo; initialize move-only `Option` slots with
explicit `none` elements, as in the example below.

Fixed insertion returns mandatory `Result<_, collections.CapacityError>`;
`Full` and `OutOfBounds` distinguish capacity from index failure. Failed
insertion destroys the supplied value exactly once and preserves the existing
container. An invalid `replace` destroys its replacement and returns `none`.
`pop`, `remove`, and `swap_remove` return ownership of the removed value.
Vector `remove` and `insert` preserve order with O(n) shifting; `swap_remove`
replaces the hole with the final value in O(1). Vector push/pop and ring
push/pop at either end take O(1) without growth.

Owned constructors initially allocate nothing. Vector capacity grows
geometrically from four elements, doubling until sufficient; reserve validates
pointer-sized arithmetic and layouts. Ring storage doubles when full and moves
a wrapped prefix while preserving FIFO order. Growth uses a fresh allocation,
moves initialized payloads, and deallocates the old block without destroying
moved values. Failed allocation leaves the original values, order, length, and
capacity intact; failed insertion destroys its incoming value exactly once.
Clear retains capacity. There is no shrinking API yet. With the bump arena,
individual deallocation does not reclaim bytes, so growth consumes additional
backing space until the caller drops all handles and resets the arena.

Owned operations return mandatory `Result<_, alloc/error.AllocError>`.
`vector.insert` currently reports an invalid index as `UnsupportedLayout`;
allocation failures distinguish `Exhausted` from `SizeOverflow` and invalid
allocator layouts. No operation silently falls back to a process heap.

Generic owned `new` constructors are unsafe because arbitrary allocator types
cannot express allocation validity as a language trait. The owned allocator
capability provides `allocate(&self, requested: &Layout) -> Block!AllocError`
and `unsafe deallocate(&self, allocation: Block)`. Allocations must be unique,
aligned, valid until exactly one matching deallocation, and remain alive while
the capability lives. Shared arena adapters establish that contract in safe
code. Allocator operations are single-threaded; concurrency requires a separate
allocator adapter with its own synchronization contract.

Zero-sized elements have ordinary independent lengths, moves, and destructor
calls while requiring zero allocation bytes. No default construction or cloning
of elements is required. Move-only structs and recursively owned fields are
supported. Current opaque-storage ownership explicitly rejects element types
containing checked references or Results, including nested aggregates, at
construction. The compiler cannot yet transfer their provenance or mandatory
Result obligations into raw storage. The restriction also applies to fixed
containers' moving `Option` operations and mutating slice algorithms. Read-only
slice algorithms retain ordinary borrowing rules and can use reference-bearing
elements. Allocator and policy values themselves retain their checked source
dependencies.

## Maps, sets, and heaps

Map insertion preserves the first equal key object and replaces its value,
returning the previous value as `some(old)`; inserting a new key returns `none`.
The incoming duplicate key is destroyed. Removal destroys the stored key and
returns its value. Container destruction destroys every remaining key and
value. Sets similarly preserve the first equal element; insertion returns
whether the element was new, and removal returns whether it existed.

Bounded maps/sets use linear search and preserve insertion order through
replacement and removal. Ordered maps/sets use binary search in sorted storage:
O(log n) lookup, O(n) insertion/removal, and compact ascending iteration. They
are sorted-vector maps rather than trees. Equal ordered keys are those for
which the comparison policy returns zero.

Hash policies supply `hash(&self, key: &K) -> u64` plus `equal(&self, a: &K,
b: &K) -> bool`. Equal keys must have equal hashes. Tables start at eight
buckets, maintain at most 75% occupancy, and double on growth. Resizing
rehashes every entry. Removal reinserts the following probe cluster instead of
leaving lookup-breaking holes. Expected lookup/insertion time is O(1), growth
is expected O(n), and pathological collisions make lookup O(n), with O(n²)
cluster reinsertion possible on removal. Bucket order may change after any
mutation and is not a serialization format.

Hashing is independent of collections. `std/hash.I32` and `U64` use specified
field-wise encodings. For untrusted keys, supply caller-keyed `SipI32` or
`SipU64`; the caller must obtain unpredictable keys from a separate entropy
source. Fixed seeds in examples and tests are deterministic examples, not an
entropy source. Custom key hashing must encode fields explicitly; padding and
native struct layout are never hash inputs.

Binary heaps return the greatest element under their comparison policy.
Push/pop take O(log n) excluding allocation growth; peek is O(1). Equal elements
have unspecified removal order. The borrowed view exposes heap order.

## Views and iteration

Iteration is explicit indexing, avoiding a hidden allocation or invalidation
counter. Vectors expose `as_slice()` / `as_mut_slice()` and optional
`get()` / `get_mut()`. Fixed vectors and both rings use logical `get(index)`;
rings index from the front. Bounded maps use `entry(0..len())`. Ordered maps
expose `entries()` and ordered sets `get(0..len())`. Hash maps and sets use
`entry(0..bucket_count())`, skipping `none` buckets.

All views are checked borrows of the container and retain its complete storage
dependencies. A live shared view prevents mutation or destruction of that
container; a mutable view is exclusive. Releasing the view allows later
mutation. No safe iterator survives insertion, removal, reserve, clear, or
container destruction, even when an operation happens not to relocate storage.
Unrelated containers sharing the allocator can still mutate independently.
Keys are never exposed mutably by map/set APIs.

```dodo
package example
import "std/collections"
import "std/collections/fixed_vector"

fn main() -> i32 {
    slots: [3]Option<i32> = [none, none, none]
    values := fixed_vector.Vector.new(&mut slots)
    match values.push(42) {
        ok() => {},
        err(_) => { return 1 },
    }
    match values.pop() {
        some(value) => { return value - 42 },
        none => { return 2 },
    }
}
```

See `examples/collections.dodo` for safe sharing of an explicit allocator.
The collection fixtures run at `-O0` and `-O3`, exercise deterministic models,
collisions, wrapping, growth, zero-sized and move-only elements, destruction,
failed allocation, and compile-time invalidation rejection. The same portable
fixtures emit WebAssembly and Cortex-M0 objects; Windows x64 execution uses the
repository's Wine test runner.
