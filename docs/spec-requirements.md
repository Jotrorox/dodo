# Requirements for implementing Dodo 0.1

This is a review and test-planning checklist derived from the
[language specification](language-spec-0.1.md). It does not describe completed
compiler work. A supported subset should be documented separately, and omitted
features should receive clear diagnostics rather than silently altered semantics.

| Area | Required behavior and useful checks |
| --- | --- |
| Source and visibility | Require packages; support string imports and qualified names; make declarations and fields private by default; reject public APIs exposing private types; expose public enum variants. |
| Syntax | Newline statement termination, braced blocks, void as the default return type, explicit function returns, name-first declarations, receiver and field shorthand, inferred mutable locals, constants, struct-local methods. |
| Values | Primitive integers, pointer-sized integers, floats, bool, arrays, shared/mutable slices and references, raw pointers, UTF-8 string views, Result, Option, structs, enums, and basic generics. |
| Literals | Contextual integer inference with isize default; typed integers; byte, string, byte-string, and array literals; immutable program-lifetime string storage; inferred array lists and copy-only repetition with one initializer evaluation. |
| Initialization | Reject reads before initialization and after a move, including branch-dependent paths and subobjects; allow reinitialization. |
| Moves | Copy scalars/shared references/raw pointers; move structs/arrays/tagged values/mutable references; no implicit deep copy; consuming receivers transfer ownership. |
| Methods | Infer receiver borrowing for shared/mutable methods; support consuming receivers and associated functions; require explicit new borrows in free calls. |
| Loans | Reject live overlapping writer/writer or reader/writer loans, owner moves and destruction during loans, and conflicting owner access; allow independent struct fields and non-lexical last-use loan endings. |
| Reborrows | Suspend the original mutable reference during a reborrow; do not duplicate exclusive access; reborrow reference arguments where needed. |
| Borrowed returns | Infer from borrowed self or one borrow-carrying argument; require from(...) when ambiguous and from(static) without an input; validate bodies; never return destroyed local storage. |
| Borrowed aggregates | Conservatively retain all source lifetimes, including through Result/Option/containers; replacement cannot lengthen the inferred lifetime; disallow independent field lifetimes and owning self-reference. |
| Destruction | Reverse declaration order on normal exit/return/?/break/continue; custom drop before fields; destroy replaced values; core.drop consumes; do not double-drop moved values, directly call drop, or move a field from a custom-drop value. |
| Numeric behavior | Trap on safe overflow, division by zero, invalid shifts, bounds failures, and invalid checked conversions in every profile; use named operations for wrapping/truncation. |
| Constants and inference | Resolve named constant dependencies and array lengths; diagnose cycles, target-width overflow, and ambiguous local generic arguments; infer some(payload). |
| Value blocks | Unify branch result types, evaluate selected branches only, transfer values before cleanup, retain outer-expression loans, and reject escaping local borrows. |
| Ranges and slicing | Capture integer range bounds once, create fresh bindings, preserve loop cleanup, and bounds-check subslices with source lifetimes. |
| Foreach | Evaluate the collection once; borrow arrays/slices without consuming; fresh bindings per iteration; usize indices; &T or &mut T elements; forbid mutable element loans escaping their iteration. |
| Matching | Exhaustive statements and value expressions with no fallthrough; literals/variants/payload bindings/wildcard; consume an owned non-copy scrutinee and borrow payloads of reference scrutinees. |
| Errors | T!E aliases Result<T,E>; ! binds outside borrow/slice syntax; ok/err and ok() for void; ? requires identical error types and ordinary cleanup; reject discarded or unhandled Results, including `_ =`. |
| Unsafe boundary | Restrict pointer dereference/arithmetic/reference conversion, foreign calls, unchecked access, and assembly; require unsafe blocks even inside unsafe fn; keep ordinary type, borrow, and bounds checks active. |
| Raw validity | Preserve requirements for bounds, alignment, initialization, valid values, aliasing, and originating object; unsafe syntax cannot validate an invalid checked reference. |
| Memory and hardware | Provide specified core.ptr/core.mem facilities; allocation remains optional and recoverable failures use Result; MMIO accesses use supported widths without splitting and never fabricate ordinary mutable references. |
| Volatile and concurrency | Preserve volatile accesses and mutual compiler ordering; do not imply atomicity or barriers; unsafe mutable-static access unless synchronized; atomics expose no ordinary mutable reference to concurrent storage. |
| FFI and targets | Explicit C ABI and repr(C); target-specific attributes, assembly, barriers, entry/linker integration, and non-returning panic path; no mandatory heap, collector, OS, or scheduler. |
| Diagnostics | Show conflicting access, loan origin, live use, and a safe repair direction; do not offer unsafe as the default borrow-conflict repair. |

The worked examples establish three useful execution checks: `samples.demo()`
returns 131; `hex.parse_or(b"2a", 0)` returns 42; and
`hex.parse_or(b"zz", 7)` returns 7. The GPIO example exercises unsafe-contract
boundaries; it is not a host-side device execution test.

Safety-sensitive checks should include rejection cases across branches, loops,
reborrows, aggregate fields, calls, and destructors. Passing examples and LLVM
verification alone do not establish borrow-checker soundness. DMA and interrupt
contracts also require a sound platform/core-library design.

The [open specification items](language-spec-0.1.md#appendix-c-open-specification-items)
must remain visible. Implementations need documented choices for lexing,
operator precedence/evaluation, further generic inference, patterns, entry conventions,
package resolution, layout, and exact core APIs. An implementation choice does
not retroactively become a rule of the supplied design.
