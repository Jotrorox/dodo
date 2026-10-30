# Ownership checking on the control-flow graph

Semantic analysis runs in two passes over every function body:

1. The **AST checker** (`src/sema.rs`) resolves names and types, checks
   visibility, unsafe operations, patterns and exhaustiveness, and the static
   restrictions that depend only on types or syntax (for example, moving a
   field out of an aggregate, or assigning a Result through a reference). It
   also desugars the body for later passes: method calls become calls, range
   loops become counted loops, coercions get explicit casts, and closures are
   lifted into functions with environment parameters.
2. The **flow checker** (`src/sema/flow`) lowers each successfully checked
   body, including lifted closures and generic specializations, to a graph of
   basic blocks and checks ownership: initialization, moves, loans and their
   conflicts, escapes, destruction order, and Result obligations.

There is no fallback. Every construct the backend compiles has a lowering, so
every body is analyzed. A body whose AST check failed is not lowered; its
errors are reported instead. Normal checking stops at the first error; editor
(recovering) checking reports the first ownership error of each body.

## Lowering

`flow/lower.rs` lowers the typed AST in the backend's evaluation order:
operands left to right, an assignment's destination before its right-hand
side, a call's arguments before the call. Every expression result is a
single-use temporary. Places are a root (a binding, temporary, or global) and
projections: dereferences, fields, and indices, with implicit dereferences
recorded so loan paths match the checker's rules. Values never prune paths:
both successors of every branch, pattern test, and loop header stay possible,
including zero iterations of a loop with literal bounds.

Scopes are explicit. Leaving a scope, by fallthrough, `break`, `continue`, a
value-block exit, `return`, or `?`, checks the scope and then cleans up its
bindings in reverse declaration order. A `continue` inside a `for` step repeats
the step, as in the backend.

## Analysis

`flow/analysis.rs` runs a forward analysis to a fixed point, then a single
checking pass that uses backward liveness.

Each local has a state: definitely and possibly initialized, one possible move
origin, the loans it holds, and whether it has an unhandled Result. Joins
intersect definite initialization and union everything else.

A **loan** names the storage it borrows (a local, a parameter's caller-owned
storage, or static storage) with a field path where `[]` is any element, and
records whether it is exclusive, where it began, and which bindings it was
read through. References, aggregates, closures, and call results carry the
loans of the values they were built from. A call's result carries the loans of
the arguments its `from(...)` contract names, and `stores(...)` deposits loans
into an owner's storage. Opening a `SplitMut` pair gives each side a disjoint
partition.

An access **conflicts** with an incompatible, overlapping loan held by a local
that is used later. A local whose type has a destructor is used when it is
destroyed. Accessing through a reference does not conflict with the reference
itself. A conflict with a temporary is reported as overlapping borrows within
the same expression; a conflict with a binding names it and labels its next
use: on a later loop iteration, at its destruction, or on the next iteration
of a loop that still holds its collection.

The checking pass also reports:

- uses of uninitialized or moved bindings, including moves repeated by a loop;
- returned loans outside the return contract, and returned local borrows;
- bindings that still borrow storage of a scope or value block being left;
- mutable `foreach` element borrows and split views that outlive their loop;
- destructors that would run after storage they borrow is destroyed;
- Results that are never handled, left unhandled on an exit, or overwritten;
- moves, mutation, and mutable borrows in match guards; and
- stores that would hide a borrow in caller or closure-captured storage.

Long field paths are truncated to a conservative prefix, so loops that project
repeatedly still reach a fixed point. Findings are reported in evaluation
order.

## Testing

- `tests/flow_ownership.rs` and `tests/flow_ownership/loans.rs` specify
  outcomes independently for initialization, moves, patterns, loops, loans,
  escapes, destructors, Result obligations, closures, and coverage.
- `tests/compiler_safety_generated.rs` compares every finding with an
  independent path-execution oracle on generated programs at both target
  widths; the same generator drives the libFuzzer target in `fuzz/`.
- The `sema::tests` unit tests and the library and integration suites pin down
  diagnostics and labels.

The hidden developer API `sema::flow::check_with_report(program,
pointer_bits)` runs recovering checking and returns every analyzed body's
findings.

```sh
cargo test --locked --no-default-features --test flow_ownership
cargo test --locked --no-default-features --test compiler_safety_generated
cargo test --locked --no-default-features --all-targets
```
