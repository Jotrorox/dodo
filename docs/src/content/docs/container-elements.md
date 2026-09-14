---
title: "Container element safety"
description: "Checked storage provenance, mutation effects, supported shared elements, and mandatory Result restrictions."
section: "Project"
order: 310
---

Allocated vectors, deques, hash maps/sets, ordered maps/sets, and heaps support
shared-reference elements and owned strings with shared allocator dependencies.
The acceptance scope below was established before implementation. Caller-backed
moving containers retain their previous restrictions. Result elements remain
unsupported; a capacity/allocation Result is always mandatory to handle.

## Supported combinations

| Element category | Allocated collections, including `shared_*` | Caller-backed `fixed_*` |
| --- | --- | --- |
| Plain scalars, raw pointers, move-only owned aggregates, zero-sized values | Supported | Supported |
| `&i32`, `&str`, shared slices | Supported | Rejected |
| Structs, enums, Options and arrays containing shared references | Supported recursively | Rejected |
| `text_shared.String`, including nested owned aggregates | Supported; retains its allocator | Rejected |
| Exclusive references/slices, or aggregates owning them | Rejected recursively | Rejected |
| `Result`, including nested Results, Result-bearing references and already matched Results | Rejected recursively | Rejected |

Both keys and values follow these rules. References to Result-bearing sources also remain rejected in typed storage:
matching one element cannot discharge obligations for its conservative source
union. Ordinary checked Result borrowing outside these collections is unchanged. Read-only slice algorithms retain ordinary borrowing rules.
Mutating slice algorithms and unrestricted mutable element views require plain,
borrow-free, Result-free payloads. `get_mut` and `as_mut_slice` therefore remain
available for plain allocated elements, but cannot expose stored references.
Heap swaps and ordered-map replacement use ownership movement internally.
To edit an owned String element, remove it, edit the owned value, and reinsert it;
mutable String element views are not supported in this scope.

## Allocation and text

Use one `alloc/shared_arena.SharedArena` over caller-owned bytes, and pass a fresh
`arena.handle()` to each `shared_*` container and `std/text_shared` constructor.
Declare sources before containers that retain them. Empty text construction is
`text_shared.new(handle, byte_limit)?`; copying valid UTF-8 is
`text_shared.from_str(handle, value, byte_limit)?` or `from_text(handle, &text,
byte_limit)?`. No intermediate byte buffer is needed. Each String owns its
buffer and shared handle. The byte limit bounds logical length; geometric
capacity can be larger. Allocation failure remains explicit.

The existing exclusive arena path has direct safe `text_alloc.empty`,
`text_alloc.from_str`, and `text_alloc.from_text` constructors. It exclusively
borrows its arena and is useful for one owner at a time. Its String is not a
supported reference-bearing element because it retains an exclusive dependency.
The buffer-taking `text_alloc.String.new` remains available for UTF-8 validation.

## Checked region invariant

Three facts stay separate: the storage loan protecting a container, ordinary
allocator/policy dependencies, and the union of sources retained by its stored
elements. `Loan.stored` in `src/sema/ownership.rs` marks the third category;
`dependency` alone cannot distinguish it from an ordinary reborrow. The checker
uses symbolic `owner` and `owner.stored` sources when checking borrowed parameters.

Allocated storage has a private `[0]T` witness field. It carries T's type facts
without owning an active element or creating a runtime lifetime token. The
witness alone grants no raw-storage access. Constructors call
`mem.storage_type::<T>()`, which recursively rejects Results (including Result-bearing referents) and exclusive
reference elements. Stored dependency summaries never discharge source Result obligations.
Existing `mem.init`, raw pointer access, memory exchange,
and `ptr.borrow*` restrictions remain in force.

Library implementations use new **unsafe** owner-bound primitives:

| Primitive | Checked behavior |
| --- | --- |
| `ptr.store(pointer, value, &mut owner)` | Requires a matching typed witness; consumes the value and deposits all its dependencies into the owner. |
| `ptr.take(pointer, &mut owner)` | Transfers ownership and returns the complete stored-source union without a borrow of the vacated storage. |
| `ptr.relocate(source, destination, count, &mut owner)` | Preserves the region union while moving potentially overlapping storage. |
| `ptr.view(pointer, &owner)`, `ptr.view_slice(pointer, count, &owner)` | Returns a checked shared view retaining both storage and source dependencies. No mutable counterpart exists for reference elements. |

The unsafe caller must prove pointer/owner correspondence, alignment, initialized
extents, destination validity, and exactly-once ownership transfer. Relocation
moves within one owner's allocations; old moved bytes cannot be used or destroyed
again. `take` leaves uninitialized storage; `store` requires an uninitialized or
otherwise already disposed destination. A zero-length witness is not evidence
that arbitrary raw bytes contain live references. The safe library maintains
occupancy and allocation invariants around these operations.

## Mutation and return contracts

`stores(target, source...)` declares a conservative dependency deposition into
an exclusive reference to a typed storage owner. There is one target per
function. Every external source deposited by its body, including forwarded
calls, must be listed. Local sources cannot escape through this effect. Sources
already in `target.stored`, and static sources, need no new input effect.
Borrow-free generic arguments contribute no dependencies.

