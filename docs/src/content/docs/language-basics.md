---
title: "Values, variables, and arrays"
description: "Learn declarations, primitive types, arithmetic, strings, arrays, and slices with complete examples."
section: "Learn Dodo"
order: 30
---

After [running your first program](first-program.md), the next step is learning
how to represent data. Dodo checks the type of every value before your program
runs. A type describes both what a value contains and which operations are valid.

Examples marked as complete programs can be saved as `main.dodo` and checked with
`dodo check` or executed with `dodo run`. Smaller examples belong inside a function
unless they show a top-level declaration.

## A small complete program

```dodo test
package main

fn main() {
    let price: u32 = 12
    quantity := 2u32
    quantity += 1
    let total = price * quantity
    core.assert_eq(total, 36u32)
}
```

`package main` names the package. `fn main()` is the entry point. `let` introduces
a value you will not reassign. `:=` introduces a mutable variable and infers its
type from the initializer. `quantity += 1` adds one to its current value.
`core.assert_eq` checks the result; this program exits successfully without output.

Newlines separate statements. Braces delimit blocks. Parentheses belong around
function arguments, but are optional around an `if` condition. A `//` comment
continues to the end of its line.

## Choose the declaration you need

| Declaration | Meaning |
| --- | --- |
| `let limit = 10u32` | Immutable runtime binding with inferred type. |
| `let limit: u32 = 10` | Immutable runtime binding with explicit type. |
| `count := 0u32` | Mutable runtime binding with inferred type. |
| `count: u32 = 0` | Mutable runtime binding with explicit type. |
| `const CAPACITY: usize = 16` | Compile-time constant, usable as an array length. |

Use `let` when a value will not change, and `:=` when it will. An ordinary typed
declaration is mutable even though it does not contain the word `mut`. Dodo does
not use `let mut` for mutable local variables. `const` is different from `let`:
its initializer must be computable during compilation, without ordinary function
calls. Constants can appear at package scope or inside functions.

A `let` always needs an initializer. A typed mutable local can be declared first
and initialized later, but the compiler rejects every read that could occur before
initialization. Types are fixed after declaration: an `i32` variable cannot later
hold a string.

## Numbers and Booleans

| Types | Use |
| --- | --- |
| `i8`, `i16`, `i32`, `i64` | Signed integers with the named bit width. |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers with the named bit width. |
| `isize`, `usize` | Signed and unsigned integers matching the target pointer width. |
| `f32`, `f64` | 32-bit and 64-bit floating-point numbers. |
| `bool` | Exactly `true` or `false`. |

For example, `u8` holds 0 through 255, and `i8` holds -128 through 127. Collection
lengths and indices normally use `usize`. It follows the selected compilation
target, including when you cross-compile.

```dodo
let decimal = 1_000u32
let hex = 0xffu8
let binary = 0b1010u8
let octal = 0o755u16
let ratio = 0.5f32
let large = 1.25e3
let ready = true
```

A suffix fixes the literal's type. Otherwise context supplies the type when
possible; an unconstrained integer defaults to `isize`, and an unconstrained
floating literal defaults to `f64`. Underscores separate digits for readability.
There are no implicit conversions between already typed numeric values:

```dodo
small := 20u8
large := 22u32
total := small as u32 + large
```

`as` explicitly converts the value. Integer conversions check the destination's
range: `300u32 as u8` fails rather than keeping the low eight bits. Ordinary
integer arithmetic also checks overflow in every optimization level. Use
`core.wrapping_add`, `core.wrapping_sub`, or `core.wrapping_mul` when wrapping is
intentional. See [numeric behavior](implementation-syntax.md#numeric-behavior)
for division, shifts, and floating-point conversion details.

Arithmetic uses `+`, `-`, `*`, `/`, and `%`. Comparisons such as `==`, `!=`, and
`<` produce `bool`. Combine conditions with `&&`, `||`, and prefix `!`.
Integers are not truthy: write `count != 0`, not `if count`.

## Strings and bytes

`"hello"` has type `&str`: a borrowed view of valid UTF-8 text. A literal's bytes
live for the entire program, so creating the view allocates nothing.

```dodo test
package main

fn main() {
    let greeting = "Hello, Dodo!"
    let letter = b'A'
    let bytes = b"ABC"
    core.assert_eq(greeting.len, 12usize)
    core.assert_eq(letter, 65u8)
    core.assert_eq(bytes[1], b'B')
    core.assert_eq("é".len, 2usize)
}
```

`.len` counts bytes, including for UTF-8 text. String indexing is unavailable
because one character can span several bytes. `b'A'` is one `u8` byte;
`b"ABC"` is a shared byte slice, `&[u8]`. Ordinary character literals such as
`'A'` are not supported.

Use escapes such as `\n`, `\t`, `\"`, `\\`, `\0`, and `\xHH` in literals.
Strings also allow Unicode escapes such as `"\u{1F426}"`. Byte literals use ASCII
source characters or hexadecimal byte escapes. Literal strings are not guaranteed
to have a trailing NUL for C APIs. Growing text, character-aware operations, and
UTF-8 validation are covered in [text](text.md).

## Arrays own their elements

An array has a fixed length that is part of its type. `[3]i32` means exactly
three `i32` values; it is distinct from `[4]i32`.

```dodo test
package main

fn main() {
    scores: [3]i32 = [10, 20, 30]
    scores[1] = 25
    core.assert_eq(scores.len, 3usize)
    core.assert_eq(scores[1], 25i32)

    const CAPACITY: usize = 4
    bytes := [0u8; CAPACITY]
    core.assert_eq(bytes[3], 0u8)
}
```

Index zero is the first element. An out-of-bounds access traps. `.len` is a
property, so use `scores.len`, not `scores.len()`. `[value; count]` fills an array
by evaluating `value` once and repeating it. The element must be copyable; this
does not clone structs or mutable references. An empty array needs type context,
for example `empty: [0]u8 = []`.

## Slices borrow a region

A slice describes an existing sequence using a pointer and a length. It does not
copy elements or own the underlying allocation. `&[T]` grants shared read access;
`&mut [T]` grants exclusive read/write access.

```dodo test
package main

fn sum(values: &[i32]) -> i32 {
    total := 0i32
    for &value in values {
        total += value
    }
    total
}

fn main() {
    numbers := [10i32, 20, 30, 40]
    let middle = &numbers[1..3]
    core.assert_eq(sum(middle), 50i32)
    let writable = &mut numbers[..2]
    writable[0] = 5
    core.assert_eq(numbers[0], 5i32)
}
```

`start..end` includes `start` and excludes `end`. An omitted start means zero;
an omitted end means the source length. A complete view is `&numbers[..]`, and
`&numbers` also converts to a slice where the expected type is `&[i32]`.
The compiler ends the shared borrow after `sum(middle)`, allowing the later
mutable borrow. The `let` binding prevents replacing `writable`, but its
`&mut [i32]` type still permits writes to the borrowed elements.

Continue with [types and functions](types-and-functions.md), then read
[ownership and borrowing](ownership.md) before building structures that retain
references.
