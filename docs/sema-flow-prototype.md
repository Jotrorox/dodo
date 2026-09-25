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
Only the first graph diagnostic per body is retained during normal
production checking, so graphs and fixed-point states are released as each body
is analyzed. Developer reports retain all initialization/move findings, but still
release graphs and fixed-point states immediately.

## Coverage reporting and independent comparisons

The hidden developer API `sema::flow::check_with_report(program, pointer_bits)`
runs combined production recovery checking and collects coverage at the actual
integration point in `check_program()`: after preparation, generic instantiation,
contract inference, and global-initializer checking, before AST body checking.
It does not independently lower the original parse. Each prepared function name
and span has one of these statuses:

- `Analyzed`: lowering and initialization analysis completed without findings.
- `Diagnostics`: analysis completed with initialization/move findings, including
  use/declaration spans and move provenance. These may be suppressed in the
  user-visible result by AST diagnostic precedence.
- `Skipped`: a structured `LimitationKind`, span, and readable reason, scoped to
  either a **program-wide** adapter restriction or **body-local** lowering failure.
- `NoBody` or `Unavailable`: not eligible for body analysis. Unavailable functions
  are recorded before normal checking removes them from the program.

Coverage is `None` if shared preparation/declaration checks stop before the
integration point; this is not successful empty analysis. Unused generic templates
may disappear before measurement, while instantiated bodies appear under their
prepared names. Import spans are currently unavailable in `Program`, so import
limitations use the default span; other declaration restrictions retain their
source spans. No fallback reasons become normal compiler warnings.

`CoverageReport::skip_counts()` groups skipped bodies by scope and category.
These are **first-blocker frequencies**, not an exhaustive inventory: validation
and lowering stop at the first restriction, and a program restriction counts once
for each eligible body it excludes. Per-body messages provide further detail.

Run the bounded representative corpus (real examples, diagnostic fixtures, and
loaded bundled imports) with per-body statuses and a ranked fallback summary:

```sh
cargo test --locked --no-default-features --test flow_prototype representative_corpus_coverage -- --nocapture
```

`sema::flow::compare_checkers(program, pointer_bits)` is a hidden test/developer
helper that always runs three configurations on fresh clones:

| Configuration | Purpose |
| --- | --- |
| AST only | Existing body checker without adapter construction or flow findings |
| Flow only | Initialization/move findings and explicit subset skips, without AST body checking |
| Combined | Production recovery checking, including AST diagnostic precedence |

All configurations share preparation and declaration checks. A flow-only empty
diagnostic list is **not acceptance**: inspect coverage, and remember that flow
does not check borrowing or full type safety. AST-only coverage is intentionally
`None`. The configuration selector is private; there is no compiler flag to
disable safety checks, and the comparison helper returns reports, not unchecked
programs for compilation.

`tests/flow_prototype.rs` checks these configurations against independently
specified expected acceptance/rejection and initialization findings; the AST
checker is not the oracle. It additionally checks normal fail-fast behavior and
lowers original parses for graph-shape assertions. `tests/flow_prototype/reporting.rs`
locks down prepared/instantiated coverage, whole-program restrictions, complete
graph discard on late lowering failure, continued AST checking, and diagnostic
precedence without duplicates.

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
