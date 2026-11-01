---
title: "Generics and reusable behavior"
description: "Write type-parameterized functions and structs, constrain them with interfaces, and dispatch through dyn references."
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

It does not make addition available for arbitrary structs. Operators stay
compiler-defined; there is no operator overloading, overload resolution, or
specialization.

## Reusable behavior through public methods

A generic function may call methods on its type parameter. The concrete
object supplies that method, and compilation checks the call:

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

Such an unconstrained parameter accepts any type, and a type without `apply`
is only reported inside `transform`'s specialized body. A private struct may
expose public methods for this purpose without publishing its type name. Across
package boundaries the called method must be public. A method object can carry
state that outlives the call; for a one-off callback, a
[closure](types-and-functions.md#functions-are-values-and-closures-capture-locals)
borrowing local variables is often shorter.

## Interfaces

An interface names a set of method signatures. Declaring one documents what a
generic function needs and lets the compiler check arguments where they are
passed:

```dodo test
package main

interface Shape {
    fn area(&self) -> u32
    fn scale(&mut self, factor: u32)
}

struct Square {
    side: u32

    pub fn area(&self) -> u32 {
        self.side * self.side
    }

    pub fn scale(&mut self, factor: u32) {
        self.side *= factor
    }
}

fn doubled_area<S: Shape>(shape: &mut S) -> u32 {
    shape.scale(2)
    shape.area()
}

fn main() {
    square := Square { side: 3 }
    core.assert_eq(doubled_area(&mut square), 36u32)
}
```

`S: Shape` is a bound: every type used for `S` must implement `Shape`. Several
bounds join with `+`, as in `<T: io.Reader + io.Writer>`. Generic structs take
bounds the same way: `struct Holder<S: Shape> { shape: S }`.

Implementation is structural. A type implements an interface when it has, for
every method the interface declares, a method with the same name, receiver, and
parameter and return types, with `Self` replaced by that type. Methods used
from another package must be `pub`. There is no separate implementation block,
and no runtime cost: bounded calls are specialized exactly like unbounded ones.

When a type does not fit, the error points at the call and explains the
difference, instead of failing somewhere inside the generic body. Here a
`Circle` has an `area` method returning `u64` and no `scale` method:

```text
error: `Circle` does not implement interface `Shape`
 ::: main.dodo:4:5
    |
  4 |     fn area(&self) -> u32
    |     --------------------- required by this interface method
 ::: main.dodo:5:5
    |
  5 |     fn scale(&mut self, factor: u32)
    |     -------------------------------- required by this interface method
 ::: main.dodo:11:5
    |
 11 |     pub fn area(&self) -> u64 {
    |     ------------------------- this method has a different signature
 ::: main.dodo:16:17
    |
 16 | fn doubled_area<S: Shape>(shape: &mut S) -> u32 {
    |                 -------- `doubled_area` requires `S: Shape`
 --> main.dodo:23:14
    |
 23 |     total := doubled_area(&mut circle)
    |              ^^^^^^^^^^^^^^^^^^^^^^^^^ `Circle` is used as `S` here
  = note: method `area` has signature `fn area(&self) -> u64`, but `Shape` requires `fn area(&self) -> u32`
  = note: missing method `fn scale(&mut self, factor: u32)`

```

A struct may also state what it implements: `struct Square: Shape { ... }`. The
compiler then checks the claim at the declaration, which documents the type and
catches a drifted signature before anyone uses it. For a generic struct, each
specialization is checked.

An interface can include others. `interface Sensor: Named { ... }` requires
`Named`'s methods as well as its own, and two interfaces that require the same
method name must agree on its signature.

Interface methods may declare `unsafe` and borrowed-return contracts. A method
`fn name(&self) -> &str` borrows from `self`, as for any method. An
implementation may return a borrow with fewer sources than the interface
allows, never more. Interface methods cannot declare `stores(...)` or
`requires_plain(...)`, have type parameters, or be named `drop`.

## Dynamic dispatch with `dyn`

Generics produce one copy of the code per type. When the type is only known at
run time, or one collection holds different types, use a `dyn` reference
instead. `&dyn Shape` and `&mut dyn Shape` refer to some value implementing
`Shape`, together with a table of its methods:

```dodo test
package main

interface Sensor {
    fn read(&mut self) -> i32
}

struct Thermometer {
    celsius: i32

    pub fn read(&mut self) -> i32 {
        self.celsius
    }
}

struct Counter {
    count: i32

    pub fn read(&mut self) -> i32 {
        self.count += 1
        self.count
    }
}

// The station does not know, or care, which sensor it polls.
struct Station {
    sensor: &mut dyn Sensor

    fn poll(&mut self) -> i32 {
        self.sensor.read()
    }
}

fn total(sensors: &mut [&mut dyn Sensor]) -> i32 {
    sum := 0i32
    for sensor in sensors {
        sum += sensor.read()
    }
    sum
}

fn main() {
    thermometer := Thermometer { celsius: 21 }
    counter := Counter { count: 0 }
    {
        sensors: [2]&mut dyn Sensor = [&mut thermometer, &mut counter]
        core.assert_eq(total(&mut sensors), 22i32)
    }
    station := Station { sensor: &mut counter }
    core.assert_eq(station.poll(), 2i32)
}
```

A reference to a concrete type converts to a `dyn` reference wherever one is
expected: arguments, annotated bindings, fields, array elements, and returns.
`x as &dyn Shape` converts explicitly. `&mut dyn I` also converts to `&dyn I`,
and `&dyn I` converts to `&dyn P` when `P` is `I`'s first parent interface (or
its first parent, and so on), because `P`'s methods begin `I`'s method table.

