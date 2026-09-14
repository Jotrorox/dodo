---
title: "Container element safety design"
description: "Separate reference dependencies from Result obligations, with a bounded checker milestone and executable blockers."
section: "Project"
order: 310
---

Status: design and conservative checker hardening. This change enables **no new
container element types**. The first candidate, `fixed_vector.Vector<&i32>`, is
blocked on mutation effects in the ownership checker. Result elements need an
additional, independent obligation model. Proposed rules below are not language
features or new syntax.

The delivered milestone rejects assignment of Result-containing values through
fields, indices, and references. Previously these writes could discard an old
pending Result or publish a new one without recording its handling obligation.
Whole owned bindings retain their existing assignment and handling rules.

## Existing rules and implementation evidence

The starting points are [collections](collections.md),
[ownership](ownership.md), [memory and FFI](memory-and-ffi.md), and
[Result handling](patterns-and-results.md). In the source tree:

| Implementation | Relevant behavior |
| --- | --- |
| `src/sema.rs`: `Context::carries_borrow`, `contains_result` | Recursively inspect owned arrays, Options, structs and enum alternatives. References keep sources alive but do not own their referent's Result obligation. Raw pointers and `MaybeUninit` do not expose their payload's facts. |
| `Variable`, `Value`, `Place` and `merge_states` in `src/sema/ownership.rs` | Variables have dependency loans and one `pending_result` bit. Values carry loans, not per-element handling state. A place has `direct: Option<usize>` for a whole local binding. Joins union loans and OR pending bits. |
| `Checker::call` in `src/sema.rs` | `from(...)` contributes argument dependencies to a borrowed return. It does not describe changes to an argument's stored dependencies or obligations. |
| Assignment checking in `src/sema.rs` | Whole bindings replace their dependency set and reset their pending bit. Borrow-containing writes through references are rejected. Direct owned field updates conservatively retain old reference dependencies. This milestone rejects non-whole-binding Result writes as well. |
| `mark_matched_result`, `bind_pattern` | Matching can discharge a named aggregate's pending bit; nested Result pattern bindings acquire obligations. This is not an indexed storage ledger or an interprocedural handling effect. |
| `core/option` and `mem.replace` / `mem.swap` | `take` and `replace` lower to memory exchange. Exchange rejects both categories recursively. Returning an old reference value alone would not account for the newly stored reference. |
| `fixed_vector`, `fixed_deque` | Borrow `&mut[Option<T>]`; construction clears preexisting slots; insertion assigns through the borrowed slots; removal exchanges an Option; clear/drop destroy the occupied payloads. |
| `fixed_map`, `fixed_set` | Wrap the fixed vector. Duplicate keys, removal, replacement, and destruction add payload destruction paths. |
| `vector`, `shared_vector` | Store an allocator capability plus raw pointer, length and capacity. Construction uses `mem.init(Option<T>)` as a recursive restriction. Insertion uses `mem.init` and raw writes; removal reads raw storage; growth copies ownership to a new allocation. |
| Allocated deque, ordered maps/sets and heap; hash maps/sets | Build on vector storage or raw bucket storage. Shifting, ring relocation, cluster reinsertion, and heap swaps all transfer ownership. Hash map construction also checks a nested entry type. |

`ptr.borrow*` retains the owner's complete dependencies but cannot invent the
dependencies of an element stored behind its raw pointer. Both element
categories remain rejected there. Adding a phantom type parameter or changing
`from(self)` does not establish missing facts. Unsafe casts are not a solution.

Fixed-container rejection can first appear as an ambiguous borrowed return in
`option.replace`, an unsupported memory exchange, or an unhandled insertion
argument on a failure path. It is a generic specialization failure, not a
dedicated fixed-vector constructor diagnostic. The lower-level tests isolate
each underlying blocker instead of depending on which specialization fails first.

## Reference dependencies: proposed invariant

Keep three facts distinct: the storage loan protecting a container and its
slots, the allocator/policy dependencies that keep that storage usable, and the
referent dependencies carried by stored elements. Moving an element must move
its referent dependencies without manufacturing a fresh exclusive capability.
A checked view needs both the container storage loan and those dependencies;
a value removed by ownership needs its referent dependencies, independently of
the container storage it has left.

For the first candidate use a conservative union of referent dependencies for
the complete backing slot region, rather than facts for individual indices.
Every insertion adds its sources to this union. Moving the vector or a nested
aggregate retains that union. Branches and loops union possible sources.
Removal/replacement can return the complete union, which may retain unrelated
sources longer than necessary. Internal shifting never changes it.