The call checker updates the actual owner and existing aliases, including field
owners and reborrows. Updating only an `&mut self` temporary would lose facts.
The body checker also records deposits into external parameter storage: a helper
cannot omit its effect or return a newly inserted source under an old-source-only
contract. Effects apply conservatively on failure as well as success.

`from(owner.stored)` returns only retained element dependencies. The checker
verifies this contract against the body. Returning `get`/`as_slice` under it is
rejected because those views also depend on `owner` storage. A helper that inserts
an input and returns it must name that input in its return contract as well.
Ordinary `from(owner)` continues to retain the complete owner borrow.

`requires_plain(T, ...)` limits a specialization to borrow-free, Result-free
types. Unavailable bodies are not emitted, and calls produce a diagnostic.
This keeps mutable access methods usable for plain elements without making an
unchecked accessor for reference-bearing elements. Destructors cannot carry
this constraint.

| Operation | Conservative source behavior |
| --- | --- |
| Insert, duplicate-key replacement, invalid-index replacement, failed allocation | Union incoming dependencies into the complete owner. Existing destruction/failure behavior remains exactly once. |
| Shared access or `next(&mut cursor)` | Keep the storage borrow and complete dependencies. Mutation, relocation, clear and destruction are forbidden while the view is live. |
| Pop/remove/replace returning ownership | Return the previous stored union; the removed value can outlive the container but cannot outlive its sources. |
| Move or aggregate movement | Transfer dependencies with ownership. |
| Shift, ring growth, heap swaps, hash cluster reinsertion | Preserve dependencies and exactly one owner of each payload. |
| Clear | Destroy active payloads; retain capacity and the conservative source union. |
| Whole-owner replacement/destruction | Release its retained union after destruction; separately removed values still retain theirs. |
| Branches and loops | Union possible sources, including break/continue effects. Mutations conflicting across loop back edges are rejected. |

No per-index release is inferred from lengths, occupancy, successful removal,
or clear. Sources can be retained longer than necessary, including after failed
insertion. For loops, the checker conservatively rejects a source mutation that
could conflict with any retained deposit across an iteration, even if a runtime
condition would prevent another iteration. Caller-backed regions do not gain
these effects. Direct reference-bearing writes through borrowed fields/indices
and memory exchange remain restricted.

## Positive acceptance example

This program forwards a checked insertion effect and uses a removed reference
after destroying the vector. It requires the pending compiler support in
[PR #24](https://github.com/Jotrorox/dodo/pull/24); until that lands, it is an
illustration of the proposed syntax rather than an executable doctest.

```dodo
package example
import "alloc/error"
import "alloc/shared_arena"
import "std/collections/vector"
import "std/collections/shared_vector"
fn put(out: &mut vector.Vector<&i32, shared_arena.Handle>, value: &i32) -> void!error.AllocError stores(out, value) {
    return out.push(value)
}
fn run() -> i32!error.AllocError {
    bytes := [0u8; 4096]
    arena := shared_arena.SharedArena.new(&mut bytes)?
    source := 42i32
    values := shared_vector.new::<&i32>(arena.handle())
    put(&mut values, &source)?
    removed := values.pop()
    core.drop(values)
    match removed {
        some(value) => { return ok(*value - 42) },
        none => { return ok(1) },
    }
}
fn main() -> i32 {
    match run() { ok(code) => { return code }, err(_) => { return 2 } }
}
```

See `examples/string_map.dodo` for complete text construction, string-key map
insertion, replacement, lookup, iteration and removal with one allocator.
`hash.Str`, `hash.Text` and `text_shared.Key` compare UTF-8 bytes and hash
contents. `hash.SipStr` accepts explicit caller-provided keys for keyed hashing.

## Negative acceptance and Results

The following operations remain errors in the example above:

```dodo
values.push(&source)?
source = 0                 // Stored dependency is still live.
{ short := 1i32
  values.push(&short)? }   // Source cannot escape this scope.
view := values.as_slice()
values.clear()
core.drop(view)            // A live view forbids the preceding clear.
values.push(&source)       // The insertion Result must be handled.
```

Every Result alternative requires handling. Moving a Result is not handling it,
and destroying its payload does not discharge its obligation. The current
failure, duplicate-key, replacement, clear and drop APIs can destroy incoming
or stored payloads, so they cannot accept owned Results. The existing whole-binding
pending bit is not an indexed obligation ledger. No Result storage, drain proof,
or handled-Result wrapper is introduced. Result-containing assignments through
fields, indices and references remain rejected; whole-binding replacement still
requires handling the previous value and creates a fresh obligation.

`tests/container_elements.rs` contains positive O0/O3 execution and negative
programs for source mutation/destruction, lifetime escape, omitted/misdeclared
effects, aliases, moves, stale views, loop exits and discarded Results.
`reference_collections.dodo` covers reference aggregates, rings, maps/sets,
heaps, relocation and destructor counters. `shared_text_collections.dodo` covers
safe constructors, shared allocator strings, limits, UTF-8 boundaries, allocation
failure and owned string maps. Existing collection fixtures continue to test
move-only values, zero-sized destruction, collisions and allocation failures.

Run the focused checks with:

```sh
cargo test --locked --lib --test container_elements --test collections_library --test owned_storage --test stdlib_safety --test std_text --test hash_library
target/debug/dodo test docs --doc
```