A `dyn` value has no fixed size, so it only exists behind a reference; `dyn
Shape` alone, an array of `dyn Shape`, or a raw `dyn` pointer is an error. The
reference is two pointers wide, holding the value's address and its method
table. Method tables are constant data built at compile time, one per type and
interface; nothing is allocated. Borrow checking is unchanged: a `dyn`
reference keeps every loan of the reference it was made from.

An interface can be used with `dyn` when each of its methods takes `&self` or
`&mut self` and mentions `Self` nowhere else. Other interfaces, for example
with a by-value `self` or a `fn new() -> Self` constructor, still work as
bounds.

A `dyn` type also implements its own interface, so bounded generic code accepts
it. `doubled_area(shape)` with `shape: &mut dyn Shape` specializes `S` as `dyn
Shape` once and dispatches through the table, which keeps code size small when
one generic function serves many types.

## Interfaces in the standard library

The standard library declares interfaces for its byte streams and devices:
`io.Reader`, `io.Writer`, and `io.ReadWriter` in [`std/io`](io.md), and
`hal.OutputPin`, `hal.InputPin`, `hal.Delay`, `hal.I2c`, `hal.Spi`,
`hal.Serial`, `hal.PwmOutput`, `hal.PwmInput`, `hal.AnalogInput`, `hal.Can`,
`hal.I2sOutput`, `hal.I2sInput`, their polling and target forms, the DMA
protocols, and `hal.Registers` in
[`std/embedded/hal`](hardware.md#device-protocols). Its
helpers are bounded by them, so `io.write_all(&mut stream, bytes)` reports a
stream without the right `write` method at the call.

Formatters, allocators, hash and equality policies, and callbacks still use
unconstrained parameters with documented method signatures. Read each API's
required signature carefully; a similar method name with a different receiver
or return type does not satisfy it.

## Generic ownership contracts

Returning borrowed data still requires a valid source. `from(value)` on a generic
function refers to the concrete argument's dependencies: a borrow-free
specialization contributes none, while a borrow-carrying specialization retains
its sources. A type parameter never permits returning a reference to destroyed
local storage.

Allocated container methods may use the narrow `requires_plain(T, ...)` contract
to restrict mutation to borrow-free, Result-free payloads. `stores(...)` tracks
sources deposited into typed storage. These are checked storage effects, not
interface bounds; their syntax and rules are in
[container element safety](container-elements.md#mutation-and-return-contracts).

All used generic bodies must be available in the loaded source packages. There
are no separately compiled generic interfaces. Recursive expansion is bounded;
exceeding the compiler's limit produces a diagnostic instead of unbounded
compilation. For concrete language restrictions and limits, see
[compiler support](implementation.md).