Do not subtract dependencies merely because `length` became zero: the current
checker does not prove slot occupancy or runtime index equality. The initial
candidate keeps the union until the backing storage capability ends or the
whole backing binding is safely reinitialized. Existing caller-backed storage
may retain conservative dependencies even after the vector is destroyed.
Ending the vector's storage borrow and releasing all element source loans are
different operations.

This needs a checked write effect across calls. Conceptually `push(self, value)`
deposits `deps(value)` into the slot-region owner on success. A conservative
first implementation may also retain them on failure. The function checker
must verify that effect, and the caller must apply it to the actual place,
including wrapper methods, forwarded parameters, and reborrows. The new sources
must outlive the region's retained uses, including destruction. An ordinary
return-source contract cannot express this side effect.

For external parameter storage, there may be no local owner variable to update.
The checker must either verify and export a mutation summary or reject the
write. Updating only locally known roots is unsound. Updating only the temporary
`&mut self` binding also loses the facts when that temporary ends.

| Operation | Proposed reference behavior |
| --- | --- |
| Construction | Retain the backing storage loan and its existing dependencies; clearing old payloads must respect their destructors. An empty slot array does not confer a lifetime for future inputs. |
| Insertion | Add incoming referent dependencies to the backing-region summary; check their lifetime at the caller. Reject insertion from storage that will expire too soon. Failed insertion may conservatively retain the input loan. |
| Removal / pop | Transfer the element and a conservative referent summary to the result. Do not return a borrow of the vacated slot. Removing `&mut T` eventually requires exclusive capability transfer, not copying the union. |
| Replacement | Return the old element's referent summary and deposit the replacement's. Replacing in place must not erase sources still used by other slots or prior returned elements. |
| Shifting / swapping / reallocation | Preserve exactly one owner of each element and the same dependency summary. Moving bytes changes element addresses, not the lifetime of referenced objects. No live view may survive the operation. |
| Clear / drop | Destroy occupied elements exactly once while their sources remain live. Conservatively retain the region summary; do not infer that a source is dead from an unchecked length update. |
| Nested aggregates | Traverse structs, Options, arrays, enums and both Result alternatives; aggregates with both categories must satisfy both systems. Initially reject nested categories outside the exact candidate specialization. |
| Allocator lifetime | Retain allocator and backing bytes independently of element sources. Shared allocator dependencies remain shared when the vector is mutably borrowed. Allocator reset/destruction is forbidden while any container or view depends on it. |

External mutation includes `get_mut`, mutable slice views, taking `&mut` of a
slot, and a user helper accepting `&mut Container`. A writable element view could
install a short-lived reference without calling `push`. Such stores need the
same checked effect on the original owner. In the first candidate, explicitly
reject `get_mut` for reference-bearing elements. Internal authorized slot
operations need checked effects, not a public unrestricted `&mut &i32` escape.
Mutating plain referent data is a different permission from replacing the stored
reference itself.

## Reference examples and the smallest candidate

Accepted today: visible owned aggregates transfer dependencies without hidden
storage. This example also shows the by-value API shape already supported by
the checker; it is not a replacement container API.

```dodo test
package example
struct Holder { value: Option<&i32> }
fn identity(value: Holder) -> Holder from(value) { return value }
fn main() -> i32 {
    source := 42i32
    holder := identity(Holder { value: some(&source) })
    let Holder { value } = holder
    match value {
        some(value) => { return *value - 42 },
        none => { return 1 },
    }
}
```

Rejected today, including when the caller happens to provide a long-lived
source; this is the minimal insertion blocker:

```dodo
fn put(slots: &mut[Option<&i32>], value: &i32) {
    slots[0] = some(value)
}
```

Even `*slot = none` through `&mut Option<&i32>` and
`option.take(slot)` are rejected. Removing the memory-exchange guard would
still leave insertion, effects through wrappers, and independent removal
provenance unimplemented. An explicit `from(slot, value)` only addresses the
return value; it does not make `put` safe.

Proposed first candidate: only `fixed_vector.Vector<&i32>`, using shared scalar
references and a conservative backing-region union. Construction, push/insert,
pop/remove/swap_remove, replace, shared get, clear and drop must all respect
that invariant; mutable element views stay rejected. No allocated vector,
mutable reference elements, arbitrary reference-bearing structs, or Result
elements would be enabled by this candidate. A static string exception is also
out of scope: `&str` does not mean its value is necessarily static.

