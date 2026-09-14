---
title: "Implementation checklist"
description: "Design requirements and review checks for Dodo 0.1, grouped by compiler area."
section: "Language reference"
order: 250
---

This review and test-planning checklist comes from the
[language specification](language-spec-0.1.md). The specification's Appendix B
maps the retained rules to acceptance, rejection, and execution tests; Appendix C
records implementation-defined choices and excluded features. This checklist also
contains future library/platform goals; those do not expand the specified 0.1
compiler surface. For compiler 0.1.2, start with [compiler support and limits](implementation.md).

## How to use this checklist

Review the relevant area against the implementation and its tests. Unsupported
features should receive clear diagnostics. Record implementation choices and
limits separately from the language design; passing examples alone do not
establish conformance or borrow-checker soundness.

## Source, packages, and syntax

| Area | Required behavior and useful checks |
| --- | --- |
| Source and visibility | Require packages; support string imports and qualified names; make declarations and fields private by default; reject public APIs exposing private types; expose public enum variants. |
| Syntax | Newline statement termination with leading-dot continuation and explicit semicolon boundaries, canonical name-first declarations/bracket arrays/`f::<T>()` calls, automatic legacy migration with `dodo fmt`, braced blocks, void as the default return type, explicit and final-expression function returns, receiver and field shorthand, immutable runtime let bindings, inferred mutable locals, constants, struct-local methods. |
| Values | Primitive integers, pointer-sized integers, floats, bool, arrays, shared/mutable slices and references, raw pointers, UTF-8 string views, Result, Option, structs, enums, and basic generics. |
| Literals | Contextual integer inference with isize default; typed integers; byte, string, byte-string, and array literals; immutable program-lifetime string storage; inferred and explicitly annotated bracket array lists and copy-only repetition with one initializer evaluation. |

## Ownership and lifetime checks

| Area | Required behavior and useful checks |
| --- | --- |
| Initialization | Reject reads before initialization and after a move, including branch-dependent paths and subobjects; allow reinitialization. |
| Moves | Copy scalars/shared references/raw pointers; move structs/arrays/tagged values/mutable references; no implicit deep copy; consuming receivers transfer ownership. |
| Methods | Infer receiver borrowing for shared/mutable methods; support consuming receivers and associated functions; require explicit new borrows in free calls. |
| Loans | Reject live overlapping writer/writer or reader/writer loans, owner moves and destruction during loans, and conflicting owner access; allow independent struct fields and non-lexical last-use loan endings. |
| Reborrows | Suspend the original mutable reference during a reborrow; do not duplicate exclusive access; reborrow reference arguments where needed. |
| Borrowed returns | Infer from borrowed self or one borrow-carrying argument; require from(...) when ambiguous and from(static) without an input; validate bodies; never return destroyed local storage. |
| Borrowed aggregates | Conservatively retain all source lifetimes, including through Result/Option/containers; replacement cannot lengthen the inferred lifetime; disallow independent field lifetimes and owning self-reference. |
| Destruction | Reverse declaration order on normal exit/return/?/break/continue; custom drop before fields; destroy replaced values; core.drop consumes; do not double-drop moved values, directly call drop, or move a field from a custom-drop value. |

## Expressions, control flow, and errors

| Area | Required behavior and useful checks |
| --- | --- |
| Numeric behavior | Trap on safe overflow, division by zero, invalid shifts, bounds failures, and invalid checked conversions in every profile; use named operations for wrapping/truncation. |
| Constants and inference | Resolve named constant dependencies and array lengths; diagnose cycles, target-width overflow, and ambiguous local generic arguments; infer some(payload). |
| Value blocks | Unify branch result types, evaluate selected branches only, transfer values before cleanup, retain outer-expression loans, and reject escaping local borrows. |
| Ranges and slicing | Capture integer range bounds once, create fresh bindings, preserve loop cleanup, and bounds-check subslices with source lifetimes. |
| Foreach | Evaluate the collection once; borrow arrays/slices without consuming; fresh bindings per iteration; usize indices; &T or &mut T elements; opt-in `&value` patterns copy shared copyable elements while preserving collection loans and reference dependencies; forbid mutable element loans escaping their iteration. |
| Matching | Uniform expression/block arms; shared recursive enum/struct patterns, literals, ranges, alternatives, and guards; exhaustive unguarded coverage; conditional if-let and diverging let-else; preserve ownership, borrow permissions, and mandatory nested Result handling. |
| Errors | T!E aliases Result<T,E>; ! binds outside borrow/slice syntax; ok/err and ok() for void; ? requires identical error types and ordinary cleanup; reject discarded or unhandled Results, including `_ =`. |

## Unsafe code and platform integration

| Area | Required behavior and useful checks |
| --- | --- |
| Unsafe boundary | Restrict pointer dereference/arithmetic/reference conversion, foreign calls, unchecked access, and assembly; require unsafe blocks even inside unsafe fn; keep ordinary type, borrow, and bounds checks active. |
| Raw validity | Preserve requirements for bounds, alignment, initialization, valid values, aliasing, and originating object; unsafe syntax cannot validate an invalid checked reference. |
| Memory and hardware | Provide specified core.ptr/core.mem facilities; allocation remains optional and recoverable failures use Result; MMIO accesses use supported widths without splitting and never fabricate ordinary mutable references. |
| Volatile and concurrency | Preserve volatile accesses and mutual compiler ordering; do not imply atomicity or barriers; unsafe mutable-static access unless synchronized; atomics expose no ordinary mutable reference to concurrent storage. |
| FFI and targets | Explicit C ABI and repr(C); target-specific attributes, assembly, barriers, entry/linker integration, and non-returning panic path; no mandatory heap, collector, OS, or scheduler. |

## Diagnostics and editor information

| Area | Required behavior and useful checks |
| --- | --- |
| Diagnostics | Label source snippets for conflicting access, loan origin, and live use; show move origins and borrowed-return contract/source violations; provide a safe repair direction without unsafe as the default. Editor hovers expose inferred types, receiver ownership, and borrowed-return sources. |

## Execution and rejection checks

The worked examples establish three useful execution checks: `samples.demo()`
returns 131; `hex.parse_or(b"2a", 0)` returns 42; and
`hex.parse_or(b"zz", 7)` returns 7. The GPIO example exercises unsafe-contract
boundaries; it is not a host-side device execution test.

Safety-sensitive checks should include rejection cases across branches, loops,
reborrows, aggregate fields, calls, and destructors. Passing examples and LLVM
verification alone do not establish borrow-checker soundness. DMA and interrupt
contracts also require a sound platform/core-library design.

## Implementation-defined choices and future work

The [implementation profile](language-spec-0.1.md#appendix-c-implementation-defined-behavior-and-excluded-features)
must remain explicit. Evaluation order, representation structure, pointer
contracts, package resolution, and the supported C interface are normative rules.
Target alignments, native ABI details, floating constant precision, and runtime
integration have documented implementation-defined choices. Future work includes
interrupt/DMA facilities and the excluded syntax listed in Appendix C; successful
target emission alone does not establish platform conformance.
