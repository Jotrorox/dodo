---
title: "Functions, structs, and enums"
description: "Define reusable behavior and data with functions, struct methods, enums, Options, and Results."
section: "Learn Dodo"
order: 40
---

Functions name a computation. Structs group named fields. Enums represent a choice
between alternatives. These are the main building blocks for Dodo programs.

## Functions declare their interface

```dodo test
package main

fn add(left: i32, right: i32) -> i32 {
    left + right
}

fn checked_add(left: i32, right: i32) -> i32 {
    if right == 0 {
        return left
    }
    add(left, right)
}

fn main() {
    core.assert_eq(checked_add(20, 22), 42i32)
}
```

Parameters use `name: Type`. The `-> Type` after the parameter list declares the
return type. Local types can be inferred, but function parameter and non-void
return types must be written explicitly.

The final expression supplies the return value if it has no trailing semicolon.
Use `return value` for an early exit or whenever it reads more clearly. An omitted
return type means `void`: the function does not return a value. `return` by itself
exits a void function.

Arguments are positional and evaluate left to right. Numbers and shared
references copy; passing an owned struct or array transfers ownership. To let a
function inspect or update a value without taking it, pass `&value` or
`&mut value`. [Ownership](ownership.md) explains these distinctions in detail.

## Structs group related values

```dodo test
package main

struct Point {
    x: i32
    y: i32
}

fn main() {
    x := 10i32
    point := Point { x, y: 20 }
    point.x += 2
    core.assert_eq(point.x + point.y, 32i32)
}
```

A struct literal names every field. `x` in a literal is shorthand for `x: x`.
Field expressions run in the order you write them. Fields do not receive implicit
default values; constructors are ordinary functions you define yourself.

Struct names and fields are private to their package unless marked `pub`.
Publishing a struct does not automatically publish its fields. This lets you
expose methods while keeping the representation private.

Structs always move, including a struct whose fields are all integers. Reading
`point.x` copies an integer, but extracting a move-only field with ordinary field
access is restricted. Use a borrow or an owned struct pattern when you need to
unpack an aggregate; see [patterns](patterns-and-results.md).

## Methods and associated functions

Declare methods inside the struct, alongside its fields:

```dodo test
package main

struct Counter {
    value: u32

    fn new(start: u32) -> Self {
        Counter { value: start }
    }

    fn current(&self) -> u32 {
        self.value
    }

    fn increment(&mut self) {
        self.value += 1
    }

    fn finish(self) -> u32 {
        self.value
    }
}

fn main() {
    counter := Counter.new(40)
    counter.increment()
    core.assert_eq(counter.current(), 41u32)
    core.assert_eq(counter.finish(), 41u32)
    // counter has moved into finish and cannot be read here.
}
```

| First parameter | Meaning at the call site |
| --- | --- |
| `&self` | Borrow the receiver for shared reading. |
| `&mut self` | Borrow it exclusively for mutation. |
| `self` | Transfer ownership into the method. |
| No receiver | Call an associated function as `Type.name(...)`. |

`counter.increment()` borrows automatically. A free function needs an explicit
borrow, such as `increment(&mut counter)`. Inside the struct, `Self` names the
struct's type, including its generic arguments. Method dispatch is static; there
is no inheritance, separate `impl` block, or method overloading.

A reserved `fn drop(&mut self)` method performs custom cleanup. You do not call
it directly, and it cannot return an error. See [destruction and cleanup](ownership.md#destruction-and-cleanup).

## Enums represent alternatives

An enum stores exactly one of its variants. Variants can carry values:

```dodo test
package main

enum Command {
    Stop
    Add(amount: i32)
    Pair(i32, i32)
}

fn evaluate(command: Command) -> i32 {
    match command {
        Command.Stop => 0,
        Command.Add(amount) => amount,
        Command.Pair(left, right) => left + right,
    }
}

fn main() {
    core.assert_eq(evaluate(Command.Add(42)), 42i32)
    core.assert_eq(evaluate(Command.Pair(20, 22)), 42i32)
}
```

The payload names in the declaration document their meaning; construction and
matching use positional payloads. `match` must account for every possible value.
Only the active variant owns and destroys its payload. Enum values move even
when they have no payload. Payload-free enums support `==` and `!=`; enums with
payloads do not have automatic aggregate equality.

Variant tags are assigned by declaration order. Explicit numeric discriminants
are not supported. The [memory reference](memory-and-ffi.md#enum-result-and-option-layout)
describes layout when you need to work at the ABI boundary.

## Option represents a possibly missing value

`Option<T>` is built in. `some(value)` contains a `T`; `none` contains no value.
Use it when absence is an ordinary outcome rather than a failure that requires an
error explanation.

```dodo test
package main

fn first(values: &[i32]) -> Option<i32> {
    if values.len == 0 {
        return none
    }
    some(values[0])
}

fn main() {
    values := [42i32]
    match first(&values) {
        some(value) => { core.assert_eq(value, 42i32) },
        none => { core.assert(false) },
    }
}
```

The return type supplies context for both constructors. `some(42i32)` can infer
its type on its own, but a standalone `none` needs context, such as
`missing: Option<i32> = none`. `none()` is also accepted. There is no null value
for checked references; use `Option<&T>` when a reference may be absent.

## Result represents success or failure

`Result<T, E>`, also written `T!E`, contains `ok(value)` or `err(error)`. A void
success uses `ok()`. Unlike an Option, a Result must be handled, propagated, or
returned. This ensures that a failed operation cannot disappear accidentally.

```dodo
enum DivideError { Zero }

fn divide(value: i32, divisor: i32) -> i32!DivideError {
    if divisor == 0 {
        return err(DivideError.Zero)
    }
    ok(value / divisor)
}
```

This helper handles a zero divisor. Ordinary checked arithmetic rules still apply
to the division, including the signed minimum divided by `-1`. The next chapter
explains [control flow](control-flow.md); [patterns and Results](patterns-and-results.md)
shows complete programs with matching, `?`, and postfix `!`.

## Remaining type forms

| Form | Purpose | Reference |
| --- | --- | --- |
| `[N]T`, `&[T]`, `&mut [T]`, `&str` | Arrays and borrowed views. | [Values and arrays](language-basics.md#arrays-own-their-elements) |
| `&T`, `&mut T` | Checked shared and exclusive references. | [Ownership](ownership.md) |
| `Name<T>` | A struct or enum specialized for a type. | [Generics](generics.md) |
| `*const T`, `*mut T` | Nullable raw pointers without checked lifetimes. | [Memory and foreign calls](memory-and-ffi.md) |
| `MaybeUninit<T>` | Opaque storage that may not yet contain a valid `T`. | [Memory intrinsics](memory-and-ffi.md#implemented-core-calls) |

There are no tuples, type aliases, trait objects, general function-value types,
closures, or user-defined implicit conversions in this implementation. Use named
struct fields and generic methods to express reusable data and behavior.
