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
`tests/flow_prototype/tagged.rs` adds independently specified tagged-value,
pattern, and propagation outcomes, including payload moves, guard retries,
borrowed bindings, value-block exits, and cleanup on early error returns.

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

## Tagged values, patterns, and Result exits

Tagged-value adoption has three stages:

1. **Representation and construction.** Non-generic enums, `Option<T>`, and
   `Result<T, E>` accept recursively plain payloads, including structs, arrays,
   and nested tagged values. Structs now accept these plain fields too. Tagged
   construction transfers payload operands in source order; structs and arrays
   likewise transfer their elements. Whole-slot moves and reinitialization keep
   the same must/may facts as other owned values. `void` success Results use
   `ok()` with no payload. Borrow-containing aggregates, custom destructors,
   unresolved types, and recursive or ambiguous declarations still cause
   body-local fallback with the offending declaration or field span.
2. **Pattern selection and payload transfer.** `match`, `if let`, and
   destructuring `let` support bindings, wildcards, scalar literals/ranges,
   structs, variants, recursive patterns, and alternatives. Tests read the
   scrutinee without consuming it, checking a variant before testing its
   payload. Alternatives expand within a 4,096-plan limit and share a selected
   body. Guard bindings are non-owning previews: a false guard cleans up its
   preview slots and retries the next alternative, while a true guard commits
   ownership exactly once. Owned selection transfers all bound payloads in one
   operation and discards the remainder of the scrutinee temporary. Reference
   selection creates shared/mutable payload references while preserving the
   owner's availability. Conditional patterns consume owned scrutinees on both
   success and failure. Match-arm, scope, return, loop-exit, and value-block
   cleanup preserve the selected payload's slot until it is transferred.
3. **Result exits.** `?` evaluates its operand once and branches on its tag. The
   success edge consumes the Result and extracts its payload; the error edge
   transfers the owned error into the enclosing return type, cleans up live
   scopes in reverse order, and returns that temporary intact. This includes
   partially evaluated call arguments, constructors, and captured destinations.
   Error returns do not reach ordinary joins or loop back edges. Nested Results
   and `void` success payloads use the same operations. Postfix `!` shares the
   success extraction but ends its error path without unwinding or cleanup.

The analysis tracks slot availability, not tag values or exhaustiveness. Both
successors of a pattern test remain possible. Failed exhaustive matches and
unmatched irrefutable lets have unreachable terminators; the AST checker still
validates exhaustiveness, let-else divergence, guard restrictions, Result handling
obligations, borrowing, and destruction. Payload projection paths describe an
atomic transfer from a matched temporary; they do not introduce partial-move
facts on user storage or a native payload destruction plan.

## Current subset and next adoption steps

The adapter supports Boolean/integer scalars, recursively plain structs/enums,
Options, Results, arrays of those values (including nested arrays), direct
references and slices of those values, aggregate and tagged literals, whole-local
moves, direct calls, scalar operators (including signed integer negation),
field/dereference/index reads and assignments, array/slice `.len`, slice
expressions, compound assignments, value blocks with branch-local yields,
patterns, Result propagation/unwrap, blocks, `if`, basic `for`, integer range foreach, return, break,
and continue.
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
literal forms still require fallback. Owned aggregates and slice elements containing
references or custom destructors remain excluded. Index/bound evaluation records
initialization effects; bounds safety, slice provenance, captured-address
reservations, and loan conflicts remain responsibilities of the existing
checker/backend. Abstract cleanup records slot availability only; it does not
schedule array element destruction.

Imports and unrelated globals, enums, structs, and functions do not restrict
analysis. Imported free functions and their qualified direct calls use the same
subset checks as local functions. Globals remain unsupported when used by a
body. Generic functions, methods, FFI, borrowed returns, and borrow contracts
still require fallback for the affected function and its callers. Referenced
structs/enums must have distinct plain fields/variants, no generic parameters,
and no custom destructor; this applies to signatures, locals, literals, and
callee signatures. Ambiguous referenced names also require fallback. Casts,
collection foreach, for-init/step, unsafe blocks, and value blocks with no yielding path
remain outside the body subset. Guard assignments, moves, mutable borrows, and
Result exits cause explicit fallback.
Lowering is not a complete type checker or proof of safety. Tests keep examples
where initialization succeeds but borrowing correctly fails in production.

The representative corpus now uses an explicit `x86_64-unknown-linux-gnu`
package-loading target, so its hosted dependencies are comparable across hosts.
Sampled runs on main with range loops and after the tagged-value extension
produced:

| Outcome / first blocker | Before | After |
| --- | ---: | ---: |
| Analyzed without findings | 34 | 55 |
| Initialization/move findings | 1 | 1 |
| Skipped bodies | 225 | 206 |
| Function declaration | 152 | 156 |
| Type | 43 | 4 |
| Struct declaration | 12 | 1 |
| Unsupported construct | 11 | 23 |
| Statement | 3 | 4 |
| Expression | 4 | 18 |

Prepared generic specialization counts and consequently function-declaration
skips and total skipped bodies vary between runs; assertions lock down named
outcomes instead of total counts. `patterns.unpack` and `patterns.category` are newly analyzed, along with
`ascii.digit_value`, `ascii.hex_value`, `num.checked_sub`, `num.checked_div`,
`num.checked_rem`, `num.align_up`, `io.failure`, `text.validate`,
`text.encoded_len`, `text.parse_u64`, and several plain error/adapter helpers.
`io.transfer` now reaches the implicit mutable-reborrow restriction instead of
its match blocker. `bytes.copy_from` is now analyzed because its Result signature
and range loop are both supported. Coverage also preserves analysis of
`bytes.equal`, `bytes.compare`, `bytes.fill`, and `bytes.reverse` from the
range-loop extension. Collection iteration, casts, and implicit mutable reborrows
remain subsequent incremental work; removing one restriction does not necessarily
complete a body.

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
