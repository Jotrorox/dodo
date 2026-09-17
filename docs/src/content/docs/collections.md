---
title: "Collections"
description: "Choose a list, queue, map, set, or heap; sort slices; and manage container capacity, iteration, and ownership."
section: "Standard library"
order: 147
---

Use `std/collections/fixed_vector.Vector.new` to store a small bounded list.
Its `Option<T>` slots are caller-owned storage; this portable path needs no
allocator. Import the container directly; the parent `std/collections` package
contains slice algorithms, not all child constructors.

A **sequence** stores values in a chosen order; a **map** associates keys with
values; a **set** stores unique values; a **heap** gives access to the greatest
value first. Choose the shape of your data first, then choose fixed storage or
an explicitly allocated container. The element type and borrowing rules are the
same language concepts used outside collections.

## Quickstart

Save this as `collection_start.dodo`:

```dodo test
package collection_start
import "std/collections/fixed_vector"

fn main() -> i32 {
    slots: [3]Option<i32> = [none, none, none]
    values := fixed_vector.Vector.new(&mut slots)
    match values.push(42) {
        ok() => {},
        err(_) => { return 1 },
    }
    match values.pop() {
        some(value) => { assert_eq(value, 42) },
        none => { return 2 },
    }
    return 0
}
```

```sh
dodo run collection_start.dodo
```

Expected output: none; exit 0 confirms the stored value was 42. Exit 1 means
the fixed vector is full: remove an item, increase slot capacity, or report that
the input exceeds your limit. Exit 2 means there was no item to pop.

