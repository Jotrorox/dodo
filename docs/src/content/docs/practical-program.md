---
title: "Build a temperature report"
description: "Put functions, slices, options, formatting, and error handling together in a complete small program."
section: "Learn Dodo"
order: 70
---

This walkthrough builds a report from a few temperature readings. It assumes you
have run [your first program](first-program.md) and read
[language basics](language-basics.md) and [control flow](control-flow.md).
You will separate calculation from output, borrow an array without moving it,
handle an empty input, and propagate an output error.

## The result

The finished program prints:

```text
Readings: 4
Average: 21 C
Status: comfortable
```

We use small integer readings to focus on the language. Integer division
truncates toward zero: this report does not calculate a fractional average.
Production measurement code should also choose a policy for overflow, units,
missing readings, and floating-point accuracy.

## Create the program

Create a fresh directory named `temperature` and save this as `main.dodo`:

```dodo test
package main

import "std/console"
import "std/io"

fn average(readings: &[i32]) -> Option<i32> {
    if readings.len == 0 {
        return none
    }
    total := 0i32
    for &reading in readings {
        total += reading
    }
    return some(total / (readings.len as i32))
}

fn describe(temperature: i32) -> &str from(static) {
    if temperature < 18 {
        return "cool"
    }
    if temperature > 25 {
        return "warm"
    }
    return "comfortable"
}

fn report(readings: &[i32]) -> void!io.Error {
    console.printf("Readings: {}\n", readings.len)?
    match average(readings) {
        some(value) => {
            console.printf("Average: {} C\n", value)?
            console.printf("Status: {}\n", describe(value))?
        },
        none => {
            console.println("No readings yet.")?
        },
    }
    return ok()
}

fn main() -> i32 {
    readings := [18i32, 21, 23, 22]
    match report(&readings) {
        ok() => { return 0 },
        err(_) => { return 1 },
    }
}
```

From that folder:

```sh
dodo fmt
dodo check
dodo run
```

Expect the three lines shown above. An exit status of 1 means console output
failed. The example does not attempt another write to report that same failure.

## Follow the data

`main` owns an array of four `i32` values. `&readings` borrows the array as a
read-only slice. A slice carries access to the elements and their length; it
does not allocate or copy the array. `report` lends the same data to `average`.
After these calls, `main` still owns the original array.

Inside `average`, `total := 0i32` is mutable because each iteration adds a value.
`for &reading in readings` copies one integer into the loop binding. This copy
form works for copyable elements. Without `&` in the loop pattern, the element
binding would be a reference and the addition would use `*reading`.

`readings.len` is `usize`. The explicit `as i32` makes the divisor match `total`.
That conversion is checked, and addition can trap on overflow. The four chosen
values and their sum fit easily; the function is not an arbitrary-size,
overflow-proof statistics implementation.

## Represent absence separately from failure

There is no average for an empty list. `average` returns `Option<i32>` to express
that fact: `some(number)` for a nonempty input, `none` for an empty one. `report`
handles both cases with `match`. Neither is an I/O error.

Printing can fail, so `report` returns `void!io.Error`. `void` means there is no
success payload; `io.Error` describes the failure. Each postfix `?` continues
on success or returns the error to `main`. `return ok()` marks successful
completion. The final `match` in `main` converts this result into an exit code.

These two return types answer different questions: *is there a value?* and *did
the operation succeed?* The [Results and options guide](patterns-and-results.md)
explains consuming matches, propagation, and unwrap in more detail.

## Return text without allocation

`describe` returns one of three string literals. Literal storage lives for the
program's lifetime; `from(static)` declares that source for the returned `&str`.
The function does not build an
owned string or allocate memory. Returning a view of a local byte array instead
would be invalid because that array disappears when the function returns.

Use the [text guide](text.md) when you need to validate input, build text in a
buffer, or keep an owned string. Use [formatting](formatting.md) to control how
values appear in a report.

## Add a test beside the code

Append this test to the same `main.dodo`:

```dodo
@test
fn average_handles_values_and_empty_input() {
    values := [18i32, 21, 23, 22]
    match average(&values) {
        some(value) => { assert_eq(value, 21) },
        none => { assert(false, "the input is not empty") },
    }
    empty: [0]i32 = []
    match average(&empty) {
        some(_) => { assert(false, "an empty list has no average") },
        none => {},
    }
}
```

Run `dodo test`. It runs the named test without calling your application's
`main`. The calculation can be tested without capturing console output. This
snippet depends on the `average` function above; it is not a standalone file.
See [testing](testing.md) for test selection and companion files.

## Try it

1. Change the readings to `[10i32, 12, 14]`. Expect average 12 and status `cool`.
2. Replace the array binding with `readings: [0]i32 = []`. Expect `Readings: 0`
   and `No readings yet.`
3. Add a `hottest` function returning `Option<i32>`. Keep the empty-input case
   explicit and test it before adding output.
4. Move calculation into an imported local package using
   [projects and imports](packages.md). Make the functions the application calls
   `pub`.

For another step, read decimal input with [console I/O](console.md) and
[text parsing](text.md), or serialize a report using [JSON](json.md). Each step
adds an explicit boundary for validation, storage, or failure handling.
