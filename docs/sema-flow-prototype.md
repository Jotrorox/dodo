# Semantic checker extraction and control-flow experiment

## Recommendation

Keep the three production module extractions. Continue a **bounded, read-only
initialization experiment**, and fix the existing checker's exit-state handling
in a separate behavior-changing task. Production migration is premature.

Explicit edges provide a measurable correctness benefit: the experiment handles
five valid cases rejected by the conservative checker and detects two move errors
that it currently misses. However, a small dataflow solver is only part of the
cost. The source adapter is substantially larger than the solver/representation,
and lacks most elaboration, borrowing, and destruction semantics. Turning that
adapter into another general type checker would make maintenance worse.

This work changes no production acceptance rules, diagnostics, or LLVM lowering.
In particular it adds neither slice splitting nor container rules, and does not
change generic semantics. The baseline is `07c9307` (Dodo 0.1.2). Concurrent slice
and container work in the original checkout is excluded from this branch.

## Stage 1: behavior-preserving extraction

The checking order remains preparation, public-interface validation,
specialization, context construction, constant checking, and body checking.
Public `check`, `check_for_target`, and `check_recovering` signatures and behavior
remain intact; the crate-private `array_length` bridge remains intact too.

| Boundary | Responsibility and decision |
| --- | --- |
| `src/sema/generics.rs` | Template expansion, substitution, inference of provisional shapes, specialization budgets/names. Extracted as one unit; every generated body still gets ordinary checking. Only `instantiate` and `intrinsic_result_type` are exposed to the parent module. |
| `src/sema/uses.rs` | Future-use scans and lexical diagnostic use-site resolution. Extracted together, with the distinction between conservative name-based liveness and binding-aware diagnostic evidence retained. |
| `src/sema/ownership.rs` | Loan/value/variable/place facts and pure overlap, conflict-kind, static-loan, and state-join operations. Extracted with parent-only visibility. Dependencies, Result obligations, move origins, sorting, and deduplication are unchanged. |
| `src/sema.rs` | Orchestration, context/type properties, statement/expression checking, pattern coverage, loan decisions, and diagnostic construction remain here. Separating context or pattern checking is plausible later, but splitting the stateful `Checker` implementation across files now would provide a weaker boundary. |
| `src/prepare.rs` | Already a coherent constant/array-length preparation pass. Its call into `sema::array_length` uses an empty checking context to type/evaluate an array length. Moving it would enlarge this refactor unnecessarily. |
| `src/ast.rs`, `src/codegen.rs` | The annotated AST remains the interface to LLVM. Inferred types/contracts, specialized declarations, resolved calls, and desugared statements still reach the same backend. No change. |

The original checker shrinks from 7,582 to 5,730 lines. The extracted modules
contain 1,452, 329, and 95 lines respectively, including imports/documentation.
They are not a claim that ownership is now an independent pass: the parent still
owns all stateful checking decisions. The generic module still uses the parent's
small `qualified_name`, `dereferenced`, and `match_subject` helpers, explicitly
imported rather than hidden by a wildcard import.

A mechanical comparison against the baseline confirmed that all extracted bodies
are byte-identical after removing `pub(super)` visibility and module headers;
the remaining checker is byte-identical after removing its new imports/modules.
Existing regression tests were not rewritten to accommodate the extraction.

## Stage 2: test-only typed control flow

Run `cargo test --test flow_prototype`. The integration-test target alone imports
`tests/support/flow`; neither the library nor the CLI imports it. There is no
feature switch that could accidentally enable it for production compilation.

`lower(program, function_name)` works from a fresh parser AST and returns either
a `Body` or a `Limitation { span, reason }`. It does not call production checking,
or trust annotations left behind by a failed check. It lowers **one named body**,
checks the restricted declaration/signature shapes, and does not certify callee
bodies. Unsupported or ill-typed syntax discards the entire partial graph.
Reporting is fail-fast: the limitation is the first obstacle, not a complete
inventory of every unsupported node in that program.

The minimum representation is:

| Representation | Meaning |
| --- | --- |
| `Place(local_id)` and `Local { name, ty, span, parameter }` | Stable lexical identity; type stored once in the local table. Shadowed names remain distinct. Only whole locals are implemented. |
| `Operand::Constant(Type)`, `Copy(place)`, `Move(place)` | A typed constant abstraction or explicit access. Constant values are unnecessary for this analysis; no branch is pruned on their basis. |
| `Assign { target, value }` | Consume/copy the source, then initialize the destination. |
| `Borrow { target, source, mutable }` | Require an initialized source and initialize a reference temporary. Borrow conflicts and lifetime validity are **not analyzed**. |
| `Call { function, args, target }` | Arguments are typed against the direct signature and materialized left to right. Each source move happens before evaluation of the next argument. Assume the call returns normally. |
| `Goto`, `Branch`, `Return`, `Unreachable` | Explicit successors. A `break` targets its own loop exit; a `continue` targets its own header; a return has no successor. |
| `Cleanup(place)` | Conditional cleanup of an initialized slot, then kill its initialization fact. An uninitialized slot imposes no read obligation. Only types without custom destruction or borrowed aggregate fields are modeled. |

