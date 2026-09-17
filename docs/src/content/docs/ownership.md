---
title: "Ownership and borrowing"
description: "Learn copying, ownership transfer, shared and mutable borrows, return contracts, and deterministic cleanup."
section: "Learn Dodo"
order: 55
---

Dodo tracks who owns each value and which parts of it are borrowed. The owner is
responsible for eventual cleanup. A borrow grants temporary access without
transferring that responsibility. The compiler checks these relationships before
the program runs; ordinary checked references do not use a garbage collector or
runtime reference counting.

Start here after [functions and structs](types-and-functions.md). The examples
below use small values so that the same rules are easier to recognize later in
files, strings, vectors, and other resources.

## Copying and moving

| Category | Types | Assignment or by-value argument |
| --- | --- | --- |
| Copy | Numbers, `bool`, `&T`, `&[T]`, `&str`, raw pointers | Copies the value; the source remains usable. |
| Move | Structs, arrays, enums, `Result`, `Option`, `&mut T`, `&mut [T]` | Transfers ownership or exclusive access; the source cannot be read afterward. |

A struct moves even when every field is copyable. A shared reference copies the
reference, without copying its referent. `&str` is a string view and copies;
an owned library `String` is a struct and moves.

```dodo test
package main

struct Parcel { weight: u32 }

fn deliver(parcel: Parcel) -> u32 {
    parcel.weight
}

fn main() {
    count := 2u32
    copied := count
    core.assert_eq(count + copied, 4u32)

    parcel := Parcel { weight: 12 }
    transferred := parcel
    core.assert_eq(deliver(transferred), 12u32)
    // parcel and transferred have both moved.
}
```

Attempting `deliver(parcel)` after the transfer is a compile error. A mutable
binding can become usable again by assigning it a new value. Copying an owned
aggregate requires an explicit function, such as a library `clone` method; Dodo
does not automatically deep-copy it.