Under that proposal, the following would be accepted once the checked effects
exist. It is **rejected by the current compiler**:

```dodo
source := 42i32
slots: [1]Option<&i32> = [none]
values := fixed_vector.Vector.new(&mut slots)
match values.push(&source) { ok() => {}, err(_) => {} }
removed := values.pop()
core.drop(values)
match removed { some(value) => { observed := *value }, none => {} }
```

The same sequence with insertion inside `{ short := 42; ...push(&short)... }`
and use of the vector or removed value outside that block must be rejected.
So must `source = 0` while either stored or removed references remain live.
Inserting a reference to another element of the same vector must fail because
it conflicts with the required exclusive storage loan. A failed push does not
justify releasing sources if the initial summary cannot distinguish outcomes.

Decision: this candidate is **blocked**, not partially enabled. Needed checker
work is verified argument mutation effects, caller-root writeback through
external references, and separation of removed referent dependencies from a
container storage loan. None is supplied by removing a type restriction.

## Result obligations: a separate proposed invariant

A Result's pending handling obligation is not a reference dependency and is
not its destructor. Both `ok` and `err` require explicit handling. Moving a
pending Result transfers responsibility; destroying its payload does not handle
it. Matching an old value cannot pre-handle a replacement with identical type.

A future container must associate obligations with the owned active payloads,
including Results nested in fields or variants. A conceptual obligation ledger
assigns new obligations to new Results, moves them with values, and discharges
them only through checked matching/propagation/forwarding. This is a semantic
model, not a proposed heap allocation or runtime flag for every element.

For dynamically indexed storage, a conservative summary may say “some element
may still be pending.” Reading or matching one element must never clear that
summary for all elements. A draining proof or verified handling effect covering
every active element is required to clear it. Branch joins preserve every
possible obligation; a loop body handling one pop does not by itself prove the
container is drained on all exits. Breaks, early returns and failure paths count.

| Operation | Proposed Result behavior |
| --- | --- |
| Construction over caller slots | Existing destructive clearing cannot accept arbitrary pending payloads. Require checked empty slots or transfer every preexisting obligation back to the caller. |
| Successful insertion | Move the input obligation into the logical storage owner. The Result reporting insertion success is separate from the stored element's obligation. |
| Failed insertion | Existing methods destroy the incoming value: incompatible with pending Result elements. A future API must return the input, for example in `PushFailure<T> { reason, value }`, with recursive mandatory handling. Handling just the capacity/allocator error is insufficient. |
| Removal / pop | Move the removed element's obligations to the returned `Option<T>`. Ignoring it, using `_`, or `core.drop` must fail when it contains Results. Returning `none` carries no active payload but may remain conservatively obligation-bearing until matched. |
| Replacement | Transfer the old value's obligations to the return value and the new value's into storage. Invalid indices must return the incoming Result rather than destroy it. A plain assignment requires the old value already handled. |
| Internal moves / growth | Transfer obligations with ownership; neither copying bytes nor deallocating the old block handles any Result. Allocation failure preserves existing obligations and returns the incoming one. |
| Clear / implicit drop | Reject while anything may be pending. Clear cannot silently “handle” each element with an internal wildcard. A future explicit drain/handler API must have a verified complete handling effect, including early exits. |
| Nested aggregates / containers | Transfer every active nested obligation. Handling an outer insertion Result or one outer `Option` does not discharge pending inner Results. Maps must also account for destroyed duplicate keys and stored keys removed without returning them. |
| External mutation | Writing through a view must check the old obligation and register the new one with the original owner. Even replacing `Option<Result<...>>` with `none` may erase a pending error. |

The current `contains_result` deliberately stops at references: borrowing a
Result is not transferring ownership of it. Changing that traversal would charge
every allocator/policy/view reference with its referent's obligations and still
would not provide slot identities or mutation effects. Similarly, the current
`pending_result` bit is insufficient for partially handled, dynamically sized
collections. Merely adding a bit to the vector would let a match of one element
hide errors in the rest.

An already matched Result keeps its Result type. There is no `Handled<T>` type
or persistent proof that can be put into opaque storage. `core.drop` and memory
exchange retain their existing type-based prohibitions, even after a borrowed
match. Such proof transport would be another language design, not this milestone.

