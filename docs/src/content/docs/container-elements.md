---
title: "Container element safety"
description: "Checked storage provenance, mutation effects, supported shared elements, and mandatory Result restrictions."
section: "Language reference"
order: 237
---

This chapter answers three practical questions: which values can go in a
collection, how long their referenced sources stay borrowed, and how stored
strings can be edited safely. Start with the [collections guide](collections.md)
for ordinary construction, insertion, access, and removal.

An allocated collection owns its elements, but an element may itself refer to
something else. For example, a vector of `&i32` owns its reference values while
the original integers remain elsewhere. Dodo tracks those sources so the
collection cannot leave a reference dangling.

Allocated vectors, deques, hash maps/sets, ordered maps/sets, and heaps support
shared-reference elements and owned strings with shared allocator dependencies.
Caller-backed moving containers have stricter element restrictions. Result
elements remain unsupported; a Result returned by insertion or allocation must
still be handled.

## A vector of borrowed values

```dodo test
package main

import "alloc/shared_arena"
import "std/collections/shared_vector"

fn main() {
    bytes := [0u8; 2048]
    arena := shared_arena.SharedArena.new(&mut bytes)!
    source := 42i32
    values := shared_vector.new::<&i32>(arena.handle())
    values.push(&source)!

    removed := values.pop()
    core.drop(values)
    match removed {
        some(reference) => { core.assert_eq(*reference, 42i32) },
        none => { core.assert(false) },
    }
}
```

The backing bytes outlive the arena; the arena and `source` outlive the vector.
`pop` transfers ownership of the stored reference. That reference can outlive
the vector, but it still cannot outlive `source`. The postfix `!` handles
allocation failure by panicking; use `?` or `match` when the caller should recover.

Three kinds of dependency are relevant:

| Dependency | Example | What must remain valid |
| --- | --- | --- |
| Storage borrow | `values.get(0)` or `values.as_slice()` | The collection's element storage; mutation and reallocation must wait. |
| Allocator or policy dependency | A shared arena handle retained by the vector. | The allocator or policy's sources. |
| Stored-element source | `&source` inserted into the vector. | The original referent, even though the reference is stored elsewhere. |

The compiler keeps a conservative union of all stored-element sources. Removing
one element or calling `clear` does not release a particular source from that
union. Whole-owner replacement or destruction releases the container's union;
previously removed values still retain their own source dependencies. This can
keep a source borrowed longer than its last actual element requires.

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
Vectors and hash-map values also support scoped `try_update` callbacks for owned
String edits. Mutable String element views remain unavailable.

## Scoped owned-element mutation

Use `vector.try_update(index, &mut mutation)` or
`hash_map.try_update(&key, &mut mutation)`. The mutation object supplies a public
`apply(&mut self, value: &mut T) -> void!E` method (with `V` for map values).
The error type `E` must be borrow-free and Result-free. Annotate the call's
`bool!E` result to supply the error type for generic inference:

```dodo test
package example
import "alloc/error"
import "alloc/shared_arena"
import "std/collections/shared_vector"
import "std/text_shared"

pub struct Append {
    suffix: &str
    pub fn apply(&mut self, value: &mut text_shared.String) -> void!error.AllocError {
        return value.append_str(self.suffix)
    }
}
fn run() -> void!error.AllocError {
    bytes := [0u8; 4096]
    arena := shared_arena.SharedArena.new(&mut bytes)?
    values := shared_vector.new::<text_shared.String>(arena.handle())
    values.push(text_shared.from_str(arena.handle(), "Dodo", 64)?)?
    append := Append { suffix: " bird" }
    updated: bool!error.AllocError = values.try_update(0, &mut append)
    core.assert(updated?)
    match values.get(0) {
        some(value) => { core.assert_eq(value.as_str(), "Dodo bird") },
        none => { core.assert(false) },
    }
    return ok()
}
fn main() -> i32 {
    match run() { ok() => { return 0 }, err(_) => { return 1 } }
}
```

`ok(true)` means the callback succeeded. A missing index or key returns
`ok(false)` without calling it. The Result must be handled even for a missing
element. The callback's error is propagated **after restoring ownership of the
element**. Changes made before an error remain; this operation does not roll
them back. Length, capacity, vector order, map keys and bucket placement stay
unchanged. The update itself allocates nothing; the callback may allocate.
Vector access takes O(1), and map access uses the ordinary key lookup.

This API uses checked `ptr.take`/`ptr.store` around a local owned value. The
callback receives a temporary exclusive borrow. It cannot retain that borrow,
return borrowed errors, reenter the collection through an alias, or introduce
new stored sources. Existing views prevent an update, and the complete stored
source union remains retained after success or failure. String methods such as
append, truncate and clear work because they preserve their allocator dependency.

This scope does not enable replacing stored references or reference-bearing
fields, replacing a stored String wholesale, storing Results,
or exposing unrestricted mutable String views. Other container kinds still use
their existing removal and insertion APIs. Broader mutation effects and indexed
Result handling need separate ownership and error-handling analysis.

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

The rest of this chapter specifies the library implementation contracts. You
do not need to write these primitives to use the safe collections above.

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

Functions taking a collection by value retain its stored sources under the
ordinary `from(value)` contract. This also applies when the collection is
projected from an aggregate or a checked slice parameter. Moving a collection
into a helper cannot erase the dependencies of an element removed there.

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
after destroying the vector:

```dodo test
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

The following are separate examples of rejected operations, assuming the
corresponding bindings from the earlier vector example are still in scope:

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
`collection_mutation.dodo` covers scoped String updates, missing entries,
callback errors, allocation/UTF-8 failures and exactly-once destruction.
`tests/collection_mutation.rs` checks callback escape and dependency restrictions
without requiring LLVM.

Run the focused checks with:

```sh
cargo test --locked --lib --test collection_mutation --test container_elements --test collections_library --test owned_storage --test stdlib_safety --test std_text --test hash_library
target/debug/dodo test docs --doc
```
