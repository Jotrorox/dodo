---
title: "Patterns and Results"
description: "Match coverage, guards, destructuring, and mandatory Result handling in Dodo 0.1.1."
section: "Language reference"
order: 225
---

Dodo uses patterns to match tagged values, unpack aggregates, and handle errors.
This page describes the implemented rules. Read [ownership and borrowing](ownership.md)
for the copy, move, and cleanup rules these operations preserve.

## Handle every Result

A `Result` must be forwarded, propagated, or matched with explicit `ok` and `err`
arms. Binding it and leaving scope, overwriting it unhandled, assigning it to
`_`, or passing it to `core.drop` is rejected.

## Match patterns and guards

Patterns compose recursively across enum payloads and structs. They include
bindings, wildcards, Boolean/integer literals, literal integer ranges (`..` and
`..=`), alternatives (`|`), and optional Boolean match guards. Struct patterns
list every field or use `..` for the remainder. Alternatives must introduce the
same bindings with compatible types and borrow modes. Pattern tests and guards
run in source order, retrying alternatives after a false guard. Ownership
transfers and destruction occur only after the guard succeeds. The current
checker conservatively rejects moves of non-copy values, assignment, and mutable
borrows anywhere in a guard, including operations on unrelated local storage.
Shared borrows and observer calls are permitted. Guarded arms do not count toward
coverage. Existing match-arm bindings remain mutable locals; names introduced by
`let` and `if let` are [immutable bindings](implementation-syntax.md#mutable-and-immutable-bindings)
that retain the permissions of any references they hold.
Exhaustiveness checks preserve correlations between nested fields and partition
integer ranges by their endpoints. Each pattern may expand to at most 4,096
alternatives; coverage checking is limited to 131,072 work units. Excessive
patterns produce a diagnostic.

## Conditional and destructuring bindings

`if let` scopes bindings to the success block. Destructuring `let` introduces
immutable bindings in the surrounding block; refutable patterns require an
`else` that diverges. Both forms consume owned scrutinees on success or failure
and borrow reference scrutinees. Conditional patterns must cover every state
whose active payload contains a Result, including borrowed values. For example,
`some(result)` may match `Option<Result<T, E>>` if the bound result is handled;
its unmatched `none` path has no obligation. Success-only `ok` patterns and
ignored nested Results are rejected. Borrowed patterns preserve shared/mutable
permissions recursively, including separate loans for disjoint struct fields;
owned patterns cannot destructure structs with custom `drop`.

See [patterns.dodo](https://github.com/Jotrorox/dodo/blob/main/examples/patterns.dodo) for conditional bindings and
[hex.dodo](https://github.com/Jotrorox/dodo/blob/main/examples/hex.dodo) for range matching with Result propagation.
