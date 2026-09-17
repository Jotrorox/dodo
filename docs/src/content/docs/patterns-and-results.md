---
title: "Patterns and Results"
description: "Handle errors with Result, propagate with ?, unwrap with !, and unpack values using exhaustive patterns."
section: "Learn Dodo"
order: 56
---

Dodo represents a recoverable failure as an ordinary value: `Result<T, E>`, also
written `T!E`. `T` is the success type and `E` is the error type. A Result contains
exactly one of `ok(value)` or `err(error)`. Error types are usually enums, but do
not need to inherit from a special error class.

Patterns describe which alternative you expect and which values you want to
bind. They are used in `match`, `if let`, and destructuring `let`. This chapter
starts with error handling, then explains the full pattern vocabulary.

## Return and match a Result

```dodo test
package main

enum ParseError { NotADigit }

fn digit(byte: u8) -> u32!ParseError {
    if byte < b'0' || byte > b'9' {
        return err(ParseError.NotADigit)
    }
    ok((byte - b'0') as u32)
}

fn main() {
    match digit(b'7') {
        ok(value) => { core.assert_eq(value, 7u32) },
        err(ParseError.NotADigit) => { core.assert(false) },
    }
    match digit(b'x') {
        ok(_) => { core.assert(false) },
        err(_) => {},
    }
}
```

The first match extracts the success value. The second explicitly accepts an
error, so its empty error arm is intentional handling. `match` has no fallthrough
and must cover every possible input. Arms can contain an expression or a block.
The success and error constructors use the expected return type to infer the
complete Result type; a standalone local may need an annotation such as
`outcome: u32!ParseError = ok(7)`.

## Propagate with `?`

Postfix `?` obtains the success value or immediately returns the error from the
enclosing function. It lets the caller decide what to do about failure:

```dodo test
package main

enum ParseError { NotADigit }

fn digit(byte: u8) -> u32!ParseError {
    if byte < b'0' || byte > b'9' {
        return err(ParseError.NotADigit)
    }
    ok((byte - b'0') as u32)
}

fn pair(high: u8, low: u8) -> u32!ParseError {
    let tens = digit(high)?
    let units = digit(low)?
    ok(tens * 10 + units)
}

fn main() {
    match pair(b'4', b'2') {
        ok(value) => { core.assert_eq(value, 42u32) },
        err(_) => { core.assert(false) },
    }
}
```

If the first digit fails, the second call never executes. Locals already created
receive normal cleanup. The enclosing function must return a Result with the
**same error type**; Dodo does not insert error conversions. To translate errors,
match the original error and construct a new error explicitly.

A fallible operation with no success payload returns `void!E` and succeeds with
`ok()`. `operation()?` performs it and continues without binding a value.
`?` applies to Results, not to Options.

## Handle every Result

A `Result` must be forwarded, propagated with `?`, unwrapped with `!`, or matched
with explicit `ok` and `err` arms. Binding it and leaving scope, overwriting it
unhandled, assigning it to `_`, or passing it to `core.drop` is rejected.

| Intent | Write |
| --- | --- |
| Let the caller handle the same Result | `return operation()` |
| Continue on success and return failure | `value := operation()?` |
| Recover locally or translate errors | `match operation() { ok(value) => ..., err(error) => ... }` |
| Treat failure as a terminating panic | `value := operation()!` |

Moving a Result into a new binding transfers the obligation; it does not handle
it. This also applies to nested Results in structs, arrays, enums, and Options.
A wildcard cannot silently discard a pending nested Result. A plain `Option<T>`
does not carry a mandatory handling obligation unless its payload contains one.

After matching by reference, replacing the whole owned binding creates a fresh
handling obligation. Assignments of Result-containing values through a field,
index, or reference are unsupported, including writes through local aliases:
the checker cannot transfer their handling state to the storage owner. Matching
plain payloads through `&mut` still permits mutation of non-Result data. The
[container element design](container-elements.md) explains why storing and
destroying pending Results needs additional checker support.

## Unwrap or panic with `!`

Postfix `!` evaluates a Result once and produces its success value. On error it
panics at the expression's source location using the configured panic handler.
It works in any function, including `fn main()`:

```dodo test
package main

import "std/console"

fn main() {
    console.println("Hello, world!")!
}
```

`?` instead returns the error from the enclosing function, which must return a
Result with the same error type. `!` never propagates: panic terminates execution
without unwinding or running destructors. On success, `!` consumes the Result
and preserves the payload's ownership and borrow dependencies. A nested Result
still needs handling. A `void` success payload produces no value.

Postfix `!` has the same precedence as `?` and supports chaining, for example
`nested()!!` or `read()!.field` when the payload is a reference. Bind an owned
array or struct payload to a local before indexing or accessing its fields.
Prefix `!flag` remains Boolean negation; `T!E` remains Result type shorthand.

## Match patterns and guards

The following program combines integer ranges, alternatives, a binding with a
guard, and a final wildcard:

