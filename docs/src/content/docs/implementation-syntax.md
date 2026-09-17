---
title: "Syntax and expressions"
description: "Implemented syntax, inference, constants, control flow, and numeric behavior in Dodo 0.1.3."
section: "Language reference"
order: 210
---

This is the compact reference for syntax accepted by Dodo 0.1.3. New to the
language? Begin with [values and arrays](language-basics.md),
[functions and data types](types-and-functions.md), and [control flow](control-flow.md).
The [language specification](language-spec-0.1.md) states the normative rules;
the examples and limits here describe the current compiler.

## Source-file structure

```dodo test
package main

import "core/slice"

const LIMIT: usize = 3

struct Sample { value: i32 }

fn main() {
    values := [10i32, 20, 30]
    core.assert_eq(values.len, LIMIT)
    match slice.get(&values, 1) {
        some(value) => { core.assert_eq(*value, 20i32) },
        none => { core.assert(false) },
    }
}
```

Every source file starts with `package name`, apart from comments and separators.
Top-level declarations are `fn`, `struct`, `enum`, `const`, and `static`; imports
make another package's public names available through a qualifier. There is no
top-level executable statement or automatic `init` function. See
[projects and imports](packages.md) for file discovery and visibility.

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

Block comments, raw strings, ordinary character literals, Unicode identifiers,
and a UTF-8 byte-order mark are unsupported. `///` is an ordinary line comment.
Reserved keywords are:

```text
package import pub fn struct enum const let return if else for in
break continue match unsafe extern as from static void mut true false
```

