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
64-bit target width. Each body validates its own signature and the declarations
it uses: direct callee signatures and referenced struct definitions. Callee bodies
are analyzed independently, so unsupported syntax in one does not exclude its
callers. Unrelated declarations and imports do not block supported bodies.
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
  either a **program-wide** target restriction or **body-local** lowering failure
  (including restrictions on declarations used by that body).
- `NoBody` or `Unavailable`: not eligible for body analysis. Unavailable functions
  are recorded before normal checking removes them from the program.

Coverage is `None` if shared preparation/declaration checks stop before the
integration point; this is not successful empty analysis. Unused generic templates
may disappear before measurement, while instantiated bodies appear under their
prepared names. Referenced declaration restrictions retain the offending
declaration's source span. No fallback reasons become normal compiler warnings.

`CoverageReport::skip_counts()` groups skipped bodies by scope and category.
These are **first-blocker frequencies**, not an exhaustive inventory: validation
and lowering stop at the first restriction, and a program restriction counts once
for each eligible body it excludes. Per-body messages provide further detail.

Run the bounded representative corpus (real examples, diagnostic fixtures, and
loaded bundled imports) with per-body statuses and a ranked fallback summary.
The corpus explicitly loads the `x86_64-unknown-linux-gnu` target so hosted
imports are independent of the machine running the frontend tests:

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
locks down prepared/instantiated coverage, declaration-local fallback, imported
free functions, complete graph discard on late lowering failure, continued AST
checking, and diagnostic precedence without duplicates.

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
root before the RHS. Indexing captures the base before evaluating the index,
then captures the element address before the RHS. Nested projections evaluate
each index exactly once, from left to right. Slices likewise read the base before
evaluating their bounds. Compound assignment additionally loads the old value
before the RHS. A projected store never initializes the whole root and never introduces
partial-move facts. Capture operations describe abstract addresses, not loans;
the AST checker's reservation rules still prevent their invalidation by the RHS.

Cleanup is explicit at full-expression boundaries, scope exits, overwrite,
discard, return, break, and continue. Exiting scopes clean up in reverse order.
Return and value-block results survive cleanup until transferred. Cleanup facts
classify each reachable site as `Never`, `Always`, or `Conditional` from the
converged must/may state. These are slot-liveness facts, not a native destructor
plan, and the LLVM backend does not consume them yet.

## Current subset and next adoption steps

The adapter supports Boolean/integer scalars, structs with scalar fields, arrays
of those values (including nested arrays), direct references and slices of those
values, struct and array literals, whole-local moves, direct calls, scalar
operators (including signed integer negation), field/dereference/index reads and
assignments, array/slice `.len`, slice expressions, compound assignments, plain value blocks
with a final value, blocks, `if`, basic `for`, integer range foreach, return,
break, and continue.
Reference and slice arguments requiring implicit mutable reborrowing are still
excluded. Projected moves are rejected explicitly. Array literals transfer their
elements left to right; moving an array kills the whole binding, and only a
whole-binding assignment restores it. Writing an element requires an initialized
base and does not establish element-level or whole-array initialization.

Range bounds have a common integer type, inferred without evaluating them. They
execute once, left to right, before the loop header, and their captured values
survive every back edge. Each iteration initializes a fresh scoped value binding
from a separate counter; `_` omits that binding. Reassigning the binding does not
change the counter. Continue cleans up iteration locals and reaches the increment;
break skips the increment and exits the range scope. The header keeps both
successors, including for literal empty/reversed ranges, so assignments in the
body never establish initialization on the zero-iteration path. Signed negation
preserves operand reads/effects and accepts minimum-value literals using the
selected target width (for example, `-128i8` and 32-bit `-2147483648isize`).

Array lengths must already be resolved. Repetition expressions and some inferred
literal forms still require fallback. Arrays and slice elements containing
references or custom destructors remain excluded. Index/bound evaluation records
initialization effects; bounds safety, slice provenance, captured-address
reservations, and loan conflicts remain responsibilities of the existing
checker/backend. Abstract cleanup records slot availability only; it does not
schedule array element destruction.

Imports and unrelated globals, enums, structs, and functions do not restrict
analysis. Imported free functions and their qualified direct calls use the same
subset checks as local functions. Globals and enums remain unsupported when used
by a body. Generic functions, methods, FFI, borrowed returns, and borrow contracts
still require fallback for the affected function and its callers. Referenced
structs must have distinct scalar fields, no generic parameters, and no custom
destructor; this applies to signatures, locals, literals, and callee signatures.
Ambiguous referenced names also require fallback. Casts, Result propagation,
patterns, collection foreach, for-init/step, unsafe blocks, diverging value blocks,
and nested yield exits are outside the body subset.
Lowering is not a complete type checker or proof of safety. Tests keep examples
where initialization succeeds but borrowing correctly fails in production.

The representative corpus was rerun after the range-loop/negation extension:

| Outcome / first blocker | Before | After |
| --- | ---: | ---: |
| Analyzed without findings | 30 | 34 |
| Initialization/move findings | 1 | 1 |
| Skipped bodies | 229 | 225 |
| Function declaration | 152 | 152 |
| Type | 43 | 43 |
| Struct declaration | 12 | 12 |
| Unsupported construct | 11 | 11 |
| Statement | 7 | 3 |
| Expression | 4 | 4 |

`bytes.equal`, `bytes.compare`, `bytes.fill`, and `bytes.reverse` are newly
analyzed, completing the previously selected range-iteration step. Coverage
regressions require all four bodies, plus `bytes.starts_with` and
`bytes.ends_with`, to be analyzed without findings. These are sample counts;
prepared generic method instance counts, and consequently function-declaration
skips and total skipped bodies, vary between runs. The four new analyzed bodies
and the reduction in statement skips are stable.

Collection foreach remains exposed in `fibonacci_known_sequence`, and match
patterns in `io.transfer` and `patterns.category`. Result signatures still account
for many type skips, but propagation requires additional payload and early-return
modeling. These first blockers identify remaining incremental work, not a promise
that removing one blocker completes each body.

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