```dodo test
package main

fn category(value: u32) -> u32 {
    match value {
        0..=9 => 1,
        10 | 20 => 2,
        number if number < 100 => 3,
        _ => 4,
    }
}

fn main() {
    core.assert_eq(category(7), 1u32)
    core.assert_eq(category(20), 2u32)
    core.assert_eq(category(42), 3u32)
    core.assert_eq(category(100), 4u32)
}
```

| Pattern | Meaning |
| --- | --- |
| `value` | Bind the matched value. |
| `_` | Ignore a value, subject to Result handling rules. |
| `true`, `false`, `42`, `b'A'` | Match a Boolean or integer literal. |
| `0..10`, `b'a'..=b'z'` | Match an exclusive or inclusive integer literal range. |
| `some(value)`, `none`, `ok(value)`, `err(error)` | Match built-in tagged alternatives. |
| `Command.Add(value)` | Match a user-defined enum variant and its payload. |
| `Point { x, y: vertical }` | Bind struct fields, optionally renaming them. |
| `Point { x, .. }` | Bind selected fields and ignore the remainder. |
| `pattern_a | pattern_b` | Match either alternative. |

Range endpoints must be integer literals, fit the input type, and form a nonempty
ordered range. Strings, floats, tuple patterns, and array/slice destructuring
patterns are unavailable. Enum qualifiers can be omitted when the input type
identifies the variant, but qualified names are often clearer.

Patterns nest recursively, as in `some(some(value))` or a struct field containing
an enum pattern. Struct patterns list every field or use `..` for the remainder.
Alternatives must introduce the
same bindings with compatible types and borrow modes. Pattern tests and guards
run in source order, retrying alternatives after a false guard. Ownership
transfers and destruction occur only after the guard succeeds. The current
checker conservatively rejects moves of non-copy values, assignment, and mutable
borrows anywhere in a guard, including operations on unrelated local storage.
Shared borrows and observer calls are permitted. Guarded arms do not count toward
coverage. Existing match-arm bindings remain mutable locals; names introduced by
`let` and `if let` are [immutable bindings](implementation-syntax.md#mutable-and-immutable-bindings)
that retain the permissions of any references they hold.
Exhaustiveness checks preserve correlations between nested fields and partition
integer ranges by their endpoints. Each pattern may expand to at most 4,096
alternatives; coverage checking is limited to 131,072 work units. Excessive
patterns produce a diagnostic.

## Conditional and destructuring bindings

Use `if let` when you want to act only on one ordinary optional state:

```dodo test
package main

fn main() {
    optional := some(42i32)
    if let some(value) = optional {
        core.assert_eq(value, 42i32)
    } else {
        core.assert(false)
    }
}
```

Use an early-exit `let` to keep the successful path at the surrounding indentation:

```dodo test
package main

struct Reading { value: Option<i32>, valid: bool }

fn usable(reading: Reading) -> i32 {
    let Reading { value: some(value), valid: true } = reading else {
        return 0
    }
    value
}

fn main() {
    core.assert_eq(usable(Reading { value: some(42i32), valid: true }), 42i32)
    core.assert_eq(usable(Reading { value: none, valid: true }), 0i32)
}
```

The `else` must leave the current path: use `return`, `break`, `continue`, or a
nonterminating loop as appropriate. If the pattern cannot fail, no `else` is
needed, for example `let Point { x, y } = point`. Names in an `if let` exist only
inside its success block; names in a destructuring `let` are available afterward.

Both forms introduce immutable bindings and consume owned scrutinees on success or failure
and borrow reference scrutinees. Conditional patterns must cover every state
whose active payload contains a Result, including borrowed values. For example,
`some(result)` may match `Option<Result<T, E>>` if the bound result is handled;
its unmatched `none` path has no obligation. Success-only `ok` patterns and
ignored nested Results are rejected. Borrowed patterns preserve shared/mutable
permissions recursively, including separate loans for disjoint struct fields;
owned patterns cannot destructure structs with custom `drop`.

## Match by reference to keep the owner

Matching an owned enum, struct, Option, or Result consumes it. Match `&value` to
inspect it without taking ownership, or `&mut value` to borrow its payloads for
mutation:

```dodo test
package main

fn main() {
    optional := some(40i32)
    match &mut optional {
        some(value) => { *value += 2 },
        none => {},
    }
    match &optional {
        some(value) => { core.assert_eq(*value, 42i32) },
        none => { core.assert(false) },
    }
}
```

The pattern syntax stays the same; the scrutinee's shared or mutable reference
determines the bindings' access. Permissions propagate recursively through
nested payloads. An ignored owned payload is destroyed exactly once after its
arm is selected. Guards run before ownership transfers, so a failed guard cannot
consume the value needed by later arms.

See [ownership](ownership.md) for lifetime and partial-move restrictions, and
the repository's `examples/patterns.dodo` and `examples/hex.dodo` for larger
examples with nested patterns and parsing.