All operations and terminators carry source spans. Each expression gets a typed
temporary, including call arguments and returns. Cleanup occurs in reverse
allocation/declaration order on lexical exits, `break`, `continue`, and return.
A return temporary survives cleanup until the return consumes it. Overwrite
cleans up the old destination after evaluating the RHS. This represents where
cleanup belongs; it does **not** prove native destruction or destructor order.

Future projections would need typed field/dereference/index steps and explicit
rules for whole-value versus partial initialization. They are deliberately absent
rather than encoded as whole-local operations. Cleanup with custom destructors
would need explicit read/dependency effects and runtime initialization flags.
Borrow checking would additionally need loan identities, provenance, access
routes, and liveness, including call reborrows and retained container dependencies.

### Implemented analysis and limits

The forward must analysis records an optional initialized-bit vector at each
block entry. `None` means unreachable, distinct from a reachable block with no
initialized locals. Entry initializes parameters only. Assignments and calls
generate initialization; moves and cleanup kill it; borrows require it.
Reachable predecessor facts are intersected. A worklist reaches a fixed point
before issues are collected in stable block/instruction order. Diagnostics retain
both use and declaration spans, but do not attempt production wording or labels.

The finite bit domain and monotone transfers ensure convergence for these graphs.
A newly reached block receives its first incoming state; subsequent predecessor
updates can only remove entry facts. Loop exits therefore include actual break
states, while zero-iteration condition edges remain represented. A join does not
retain predicate correlations: repeated tests of the same boolean can still
produce conservative initialization errors.

Supported source:

- 64-bit target; boolean and nonnegative integer literals, integer types, whole
  local reads/moves, and explicitly typed or inferred local bindings.
- Struct values supplied by parameters or calls, where all declared fields are
  scalars and no custom destructor exists. Struct construction and field access
  inside the analyzed body are outside the adapter.
- Direct references to scalar/owned locals, explicit shared/mutable borrows,
  direct calls, scalar/owned returns, simple assignment and discard.
- Lexical blocks, `if`/`else`, `for {}` or `for condition {}` without initializer
  or step, nested loops, `break`, `continue`, and return.

Explicitly unsupported: imports/globals, generic declarations or specialization,
methods/FFI/unsafe, enums, arrays/slices/containers, `Result`/`Option`/`MaybeUninit`,
raw pointers, floating point/strings, nested references, borrowed returns and
contracts, implicit mutable reborrows, field/index/dereference places, aggregate
literals, arithmetic/logical operators (including short circuit), casts,
propagation, patterns/guards/match, foreach, for initializer/step, constants,
value blocks/yield, and custom destruction. Unsupported syntax is not skipped
even under `if false`. Syntax after a terminating statement is reported as a
limitation. Non-void fallthrough, including some unmodeled divergence cases, is
also a limitation. Type/arity/range errors return a limitation, never a graph.

**An empty `Initialization::issues` list means only that this analysis found no
uninitialized whole-local use in the lowered body. It is not a validated Dodo
program.** References can be initialized and still be invalid. Tests deliberately
retain an invalid live-shared-borrow example whose initialization result is empty.

### Concrete comparison evidence

`tests/flow_prototype.rs` checks production acceptance/rejection independently of
prototype results, always lowering the original parse. The three scalar branch
cases use the exact source bodies from existing `sema` initialization regressions.
Additional cases exercise moves, reinitialization, argument order, lexical
shadowing, nested loops, zero iterations, alternate breaks, and cleanup.

| Case | Production at baseline | Prototype initialization |
| --- | --- | --- |
| Both branches initialize / early return bypasses use | Accept | No issues |
| One branch or zero iterations leaves local uninitialized | Reject | Reports local |
| Repeated move / conditional move / repeated loop move | Reject | Reports moved local |
| Reinitialize after move / move followed by return | Accept | No issues |
| Unconditional loop initializes before its sole break | Reject | No issues |
| Both break arms initialize before leaving a loop | Reject | No issues |
| Continue paths do not reach the use; remaining break initializes | Reject | No issues |
| Move followed by unconditional break or return | Reject | No issues (two cases) |
| Conditional continue loses moved state at production branch join | **Accept** | Reports moved local on back edge |
| Conditional break loses moved state before post-loop use | **Accept** | Reports moved local after loop |
| Initialized reference conflicts with a later write | Reject | No initialization issues; loan safety is unmodeled |

Two minimal valid programs rejected by production are:

```dodo
package example
fn initialized() -> u8 {
    u8 x
    for {
        x = 1
        break
    }
    return x
}
```

