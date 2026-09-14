---
title: "Syntax and expressions"
description: "Implemented syntax, inference, constants, control flow, and numeric behavior in Dodo 0.1.1."
section: "Language reference"
order: 210
---

This page describes syntax accepted by Dodo 0.1.1. Start with
[your first program](first-program.md) for a short introduction, or use the
[full language specification](language-spec-0.1.md) for the design's complete
syntax and worked examples.

## Source text and statement boundaries

Source is UTF-8. Identifiers are case-sensitive ASCII letters/underscores followed
by ASCII letters/digits/underscores. Comments use `//`. Newlines terminate
statements; semicolons also separate statements and three-part loop clauses.
Delimited expressions and expressions continued after an operator accept
newlines. A newline also continues an expression when the next non-newline token
is `.`; indentation, blank lines, and line comments do not change this rule.
An explicit semicolon ends the expression. Other leading operators do not gain
continuation outside delimited expressions. Braces delimit control-flow bodies;
match arms also accept expressions.

## Literals and strings

Integer literals support decimal, `0x`, `0o`, `0b`, digit separators, and type
suffixes. Unsuffixed integer literals use contextual types and otherwise default
to `isize`. Floating literals use decimal fractions/exponents and optional
`f32`/`f64` suffixes; their default is `f64`. Boolean literals are `true` and
`false`. Strings and byte strings have immutable program-lifetime storage.
Strings support conventional escapes; `&str.len` counts UTF-8 bytes. String
indexing is rejected; use a byte slice for byte indexing. There is no implicit
string allocation or mandatory NUL terminator.

## Operators and evaluation order

From lowest to highest, binary precedence is `||`, `&&`, `|`, `^`, `&`, equality
(`==`, `!=`), ordering (`<`, `<=`, `>`, `>=`), shifts (`<<`, `>>`), addition and
subtraction, multiplication/division/remainder, then `as`. Prefix operators
bind more tightly, followed by calls, fields, indices, and propagation `?`.
Binary operators associate left. Operands, call arguments, and literal fields
are evaluated left to right; `&&` and `||` short-circuit. Compound assignment
evaluates the destination address once, then its previous value, then the right
operand. Numeric operands must have compatible types; there are no implicit
mixed-width conversions between already typed values.

## Declarations and returns

Canonical bindings, fields, constants, and parameters use `name: Type`. The earlier
type-first forms remain supported for locals, fields, constants, and enum
payloads. An omitted function return type means `void`. Struct receivers accept
`self`, `&self`, and `&mut self`; field literals accept same-name shorthand.
Non-void functions return their final expression unless it ends with a semicolon;
final conditionals, matches, and blocks follow the same rule. Explicit `return`
provides early exits. Void functions retain statement semantics.

## Mutable and immutable bindings

`let name = value` and `let name: Type = value` create immutable runtime bindings.
`:=` and ordinary typed locals remain mutable; `const` remains compile-time-only.
Immutability prevents reassignment, mutable borrowing of owned storage, and writes
to owned fields/elements. It does not weaken a stored `&mut T` or mutable slice:
writes through those references remain permitted, including field/index writes.

## Generic syntax and inference

Explicit generic calls canonically use `f::<T>()`; the earlier `f<T>()` spelling
remains accepted. Types and generic declarations use angle brackets without `::`.

Generics are monomorphized. Function calls and struct literals infer omitted
type arguments from arguments, fields, and expected result types. Examples are
`identity(42i32)`, `value: i32 = identity(42)`, and `Box{value: 42i32}`. `some(value)`
infers its Option payload. Ambiguous calls require explicit arguments, such as
`make::<i32>()`; explicit forms remain accepted. Inference follows local expression
context, not later uses of a binding. Function signatures remain declared.
Constraints/traits, specialization, and separately compiled generic interfaces
are not implemented. Recursive expansion is bounded and diagnosed.

## Formatting and syntax migration

`dodo fmt [FILE|DIRECTORY]` migrates historical syntax and applies four-space
indentation and consistent spacing while preserving comments and literal text.
Directory formatting recurses through `.dodo` files, excluding hidden directories,
`target`, `build`, and symlinks. The default input is the current directory.
`--check` exits 1 for formatting differences and writes nothing; `--stdout`
previews one file. Input `-` reads stdin and writes stdout (or checks with
`--check`). Formatting parses every input before replacing any file, requires no
imports or type checking, and stages replacements for atomic per-file renames.
An unchanged file is not rewritten. Syntax errors leave all inputs untouched.
Legacy forms remain supported in 0.1; removal or warnings belong to a future
announced deprecation after this migration path is available.