`self`, `Self`, primitive type names, and constructor names have contextual
meanings. `stores` and `requires_plain` introduce specialized function contracts
described in [container element safety](container-elements.md#mutation-and-return-contracts).

## Literals and strings

Integer literals support decimal, `0x`, `0o`, `0b`, digit separators, and type
suffixes. Unsuffixed integer literals use contextual types and otherwise default
to `isize`. Floating literals use decimal fractions/exponents and optional
`f32`/`f64` suffixes; their default is `f64`. Boolean literals are `true` and
`false`. Strings and byte strings have immutable program-lifetime storage.
Strings support the escapes listed below; `&str.len` counts UTF-8 bytes. String
indexing is rejected; use a byte slice for byte indexing. There is no implicit
string allocation or mandatory NUL terminator.

| Literal | Example | Type or interpretation |
| --- | --- | --- |
| Integer | `42`, `42u32`, `0xffu8`, `0o755`, `0b1010` | Contextual integer; default `isize`. |
| Float | `1.5`, `1e3`, `2f32` | Contextual float; default `f64`. |
| Boolean | `true`, `false` | `bool`; no numeric truthiness. |
| Byte | `b'A'`, `b'\xFF'` | Exactly one `u8`. |
| Byte string | `b"ABC"` | Shared `&[u8]` with program-lifetime bytes. |
| String | `"Dodo"`, `"\u{1F426}"` | `&str`, valid UTF-8. |

All quoted literals accept `\n`, `\r`, `\t`, `\0`, `\\`, `\"`, `\'`, and
`\xHH`. Only strings accept `\u{...}` with one to six hexadecimal digits for a
Unicode scalar value. Byte literals and byte strings require ASCII source
characters or byte escapes. Physical newlines cannot appear inside a literal.
Integer literal magnitude must fit `u64`; `-` is a separate prefix operation.

## Type syntax

| Syntax | Meaning |
| --- | --- |
| `bool` | Boolean value. |
| `i8`, `i16`, `i32`, `i64`; `u8`, `u16`, `u32`, `u64` | Fixed-width signed and unsigned integers. |
| `isize`, `usize` | Target-pointer-sized integers. |
| `f32`, `f64` | IEEE binary floating-point values. |
| `void` | No return value. |
| `[N]T` | An owned fixed-length array. |
| `&T`, `&mut T` | Checked shared or exclusive reference. |
| `&[T]`, `&mut [T]` | Shared or exclusive slice. |
| `&str` | Shared UTF-8 string view. |
| `*const T`, `*mut T` | Nullable raw pointer. |
| `Name`, `package.Name`, `Name<T, U>` | User-defined struct or enum type. |
| `Option<T>` | `some(T)` or `none`. |
| `Result<T, E>`, `T!E` | `ok(T)` or `err(E)`. |
| `MaybeUninit<T>` | Opaque storage for potentially uninitialized `T`. |

`Self` is valid inside a struct declaration and names its current type. Result
shorthand binds outside references and slices: `&T!E` means `Result<&T, E>`.
Use the full spelling to make nested type relationships clear. Tuples, type
aliases, general function-value types, and trait objects are not implemented.

## Operators and evaluation order

From weakest to strongest:

| Precedence | Operators |
| --- | --- |
| 1 | `\|\|` |
| 2 | `&&` |
| 3 | `\|` |
| 4 | `^` |
| 5 | `&` |
| 6 | `==`, `!=` |
| 7 | `<`, `<=`, `>`, `>=` |
| 8 | `<<`, `>>` |
| 9 | `+`, `-` |
| 10 | `*`, `/`, `%` |
| 11 | `as Type` |
| 12 | Prefix `-`, `!`, `~`, `*`, `&`, `&mut` |
| 13 | Calls, `.field`, `[index]`, subslices, postfix `?` and `!` |

Parentheses override precedence. Prefix `!` negates a Boolean; postfix `!`
unwraps a Result or panics. `&` is a shared borrow in prefix position and integer
bitwise AND in binary position. Prefix `*` dereferences; binary `*` multiplies.
Binary operators associate left. Operands, call arguments, and literal fields
are evaluated left to right; `&&` and `||` short-circuit. Compound assignment
evaluates the destination address once, then its previous value, then the right
operand. Numeric operands must have compatible types; there are no implicit
mixed-width conversions between already typed values.

`=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, and `>>=` are
statements, not expressions. Comparisons do not chain mathematically: use
`low <= value && value < high`. Equality supports primitive scalars, raw pointers,
and payload-free enums; arbitrary aggregate equality is unavailable. Operators
cannot be overloaded.

## Declarations and returns

```dodo
const LIMIT: usize = 16

struct Point {
    x: i32
    y: i32

    fn magnitude_squared(&self) -> i32 {
        self.x * self.x + self.y * self.y
    }
}

enum Message { End, Count(value: u32), Pair(u8, u8) }

fn add(left: i32, right: i32) -> i32 {
    left + right
}
```

Canonical bindings, fields, constants, and parameters use `name: Type`. The earlier
type-first forms remain supported for locals, fields, constants, and enum
payloads. An omitted function return type means `void`. Struct receivers accept
`self`, `&self`, and `&mut self`; field literals accept same-name shorthand.
Non-void functions return their final expression unless it ends with a semicolon;
final conditionals, matches, and blocks follow the same rule. Explicit `return`
provides early exits. Void functions retain statement semantics.

All non-void parameter and return types are declared; inference does not derive
function signatures. Struct literals use named fields, enum payloads are
positional, and function arguments are positional. Methods live inside structs;
associated functions omit a receiver and use `Type.name(...)`. See
[functions, structs, and enums](types-and-functions.md) for complete examples.

## Mutable and immutable bindings

| Form | Mutable binding? | Initialization |
| --- | --- | --- |
| `let value = expression` | No | Required; inferred type. |
| `let value: T = expression` | No | Required; explicit type. |
| `value := expression` | Yes | Required; inferred type. |
| `value: T = expression` | Yes | Explicit type. |
| `value: T` | Yes | Must initialize on every path before reading. |
| `const VALUE: T = expression` | No | Compile-time expression required. |

`let name = value` and `let name: Type = value` create immutable runtime bindings.
`:=` and ordinary typed locals remain mutable; `const` remains compile-time-only.
Immutability prevents reassignment, mutable borrowing of owned storage, and writes
to owned fields/elements. It does not weaken a stored `&mut T` or mutable slice:
writes through those references remain permitted, including field/index writes.

A moved-from binding is uninitialized for reading until it is reinitialized.
See [ownership and borrowing](ownership.md) for moves, loans, and cleanup.

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

The [generics tutorial](generics.md) explains generic structs, associated
functions, and structural method protocols with executable examples.

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

Pattern expansion has a separate limit of 4,096 alternatives per pattern and
131,072 coverage work units. See [patterns](patterns-and-results.md#match-patterns-and-guards)
for coverage semantics.

## Value-producing blocks

`if`, `match`, `unsafe`, and plain blocks produce values in expression positions.
Every continuing path must yield the same type; at least one value-producing
path is required. Results are transferred before local cleanup. Explicit returns,
propagation, break, and continue retain their surrounding control-flow meaning.
The checker rejects local-storage borrows escaping a value block and preserves
loans from earlier arguments while checking nested blocks.

## Ranges and collection loops

`for` is the only loop keyword. The statement forms are `for { ... }`,
`for condition { ... }`, `for init; condition; step { ... }`, and
`for item in collection_or_range { ... }`. `break` and `continue` target the
nearest enclosing loop. There are no labeled loops or loop result values.
See [decisions and loops](control-flow.md) for examples of every form.

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
Integer-to-float conversion rounds directly to the target floating representation,
with identical results for constant and runtime conversions.
Narrowing floats checks finite range, rejecting NaN and infinity. Floating
arithmetic follows LLVM's IEEE operations without fast-math flags. Constant
floating arithmetic uses binary64 intermediates and rounds to the expression's
type at each node. Integer-to-f32 casts round directly to binary32; an explicit
cast through f64 still performs both conversions. Decimal literals are parsed
through binary64. These implementation-defined choices are recorded as ID-FLOAT in
[specification Appendix C](language-spec-0.1.md#c1-required-implementation-profile).

Explicit wrapping operations are `core.wrapping_add(a, b)`,
`core.wrapping_sub(a, b)`, and `core.wrapping_mul(a, b)`, available without an
import. They wrap modulo the operand width, interpreting signed results as
two's complement. Both arguments and the result have the same integer type;
type inference or `::<T>` selects it. Argument expressions still use their
ordinary checked arithmetic. Wrapping calls are not constant expressions.

```dodo test
package main

fn main() {
    core.assert_eq(core.wrapping_add(255u8, 1), 0u8)
    core.assert_eq(core.wrapping_sub::<u8>(0, 1), 255u8)
    core.assert_eq(core.wrapping_mul(127i8, 2), -2i8)
}
```

Floating-point overflow and division by zero can produce infinity or NaN;
integer traps do not apply to these operations. With NaN, `!=` is true and the
other comparisons are false. Dodo does not provide numeric casts to or from
`bool` or enum tags. See [math](math.md) for numerical algorithms and functions.

## Static storage

Package-level `static NAME: T = expression` declares immutable storage, and
`static mut NAME: T = expression` declares mutable storage. Both initializers
must be constant expressions. Static storage lasts for the program and is not
automatically destroyed at exit. A read or write of mutable static storage
requires an explicit `unsafe` block; the programmer must prevent races and
conflicting aliases. Prefer passing state explicitly or using
[synchronization](synchronization.md) for shared threaded state.

## Attributes and unsafe boundaries

| Attribute | Placement and purpose |
| --- | --- |
| `@repr(C)` | Struct: C-compatible field order, alignment, and padding. |
| `@unsafe_send`, `@unsafe_sync` | Struct: explicit unsafe thread transfer/sharing contract; see [threads](threads.md). |
| `@test` | Safe nongeneric top-level void function with no parameters; see [testing](testing.md). |
| `@ignore("reason")` | Test function: skipped unless ignored tests are selected. |
| `@derive(Json)` | Struct: generate supported JSON methods; see [JSON](json.md). |
| `@json_name("name")` | Field of a JSON-derived struct: select its external key. |
| `@json_deny_unknown` | JSON-derived struct: reject unknown fields. |

`@compiler(print)`, `@compiler(println)`, and `@compiler(printf)` are reserved for
bundled printing declarations; user packages cannot define them. Other unknown
attributes are errors. There is no general macro or attribute-extension system.

`unsafe { ... }` permits specific unchecked operations; it does not turn off
type checking, borrow checking, or checked indexing. An `unsafe fn` requires
callers to satisfy its documented preconditions, and its body still needs
explicit unsafe blocks around unchecked operations. `extern "C" fn` declares or
defines a C-ABI function. The complete rules and examples are in
[memory and foreign calls](memory-and-ffi.md).
