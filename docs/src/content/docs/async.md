---
title: "Async functions"
description: "Write tasks as straight-line async functions that await times, signals, and other futures, on top of the embedded executor."
section: "Learn Dodo"
order: 57
---

An `async fn` is a function that can wait. Where an ordinary function would
block, an async one writes `await` and lets other work run until what it waits
for has happened. The compiler turns its body into a state machine, so waiting
costs no thread, no stack of its own, and no allocation. Async functions are
optional: they run on the cooperative
[executor](hardware.md#tasks-and-the-executor) from `std/embedded/executor`,
next to tasks written by hand, on a microcontroller or on a desktop.

```dodo test
package main

import "std/embedded/executor"
import "std/embedded/hal/fake"

async fn countdown(count: u32, period_ms: u32) -> u32 {
    remaining := count
    for remaining > 0 {
        await executor.sleep_ms(period_ms)
        remaining -= 1
    }
    return count * period_ms
}

fn main() {
    clock := fake.Clock.new()
    future := countdown(3, 250)
    {
        tasks: [1]&mut dyn executor.Task = [&mut future]
        executor.run(&mut tasks, &mut clock)
    }
    assert_eq(future.output(), 750u32)
    assert_eq(clock.now_us(), 750_000u64)
}
```

`fake.Clock` is a simulated clock for tests; on a board, the clock is a timer
alarm and the executor sleeps the core between steps.

## Calling an async fn makes a future

Calling `countdown(3, 250)` runs none of its body. It stores the arguments in
a **future**, a value of the struct type `CountdownFuture` that the compiler
declares next to the function. (The name is the function's in CamelCase with
`Future` appended; a method `Led.blink` gets `LedBlinkFuture`.) The future is
an executor task: it has the task's method,

```dodo
pub fn poll(&mut self, cx: &mut executor.Context) -> executor.Poll
```

so it goes into the executor's task list like any hand-written task, and the
two kinds run side by side. Each `poll` runs the body up to the next `await`
whose future is not ready and returns `Poll.Pending`; once the body returns,
`poll` returns `Poll.Ready`. A future whose function returns a value also has
`output(&mut self) -> T`, which hands the result over once, after Ready.

Like any value, a future is dropped when its owner goes out of scope. If it
is dropped before it finished, it drops whatever its body still owned at the
`await` where it stopped, and the arguments if it never started.

## Await

`await future` inside an async fn waits until `future` is ready and evaluates
to its output, or to nothing for a future without one. Anything with the
`poll` method above can be awaited: another async fn's future, the
executor's [waiting helpers](#what-to-await), or a hand-written task.

```dodo
// Reads a line from a serial port whose receive interrupt raises RX.
async fn read_line(port: &mut pl011.Port, line: &mut [64]u8) -> usize!io.Error {
    length := 0usize
    for length < 64 {
        byte := [0u8; 1]
        progress := port.poll_read(&mut byte)?
        if progress.transferred == 0 {
            cortex_m.enable(chip.UART0_IRQ)
            await RX.wait()
        } else if byte[0] == 10 {
            return ok(length)
        } else {
            line[length] = byte[0]
            length += 1
        }
    }
    return ok(length)
}
```

While the future is pending, the async fn's locals keep their values, and
references between them stay valid: they live in the future, not on a stack.
That is why `line` and `length` above survive every wait.

`await` must begin its statement's evaluation. It may be the statement
itself, a binding's initializer, an assigned or returned value, or the
condition of an `if`, `for`, or `match`, optionally followed by `?`, `!`, or
`as`:

```dodo
await executor.sleep_ms(10)
value := await sensor.read_celsius()
count = await BUTTON.wait()
total += await next_batch()
reading := (await sensor.read_raw())!
code := await port.read_status()?
if await ready() { start() }
match await executor.select(BUTTON.wait(), executor.sleep_ms(500)) { ... }
return await finish()
```

A trailing `?`, `!`, or `as` after the awaited call applies to the awaited
result, so `await sensor.read_raw()!` unwraps the reading, not the future.
Inside a larger expression, bind the awaited value first:
`send(await receive())` is written `value := await receive()` and then
`send(value)`. This rule keeps every partly evaluated expression out of the
suspended state, so a cancelled future never leaves a half-built value behind.

`await` cannot appear in an ordinary function, or in a closure inside an
async fn: write the closure's work as an async fn instead.

## What to await

The executor provides the leaves that async code waits on:

| Await | Ready when | Output |
| --- | --- | --- |
| `executor.sleep_ms(ms)`, `sleep(us)`, `sleep_until(time_us)` | The time has come, counted from the first poll. | — |
| `executor.yield_now()` | The other due tasks have run. | — |
| `executor.now()` | At once. | The time the current poll started, `u64` microseconds |
| `ticker.tick()` on an `executor.Ticker` | Its next tick; ticks keep their schedule. | — |
| `SIGNAL.wait()` on an `executor.Signal` | The signal was raised since it was last taken. | Raises taken, `u32` |
| `SIGNAL.next_value()` | The signal was sent a value. | The value, `u32` |
| `executor.select(a, b)` | Either future is; the other is dropped. | `executor.Which.First` or `Second` |
| `executor.join(a, b)` | Both futures are. | — |

Signals are how interrupts reach async code: a handler raises a signal, and
the task waiting for it continues at its next poll. `select` with a sleep is a
timeout:

```dodo
match await executor.select(BUTTON.wait(), executor.sleep_ms(500)) {
    executor.Which.First => toggle(),
    executor.Which.Second => blink_error(),
}
```

## Async methods and generic async fns

Methods and generic functions can be async. A method's future borrows its
receiver for as long as the future lives, and every borrow an argument
carries likewise lasts as long as the future:

```dodo test
package main

import "std/embedded/executor"
import "std/embedded/hal"
import "std/embedded/hal/fake"

struct Beeper<P: hal.OutputPin> {
    pin: P
    beeps: u32

    async fn beep(&mut self, times: u32) {
        for _ in 0u32..times {
            self.pin.set_high()
            await executor.sleep_ms(50)
            self.pin.set_low()
            await executor.sleep_ms(50)
            self.beeps += 1
        }
    }
}

fn main() {
    beeper := Beeper { pin: fake.Pin.new(), beeps: 0 }
    clock := fake.Clock.new()
    {
        future := beeper.beep(2)
        tasks: [1]&mut dyn executor.Task = [&mut future]
        executor.run(&mut tasks, &mut clock)
    }
    assert_eq(beeper.beeps, 2u32)
    assert_eq(beeper.pin.changes(), 4u32)
}
```

## Futures stay in place

A suspended future can hold references into itself, such as a local buffer
that an awaited read fills. So once a future has started, it must not move
until it finishes: keep it in one local, field, or static, and give the
executor a reference to it. Polling a started future at a different address
traps with "a suspended future was moved". An unstarted future, fresh from
the call, moves freely.

A future is as large as the most its body keeps across an `await`, plus its
arguments. Futures are values like any other: a large one can live in a
`static mut` instead of on `main`'s stack. An async fn cannot await itself,
directly or through others, because its future would contain itself.

## How it fits together

| Piece | Role |
| --- | --- |
| `async fn` | A body that can wait, compiled into a future's `poll`. |
| `await` | Inside an async fn, run a future to completion and take its output. |
| Future | A value with `poll` (and `output`); every future is an `executor.Task`. |
| `executor.run` | Polls tasks when what they wait for has happened, and sleeps the core between. |
| `executor.Signal` | Carries events from interrupt handlers and other tasks to waiting futures. |
| `sync.Mutex` | Shares values between handlers, tasks, and cores; see [hardware](hardware.md#sharing-values-with-a-mutex). |

Async functions add no runtime of their own: futures are plain structs, and
the executor is the same one hand-written tasks use. See
[tasks and the executor](hardware.md#tasks-and-the-executor) for running
them on a board, waking them from interrupts, and testing them on a desktop.
