---
title: "Generics and reusable behavior"
description: "Write type-parameterized functions and structs, infer arguments, and understand static method protocols."
section: "Learn Dodo"
order: 60
---

A generic function or type works with more than one concrete type. Write type
parameters in angle brackets, then use those names wherever a type is expected.
The compiler specializes generic code for the concrete types your program uses
and checks each specialization.

## A generic function

```dodo test
package main

fn identity<T>(value: T) -> T {
    value
}

fn main() {
    core.assert_eq(identity(42i32), 42i32)
    core.assert_eq(identity::<u8>(7), 7u8)
    let answer: u32 = identity(42)
    core.assert_eq(answer, 42u32)
}
```

`T` is a type parameter. `identity(42i32)` infers `T = i32` from the argument.
`identity::<u8>(7)` provides it explicitly. The expected result type in
`let answer: u32 = ...` also guides inference. Generic calls canonically use
`::<...>`; generic declarations and type names use `<...>` without `::`.

Inference uses the current expression and its expected type. It does not infer a
function's public signature, or look ahead to later uses of a variable. If a
constructor has no argument that determines `T`, provide explicit arguments or
an expected result type. Conflicting type information is an error.

## Generic structs and associated functions

```dodo test
package main

struct Slot<T> {
    value: T

    fn new(value: T) -> Self {
        Slot<T> { value }
    }

    fn get(&self) -> &T {
        &self.value
    }
}

fn main() {
    inferred := Slot { value: 42i32 }
    explicit := Slot.new::<u8>(7)
    core.assert_eq(*inferred.get(), 42i32)
    core.assert_eq(*explicit.get(), 7u8)
}
```

The literal infers its type from the field initializer. `Slot<u8>` is a concrete
type in a type annotation; an explicit associated function call puts its type
arguments on the function: `Slot.new::<u8>(...)`. `Self` inside a generic struct
means that struct with its current arguments.

Enums can also declare type parameters, for example
`enum Choice<T> { Empty, Value(T) }`. Different specializations are distinct types:
`Slot<u8>` cannot be assigned to `Slot<u32>` by an implicit conversion.

## Operations are checked for each concrete type

Generic code does not erase types or bypass ownership checks. A generic function
that applies `+` can only be used with types for which that operation exists.
An operation that copies `T` only works when the concrete `T` is copyable.
Likewise, a collection's element restrictions still apply to every specialization.

For example, this is a useful numeric helper:

```dodo test
package main

fn add<T>(left: T, right: T) -> T {
    left + right
}

fn main() {
    core.assert_eq(add(20u32, 22u32), 42u32)
    core.assert_eq(add(1.25f64, 0.75f64), 2.0f64)
}
```

It does not make addition available for arbitrary structs. Dodo has no traits,
general constraint declarations, overload resolution, or specialization.

## Reusable behavior through public methods

Many standard-library APIs accept a generic object with an expected method.
The concrete object supplies that method, and compilation checks the call:

```dodo test
package main

struct Doubler {
    pub fn apply(&self, value: i32) -> i32 {
        value * 2
    }
}

fn transform<F>(operation: &F, value: i32) -> i32 {
    operation.apply(value)
}

fn main() {
    doubler := Doubler {}
    core.assert_eq(transform(&doubler, 21), 42i32)
}
```

This is a statically checked method protocol. There is no runtime interface
object or implicit allocation. A private struct may expose public methods for
this purpose without publishing its type name. Across package boundaries the
called method must be public.

The standard library uses this pattern for readers and writers, formatters,
allocators, hash/equality policies, and callbacks. Read each API's required
signature carefully; a similar method name with a different receiver or return
type does not satisfy it. Closures and general first-class function values are
unavailable; use an object with fields to carry callback state.

## Generic ownership contracts

Returning borrowed data still requires a valid source. `from(value)` on a generic
function refers to the concrete argument's dependencies: a borrow-free
specialization contributes none, while a borrow-carrying specialization retains
its sources. A type parameter never permits returning a reference to destroyed
local storage.

Allocated container methods may use the narrow `requires_plain(T, ...)` contract
to restrict mutation to borrow-free, Result-free payloads. `stores(...)` tracks
sources deposited into typed storage. These are checked storage effects, not
general trait bounds; their syntax and rules are in
[container element safety](container-elements.md#mutation-and-return-contracts).

All used generic bodies must be available in the loaded source packages. There
are no separately compiled generic interfaces. Recursive expansion is bounded;
exceeding the compiler's limit produces a diagnostic instead of unbounded
compilation. For concrete language restrictions and limits, see
[compiler support](implementation.md).
