---
title: "Decisions and loops"
description: "Use Boolean conditions, value-producing blocks, the four for-loop forms, and early exits."
section: "Learn Dodo"
order: 50
---

Dodo uses `if` for decisions, `match` for pattern selection, and `for` for all
loops. Braces are required around bodies. Conditions must have type `bool`.

## If and else

```dodo test
package main

fn sign(value: i32) -> i32 {
    if value < 0 {
        -1
    } else if value == 0 {
        0
    } else {
        1
    }
}

fn main() {
    core.assert_eq(sign(-7), -1i32)
    core.assert_eq(sign(0), 0i32)
    let limit = if sign(7) > 0 { 100u32 } else { 10u32 }
    core.assert_eq(limit, 100u32)
}
```

Only the selected branch runs. In statement position, an `else` is optional.
When an `if` produces a value, every path that continues must produce the same
type. A branch may instead leave the function with `return` or `?`.

`&&` and `||` short-circuit, so conditions can safely guard a later operation:

```dodo
if values.len > 0 && values[0] == 42 {
    // The index is evaluated only when the slice is nonempty.
}
```

## Blocks can produce values

A block's final expression can provide a result in expression position:

```dodo
let total = {
    let subtotal = 20i32 + 20
    subtotal + 2
}
```

`subtotal` exists only inside the block. The block transfers its result before
destroying its locals. A trailing semicolon turns an expression into a statement,
so `subtotal + 2;` would not produce the required value. The same final-expression
rule applies to functions, `if`, `match`, and `unsafe` blocks. A reference to a
block-local value cannot escape the block.

## Range loops

Use a range to count through integers:

```dodo test
package main

fn main() {
    total := 0i32
    for number in 1i32..5i32 {
        total += number
    }
    core.assert_eq(total, 10i32)
}
```

The upper bound is excluded: this visits 1, 2, 3, and 4. Bounds are evaluated
once before the loop. An empty or reversed range performs no iterations. The
binding has the bounds' integer type, and reassigning it does not alter the loop
counter. Use `_` when you only need repetition, such as `for _ in 0..3`.

Inclusive `..=` ranges are supported in patterns, but not in range loops.
Ranges are syntax for loops and slices, not general values or iterators.

## Collection loops

Arrays and slices support these forms:

| Form | Element binding |
| --- | --- |
| `for value in values` | A shared `&T` reference. |
| `for index, value in values` | `usize` index and shared `&T` reference. |
| `for &value in values` | A copied `T`, which must be copyable. |
| `for index, &value in values` | `usize` index and copied `T`. |
| `for value in &mut values` | An exclusive `&mut T` reference. |
| `for index, value in &mut values` | `usize` index and exclusive `&mut T`. |

```dodo test
package main

fn main() {
    values := [1i32, 2, 3]
    for value in &mut values {
        *value *= 2
    }
    total := 0i32
    for index, &value in values {
        total += (index as i32 + 1) * value
    }
    core.assert_eq(total, 28i32)
}
```

`*value` dereferences the mutable reference. A copied binding lets arithmetic use
`value` directly. Copy iteration works for scalars and shared references, but
does not clone structs, arrays, enums, or mutable references.

The collection expression is evaluated once and stays borrowed for the loop.
Iteration does not consume the collection. A mutable element borrow cannot be
retained beyond its iteration. To traverse a library collection, use the slice
or cursor API documented by that [collection](collections.md); there is no
user-definable iterator protocol.

## Condition, three-part, and infinite loops

```dodo test
package main

fn main() {
    count := 0i32
    for count < 3 {
        count += 1
    }
    core.assert_eq(count, 3i32)

    sum := 0i32
    for i := 0i32; i < 5; i += 1 {
        if i == 2 {
            continue
        }
        sum += i
    }
    core.assert_eq(sum, 8i32)

    for {
        count -= 1
        if count == 0 {
            break
        }
    }
}
```

`for condition` tests the condition before each iteration. The three-part form
runs an initializer once, tests a condition, runs the body, then runs its step.
`continue` skips to the next iteration, including the step in a three-part loop.
`for { ... }` repeats until a `break`, return, or other exit.

`break` and `continue` target the nearest enclosing loop. Locals receive ordinary
cleanup when their scopes are exited. Dodo has no labeled loops, loop result
values, `while`, `loop`, or `do` statement.

## Match and conditional patterns

`match` selects the first matching arm and requires complete coverage. `if let`
selects a branch by pattern, and `let pattern = value else { ... }` unpacks a
value or exits early. Continue to [patterns and Results](patterns-and-results.md)
for enum matching, nested patterns, guards, and error handling.