Ordinary field or index access cannot partially move a non-copy value out of an
aggregate. Borrow the field, move the whole aggregate, or use an owned
[destructuring pattern](patterns-and-results.md#conditional-and-destructuring-bindings)
when supported. Array indexing never removes an element; collection APIs such
as `pop` and `remove` explicitly transfer ownership.

## Shared and exclusive access

`&value` creates a shared borrow. Several shared borrows can coexist. While they
remain live, the owner cannot change the borrowed storage, move, or be destroyed.
`&mut value` creates an exclusive borrow: other accesses to overlapping storage
are suspended until that borrow is no longer needed.

```dodo test
package main

fn observe(value: &i32) -> i32 {
    *value
}

fn increment(value: &mut i32) {
    *value += 1
}

fn main() {
    number := 40i32
    view := &number
    core.assert_eq(observe(view), 40i32)
    // view's last use is above, so its loan can end here.
    increment(&mut number)
    increment(&mut number)
    core.assert_eq(number, 42i32)
}
```

`*value` reads or writes the referent. Field access and indexing automatically
dereference checked references, so a `&Point` can use `point.x` without `(*point).x`.
Raw pointers do not get this automatic access.

The relevant endpoint is usually the borrow's **last use**, not the closing brace
of its binding. If the example used `view` after `increment`, the earlier shared
borrow would still be needed, and the mutable call would be rejected.

## Reborrowing and immutable bindings

A function can temporarily borrow through an existing mutable reference:

```dodo test
package main

fn increment(value: &mut i32) {
    *value += 1
}

fn twice(value: &mut i32) {
    increment(value)
    increment(value)
}

fn main() {
    number := 40i32
    let reference = &mut number
    twice(reference)
    core.assert_eq(*reference, 42i32)
}
```

These calls reborrow access rather than permanently consuming the caller's
reference. The original reference is unavailable while a conflicting derived
borrow is live and becomes usable afterward.

`let reference` prevents replacing the reference binding. It does not weaken the
`&mut i32` it holds. By contrast, `let number = 40i32` owns immutable storage and
cannot be mutably borrowed. Shared access to a struct containing an `&mut` field
does not grant exclusive access through that field: it produces a shared reborrow.

## Fields, indices, and disjoint views

Separate fields of a directly owned struct can be borrowed independently:

```dodo test
package main

struct Pair { left: i32, right: i32 }

fn main() {
    pair := Pair { left: 1, right: 2 }
    left := &mut pair.left
    right := &mut pair.right
    *left += 10
    *right += 20
    core.assert_eq(pair.left + pair.right, 33i32)
}
```

The checker conservatively treats all indices of the same collection as
overlapping. Two different index expressions or slice ranges are not proof that
their mutable borrows are disjoint. Use
[`slice.split_at_mut`](slice-splitting.md) to obtain two checked independent
mutable slices, then borrow their elements.

## Borrows and returned references

When a function returns a reference, a slice, a string view, or an aggregate
containing these, the compiler must know which input keeps it alive:

| Signature | Source contract |
| --- | --- |
| A borrowed receiver is present | The receiver is the inferred source. |
| No borrowed receiver and exactly one borrow-carrying parameter | That parameter is the inferred source. |
| Multiple possible input sources | Write `from(a, b, ...)`. |
| No input source; program-lifetime storage | Write `from(static)`. |

```dodo test
package main

fn choose(left: &i32, right: &i32, use_left: bool) -> &i32 from(left, right) {
    if use_left { left } else { right }
}

fn greeting() -> &str from(static) {
    "Hello"
}

fn main() {
    left := 20i32
    right := 42i32
    selected := choose(&left, &right, false)
    core.assert_eq(*selected, 42i32)
    core.assert_eq(greeting().len, 5usize)
}
```

`from(left, right)` conservatively retains **both** input dependencies, even if
the runtime choice selects only one. The body must satisfy the contract. It is
never legal to return a reference to a local value that will be destroyed on
return, and `from(static)` cannot make such a value live longer.

Declare source storage before an owner that retains its references. Destruction
runs in reverse declaration order, so this gives the owner a chance to finish
using the sources before they are destroyed.

## Advanced dependency tracking

The checker follows source-place dependencies through references and aggregates,
permits separate field loans, and conservatively overlaps all indices into the
same collection. Loans can end at last use. Loops, control-flow joins, and custom
destruction extend liveness conservatively, so some valid programs may be
rejected. Borrowed returns are checked against inferred or explicit `from(...)`
sources; contracts do not extend the lifetime of local storage. Self-referential
owning values and independently varying borrowed-field lifetimes are unsupported.
Destructuring a borrowed aggregate that contains references conservatively keeps
all of its source dependencies; purely owned nested struct fields retain separate
field loans.

[`slice.split_at_mut`](slice-splitting.md) is the narrow exception for collections:
consuming its `SplitMut` result gives two disjoint mutable slices while retaining
the original storage's lifetime and dependencies. Ordinary indexing and slicing
remain conservative; other aggregates do not gain independent field lifetimes.

The checker distinguishes the storage a view accesses from dependencies its
owner keeps alive. Borrowing a container exclusively retains shared allocator or
policy dependencies as shared; it never upgrades them into exclusive access.
Source lifetimes remain checked, including through moves, control-flow joins,
destruction, and borrowed returns. Shared access through an aggregate cannot
extract a mutable reference or slice from one of its stored `&mut` fields:
reading such a field creates a shared reborrow. A directly owned immutable
binding that contains an exclusive reference can still write through that stored
reference, as described in [syntax](implementation-syntax.md).

For generic specializations, explicit `from(...)` parameters that become
borrow-free contribute an empty dependency set. Parameters that contain borrows
retain every actual source dependency. Unknown source names are errors; a
contract never permits a borrow of a local value to escape.

[Owner-bound raw-storage primitives](memory-and-ffi.md#unsafe-memory-access)
allow library implementations to establish checked views of owned allocations.
The pointer/owner correspondence is unsafe; later use follows ordinary checked
borrowing. Allocated typed storage supports shared-reference elements through checked
mutation and removal contracts; owned Results and exclusive-reference elements
remain unsupported. See [container elements](container-elements.md). Bind a returned mutable view to a local before assigning
through its fields; chained assignment through a call can be rejected by the
conservative temporary-loan checker.

Storing a checked reference through a field, index, or dereference of borrowed
storage remains rejected: `from(...)` describes returned dependencies, not
updates to the caller's stored dependencies. Direct owned aggregate replacement
is supported. Allocated collection APIs use verified `stores(target, source...)` effects
and `from(owner.stored)` ownership-return contracts, described in
[container elements](container-elements.md). These effects retain a conservative
source union until whole-owner destruction or replacement, including after clear.

Result handling state belongs to whole owned bindings. Assignments of
Result-containing values through fields, indices, or references are rejected,
including nested aggregates and local aliases. Handle the old value and replace
the whole owned binding instead; the replacement has a new handling obligation.
Assigning plain payload data through a matched mutable Result borrow is still
allowed. These restrictions do not add support for Result container elements.

## Diagnosing ownership errors

Compiler errors label the conflicting operation, the original borrow or move,
and the use keeping it live. Borrowed-return errors also identify the contract
and the returned source. See [ownership diagnostics](diagnostics-and-editors.md)
for examples and [editor setup](editors.md) for diagnostics and hovers while you
write code.

When an error is unexpected, check these common causes:

| Symptom | What to check |
| --- | --- |
| Use after move | Did an assignment, argument, `self` method, or owned pattern take the value? Borrow it when the callee only needs access. |
| Mutation conflicts with a borrow | Find the borrow's last use, including a destructor that still needs it. Finish that use before mutating. |
| A returned view has invalid lifetime | Return a view of caller-owned storage, or return an owned value. A contract cannot extend a local's lifetime. |
| Two indices conflict | Use checked slice splitting; different indices alone do not establish disjointness. |
| A stored source remains borrowed after `clear` | Allocated containers retain a conservative source union until whole-owner replacement or destruction. |

The checker is deliberately conservative around loops, branch joins, aggregate
dependencies, and custom destruction. A rejection does not necessarily mean a
runtime alias exists; it means this implementation cannot establish the required
guarantee for that program.

## Destruction and cleanup

Owned local values are destroyed in reverse declaration order on normal exits,
including `return`, `?`, `break`, and `continue`. Replacing an initialized owned
value destroys the old value. A move transfers the cleanup responsibility;
moved-from storage is not destroyed twice.

A struct can run additional cleanup before its fields are destroyed:

```dodo test
package main

struct Guard {
    counter: &mut i32

    fn drop(&mut self) {
        *self.counter += 1
    }
}

fn main() {
    count := 0i32
    {
        guard := Guard { counter: &mut count }
        core.drop(guard)
    }
    core.assert_eq(count, 1i32)
}
```

`core.drop(guard)` consumes and destroys the value early. Without that call, the
block exit would destroy it. Custom `drop` cannot be called directly, take
additional arguments, or return an error. A custom destructor keeps the
references it may use live until destruction. Owned destructuring of a struct
with custom `drop` is rejected.

After custom cleanup, struct fields and active enum payload fields are destroyed
in reverse declaration order. Array elements are destroyed from last to first.
Temporary owned places are retained in their enclosing block until cleanup.
Traps and panic abort without unwinding or running destructors; required recovery
belongs in a [Result](patterns-and-results.md), not in a panic path.