For growth, the recommended next step is `alloc/shared_arena` plus
`std/collections/shared_vector.new::<T>(arena.handle())`; it permits several
owners in the same arena. Use `text_shared.new(arena.handle(), limit)?` or
`text_shared.from_str(arena.handle(), utf8, limit)?` for owned strings alongside
them; no intermediate byte buffer is required. See the [string-map example](https://github.com/Jotrorox/dodo/blob/main/examples/string_map.dodo). The generic owned-container constructors are unsafe
custom-allocator building blocks. Start with scalar or owned element values:
[current container restrictions](container-elements.md) still reject certain
reference-bearing and Result-bearing elements, even through nested aggregates.

## API and contracts

### Choose a container

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

Use a vector for an indexed list, a deque for a queue, a hash map for frequent
key lookup, an ordered map when traversal must be sorted, and a heap for a
priority queue. Fixed maps/sets are useful for small bounded tables because
their setup is simple and they need no allocator. Their lookups scan entries.

| Container family | Main operations |
| --- | --- |
| Vectors | `push`, `pop`, `get`, `insert`, `remove`, `swap_remove`, `replace`, `clear` |
| Deques | `push_front`, `push_back`, `pop_front`, `pop_back`, `get`, `clear` |
| Maps | `insert`, `get`, `contains`, `remove`, `clear` |
| Sets | `insert`, `contains`, `remove`, `clear` |
| Heap | `push`, `peek`, `pop`, `clear` |

`get` borrows an element, while `pop` and `remove` return its ownership. Each
container provides `len`, `capacity`, and cursor-based `next`; additional views
and mutation methods differ by container. The [API reference](stdlib-api.md)
lists those differences rather than implying every family has identical methods.

### Choose an allocator for growth

Each owned container has an independent `std/collections/shared_*` adapter:
`shared_vector`, `shared_deque`, `shared_hash_map`, `shared_hash_set`,
`shared_ordered_map`, `shared_ordered_set`, and `shared_heap`. These safe
constructors accept an `alloc/shared_arena.Handle`. Several containers can use
handles from the same arena simultaneously; the checked ownership model keeps
the allocator and its backing storage alive without exclusively lending the
entire allocator to one container.

Use this shared arena path when constructing several strings and containers.
Create one `shared_arena.SharedArena` over caller-owned bytes, then pass a fresh
`arena.handle()` to each owner. `std/text_shared.new(handle, byte_limit)?`
constructs empty owned text; `from_str(handle, utf8, byte_limit)?` and
`from_text(handle, &text, byte_limit)?` copy UTF-8 directly. Each owner retains
its allocator dependency, and allocation failures remain explicit. See the
[complete string-keyed map example](https://github.com/Jotrorox/dodo/blob/main/examples/string_map.dodo)
and [supported element combinations](container-elements.md).

### Editing stored owned values

Vectors and hash maps provide `try_update(index_or_key, &mut mutation)` for
scoped edits, including appending to a stored `text_shared.String`. A public
`apply(&mut self, value: &mut T) -> void!E` method performs the edit; hash maps
pass only the value. Annotate the call result as `bool!E`, where `E` is
borrow-free and Result-free. `ok(false)` means the element was absent;
`ok(true)` means the edit succeeded. Callback errors preserve ownership of the
element and any edits already made. Updates retain lengths, capacity, keys and
ordering, and allocate only if the callback does. See the
[complete example and checked restrictions](container-elements.md#scoped-owned-element-mutation).

## Algorithms and customization

### Sort before using binary search

The policy tells a generic algorithm how to compare your elements. This complete
program uses the supplied signed-32-bit policy; it sorts an ordinary array and
finds the first equal element. No collection or allocator is needed.

```dodo test
package sorted_lookup
import "std/collections"

fn main() -> i32 {
    values := [30i32, 10, 20, 20]
    policy := collections.I32 {}
    collections.sort(&mut values, &policy)
    assert_eq(values[0], 10)
    key := 20i32
    match collections.binary_search(&values, &key, &policy) {
        some(index) => { assert_eq(index, 1usize) },
        none => { return 1 },
    }
    assert_eq(collections.lower_bound(&values, &key, &policy), 1usize)
    assert_eq(collections.upper_bound(&values, &key, &policy), 3usize)
    return 0
}
```

Save as `sorted_lookup.dodo` and run `dodo run sorted_lookup.dodo`. Success
exits with zero. The half-open range `1..3` contains both values equal to 20.
Binary search is meaningful only while the sequence remains sorted according
to the same comparison policy.

### Policy methods and algorithm costs

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

### Fixed storage

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

### Growth and allocation errors

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

Owned allocation operations return mandatory `Result<_, alloc/error.AllocError>`.
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

### Element types and retained borrows

Zero-sized elements have ordinary independent lengths, moves and destructor
calls while requiring zero allocation bytes. No default construction or cloning
of elements is required. Owned vector/deque/map/set/heap storage supports shared
references (`&i32`, `&str`, shared slices), aggregates containing shared
references, and owned shared-arena strings. This uses explicit stored-dependency
tracking; it does not relax ordinary raw-pointer or `MaybeUninit` restrictions.

Result-containing elements (including references to Result-bearing sources) and
exclusive-reference elements remain rejected recursively.
Handling an insertion's allocation Result does not handle a Result stored as its
element. Fixed containers' moving `Option` operations and mutating slice
algorithms retain their more restrictive element rules; do not assume
`fixed_vector.Vector<&i32>` is accepted because an owned vector supports it.
Read-only slice algorithms follow ordinary borrowing rules.

Insertion and replacement retain source dependencies conservatively, including
failure paths. Moves, removal and `clear` do not erase the container's accumulated
source dependencies; whole-owner destruction/replacement releases them. A
removed reference still keeps its original source alive, even after the
container is destroyed. Views into container storage additionally borrow that
container and cannot survive mutation, growth or destruction. Mutable element
views require plain payloads without checked borrows; storing a shared reference
does not grant mutable access to its referent.

Helpers forwarding insertion must declare the stored-dependency effect, such as
`stores(out, value)`. A helper returning a removed reference uses
`from(out.stored)`. The [container element guide](container-elements.md) records
the implementation scope and remaining restrictions. Shared allocator and policy
values retain their checked dependencies; these owners are not automatically
thread-transferable.

## Maps, sets, and heaps

### Insert, replace, and look up a map value

Map insertion has two independent outcomes: allocation can fail (`err`), and a
successful insertion can either create a key (`none`) or replace its old value
(`some(old)`). `?` handles the outer `Result`; `match` handles the inner `Option`.

```dodo test
package map_values
import "alloc/error"
import "alloc/shared_arena"
import "std/collections/shared_hash_map"
import "std/hash"

fn example() -> void!error.AllocError {
    storage := [0u8; 2048]
    allocator := shared_arena.SharedArena.new(&mut storage)?
    counts := shared_hash_map.new::<i32, i32, hash.I32>(allocator.handle(), hash.I32 {})
    core.drop(counts.insert(7, 1)?)
    match counts.insert(7, 2)? {
        some(previous) => { assert_eq(previous, 1) },
        none => { assert(false, "the key was already present") },
    }
    key := 7i32
    match counts.get(&key) {
        some(value) => { assert_eq(*value, 2) },
        none => { assert(false, "the inserted key must exist") },
    }
    assert_eq(counts.len(), 1usize)
    return ok()
}

fn main() -> i32 {
    match example() {
        ok() => { return 0 },
        err(_) => { return 1 },
    }
}
```

Save as `map_values.dodo` and run `dodo run map_values.dodo`; success exits with
zero. `hash.I32` is a deterministic policy for trusted integer keys. For text
keys, see the complete [string-map example](https://github.com/Jotrorox/dodo/blob/main/examples/string_map.dodo)
and the policy choices below.

### Key identity and ordering

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
entropy source. `hash.Str`, `hash.Text`, and `text_shared.Key` compare/hash UTF-8
contents; `hash.SipStr` supplies caller-keyed borrowed text hashing. Custom key
hashing must encode fields explicitly; padding and native struct layout are
never hash inputs.

Binary heaps return the greatest element under their comparison policy.
Push/pop take O(log n) excluding allocation growth; peek is O(1). Equal elements
have unspecified removal order. The borrowed view exposes heap order.

## Views and iteration

Start iteration with `cursor := 0usize`, then call `container.next(&mut cursor)`
until it returns `none`. It returns checked views and skips empty hash buckets
and internal empty slots. Traversal uses sequence, sorted-key, heap, or hash
order as appropriate. The cursor allocates nothing; reset it to zero after
mutation or for a fresh traversal. Explicit indexing is also available: vectors
expose `as_slice()` and optional `get()`, with `as_mut_slice()` and `get_mut()`
available for plain elements. Fixed vectors and both rings use logical `get(index)`;
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

See `examples/collections.dodo` for safe sharing of an explicit allocator.
The collection fixtures run at `-O0` and `-O3`, exercise deterministic models,
collisions, wrapping, growth, zero-sized and move-only elements, destruction,
failed allocation, and compile-time invalidation rejection. The same portable
fixtures emit WebAssembly and Cortex-M0 objects; Windows x64 execution uses the
repository's Wine test runner.