## Result examples and delivered milestone

Accepted today, including native destruction of the old active payload when
the whole binding is assigned again:

```dodo test
package example
fn main() -> i32 {
    value: i32!u8 = ok(1)
    match &value { ok(_) => {}, err(_) => {} }
    value = err(2)
    match value {
        ok(_) => { return 1 },
        err(code) => { return code as i32 - 2 },
    }
}
```

Omitting the second match is rejected. This applies to whole owned aggregates
containing Results as well. Matching by reference leaves payload destruction
for replacement or normal cleanup; it does not move the payload.

Previously accepted incorrectly; now rejected at the assignment:

```dodo
fn store(out: &mut Result<i32, u8>) { *out = err(2) }
fn main() {
    value: i32!u8 = ok(1)
    match &value { ok(_) => {}, err(_) => {} }
    store(&mut value)
} // The new error used to disappear without being matched.
```

The same gap existed for `alias := &mut value; *alias = err(2)`,
`holder.result = err(2)`, and indexed writes. In `StmtKind::Assign`, only
`place.direct` updated the pending bit. A referenced parameter owns no Result
bit, and a nested field had no independent bit to update. Later cleanup checked
the unchanged, already-cleared owner bit. Overwriting an unhandled nested Result
could likewise evade the whole-binding overwrite check.

The delivered guard rejects any assignment whose target type recursively
contains a Result and whose place is not a whole owned binding. It applies to
local fields/indices, local aliases, and external references, including replacing
an entire referenced aggregate. It also conservatively rejects cases where
subsequent code would handle the new Result. The diagnostic explains that whole
owned replacement is the available route. Assigning a plain success payload
through a matched `&mut Result` remains allowed; it creates no new Result.

This is one bounded safety milestone, with no code-generation changes, container
API changes, unchecked representation casts, or relaxation of existing guards.
It closes the reproduced assignment gap; it is not a claim that all future
obligation effects or all raw-memory safety concerns are solved.

## Executable acceptance and blocker tests

Run from the repository root:

```sh
cargo test --locked --test stdlib_safety --test container_elements --test collections_library --test owned_storage
target/debug/dodo test docs --doc
```

`tests/stdlib_safety.rs` covers opaque storage and exchange restrictions plus
the new field/index/alias/external Result assignment rejection. Whole-binding
reassignment tests preserve pending obligations across branches, loops, drop,
discard and overwriting. `tests/container_elements.rs` isolates rejected
reference insertion/extraction helpers, recursively rejected fixed/allocated
vector specializations, escaped sources, source mutation after moves, custom
destructor liveness, and exclusive reference aliasing. It also tests views
against insertion, removal, replacement, growth, clear/drop, allocator reset,
allocator destruction and backing-storage mutation.

`tests/stdlib/container_element_baseline.dodo` runs at `-O0` and `-O3`. Accepted
visible aggregates exercise checked references, read-only collection search,
nested enum/struct/Option moves, whole binding replacement, success/error payload
destruction, borrowed matching, and cleanup on propagation and scope exit.
Weighted counters verify exactly one destruction of each active payload.
The counters use raw pointers solely for instrumentation; stored reference and
Result values use checked storage throughout.

Existing collection native fixtures at both optimization levels exercise the
supported element category: fixed capacity failure, allocated growth/failure,
replacement, removal, shifting, clear/drop, move-only values and zero-sized
destruction. These provide the storage-behavior baseline; they do **not** claim
that reference or Result container programs now compile. Rejected programs are
checked only, never executed after a lifetime or handling error.

## What remains unsupported

- All fixed and allocated moving containers with reference-bearing or
  Result-containing elements, recursively, including maps' keys and values.
- The proposed fixed vector of shared `&i32` elements, until verified mutation
  effects and ownership-return dependencies exist.
- Mutable reference elements, mutable reference-bearing views, arbitrary nested
  reference aggregates, and precise release of sources after individual removals.
- Result storage/drain/replace/clear/drop effects, failure APIs returning pending
  inputs, partial handling of dynamic collections, and transported handled proofs.
- Reference-bearing or Result-containing memory exchange, opaque initialization,
  and owner-bound raw checked views. Existing unsafe raw-memory contracts remain
  in force; this design grants no new raw-storage permission.
- Assigning a Result-containing field, index, or referent, even when a programmer
  can demonstrate that a particular update would be safe. Use a whole owned
  binding and explicit handling while the checker lacks these effects.