```dodo
package example
struct S { u8 n }
fn take(s: S) {}
fn once(s: S) {
    for {
        take(s)
        break
    }
}
```

In the first, every path to the read initializes `x`; the loop cannot execute
zero times. In the second, no path repeats the move. The other three valid
counterexamples are in `loop_exit_initialization_is_more_precise` and
`moves_on_exiting_loop_paths_do_not_reach_a_back_edge`.

The newly exposed production gaps are also executable comparison tests:

```dodo
package example
struct S { u8 n }
fn take(s: S) {}
fn repeated(s: S, b: bool) {
    for b {
        if b {
            take(s)
            continue
        }
    }
}
fn twice(s: S, b: bool) {
    for {
        if b {
            take(s)
            break
        }
    }
    take(s)
}
```

With `b = true`, `repeated` consumes the same moved value on the next iteration;
`twice` consumes it again after a finite loop exit. The production `if` merge
excludes a terminating arm's state as if it had returned from the function.
That loses state needed by `continue` and `break`. These acceptance gaps are
recorded, not fixed in this behavior-preserving task; an independent fix should
retain distinct exit states and add production/native regression coverage.

Existing ownership fixtures are reported honestly: `examples/borrowing.dodo`,
`moved_value.dodo`, `return_source.dodo`, and `tests/stdlib/shared_storage.dodo`
cannot be fully lowered. In `borrow_conflict.dodo`, `main` lowers but its live
loan conflict is outside initialization; `consume` fails lowering on dereference.
Custom-drop/propagation native regressions remain production regression coverage,
not evidence that this prototype validates their cleanup.

## Migration gates, risks, and validation

Further work should first fix the two exit-state gaps in the current checker,
then evaluate an initialization-only shadow pass with explicit coverage counts.
Before changing production decisions, it needs:

1. A trusted typed elaboration boundary independent of ownership success. Lowering
   only successfully checked ASTs cannot investigate rejected valid programs;
   lowering partly checked ASTs risks missing implicit moves/reborrows and types.
2. Exit and cleanup semantics for short circuit, patterns/guards, propagation,
   value blocks, foreach and temporaries, tested against existing O0/O3 native
   destruction behavior and backend initialization flags.
3. Diagnostic provenance compatible with existing move/loan origin labels and
   editor recovery transactions. The prototype's spans alone are insufficient.
4. A stated supported domain, differential results for all existing ownership
   regressions, and explicit fallback outside it. No partially modeled body may
   acquire an acceptance verdict.

There is no performance claim: this tiny experiment neither benchmarks compile
time nor establishes a scalable representation. It creates a temporary per
expression and uses dense vectors/simple worklists. Its benefit is that control
flow and initialization can be reasoned about without manipulating the AST or
cloning the entire checker. That supports continued experimentation, not a
wholesale compiler rewrite or immediate ownership/codegen migration.

Validation on Linux x86-64 with Rust 1.95.0 and LLVM 22:

| Check | Result |
| --- | --- |
| Baseline and extraction: `cargo test --lib --test diagnostics --test compiler --test regressions --test owned_storage --test reference_iteration --test array_annotations --test specification` | Same 302 tests passed before and after extraction. Includes exact diagnostic spans, editor recovery, generic/array preparation, and O0/O3 native ownership/cleanup. |
| `cargo test --all-targets`, followed by `cargo test --test tls_library --test web_library` after supplying missing system development headers | All targets covered; 527 total tests reported passing when combined with the final 15-test prototype run. The initial run stopped at three TLS build failures due to absent OpenSSL headers; those three passed on rerun, and the remaining web target passed. |
| `cargo test --test flow_prototype` | All 15 tests passed, including the two production acceptance gaps and negative/unsupported cases. |
| `cargo fmt --check` | Passed. |
| `cargo clippy --all-targets -- -D warnings` | Passed. |
| `cargo test --doc` / `git diff --check` | Passed (no Rust documentation tests). |

The native suite includes compiler, regression, owned-storage, array/reference
iteration, reachability, library, and diagnostic/CLI/editor tests. GDB is present;
the existing optional LLDB smoke test self-skips because LLDB is unavailable.
Cross-target compilation checks in the suite ran, but this is not a Windows
runtime test or a release-packaging qualification.

The host initially lacked the unversioned linker aliases for installed ffi,
zlib, and XML libraries. A temporary `/tmp/dodo-sema-libs` directory provided
aliases and was passed as `LIBRARY_PATH`. For TLS, the matching
`openssl-devel-3.5.8-1.fc44.x86_64` RPM was downloaded and extracted under
`/tmp/dodo-sema-openssl`; `CPATH` pointed to its `usr/include` and the temporary
linker directory supplied ssl/crypto aliases to the installed libraries. No
system packages, repository dependencies, or build settings were changed.

```sh
# With native development dependencies installed, no extra environment is needed.
cargo test --all-targets
cargo test --test flow_prototype
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --doc
```
