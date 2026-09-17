# Control-flow analysis and incremental adoption

The former test-only prototype now lives in `src/sema/flow`. Production semantic
analysis runs it as an additional initialization/move check for supported bodies.
The AST checker still runs for every body and retains responsibility for types,
loans, captured-destination reservations, Result obligations, and destruction
constraints. Its diagnostics take precedence; recovery does not report the same
failure twice. Unsupported lowering discards the entire graph and leaves the
body with the existing checker. The module is exposed for differential testing,
not as a stable public IR API.

Graphs are built before body checking mutates the AST, using the selected 32- or
64-bit target width. Declaration-subset validation is shared across functions.
Only the first graph diagnostic per body is retained during
production checking, so graphs and fixed-point states are released as each body
is analyzed. `tests/flow_prototype.rs` imports this production implementation and
also lowers original parses independently, including programs rejected by sema.

## Representation and analysis

Each parameter, binding, and expression temporary has a stable local identity.
Shadowed names remain distinct. Blocks end in branches, jumps, returns, or
unreachable terminators. Calls materialize arguments left to right, so an earlier
argument's move is visible while evaluating later arguments.

The worklist reaches a fixed point before reporting uses or cleanup facts.
Definite initialization intersects reachable predecessor states; possible
initialization unions them. Moves and cleanup kill both facts, and assignments
restore both. Move provenance survives joins and is cleared by reinitialization.
An unreachable block has no incoming state, rather than an empty set of facts.
Diagnostics retain declaration/use spans and a possible originating move.

`&&` and `||` branch around their right operand and assign a result on both paths.
Nested expressions, value-block assignments, and loop back edges use the same
joins. Literal conditions deliberately keep both successors, matching the
conservative AST checker and ensuring skipped syntax is still checked.

Whole-binding assignment does not read the destination; it evaluates the RHS,
conditionally cleans up the previous slot, and writes the new value. For fields
and checked-reference dereferences, an explicit capture reads the initialized
root before the RHS. Compound assignment additionally loads the old value before
the RHS. A projected store never initializes the whole root and never introduces
partial-move facts. Capture operations describe abstract addresses, not loans;
the AST checker's reservation rules still prevent their invalidation by the RHS.

Cleanup is explicit at full-expression boundaries, scope exits, overwrite,
discard, return, break, and continue. Exiting scopes clean up in reverse order.
Return and value-block results survive cleanup until transferred. Cleanup facts
classify each reachable site as `Never`, `Always`, or `Conditional` from the
converged must/may state. These are slot-liveness facts, not a native destructor
plan, and the LLVM backend does not consume them yet.

## Current subset and next adoption steps

The adapter supports Boolean/integer scalars, structs with scalar fields, direct
references, struct literals, whole-local moves, direct calls, scalar operators
(except unary negation), field/dereference reads and assignments, compound
assignments, plain value blocks with a final value, blocks, `if`, basic `for`,
return, break, and continue. Reference arguments requiring implicit mutable
reborrowing are still excluded. Projected moves are rejected explicitly.

Program declarations currently exclude imports, globals, enums, generics,
methods/custom destructors, FFI, borrowed returns, and borrow contracts. Arrays,
slices, indexing, casts, propagation, patterns, for-init/step, unsafe blocks,
diverging value blocks, and nested yield exits are outside the body subset.
Lowering is not a complete type checker or proof of safety. Tests keep examples
where initialization succeeds but borrowing correctly fails in production.

This is the first production adoption stage: graph findings can reject a body
after the existing checks succeed, while existing accepted/rejected behavior is
covered by differential regressions. Replacing AST initialization or loop-move
checks requires extending per-body coverage and preserving diagnostic/recovery
behavior. Adopting graph cleanup in codegen separately requires destructor,
borrow-containing aggregate, and native drop-order coverage. Neither replacement
is implied by an empty initialization issue list.

Validation commands:

```sh
cargo test --locked --no-default-features --test flow_prototype
cargo test --locked --no-default-features --all-targets
cargo clippy --locked --no-default-features --all-targets -- -D warnings
```
