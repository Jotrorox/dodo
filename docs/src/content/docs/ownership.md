---
title: "Ownership and borrowing"
description: "Copy and move rules, borrow lifetimes, borrowed returns, and cleanup in Dodo 0.1.2."
section: "Language reference"
order: 220
---

These are the ownership rules and checker limits implemented in Dodo 0.1.2.
For pattern matching and mandatory error handling, continue to
[patterns and Results](patterns-and-results.md).

## Copying and moving

Scalars, shared references/slices, strings, and raw pointers copy. Structs,
arrays, enums, `Result`, `Option`, and mutable references/slices move. Mutable
reference arguments are reborrowed as appropriate. Non-copy field/index partial
moves are rejected; borrow or transfer the whole aggregate instead.

## Borrows and returned references

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
borrowing. Opaque container elements containing references or Results remain
explicitly unsupported. Bind a returned mutable view to a local before assigning
through its fields; chained assignment through a call can be rejected by the
conservative temporary-loan checker.

## Diagnosing ownership errors

Compiler errors label the conflicting operation, the original borrow or move,
and the use keeping it live. Borrowed-return errors also identify the contract
and the returned source. See [ownership diagnostics](diagnostics-and-editors.md)
for examples and [editor setup](editors.md) for diagnostics and hovers while you
write code.

## Destruction and cleanup

Owned local values are destroyed in reverse declaration order on normal exits.
Custom `drop` runs before fields are destroyed, and fields/array elements are
destroyed in reverse order. A live flag prevents repeated destruction after
moves along different control-flow paths. Temporary owned places are retained
in the enclosing block until cleanup. Traps abort and do not run cleanup.