See the [command-line guide](command-line.md) for formatting commands.

## Arrays and constants

Array lists infer their length and element type from context or their elements.
Bracket arrays are canonical. A binding annotation supplies explicit types,
as in `values: [2]u16 = [1, 2]`. A list in any expression can retain its type and
length with `([1, 2]: [2]u16)` or `([]: [0]u8)`. The annotation applies only to
an unannotated bracket list and lowers to the same typed literal as the legacy
`[2]u16{1, 2}`. The formatter migrates legacy typed literals to that annotated
bracket form, including nested and constant arrays. This preserves element types
and length checks without introducing copying or runtime conversions.
Repeated arrays evaluate their initializer once, even for a zero length, and
require a copyable element. Constant initializers support checked scalar
operators/conversions, strings, arrays, repeated arrays, and struct literals.
They may reference other constants. Package constants permit forward references;
local constants follow lexical scope. Cycles and mutable-static dependencies are
rejected. Array lengths accept integer constant expressions and use the selected
target width. Compile-time function calls are not implemented.

## Compiler resource limits

Syntax nesting is bounded to 64 parser levels. Constant dependency depth is
bounded to 128; expansion work is bounded to 200,000 nodes. Array lengths must
fit the target's usize and LLVM's 32-bit element-count limit. Nonzero repeated
constant arrays are limited to 1,000,000 elements; zero initialization has a
compact representation. These limits produce diagnostics.

## Value-producing blocks

`if`, `match`, `unsafe`, and plain blocks produce values in expression positions.
Every continuing path must yield the same type; at least one value-producing
path is required. Results are transferred before local cleanup. Explicit returns,
propagation, break, and continue retain their surrounding control-flow meaning.
The checker rejects local-storage borrows escaping a value block and preserves
loans from earlier arguments while checking nested blocks.

## Ranges and collection loops

`for i in start..end` captures both integer bounds once and excludes end. Empty
and reversed ranges do not iterate. Loop bindings are fresh, and assigning them
does not change the counter. Ranges have no general value or iterator protocol.
Collection loops still bind shared `&T` elements or exclusive `&mut T` elements.
`for &value in values` and `for index, &value in values` explicitly copy a shared
element into a fresh `T` binding. `T` must satisfy the ordinary copyability rule;
owned arrays, structs, enums, and mutable references/slices are rejected. Assigning
the copied binding cannot mutate the source. The collection is evaluated once and
stays borrowed for the loop, and copied references retain their borrow dependencies.
The index remains `usize`. Reference patterns on integer ranges, on the index, or
on mutable iteration are rejected; use a shared reborrow to copy from a mutable
collection view. `&mut value` binding patterns are not supported.

## Slices and bounds

Subslices use `&data[start..end]` or `&mut data[start..end]`; bare slicing produces
a shared view. Omitted bounds default to zero and length. Source and bounds are
evaluated once and checked in every optimization profile. The captured source
is reserved while bounds run: they may read it, but cannot move or mutate it.
Exclusive access is established after the bounds. Indexed loans remain
conservative: separate ranges do not prove mutable slices disjoint.

## Numeric behavior

Signed integers use two's complement. `isize` and `usize` follow the selected
target's pointer width. Integer arithmetic and left shifts trap if the result
cannot be represented. Both division and remainder trap for a signed minimum
value and divisor `-1`, as well as for zero divisors. Shift counts must be
nonnegative and smaller than the value's bit width. Integer conversions check
range; they do not truncate or wrap. Float-to-integer conversion truncates toward
zero after checking the finite source against the destination's half-open range
before truncation (`-0.5 as u8` traps; `255.75 as u8` is 255).
Integer-to-float conversion rounds to the target floating representation.
Narrowing floats checks finite range, rejecting NaN and infinity. Floating
arithmetic follows LLVM's IEEE operations without fast-math flags. Constant
floating evaluation uses binary64 intermediates, including integer-to-f32
conversion, and can round differently from runtime conversion; the precise
implementation-defined choice is recorded as ID-FLOAT in
[specification Appendix C](language-spec-0.1.md#c1-required-implementation-profile).
There are no named wrapping operations yet.
