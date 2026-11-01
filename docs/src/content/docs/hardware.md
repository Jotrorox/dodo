---
title: "Hardware and embedded"
description: "Typed registers, interrupts, portable device drivers, and desktop testing with std/embedded/hal."
section: "Standard library"
order: 161
---

Dodo programs can talk to hardware directly, with no separate embedded
dialect. The same language, compiler, and test runner build a desktop tool and
a microcontroller firmware image. Hardware support is ordinary library code,
grouped under `std/embedded` in layers, on top of two small compiler packages:

| Layer | Import | What it gives you |
| --- | --- | --- |
| Device protocols | `std/embedded/hal` | Pin, delay, I2C, SPI, serial, PWM, analog, CAN, and I2S interfaces that drivers are written against, blocking and polling, with target (slave) sides, pulse capture, and DMA buffer transfers. |
| Peripheral drivers | `std/embedded/peripheral/...` | I2C, SPI, and UART controller drivers, shared by every chip that contains the controller. |
| Typed registers | `std/embedded/hal` | `hal.Reg<T>` and `hal.RegisterBlock` handles and mask constants for memory-mapped registers. |
| Interrupts | `std/embedded/hal`, `std/embedded/cortex_m` | `@interrupt` handlers; `hal.Critical`, a section that masks interrupts until it is dropped; `hal.Handler`, a slot for a handler chosen at run time; the Cortex-M interrupt controller and SysTick. |
| Tasks | `std/embedded/executor` | Several activities on one core, each woken on time or by an interrupt, with the core asleep in between. |
| Real-time clocks | `std/embedded/hal`, `std/embedded/chip/rp_rtc`, `std/embedded/rtc/ds3231` | `hal.Rtc` and `hal.RtcAlarm`: the time of day and an alarm, the same on the RP2040's RTC, the RP2350's always-on timer, and a DS3231 clock chip on I2C. |
| Low power | `std/embedded/power` | Sleep and deep sleep, clock gating, and a dormant state woken by pins, through one `Power` protocol that every chip and the fakes implement. |
| DMA | `std/embedded/hal`, `std/embedded/chip/rp_dma` | `hal.DmaChannel`, through which drivers move data while the program does other work; the RP2040's and RP2350's channels. |
| Persistent storage | `std/embedded/hal`, `std/embedded/storage`, `std/embedded/chip/rp_flash` | `hal.Flash`, erasing and writing NOR flash; a power-cut-safe key-value store and flash regions on any such device; the RP2040's and RP2350's program flash. |
| Desktop testing | `std/embedded/hal/fake` | In-memory pins, delays, buses, serial ports, flash, a real-time clock, a simulated clock, and simulated power controls that record what a driver did. |
| Boards | `std/embedded/board` | The selected board: one `take()` and the same LED, pin, timer, and clock fields on every board. |
| Chip support | `std/embedded/chip` | The selected microcontroller's peripherals, for custom boards. |
| Wireless chip | `std/embedded/wireless/cyw43` | The CYW43439 on the Pico 2 W: power-up, its gSPI bus, and its GPIO pins. |
| CAN controllers | `std/embedded/can/mcp2515` | The MCP2515 CAN controller over SPI, as a `hal.Can`. |
| Display panels | `std/embedded/display`, `.../display/st7789`, `.../display/ssd1306` | ST7789 colour and SSD1306 OLED panels as [graphics displays](graphics.md#display-panels), over SPI or I2C. |
| Raw access | `core/mmio`, `core/cpu`, `asm(...)` | Volatile loads/stores, barriers, interrupt masking, `wfi`, inline assembly. |

Five kinds of code meet here:

- **Chip support** (`std/embedded/chip/NAME`) knows datasheet addresses. It turns
  registers into values that implement the protocols, for example a `Pin`
  with `set_high`.
- **Peripheral drivers** (`std/embedded/peripheral/NAME`) know one controller
  design that chip vendors license, such as Synopsys's DesignWare I2C
  controller. Chip support hands them the controller's registers, so one
  driver serves every chip that contains the design.
- **Board support** (`std/embedded/board/NAME`) configures its chip for one circuit
  board: crystal frequency, which pin drives the LED, what is wired where.
- **Drivers** know a device, such as a sensor. They are generic over any value
  with the protocol's methods and never mention a chip.
- **Applications** take the board's values and hand them to drivers.

Drivers therefore run unchanged on any chip, and on a desktop with fakes.
Applications that import `std/embedded/board` build unchanged for every board.

## Quickstart: a driver and its tests

Save this as `blink.dodo` and run `dodo test blink.dodo`. No hardware is needed:

```dodo test
package blink

import "std/embedded/hal"
import "std/embedded/hal/fake"

// Works with any output pin and any delay.
pub fn blink<P: hal.OutputPin, D: hal.Delay>(led: &mut P, delay: &mut D, times: u32) {
    for _ in 0..times {
        led.set_high()
        hal.delay_ms(delay, 250)
        led.set_low()
        hal.delay_ms(delay, 250)
    }
}

@test
fn blinks_three_times() {
    led := fake.Pin.new()
    delay := fake.Delay.new()
    blink(&mut led, &mut delay, 3)
    assert_eq(led.changes(), 6u32)
    assert(!led.is_high())
    assert_eq(delay.elapsed_us(), 1_500_000u64)
}
```

`blink` names no chip. `<P: hal.OutputPin, D: hal.Delay>` are bounded type
parameters: any pin with `set_high` and `set_low`, and any delay with
`delay_us`, satisfy them, and passing something else is reported at the call.
The compiler generates code specialized for the real types, so the interfaces
cost nothing at run time. See [interfaces](generics.md#interfaces) for how
bounds work.

## Device protocols

Each device protocol is an [interface](generics.md#interfaces) in
`std/embedded/hal`: a set of method signatures. Any struct with those public
methods implements it and can be passed where a driver expects it:

| Interface | Methods |
| --- | --- |
| `hal.OutputPin` | `set_high(&mut self)`, `set_low(&mut self)` |
| `hal.InputPin` | `is_high(&self) -> bool` |
| `hal.Delay` | `delay_us(&mut self, microseconds: u32)` |
| `hal.Counter` | `now_us(&self) -> u64` |
| `hal.Alarm` | `now_us(&self) -> u64`<br>`schedule_at_us(&mut self, time_us: u64) -> void!hal.Error`<br>`cancel(&mut self)`<br>`fired(&self) -> bool`<br>`clear(&mut self)` |
| `hal.I2c` | `write(&mut self, address: u8, bytes: &[u8]) -> void!hal.Error`<br>`read(&mut self, address: u8, buffer: &mut[u8]) -> void!hal.Error`<br>`write_read(&mut self, address: u8, bytes: &[u8], buffer: &mut[u8]) -> void!hal.Error` |
| `hal.Spi` | `transfer(&mut self, bytes: &mut[u8]) -> void!hal.Error`<br>`write(&mut self, bytes: &[u8]) -> void!hal.Error` |
| `hal.PwmOutput` | `max_duty(&self) -> u16`<br>`set_duty(&mut self, duty: u16) -> void!hal.Error` |
| `hal.AnalogInput` | `max_value(&self) -> u16`<br>`read(&mut self) -> u16!hal.Error` |
| `hal.Rng` | `fill(&mut self, bytes: &mut[u8]) -> void!hal.Error` |
| `hal.Rtc` | `is_set(&mut self) -> bool!hal.Error`<br>`now_ms(&mut self) -> u64!hal.Error`<br>`set_ms(&mut self, time_ms: u64) -> void!hal.Error`<br>`resolution_ms(&self) -> u32` |
| `hal.RtcAlarm` | `set_alarm_ms(&mut self, time_ms: u64) -> void!hal.Error`<br>`alarm_fired(&mut self) -> bool!hal.Error`<br>`clear_alarm(&mut self) -> void!hal.Error` |
| Serial port | [`io.Reader` and `io.Writer`](io.md) from `std/io`; `serial.Port` adds `flush(&mut self) -> void!io.Error`. |
| `hal.Flash` | `capacity(&self) -> u32`, `erase_size(&self) -> u32`, `write_size(&self) -> u32`<br>`read(&mut self, offset: u32, buffer: &mut[u8]) -> void!hal.Error`<br>`erase(&mut self, offset: u32, length: u32) -> void!hal.Error`<br>`write(&mut self, offset: u32, bytes: &[u8]) -> void!hal.Error` |
| `hal.Registers` | `read(&mut self, offset: usize) -> u32`<br>`write(&mut self, offset: usize, value: u32)` |

Write drivers against bounded parameters, `fn probe<B: hal.I2c>(bus: &mut B)`,
for zero-cost static dispatch. Use `&mut dyn hal.I2c` (or any other interface)
when the device is chosen at run time, or to keep different devices in one
array, such as a list of sensors behind one interface of your own.

A bus is configured when it is created: its frequency, and for SPI a
`hal.SpiMode` (`Mode0` to `Mode3`, the clock polarity and phase numbering of
device datasheets). The protocols then only move bytes, so a driver works
whatever speed the application chose. A PWM output is configured the same
way, with its frequency, and the protocol then only sets its duty cycle: the
output is high for `duty / max_duty()` of each period, so 0 holds it low and
`max_duty()` holds it high. An analog input reads 0 at ground and
`max_value()`, such as 4095 for a 12-bit converter, at the converter's
reference voltage. A [random number generator](#random-numbers) fills
buffers with random bytes. A [real-time clock](#real-time-clocks) counts
milliseconds since the Unix epoch. `hal.I2C_STANDARD_HZ`,
`hal.I2C_FAST_HZ`, and `hal.I2C_FAST_PLUS_HZ` name the usual I2C speeds.
The register block protocol is for [peripheral drivers](#register-blocks)
rather than device drivers.

A counter keeps time: `now_us()` is microseconds since a fixed start, such
as the chip's, in a `u64` that never decreases and that no program outlives.
Drivers measure intervals and give up on stuck devices with one; see
[timeouts and periodic work](#timeouts-and-periodic-work). An alarm fires
once when its counter reaches the time given to `schedule_at_us`, replacing
any earlier time and clearing an earlier firing; a time already reached
fires at once, and one further ahead than the alarm reaches is
`InvalidInput`. Code polls `fired()`, or the alarm's interrupt runs a
handler, and either calls `clear()`. Turning that interrupt on is
chip-specific, as `listen` and `irq` are on the
[RP2040 and RP2350](#an-interrupt-driven-driver); the protocol only sets
times and reports them. An alarm has `now_us` too, so it is also a counter.

More protocols follow below: [polling forms](#polling-instead-of-waiting)
of the buses that never wait, [targets](#being-the-device-targets) for being
the device on an I2C or SPI bus, [pulse measurement](#measuring-pulses),
[CAN](#can), [I2S audio](#i2s-audio), and [DMA buffer
transfers](#transfers-in-the-background-dma).

Pins, delays, and counters cannot fail. Alarm, bus, PWM, analog, clock,
and random operations return
`hal.Error`, one shared enum, because such a device may itself sit on a bus,
as a PWM controller chip or a clock chip on I2C does: `Timeout`, `NoAcknowledge`, `Bus`,
`Overrun`, `Busy`, `InvalidInput`, and `Unsupported`. Every driver and chip uses the same error type, so `?` passes
errors through layers without conversions, and `==` compares them. Use
`hal.error_name(error)` for a log message.

A serial port is just a byte stream. Implement `std/io`'s `write` and `read`
for a UART and every existing helper works with it, including `io.write_all` and
[`std/fmt`](formatting.md) output. The RP2040 and RP2350
[serial ports](#serial-ports-on-the-rp2040-and-rp2350) do exactly that.

Code that reads a device and writes to a stream sees both error types. Give it
an error enum with a variant for each, and `?` wraps either error in its variant
([combining error types](patterns-and-results.md#combine-error-types)):

```dodo
enum LogError {
    Sensor(hal.Error),
    Output(io.Error),
}

fn log_reading<B: hal.I2c, W: io.Writer>(bus: &mut B, output: &mut W) -> u8!LogError {
    value := hal.read_register(bus, ADDRESS, TEMPERATURE)?
    fmt.printf(output, "temperature {}\n", value)?
    return ok(value)
}
```

The repository's `examples/sensor_log.dodo` tests this function against a fake
bus and a memory buffer.

### Random numbers

A random number generator implements `hal.Rng`: `fill(bytes)` fills a
buffer with random bytes, and `hal.random_u32`, `hal.random_u64`, and
`hal.random_below` build numbers from them. Every board has one as `b.rng`,
the best its chip offers, so code written against the protocol runs on any
board and, with a fake or `hal.SoftRng`, in desktop tests:

```dodo
package main

import "std/embedded/board"
import "std/embedded/hal"

// Roll a die with any generator.
fn roll<R: hal.Rng>(rng: &mut R) -> u32!hal.Error {
    return ok(hal.random_below(rng, 6)? + 1)
}

fn main() {
    b := board.take()!
    key := [0u8; 16]
    b.rng.fill(&mut key)!
    if roll(&mut b.rng)! == 6 {
        b.led.set_high()
    }
}
```

Hardware generators gather entropy from physical noise, such as an
oscillator's jitter, and differ in quality and speed: the
[RP2040's](#random-numbers-on-the-rp2040-and-rp2350) suits seeds and games
but not cryptographic keys, and the RP2350's is fit for keys. A generator
whose source stops reports `Timeout`, and one whose health checks fail
reports `Bus`; the bytes of a failed fill must not be used.

`hal.SoftRng` is a software generator, xoshiro128**, that implements the
protocol too. Seed it once from a hardware generator with
`hal.SoftRng.from_rng(&mut b.rng)` and it produces numbers at a few cycles
each, with `next_u32()` or the helpers, and never fails. Its output is
statistically uniform but predictable to anyone who learns its 128-bit
state, so it is for simulations, games, random delays, and test order, not
keys. `hal.SoftRng.new(seed)` repeats the same sequence for the same seed,
for reproducible tests.

### Helpers

| Helper | Purpose |
| --- | --- |
| `hal.write_pin(&mut pin, high)` | Drive a pin from a `bool`. |
| `hal.delay_ms(&mut delay, ms)` | Millisecond delay from any `delay_us`. |
| `hal.elapsed_us(&counter, since_us)` | Microseconds since an earlier `now_us()`; 0 for a time still ahead. |
| `hal.wait_until(&counter, time_us)` | Busy-wait until a counter reaches a time. |
| `hal.Deadline` | A time by which something must happen: `Deadline.after(&counter, us)` or `Deadline.at(time_us)`, then `passed(&counter)`, `remaining_us(&counter)`, and `wait(&counter)`. |
| `hal.Ticker` | Ticks at a fixed period without drift: `Ticker.start(&counter, period_us)`, then `ready(&counter)` once per tick, or `wait(&counter)`. Ticks missed entirely are skipped. |
| `hal.schedule_in_us(&mut alarm, us)` | Fire an alarm a number of microseconds from now. |
| `hal.wait_for_alarm(&mut alarm)` | Busy-wait until an alarm fires, then clear it. |
| `hal.read_register(&mut bus, address, register)` | Read one numbered 8-bit I2C register. |
| `hal.read_registers(&mut bus, address, first, buffer)` | Read consecutive I2C registers. |
| `hal.write_register(&mut bus, address, register, value)` | Write one I2C register. |
| `hal.set_duty_percent(&mut output, percent)` | Set a PWM output high for 0 to 100% of each period. |
| `hal.set_duty_fraction(&mut output, numerator, denominator)` | Set a PWM duty cycle to a fraction, rounded to the nearest step. |
| `hal.set_pulse_us(&mut output, pulse_us, period_us)` | Set a PWM pulse length, as for a servo: `set_pulse_us(&mut servo, 1500, 20_000)` centres one on a 50 Hz output. |
| `hal.read_millivolts(&mut input, reference_mv)` | Read an analog input as millivolts. |
| `hal.random_u32(&mut rng)`, `hal.random_u64(&mut rng)` | A uniformly distributed number from a [random number generator](#random-numbers). |
| `hal.random_below(&mut rng, bound)` | A uniformly distributed number from 0 to `bound - 1`, without modulo bias: `random_below(&mut rng, 6)? + 1` rolls a die. |
| `hal.SoftRng` | A fast [software generator](#random-numbers) seeded from a hardware one. |
| `hal.rtc_now(&mut clock)`, `hal.rtc_set(&mut clock, &date_time)` | Read or set a real-time clock as a UTC `time.DateTime`. |
| `hal.rtc_alarm_at(&mut clock, &date_time)`, `hal.rtc_alarm_in(&mut clock, ms)` | Set a clock's alarm for a date and time, or some milliseconds from now. |
| `hal.date_time_of_ms(ms)`, `hal.ms_of_date_time(&date_time)` | Convert between Unix milliseconds and a `time.DateTime`. |
| `hal.SpiDevice<P>` | One SPI device and its chip-select pin; see below. |
| `hal.BlockingI2c<B>`, `hal.BlockingSpi<B>` | The blocking protocol over a polling bus; see [polling](#polling-instead-of-waiting). |
| `hal.capture(&mut input, polls)` | Wait for a pulse measurement. |
| `hal.send_frame`, `hal.receive_frame` | Send or receive a CAN frame, waiting up to a number of polls. |
| `hal.write_samples`, `hal.read_samples` | Move a whole slice of I2S samples, waiting for room or data. |
| `hal.finish_transfer(&mut session)` | Wait for a DMA buffer transfer. |
| `hal.dma_copy(&mut channel, &mut destination, &source)` | Copy memory with any DMA channel. |

### Timeouts and periodic work

A driver that waits for a device names no chip when it takes a counter.
Boards' timers are both a counter and a delay, so a driver bounded by
`hal.Delay + hal.Counter` polls, delays between polls, and gives up at a
deadline. A `hal.Deadline` holds only its time, so it never borrows the
counter, and in a test `fake.Delay` reaches the deadline without waiting:
its time is the sum of the delays asked of it.

```dodo test
package ready

import "std/embedded/hal"
import "std/embedded/hal/fake"

const ADDRESS: u8 = 0x48
const STATUS: u8 = 0x00

// Poll the status register every millisecond until it is nonzero, for at
// most `timeout_us`.
pub fn wait_ready<B: hal.I2c, T: hal.Delay + hal.Counter>(bus: &mut B, timer: &mut T, timeout_us: u64) -> void!hal.Error {
    deadline := hal.Deadline.after(timer, timeout_us)
    for hal.read_register(bus, ADDRESS, STATUS)? == 0 {
        if deadline.passed(timer) {
            return err(hal.Error.Timeout)
        }
        timer.delay_us(1000)
    }
    return ok()
}

@test
fn a_stuck_device_times_out() {
    bus := fake.I2c.new(ADDRESS)
    timer := fake.Delay.new()
    match wait_ready(&mut bus, &mut timer, 5000) {
        ok() => assert(false),
        err(error) => assert(error == hal.Error.Timeout),
    }
    assert_eq(timer.now_us(), 5000u64)
    bus.set_register(STATUS, 1)
    wait_ready(&mut bus, &mut timer, 5000)!
}
```

A `hal.Ticker` runs work at a fixed period, such as sampling every 10 ms:
`ticker := hal.Ticker.start(&b.timer, 10_000)`, then `ticker.wait(&b.timer)`
before each sample. Every tick falls a whole number of periods after the
start, so the samples do not drift however long each one takes. Firmware
that runs several such jobs side by side uses the
[executor](#tasks-and-the-executor) instead, whose tasks sleep rather than
busy-wait.

### Several devices on one SPI bus

A `hal.SpiDevice` owns a device's chip-select pin and drives it around each
transaction. The bus itself is passed to each call instead of being stored, so
any number of devices share it, and the borrow checker guarantees that two
transactions never overlap:

```dodo test
package spi_example

import "std/embedded/hal"
import "std/embedded/hal/fake"

fn main() {
    bus := fake.Spi.new()
    flash := hal.SpiDevice.new(fake.Pin.new())
    display := hal.SpiDevice.new(fake.Pin.new())

    reply: [3]u8 = [0x00, 0xEF, 0x40]
    bus.reply_with(&reply)
    command: [1]u8 = [0x9F]
    id: [2]u8 = [0, 0]
    flash.write_read(&mut bus, &command, &mut id)!
    assert_eq(id[0], 0xEFu8)

    pixels: [3]u8 = [1, 2, 3]
    display.write(&mut bus, &pixels)!
}
```

### Polling instead of waiting

The protocols above wait: `bus.write_read(...)` returns when the transaction
is over. Firmware that runs several things at once, such as
[executor tasks](#tasks-and-the-executor), cannot afford that, so the buses
also come in polling forms that never wait:

| Interface | Methods |
| --- | --- |
| `hal.I2cPoll` | `poll_write(&mut self, address: u8, bytes: &[u8]) -> bool!hal.Error`<br>`poll_read(&mut self, address: u8, buffer: &mut[u8]) -> bool!hal.Error`<br>`poll_write_read(&mut self, address: u8, bytes: &[u8], buffer: &mut[u8]) -> bool!hal.Error`<br>`cancel(&mut self)` |
| `hal.SpiPoll` | `poll_transfer(&mut self, bytes: &mut[u8]) -> bool!hal.Error`<br>`poll_write(&mut self, bytes: &[u8]) -> bool!hal.Error`<br>`cancel(&mut self)` |
| `serial.PollPort` | `poll_read(&mut self, buffer: &mut[u8]) -> io.Poll!io.Error`<br>`poll_write(&mut self, bytes: &[u8]) -> io.Poll!io.Error`<br>`poll_flush(&mut self) -> bool!io.Error` |

Every call does what the hardware allows right now, such as topping up a
FIFO or collecting received bytes, and returns `false` while the operation
continues and `true` once it has finished. The first call starts the
operation; later calls pass the same arguments until it finishes, fails, or
is cancelled. The operation's state lives in the bus, not in borrowed
buffers, so the buffers stay yours between polls and a transaction can be any
length: an I2C controller holds the bus with SCL low between polls, which
devices accept as clock stretching. A call with a different address or length
while one runs is `InvalidInput` and leaves the running one alone. Errors are
those of the blocking methods, and the operation is over after one. Serial
ports follow the progress contract of [`std/io`](io.md): `poll_read` returns
what has arrived and `poll_write` takes what fits, Pending otherwise.

A task keeps its buffers in fields and returns Pending until the poll says
`true`:

```dodo test
package sampler

import "std/embedded/executor"
import "std/embedded/hal"
import "std/embedded/hal/fake"

// Reads a 16-bit sensor register every 100 ms, never waiting for the bus.
pub struct Sampler<B: hal.I2cPoll> {
    bus: B
    ticker: executor.Ticker
    reading: bool
    command: [1]u8
    reply: [2]u8
    last: u16

    pub fn poll(&mut self, cx: &mut executor.Context) -> executor.Poll {
        if self.ticker.ready(cx) {
            self.reading = true
        }
        if self.reading {
            match self.bus.poll_write_read(0x48, &self.command, &mut self.reply) {
                ok(done) => {
                    if done {
                        self.reading = false
                        self.last = ((self.reply[0] as u16) << 8) | self.reply[1] as u16
                    } else {
                        // Or wait for a signal from the bus's interrupt.
                        cx.yield_now()
                    }
                }
                err(_) => {
                    self.reading = false
                }
            }
        }
        return executor.Poll.Pending
    }
}

@test
fn samples_across_polls() {
    bus := fake.I2c.new(0x48)
    bus.set_register(0x10, 0x01)
    bus.set_register(0x11, 0x2C)
    // Each transaction takes three polls.
    bus.set_poll_steps(3)
    sampler := Sampler { bus, ticker: executor.Ticker.every_ms(100), reading: false, command: [0x10], reply: [0, 0], last: 0 }
    clock := fake.Clock.new()
    tasks: [1]&mut dyn executor.Task = [&mut sampler]
    runner := executor.Executor.new()
    runner.run_until(&mut tasks, &mut clock, 150_000)
    assert_eq(sampler.last, 300u16)
}
```

A controller that implements only the polling form still serves blocking
drivers: `hal.BlockingI2c.new(bus)` and `hal.BlockingSpi.new(bus)` implement
`hal.I2c` and `hal.Spi` by polling until the transaction is over. They give up
after `hal.DEFAULT_POLL_LIMIT` polls, cancelling the transaction and returning
`Timeout`; `set_poll_limit(polls)` changes that and `hal.NO_POLL_LIMIT` waits
forever. `bus()` reaches the wrapped bus and `release()` gives it back.
`hal.SpiDevice` also polls: `poll_transfer`, `poll_write`, and
`poll_write_read` keep the device selected from the first poll until the
transaction ends, and `cancel` deselects it.

Serial ports' protocols live in `std/embedded/hal/serial`, so that
`std/embedded/hal` itself depends on no stream package: `serial.PollPort`
above, and `serial.Port`, a serial port with `std/io`'s `read` and `write`
and a `flush` that waits until the last byte has left the transmitter, for
drivers that must know a reply was sent before they listen or power down.

### Being the device: targets

A microcontroller can also be the device that another master talks to. The
address, mode, and frame size are set when the target is created; the
protocols then move bytes:

| Interface | Methods |
| --- | --- |
| `hal.I2cTarget` | `poll(&mut self, buffer: &mut[u8]) -> hal.I2cTargetEvent!hal.Error`<br>`respond(&mut self, bytes: &[u8]) -> usize!hal.Error` |
| `hal.SpiTarget` | `queue(&mut self, bytes: &[u8]) -> usize!hal.Error`<br>`read(&mut self, buffer: &mut[u8]) -> usize!hal.Error` |

`poll` reports what the master did, in order, without waiting:
`hal.I2cTargetEvent.Write(count, start)` for bytes it wrote, now at the
start of `buffer` (`start` marks the first byte after the address, such as a
register number), `Stop` at the end of a transaction, `Read` when it reads,
and `Idle` when nothing happened. A read holds the master, with SCL low,
until `respond` answers it, so a slow program delays the master but loses
nothing. An empty `respond` answers one `0xFF`, what the master would read
from an idle bus. An SPI target `queue`s the bytes to shift out before the
master clocks them, and `read` collects what the master sent; `Overrun` means
bytes were lost.

```dodo test
package register_device

import "std/embedded/hal"
import "std/embedded/hal/fake"

// A register device: the first byte of a write picks the register, later
// bytes fill it, and reads continue from there.
pub struct Registers {
    memory: [16]u8
    pointer: usize

    pub fn serve<T: hal.I2cTarget>(&mut self, target: &mut T) -> void!hal.Error {
        buffer: [16]u8 = [0; 16]
        match target.poll(&mut buffer)? {
            hal.I2cTargetEvent.Write(count, start) => {
                first := 0usize
                if start {
                    self.pointer = buffer[0] as usize % 16
                    first = 1
                }
                for index in first..count {
                    self.memory[self.pointer] = buffer[index]
                    self.pointer = (self.pointer + 1) % 16
                }
            }
            hal.I2cTargetEvent.Read => {
                answer: [1]u8 = [self.memory[self.pointer]]
                target.respond(&answer)?
                self.pointer = (self.pointer + 1) % 16
            }
            hal.I2cTargetEvent.Stop => {}
            hal.I2cTargetEvent.Idle => {}
        }
        return ok()
    }
}

@test
fn answers_a_master() {
    target := fake.I2cTarget.new()
    write: [3]u8 = [4, 0xAB, 0xCD]
    target.master_write(&write)
    target.master_stop()
    select: [1]u8 = [5]
    target.master_write(&select)
    target.master_read(1)
    target.master_stop()
    device := Registers { memory: [0; 16], pointer: 0 }
    for !target.is_idle() {
        device.serve(&mut target)!
    }
    assert_eq(target.master_received()[0], 0xCDu8)
}
```

### Measuring pulses

`hal.PwmInput` is the input side of PWM: it measures the pulses on a pin,
such as a PWM signal's frequency and duty cycle, a fan's tachometer, or the
pulse a servo or radio receiver channel sends. Like an output, it is
configured when it is created, with how long one measurement may take, and
measurements follow one another:

| Interface | Methods |
| --- | --- |
| `hal.PwmInput` | `poll_capture(&mut self, capture: &mut hal.PulseCapture) -> bool!hal.Error` |

A `hal.PulseCapture` holds `period_ns` and `high_ns`, averaged over `cycles`
periods: hardware that timestamps edges measures single periods, hardware
that counts edges over a window averages many. A line that did not change
has `cycles` 0, with `high_ns` telling whether it stayed high. Its methods
convert: `frequency_hz()`, `frequency_millihz()`, `duty(max)` (so
`duty(100)` is a percentage, and `duty(output.max_duty() as u32)` copies the
signal onto a PWM output), `high_us()`, and `period_us()`.
`hal.capture(&mut input, polls)` waits for the next measurement.

### CAN

`hal.Can` sends and receives frames on a CAN bus. Bit rate, acceptance
filters, and modes are set when a controller is created; both directions
queue in the controller's buffers, so both methods return at once:

| Interface | Methods |
| --- | --- |
| `hal.Can` | `poll_send(&mut self, frame: &hal.CanFrame) -> bool!hal.Error`<br>`poll_receive(&mut self, frame: &mut hal.CanFrame) -> bool!hal.Error`<br>`error_state(&mut self) -> hal.CanErrorState!hal.Error` |

`poll_send` is false while every transmit buffer is full and `poll_receive`
false while no frame waits. A controller that lost frames reports `Overrun`
once, a bus-off controller refuses to send with `Bus`, and a classic
controller refuses CAN FD frames with `Unsupported`. `error_state()` tells
where the controller stands: `Active`, `Passive` after many errors, or
`BusOff`. `hal.send_frame(&mut can, &frame, polls)` and
`hal.receive_frame(&mut can, &mut frame, polls)` wait, up to a number of
polls.

A `hal.CanFrame` is a value a program keeps and reuses. Its fields are
checked as they are set, so every frame is one a controller can send:
`CanFrame.standard(id, bytes)` and `CanFrame.extended(id, bytes)` make
classic data frames, and `set_data`, `set_remote` (a request for data), and
`set_fd` (up to 64 bytes, optionally at the faster data bit rate) change one;
an identifier too large for its kind or too many bytes is `InvalidInput`.
`id()`, `is_extended()`, `is_remote()`, `is_fd()`, `len()`, `dlc()`, and
`bytes()` read it. `hal.CanFilter` describes an acceptance filter (an
identifier, a mask, and the kind), which controllers program into their
hardware: `CanFilter.exact(id, extended)` passes one identifier and
`CanFilter.all(extended)` everything of a kind.

Controller drivers share the arithmetic of bit timing:
`hal.can_timing(clock_hz, bitrate, sample_point_permille, &limits)` finds the
prescaler and segments for a bit rate within a controller's
`hal.CanTimingLimits`, preferring the exact rate, the sample point closest to
the one asked for (875 per mille is CiA's recommendation), and the most time
quanta, and returns a `hal.CanBitTiming` that each driver encodes into its
own registers. `hal.can_dlc_length` and `hal.can_length_dlc` convert between
data length codes and lengths.

```dodo test
package can_echo

import "std/embedded/hal"
import "std/embedded/hal/fake"

// Answers every request on 0x100 with its first byte plus one, on 0x101.
pub fn serve<C: hal.Can>(can: &mut C) -> void!hal.Error {
    frame := hal.CanFrame.new()
    for can.poll_receive(&mut frame)? {
        if frame.id() == 0x100 && frame.len() >= 1 {
            reply: [1]u8 = [frame.bytes()[0] + 1]
            answer := hal.CanFrame.standard(0x101, &reply)?
            hal.send_frame(can, &answer, hal.DEFAULT_POLL_LIMIT)?
        }
    }
    return ok()
}

@test
fn answers_requests() {
    can := fake.Can.new()
    request := hal.CanFrame.standard(0x100, b"\x07")!
    can.inject(&request)
    serve(&mut can)!
    assert_eq(can.sent_count(), 1usize)
    assert_eq(can.sent(0).bytes()[0], 8u8)
}
```

#### CAN controllers on SPI

`std/embedded/can/mcp2515` drives the Microchip MCP2515, the stand-alone
CAN controller on most add-on CAN boards, over any SPI bus. Its `Controller`
owns the bus and a chip-select pin and implements `hal.Can`, so CAN drivers
run unchanged on boards such as the Pico, whose chip has no CAN controller:

```dodo
spi := b.pins.spi(board.SPI_CLOCK_PIN, board.SPI_TX_PIN, some(board.SPI_RX_PIN), hal.SpiMode.Mode0, 8_000_000)!
select := b.pins.output(board.SPI_SELECT_PIN)!
config := mcp2515.Config.new(16_000_000, 500_000)   // crystal, bit rate
filters: [1]hal.CanFilter = [hal.CanFilter.all(false)]
can := mcp2515.Controller.new(spi, select, &config, &filters)!
```

`Config` holds the board's crystal frequency (usually 8 or 16 MHz), the bit
rate, the sample point (875 per mille unless changed), the mode (`Normal`,
`Loopback`, or `ListenOnly`), and whether frames are sent once without
retrying. Startup resets the chip, programs the bit timing and filters, and
switches modes; a bit rate the crystal cannot reach is `InvalidInput`, and a
missing chip `Timeout`. `mcp2515.timing(crystal, bitrate, sample_point)`
shows the register values, and `release()` gives the bus and pin back.
`Controller.new` drops them if startup fails; `attach` and `configure` keep
them.

The chip has two receive buffers, one with a mask and two filters, the other
with a mask and four. The driver groups the given filters by their mask, so
at most two masks fit, with at most two filters on one and four on the
other; a standard and an extended filter share a mask when they cover the
same identifier bits. No filters accept everything. The buffers roll over,
so two frames wait in the chip before one is lost, and `poll_receive`
returns them oldest first and reports a lost one as `Overrun`, once.
`poll_send` queues a frame in one of three transmit buffers, and frames
leave in the order they were queued. On a bus where no other node
acknowledges, frames stay queued, so `hal.send_frame` times out;
`abort_pending()` empties the queue. `tec()` and `rec()` read the error
counters, and FD frames are `Unsupported`.

Wire the chip's INT pin to a GPIO and call `can.listen(mcp2515.RECEIVE)`: a
task then waits for the pin's falling edge and calls `poll_receive` until it
returns false, which releases INT. `TRANSMIT` and `ERROR` interrupts stay
raised until `acknowledge` clears them.

### I2S audio

`hal.I2sOutput` and `hal.I2sInput` stream audio samples to a DAC or
amplifier and from a microphone or ADC. Sample rate, resolution, channels,
and the framing (`hal.I2sFormat.Philips`, `LeftJustified`, or
`RightJustified`) are set when a stream is created:

| Interface | Methods |
| --- | --- |
| `hal.I2sOutput` | `sample_rate(&self) -> u32`, `channels(&self) -> u32`<br>`poll_write(&mut self, samples: &[i32]) -> usize!hal.Error` |
| `hal.I2sInput` | `sample_rate(&self) -> u32`, `channels(&self) -> u32`<br>`poll_read(&mut self, samples: &mut[i32]) -> usize!hal.Error` |

Samples are `i32` values with the audio in the most significant bits, so
full scale is the same at every resolution: a 16-bit sample is `value << 16`
(`hal.sample_from_i16` and `hal.sample_to_i16` convert). Stereo samples
interleave, left first, and both methods move whole frames only, returning
how many samples they took; a slice that is not whole frames is
`InvalidInput`. An output plays silence when its queue runs dry, so a task
keeps it topped up; an input reports `Overrun` once if samples were lost.
`hal.write_samples` and `hal.read_samples` wait until a whole slice has
moved.

### Transfers in the background (DMA)

`hal.DmaChannel` (see [DMA in drivers](#dma-in-drivers)) is what peripheral
drivers start transfers on. Device drivers work one level up, with a session
that holds a buffer and moves it while the program does other work:

| Interface | Methods |
| --- | --- |
| `hal.DmaBuffer` | `buffer(&mut self) -> &mut[u8]!hal.Error`, `capacity(&self) -> usize`<br>`start(&mut self, count: usize) -> void!hal.Error`<br>`poll_finish(&mut self) -> bool!hal.Error`, `abort(&mut self)` |
| `hal.SpiDma` | the same, with `start_transfer(count)` (exchange in place) and `start_write(count)` (send only) instead of `start` |

The session lends its buffer out only between transfers (`buffer()` is
`Busy` while one runs), so the program never touches memory a transfer is
moving. Fill it, `start` a transfer, and poll `poll_finish` until it is
true; `Bus` and `Overrun` report failed transfers. A display driver written
against `hal.SpiDma` streams frames on any chip whose SPI bus has DMA, and
on a desktop against a fake. `hal.finish_transfer(&mut session)` waits.

`hal.DmaStream` is the session every DMA-backed peripheral driver needs,
written once against `hal.DmaChannel`: `DmaStream.transmit(&mut channel,
&mut buffer, register, width)` sends a buffer to a peripheral's data register
whenever the peripheral asks for data, `DmaStream.receive(...)` fills one from
it, and the stream implements `hal.DmaBuffer`. The peripheral's driver turns
the peripheral's DMA requests on and off around it. `hal.dma_copy(&mut
channel, &mut destination, &source)` copies memory with any channel.

### Real-time clocks

A real-time clock keeps the time of day in a counter that runs while the
processor sleeps, and sometimes through resets or on a backup battery. The
protocol is the same whatever counts: `hal.Rtc` reads and sets the time
as milliseconds since the Unix epoch, 1970-01-01 00:00:00 UTC, the count a
[`time.Timestamp`](time.md) holds, and `hal.RtcAlarm` fires once when the
clock reaches a time. Every chip below implements both, so a program
that keeps time moves between them unchanged:

| Clock | Counts | Range | Keeps time |
| --- | --- | --- | --- |
| `rp2040.Rtc`, `b.rtc` on the Pico | Seconds | Years 1970 to 4095 | While the chip is powered; a reset stops it. |
| `rp2350.Rtc`, `b.rtc` on the Pico 2 and Pico 2 W | Milliseconds | 64 bits | Through resets that leave the always-on power domain running, such as the watchdog's. |
| `ds3231.Clock` on any `hal.I2c` | Seconds | Years 2000 to 2099 | On its backup battery while the board is off. |
| `fake.Rtc` | Milliseconds, or coarser steps | 64 bits | As a test says. |

A clock runs only once its time is set. Until then, and again after it lost
power, `is_set()` is false and `now_ms()` reports `Busy`, so firmware sets the
clock once, from the network, a GPS, or the user, and later boots keep the
time where the clock allows it. A clock that counts whole seconds reports whole
seconds; `resolution_ms()` says how long a step is, and `set_ms` rounds
down to one.

`hal.rtc_now` and `hal.rtc_set` convert to and from a UTC
`time.DateTime`, so the calendar never has to be done by hand. Shift by a
time zone's offset with the `DateTime` methods:

```dodo test
package clock_example

import "std/embedded/hal"
import "std/embedded/hal/fake"
import "std/time"

// Seconds since midnight, from any clock.
fn seconds_today<R: hal.Rtc>(clock: &mut R) -> u32!hal.Error {
    now := hal.rtc_now(clock)?
    return ok(now.time_of_day().seconds_since_midnight())
}

fn main() {
    clock := fake.Rtc.new()
    assert(!clock.is_set()!)
    start := time.DateTime.new(time.Date.new(2026, 11, 1)!, time.TimeOfDay.new(8, 30, 0, 0)!)
    hal.rtc_set(&mut clock, &start)!
    clock.advance(90_000)
    assert_eq(seconds_today(&mut clock)!, 30_690u32)

    hal.rtc_alarm_in(&mut clock, 60_000)!
    clock.advance(59_999)
    assert(!clock.alarm_fired()!)
    clock.advance(1)
    assert(clock.alarm_fired()!)
    clock.clear_alarm()!
}
```

An alarm fires when `now_ms()` reaches its time, also when the time is set
past it, and stays fired until `clear_alarm`. As for a counter's alarm, a time
the clock has already reached fires at once. On clocks that count seconds the
alarm fires at the start of a second, so `hal.rtc_alarm_in` fires up to a
second early. Chips raise an interrupt when it fires; their clock types add
`listen`, `ignore`, and `irq()` for it, as the
[RP2040 and RP2350](#real-time-clocks-on-the-rp2040-and-rp2350) do.

`std/embedded/rtc/ds3231` drives a DS3231 clock chip on an I2C bus, the
common battery-backed module, through the same protocols. It counts on its
battery while the board is off, so a board without an always-on domain of its
own can copy the time into its chip's clock at start-up:

```dodo
bus := b.pins.i2c(board.I2C_SDA_PIN, board.I2C_SCL_PIN, hal.I2C_FAST_HZ)!
battery := ds3231.Clock.new(bus)
if battery.is_set()! {
    b.rtc.set_ms(battery.now_ms()!)!
}
```

Its alarm, the chip's alarm 1, matches the day of the month and the time,
so it reaches 28 days ahead; while armed, it pulls the chip's INT/SQW pin
low when it fires, which a GPIO with a pull-up can wait on.

## A complete driver

Drivers usually own their bus while they are in use and give it back with a
`release` method. This sensor starts a conversion, polls a ready flag with a
bounded number of attempts, and reports a stuck device as `Timeout` instead of
hanging:

```dodo test
package sensor

import "std/embedded/hal"
import "std/embedded/hal/fake"

const ADDRESS: u8 = 0x48
const CONTROL: u8 = 0x01
const START: u8 = 0x80
const STATUS: u8 = 0x02
const READY: u8 = 0x01
const RESULT: u8 = 0x10

pub struct Sensor<B> {
    bus: B

    pub fn new(bus: B) -> Sensor<B> {
        return Sensor<B> { bus }
    }

    // Tenths of a degree Celsius.
    pub fn measure<D>(&mut self, delay: &mut D) -> i32!hal.Error {
        hal.write_register(&mut self.bus, ADDRESS, CONTROL, START)?
        for _ in 0..10 {
            hal.delay_ms(delay, 1)
            status := hal.read_register(&mut self.bus, ADDRESS, STATUS)?
            if (status & READY) != 0 {
                raw: [2]u8 = [0, 0]
                hal.read_registers(&mut self.bus, ADDRESS, RESULT, &mut raw)?
                return ok(((raw[0] as i32) << 8) | raw[1] as i32)
            }
        }
        return err(hal.Error.Timeout)
    }

    pub fn release(self) -> B {
        let Sensor { bus } = self
        return bus
    }
}

@test
fn measures() {
    bus := fake.I2c.new(ADDRESS)
    bus.set_register(STATUS, READY)
    bus.set_register(RESULT + 1, 215)
    sensor := Sensor.new(bus)
    delay := fake.Delay.new()
    assert_eq(sensor.measure(&mut delay)!, 215)
}

@test
fn reports_missing_device() {
    sensor := Sensor.new(fake.I2c.new(0x10))
    delay := fake.Delay.new()
    match sensor.measure(&mut delay) {
        ok(_) => {
            assert(false)
        }
        err(error) => {
            assert(error == hal.Error.NoAcknowledge)
        }
    }
}
```

## Running a driver on a board

On a board, the chip's buses take the fakes' places. `b.pins.i2c` claims two
pins as an I2C bus, and each board names its usual bus pins, so this program
from
[`examples/hal_sensor_board.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/hal_sensor_board.dodo)
runs the sensor driver of
[`examples/hal_sensor.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/hal_sensor.dodo),
the one above, unchanged:

```dodo
package main

import "std/embedded/board"
import "std/embedded/hal"
import "hal_sensor"

fn main() {
    b := board.take()!
    bus := b.pins.i2c(board.I2C_SDA_PIN, board.I2C_SCL_PIN, hal.I2C_FAST_HZ)!
    sensor := hal_sensor.Sensor.new(bus)
    for {
        match sensor.measure(&mut b.timer) {
            ok(_) => {
                b.led.set_high()
            }
            err(_) => {
                b.led.set_low()
            }
        }
        b.timer.delay_ms(1000)
    }
}
```

```sh
dodo run examples/hal_sensor_board.dodo --board pico
```

The board's timer is the delay, and the sensor's `measure` is the same
specialized code either way. SPI devices work the same way: claim the bus
pins with `b.pins.spi`, and give each device an output pin as its chip
select through `hal.SpiDevice`:

```dodo
b := board.take()!
spi := b.pins.spi(board.SPI_CLOCK_PIN, board.SPI_TX_PIN, some(board.SPI_RX_PIN), hal.SpiMode.Mode0, 1_000_000)!
flash := hal.SpiDevice.new(b.pins.output(board.SPI_SELECT_PIN)!)
command: [1]u8 = [0x9F]
id: [3]u8 = [0, 0, 0]
flash.write_read(&mut spi, &command, &mut id)!
```

Pass `none` instead of a receive pin for devices that only listen, such as
many displays. See [I2C and SPI](#i2c-and-spi-on-the-rp2040-and-rp2350) for
which pins can carry which bus.

## Testing with fakes

`std/embedded/hal/fake` implements every protocol in memory:

| Fake | Behaves like | Inspect with |
| --- | --- | --- |
| `fake.Pin` | An input/output pin, initially low. | `is_high()`, `changes()` |
| `fake.Delay` | A delay that returns at once, and a counter whose time is the delays so far. | `elapsed_us()`, `now_us()` |
| `fake.Alarm` | An alarm on a counter that starts at 0 and moves only on `advance`. It reaches any time ahead. | `advance(us)`, `is_armed()`, `target_us()`, `schedules()`, `fail_next(error)` |
| `fake.I2c` | One device at an address with 256 numbered registers and an auto-incrementing register pointer. Other addresses report `NoAcknowledge`. Also `hal.I2cPoll`: a polled transaction takes `set_poll_steps(n)` polls. | `register(n)`, `set_register(n, v)`, `fail_next(error)`, `set_poll_steps(n)`, `cancels()` |
| `fake.Spi` | A bus that records sent bytes and answers from a programmed reply. Also `hal.SpiPoll`, moving `set_poll_chunk(n)` bytes per poll. | `sent()`, `clear()`, `reply_with(bytes)`, `set_poll_chunk(n)`, `polls()` |
| `fake.Serial` | A serial port, `serial.Port` and `serial.PollPort`: it records what is written and reads back bytes given to `feed`, then reports end of input; `poll_read` reports Pending instead. | `sent()`, `clear()`, `feed(bytes)`, `set_write_room(n)`, `set_flush_polls(n)`, `flushes()` |
| `fake.Pwm` | A PWM output with a given `max_duty`, initially at duty 0. | `duty()`, `changes()`, `fail_next(error)` |
| `fake.Analog` | An analog input with a given `max_value` that reads the value given to `set`, after any given to `queue`, one per read. | `set(value)`, `queue(values)`, `reads()`, `fail_next(error)` |
| `fake.Rng` | A random number generator that fills with the bytes given to `queue`, then counts up 0, 1, 2, and so on, so tests know what a driver drew. | `queue(bytes)`, `drawn()`, `fail_next(error)` |
| `fake.Rtc` | A [real-time clock](#real-time-clocks) with an alarm, unset until set, counting milliseconds (`new()`) or coarser steps (`with_resolution(ms)`). Time passes only by `advance`. | `advance(ms)`, `lose_power()`, `is_armed()`, `alarm_ms()`, `fail_next(error)` |
| `fake.I2cTarget` | An I2C target and the master that talks to it, played by the test: the master's writes, reads, and STOPs come out of `poll` in order. | `master_write(bytes)`, `master_read(count)`, `master_stop()`, `master_received()`, `is_idle()`, `fail_next(error)` |
| `fake.SpiTarget` | An SPI target and the master that clocks it. | `master_exchange(bytes)`, `queued()` |
| `fake.PwmInput` | A pulse input that reports queued measurements, one per poll. | `queue(period_ns, high_ns, cycles)`, `queue_signal(hz, percent)`, `fail_next(error)` |
| `fake.Can` | A CAN controller on a simulated bus: it records frames sent and receives injected ones, optionally its own. | `inject(&frame)`, `sent_count()`, `sent(n)`, `waiting()`, `set_loopback(on)`, `set_transmit_room(n)`, `drain_transmit()`, `set_error_state(state)`, `fail_next(error)`; `new_fd()` for CAN FD |
| `fake.I2sOutput`, `fake.I2sInput` | Audio streams that record what is played, or deliver what is fed, a limited number of samples per poll. | `played()`, `total()`, `feed(samples)`, `set_room(n)`, `drop_samples()` |
| `fake.DmaBuffer` | A DMA buffer session, transmitting or receiving, whose transfers finish after a number of polls. | `sent()`, `feed(bytes)`, `set_poll_steps(n)`, `fail_next(error)` |
| `fake.DmaChannel` | A DMA channel that runs transfers in host memory, for testing code written against `hal.DmaChannel`, such as `hal.DmaStream`. | `set_busy_polls(n)`, `set_max_count(n)`, `starts()`, `aborts()`, `fail_next()` |
| `fake.Clock` | The [executor](#tasks-and-the-executor)'s clock, and a counter: time jumps ahead when the executor sleeps, and simulated interrupts raise signals. | `now_us()`, `interrupt_at(time, &signal)`, `advance(us)`, `sleeps()`, `idle_us()` |

`fail_next` makes the next I2C transaction, duty change, analog read,
alarm schedule, clock operation, or random fill fail,
so error paths are as easy to test as success. The repository's
`examples/dimmer.dodo` tests an LED dimmer, a knob on an analog input setting
a PWM output, this way, and `examples/dimmer_board.dodo` runs it unchanged on
a board. A driver for a device on a serial line, such as a GPS or
modem module, takes any reader and writer, so `fake.Serial` stands in for the
board's port:

```dodo test
package probe

import "std/embedded/hal/fake"
import "std/fmt"
import "std/io"

// Send a command line and read the reply line into `reply`.
pub fn query<S>(serial: &mut S, command: &str, reply: &mut[u8]) -> usize!io.Error {
    fmt.printf(serial, "{}\r\n", command)?
    line := io.read_line(serial, reply)?
    return ok(line.count)
}

@test
fn sends_a_command_and_reads_the_reply() {
    serial := fake.Serial.new()
    serial.feed(b"OK 21.5\n")
    reply: [16]u8 = [0; 16]
    assert_eq(query(&mut serial, "TEMP?", &mut reply)!, 8usize)
    assert_eq(serial.sent().len, 7usize)
}
```

Peripheral drivers are tested the same way one level down: a simulated
controller implements the register block protocol, as in
[`tests/stdlib/peripheral_checks.dodo`](https://github.com/Jotrorox/dodo/blob/main/tests/stdlib/peripheral_checks.dodo).
A protocol is just methods, so writing your own fake for a device with
special behavior takes a few lines.

## Registers

A peripheral is a block of memory-mapped registers. `hal.Reg<T>` is a handle to
one register holding `T`, an unsigned integer of 8, 16, 32, or 64 bits. Each
read or write is exactly one volatile access of `T`: the compiler never removes,
merges, reorders, or splits it relative to other device accesses.

Creating a handle with `hal.Reg.at(address)` is the only unsafe step. In its
`unsafe` block you promise that the address is a valid, aligned register of
that width for your chip and that no other code uses it in a conflicting way.
After that, every operation on the handle is safe. `Reg` is move-only, so the
handle itself is the permission to use the register.

Name register fields with mask constants of the register's type, as a
datasheet or C header does. A mask must be nonzero and contiguous:

```dodo
package uart

import "std/embedded/hal"
import "std/io"

const BASE: usize = 0x4003_4000
const ENABLE: u32 = 1 << 0
const PARITY: u32 = 0b11 << 1
const BAUD_DIVISOR: u32 = 0xFFFF << 16
const TX_READY: u32 = 1 << 5

pub struct Uart {
    data: hal.Reg<u32>
    status: hal.Reg<u32>
    control: hal.Reg<u32>

    // SAFETY: call once; nothing else may use this UART's registers.
    pub unsafe fn take() -> Uart {
        unsafe {
            return Uart {
                data: hal.Reg.at(BASE + 0x00),
                status: hal.Reg.at(BASE + 0x18),
                control: hal.Reg.at(BASE + 0x30),
            }
        }
    }

    pub fn configure(&mut self, divisor: u32) {
        self.control.write(hal.encode(BAUD_DIVISOR, divisor) | ENABLE)
    }

    // The std/io Writer protocol: this UART now works with io and fmt.
    pub fn write(&mut self, bytes: &[u8]) -> usize!io.Error {
        for index in 0..bytes.len {
            match self.status.wait_set(TX_READY, 100_000) {
                ok() => {}
                err(_) => {
                    return err(io.failure(io.ErrorKind.TimedOut, index))
                }
            }
            self.data.write(bytes[index] as u32)
        }
        return ok(bytes.len)
    }
}
```

A transmitter that never becomes ready yields an `io` timeout error reporting
how many bytes were sent. The register operations are:

| Operation | Effect |
| --- | --- |
| `read()` / `write(value)` | One volatile load or store. |
| `get(MASK)` | The field's value, shifted down: `get(PARITY)` is 0 to 3. |
| `set(MASK, value)` | Read-modify-write of one field. Traps if `value` does not fit. |
| `set_bits(mask)` / `clear_bits(mask)` | Read-modify-write of individual bits. |
| `modify(clear, set)` | Clear then set bits in one read-modify-write. |
| `any(mask)` | True if any of the bits is one. |
| `wait_set(mask, attempts)` / `wait_clear(mask, attempts)` | Poll a flag; `Error.Timeout` after `attempts` reads. |
| `hal.encode(MASK, value)` / `hal.decode(MASK, raw)` | Field arithmetic on plain values, for composing a full `write`. |

Field operations compile to shifts and masks; nothing is computed at run time
that a C programmer would not also write. A value that does not fit a field
traps instead of silently corrupting the neighbouring field.

Read-modify-write is not atomic. If an interrupt handler can change the same
register, wrap the update in a critical section or use the chip's set/clear
alias registers, which many microcontrollers provide for exactly this reason.

### Register blocks

`hal.RegisterBlock` is a peripheral's 32-bit registers at byte offsets from
one base address: `hal.RegisterBlock.at(base)` is unsafe like `Reg.at`, and
`read(offset)` and `write(offset, value)` are each one volatile access.
Peripheral drivers in `std/embedded/peripheral` are generic over the
register block protocol rather than built from `Reg` fields, for two reasons:
the same controller sits at different addresses in different chips, and a
desktop test can pass a simulated controller instead. `read` takes
`&mut self` because reading a device register can change it, for example by
popping a FIFO.

| Package | Controller | Implements |
| --- | --- | --- |
| `std/embedded/peripheral/designware_i2c` | Synopsys DesignWare DW_apb_i2c | The I2C bus protocol, as the only master or as a target: 7-bit and 10-bit addresses, up to 1 MHz. |
| `std/embedded/peripheral/pl022` | Arm PrimeCell PL022 | The SPI bus protocol, as master or target: 4- to 16-bit frames, all four modes, TI and Microwire formats. |
| `std/embedded/peripheral/pl011` | Arm PrimeCell PL011 | The `std/io` Reader and Writer, and their polling forms: 5 to 8 data bits, parity, 1 or 2 stop bits, flow control, breaks, bounded waits. |

The bus drivers keep the controller's FIFOs busy without ever queueing more
than the receive FIFO holds, and bound every wait on the bus, so a device
that holds the I2C clock low yields `Timeout` instead of a hang. Chip
packages hand out their controllers as the driver's `Bus` or `Port`, which
keeps the register block from the application: `pins.i2c` on the RP2040 and
RP2350 returns a `designware_i2c.Bus`, `pins.spi` a `pl022.Bus`, and
`pins.uart` a `pl011.Port`.

Each driver's generic `Controller<R>` also runs against register models on
a desktop. The PL011's implements the `std/io` Reader and Writer, so
`io.read_exact`, `io.write_all`, buffering, and formatting work with UARTs.
`designware_i2c` and `pl022` also have a `Target` for answering another
master.

### Owning peripherals once

Chip support usually hands each peripheral out once. The bundled
[`std/embedded/chip/rp2040`](#the-rp2040-and-raspberry-pi-pico) package does this with a
safe `rp2040.take(config)` that returns `hal.Error.Busy` on a second call, and
`pins.output(n)` calls that return `hal.Error.Busy` if a pin is claimed twice.
The
[GPIO example](https://github.com/Jotrorox/dodo/blob/main/examples/gpio.dodo)
shows the same pattern as a minimal template for writing support for another
chip: an unsafe `Port.take()` at startup and safe per-pin claims. Its `Pin` implements the output and input protocols with single stores to
the chip's set/clear registers, and the whole abstraction compiles to the same
instructions as hand-written register code.

## Interrupts

Mark a function with `@interrupt(VECTOR)` and the hardware runs it for that
vector-table entry. The vector names are the datasheet's, and with a board or
chip selected the compiler checks them, so a misspelled `TIMER_IRQ0` is an
error rather than a handler that never runs:

```dodo
package firmware

import "core/cpu"
import "std/embedded/cortex_m"
import "std/embedded/hal"

static mut TICKS: u32 = 0

// Cortex-M SysTick exception.
@interrupt(SysTick_Handler)
fn tick() {
    unsafe {
        TICKS += 1
    }
}

fn ticks() -> u32 {
    section := hal.Critical.enter()
    value := unsafe { TICKS }
    core.drop(section)
    return value
}

fn main() {
    // A tick every 125,000 cycles: 1 ms at 125 MHz.
    cortex_m.systick_start(124_999, true)!
    for ticks() < 1000 {
        cpu.wait_for_interrupt()
    }
}
```

A handler takes no arguments and returns nothing, and only the hardware calls
it: calling `tick()` from code is an error. Put work that code also needs in an
ordinary function and call that from the handler. A program has one handler
per vector. The compiler exports it under the vector's name, which replaces
the runtime's default, an endless loop. On Cortex-M the hardware saves the
registers a C function may change, so a handler is an ordinary C-ABI function;
on bare-metal RISC-V the compiler saves every register it uses and returns
with `mret`.

The handler and the code it interrupts share state through static storage: a
single integer through an [atomic](#sharing-state-with-atomics), anything else
through a [mutex](#sharing-values-with-a-mutex) from `std/embedded/sync`.
Both work in a plain `static` and need no `unsafe`. Underneath them,
`hal.Critical.enter()` masks interrupts on the current core and remembers
whether they were enabled; dropping it restores exactly that state, so
sections nest. Raw `static mut` storage is still available, but reading or
writing it is unsafe, and keeping both sides consistent is then the
program's job.

### Sharing state with atomics

An integer that a handler and the main program share needs neither `unsafe`
nor a critical section when it is a `std/sync/atomic` atomic. A `static`
(not `static mut`) holds it, initialized with `atomic.Atomic.new`, and both
sides call its methods:

```dodo
package firmware

import "core/cpu"
import "std/embedded/cortex_m"
import "std/sync/atomic"

static TICKS: atomic.Atomic<u32> = atomic.Atomic.new(0u32)

@interrupt(SysTick_Handler)
fn tick() {
    TICKS.fetch_add(1, atomic.Ordering.Relaxed)!
}

fn main() {
    cortex_m.systick_start(124_999, true)!
    for TICKS.load(atomic.Ordering.Relaxed)! < 1000 {
        cpu.wait_for_interrupt()
    }
}
```

Each call is one indivisible step that an interrupt cannot split, but two
calls are two steps: to change a value based on what it was, use
`fetch_add`, `exchange`, or a `compare_exchange` loop rather than a `load`
followed by a `store`. `Ordering.Relaxed` is enough for a value read on its
own, such as this counter. To hand over other data, write the data, then
`store` a flag with `Ordering.Release`; the side that `load`s the flag with
`Ordering.Acquire` and sees it set also sees the data.
[Synchronization](synchronization.md#atomics) describes the methods and
orderings.

A static that holds an atomic is placed in RAM even though it is not `mut`.
A `const` cannot hold one: each use of a constant is a separate copy.

What the core provides decides which methods a build may use:

| Core | Atomic operations |
| --- | --- |
| Cortex-M33 (RP2350), and Cortex-M3, M4, M7, M23, M55, and M85 | All, on 8-, 16-, and 32-bit integers, with exclusive loads and stores. |
| Cortex-M0+ (RP2040) and Cortex-M0 | `load` and `store` only. `exchange`, `fetch_add`, or `compare_exchange` where the program can reach it is a build error, since the core has no instructions for them. |

On the RP2040 an atomic still shares a value that only one side writes: the
handler `load`s the count, adds one, and `store`s it, which is safe as long
as nothing else stores to it. A value that both sides change needs a critical
section there.

### Sharing values with a mutex

Anything larger than a word, such as a driver or a buffer, goes into a
`sync.Mutex<T>` from `std/embedded/sync`, in a plain static initialized
with `sync.Mutex.new`. `lock()` masks interrupts on this core and returns a
guard whose `get()` and `get_mut()` reach the value; dropping the guard
unlocks it and restores the interrupt mask:

```dodo test
package main

import "std/embedded/sync"

struct Stats {
    samples: u32
    peak: u32
}

static STATS: sync.Mutex<Stats> = sync.Mutex.new(Stats { samples: 0, peak: 0 })

// Called from an interrupt handler and from the main loop alike.
fn record(value: u32) {
    guard := STATS.lock()
    stats := guard.get_mut()
    stats.samples += 1
    if value > stats.peak {
        stats.peak = value
    }
}

fn main() {
    record(12)
    record(40)
    record(7)
    peak := 0u32
    STATS.with(fn(stats: &mut Stats) { peak = stats.peak })
    assert_eq(peak, 40u32)
}
```

`with(closure)` runs a closure on the value under the lock, `set(value)` replaces the value (dropping the old one outside the
lock), and `replace(value)` returns the old one, so `SERIAL.replace(none)`
takes a driver back. `try_lock()` returns none instead of locking a mutex
this core holds already, which `lock()` reports with a trap, since a second
guard would be a second exclusive reference. Mutexes nest: code holding one
may lock another.

| Share | With | Notes |
| --- | --- | --- |
| A counter or flag that one side changes | `atomic.Atomic<T>` | `load` and `store` everywhere; read-modify-write except on the Cortex-M0+. |
| A word both sides change | `atomic.Atomic<T>`, or a mutex on the Cortex-M0+ | One indivisible step per call. |
| A driver, buffer, or record | `sync.Mutex<T>` | Interrupts on this core wait while it is held; so does the other core of a [running RP2040 or RP2350 pair](#two-cores-on-the-rp2040-and-rp2350). |
| An event that wakes a task | `executor.Signal` | See [waking tasks from interrupts](#waking-tasks-from-interrupts). |

Every mutex locks through one system-wide section, `sync.enter()`, which
other code can take too. Keep it short: interrupts on the core wait while a
guard lives. On a desktop, where there are no interrupts, a mutex excludes
nothing, so use these mutexes from one thread there, as in tests, and
[`std/sync`](synchronization.md)'s for threads.

### An interrupt-driven driver

A driver can live in a mutex that the handler serves. The repository's
[`examples/interrupts`](https://github.com/Jotrorox/dodo/blob/main/examples/interrupts/main.dodo)
echoes a serial port this way: the UART's receive interrupt moves bytes into a
queue, SysTick counts milliseconds, and the main loop sleeps until either has
news:

```dodo
static SERIAL: sync.Mutex<Option<pl011.Port>> = sync.Mutex.new(none)
static QUEUE: sync.Mutex<Queue> = sync.Mutex.new(Queue { bytes: [0u8; 64], start: 0, queued: 0, dropped: 0 })

@interrupt(UART0_IRQ)
fn serial_received() {
    serial := SERIAL.lock()
    if let some(port) = serial.get_mut() {
        buffer := [0u8; 32]
        received := 0usize
        match port.poll_read(&mut buffer) {
            ok(progress) => {
                received = progress.transferred
            }
            err(failure) => {
                received = failure.transferred
                port.clear_interrupts(pl011.ERROR_INTERRUPTS)
            }
        }
        queue := QUEUE.lock()
        queue.get_mut().push(&buffer[..received])
    }
}

fn main() {
    b := board.take()!
    port := b.pins.uart(board.UART_TX_PIN, some(board.UART_RX_PIN), 115_200)!
    port.listen(pl011.RX_INTERRUPT | pl011.RX_TIMEOUT_INTERRUPT | pl011.ERROR_INTERRUPTS)
    SERIAL.set(some(port))
    cortex_m.enable(chip.UART0_IRQ)
    // ... drain QUEUE, sleeping with cpu.wait_for_interrupt() while it is empty
}
```

Three switches must all be on for a handler to run: the peripheral's own
interrupt enable (`port.listen`, `alarm.listen`), the interrupt's line in the
core's interrupt controller (`cortex_m.enable`), and the core's interrupt mask,
which is open at reset and closed only inside critical sections.

| Driver | Interrupt sources |
| --- | --- |
| `pl011.Port` and `Controller` | `listen(sources)`, `ignore(sources)`, `pending_interrupts()`, and `clear_interrupts(sources)` with `RX_INTERRUPT` (FIFO 3/4 full), `RX_TIMEOUT_INTERRUPT` (bytes waiting for 32 bit times), `TX_INTERRUPT`, and `ERROR_INTERRUPTS`. The receive sources end when reads drain the FIFO. |
| `Timer.alarm(n)` on the RP2040 and RP2350 | An `Alarm` with the [alarm protocol](#device-protocols): it fires once at `schedule_at_us(time)`, a `now_us()` time up to 2^31 - 1 us (35 minutes) ahead, or at the low 32 bits of the counter with `schedule_at(time)` and `schedule_in(microseconds)`. `listen` raises `alarm.irq()`, and its handler calls `clear`; a time `schedule_at_us` finds already passed raises it at once. `release_alarm` gives it back. With `now_us()` and `sleep_until(deadline)` it is also the [executor](#tasks-and-the-executor)'s clock. |
| `Pin` on the RP2040 and RP2350 | `listen(events)` raises `IO_IRQ_BANK0` for `chip.RISING_EDGE`, `FALLING_EDGE`, `LEVEL_HIGH`, or `LEVEL_LOW`; `events()` shows them, `clear_events(edges)` acknowledges edges, `ignore(events)` stops. |

### The Cortex-M core

`std/embedded/cortex_m` drives what every Cortex-M core has, whatever the
chip. Interrupt numbers are the chip package's constants, such as
`rp2040.UART0_IRQ` (also reachable as `chip.UART0_IRQ`), which match the
vector names:

| Function | Effect |
| --- | --- |
| `enable(irq)`, `disable(irq)`, `is_enabled(irq)` | Let an interrupt reach the core, or hold it back. |
| `pend(irq)`, `unpend(irq)`, `is_pending(irq)` | Raise or forget an interrupt from software, for example to test a handler. |
| `set_priority(irq, priority)`, `priority(irq)` | 0 is the most urgent; a handler is interrupted only by more urgent ones. The Cortex-M0+ keeps the top 2 bits, the RP2350's M33 the top 4. |
| `pend_sv()` | Run the `PendSV_Handler` once no more urgent handler is active, to defer work out of an urgent one. |
| `systick_start(reload, interrupt)`, `systick_stop()`, `systick_value()`, `systick_wrapped()` | The core's 24-bit down-counter at the processor clock; with `interrupt`, `SysTick_Handler` runs at each wrap. |
| `set_vector_table(address)` (unsafe), `vector_table()` | Move the vector table, for example to RAM. |
| `enable_interrupts()` (unsafe), `interrupts_masked()`, `active_exception()` | `cpsie i`, PRIMASK, and IPSR: the running handler's exception number. |
| `wait_for_event()`, `send_event()`, `data_barrier()`, `memory_barrier()`, `instruction_barrier()` | `wfe`, `sev`, `dsb`, `dmb`, and `isb`. |

### Handlers registered at run time

A vector-table entry is fixed when the firmware is linked. To choose or change
the work an interrupt does while the program runs, keep a function pointer in a
`hal.Handler` slot and call it from the handler:

```dodo
package firmware

import "std/embedded/hal"

static mut TIMER: hal.Handler = hal.NO_HANDLER
static mut TICKS: u32 = 0

fn count_tick() {
    unsafe {
        TICKS += 1
    }
}

@interrupt(TIMER_IRQ_0)
fn timer_fired() {
    unsafe {
        TIMER.run()
    }
}

fn main() {
    section := hal.Critical.enter()
    unsafe {
        TIMER.set(count_tick)
    }
    core.drop(section)
}
```

`set` stores the handler, `clear` empties the slot, `is_set` reports whether
one is stored, and `run` calls it and returns false when the slot is empty. A
handler is a plain [function pointer](types-and-functions.md#functions-are-values-and-closures-capture-locals)
`fn()`: it captures no local variables, so it stays valid for the whole program
and keeps its state in `static mut` storage. Closures that capture locals are
references to those locals and cannot be stored in a static. Change a slot that
an interrupt can read inside a critical section, as above. The same pattern
works for any table of callbacks a driver wants to select at run time, such as
`[4]Option<fn(u32)>`. For a whole table, a RAM vector table installed with
`cortex_m.set_vector_table` works too.

### Sections and alignment

`@section("name")` puts a function or static in a named section of the object
file, and `@align(N)` aligns it to `N` bytes, a power of two. The RP2040 and
RP2350 linker scripts give two sections a meaning:

| Section | Effect |
| --- | --- |
| `.ramfunc` | Copied from flash to SRAM at reset with the initialized statics, so the function runs from SRAM: no flash wait states, and it keeps running while flash is being written. |
| `.noinit` | In SRAM but neither loaded nor zeroed at reset, so the value survives a watchdog or software reset. Its value after power-up is unpredictable. |

```dodo
@section(".ramfunc")
fn checksum(bytes: &[u8]) -> u32 {
    // ...
}

// Counts resets that kept SRAM powered.
@section(".noinit")
static mut RESETS: u32 = 0

// A vector table in RAM: VTOR needs it aligned to its size.
@align(256)
static mut VECTORS: [48]u32 = [0u32; 48]
```

A function or static with `@section` is kept even when nothing refers to it,
so a custom linker script can place a header or table the hardware reads.
Other section names land wherever the linker script's rules put them.
`@align` never lowers a type's own alignment.

### Inline assembly

`asm(...)` runs instructions Dodo has no expression for, inside an `unsafe`
block. Each template string is one instruction; `{0}`, `{1}`, ... name the
operands in order:

```dodo
unsafe {
    asm("cpsie i")
    primask := asm("mrs {0}, primask", out(reg) u32, options(nomem))
    sum := asm("adds {0}, {1}, {2}", out(reg) u32, in(reg) a, in(reg) b)
    let (low, high) = asm(
        "mrs {0}, msp",
        "mrs {1}, psp",
        out(reg) u32,
        out(reg) u32,
    )
}
```

`in(reg) value` passes a value in a register, `out(reg) Type` returns one, and
`inout(reg) value` does both; `in("r0") value` picks the register. The
expression returns nothing, the one output, or a tuple of several. A template
is a memory barrier and is never removed unless `options(nomem)` or
`options(pure)` says otherwise, and `clobber("r4")` lists other registers it
overwrites. See [memory and foreign calls](memory-and-ffi.md#inline-assembly)
for the full rules.

### Naked handlers and global assembly

A `@naked` function has no prologue or epilogue, so its assembly sees the
registers and stack exactly as the caller, or the hardware, left them. That is
what a fault handler needs to find the stack frame the processor pushed. The
chip runtime's vector table refers to weak handler symbols, so a C function
with the vector's name replaces the default:

```dodo
import "core/ptr"

static mut FAULT_PC: u32 = 0

// Thumb (Cortex-M0+). Bit 2 of EXC_RETURN in lr says which stack the
// processor pushed r0-r3, r12, lr, pc, and xPSR to.
@naked
extern "C" fn HardFault_Handler() {
    unsafe {
        asm(
            "movs r0, #4",
            "mov r1, lr",
            "tst r0, r1",
            "mrs r0, msp",
            "beq 1f",
            "mrs r0, psp",
            "1:",
            "ldr r1, ={0}",
            "bx r1",
            sym record_fault,
        )
    }
}

extern "C" fn record_fault(frame: *const u32) {
    unsafe {
        FAULT_PC = ptr.read(ptr.offset(frame, 6))
    }
    for {
    }
}
```

`sym record_fault` writes the function's symbol into the template, and
`const` writes a constant's value. Top-level `unsafe asm(...)` is global
assembly, for code or data with no Dodo declaration around it. On Cortex-M,
mark a function label with `.thumb_func` so its address has the Thumb bit set:

```dodo
unsafe asm(
    ".section .text.add_three,\"ax\",%progbits",
    ".global add_three",
    ".thumb_func",
    "add_three:",
    "    adds r0, r0, #{0}",
    "    bx lr",
    const 3,
)

unsafe extern "C" fn add_three(value: u32) -> u32
```

See [global assembly](memory-and-ffi.md#global-assembly) and
[naked functions](memory-and-ffi.md#naked-functions) for the rules.

### Sleeping and other targets

`cpu.wait_for_interrupt()` sleeps until an interrupt is pending, which is the
usual idle loop. How deeply it sleeps is set with
[`std/embedded/power`](#low-power). `cpu.fence()` is a full hardware memory barrier, for example
between filling a DMA buffer and starting the transfer.

| Target | Critical section and `wait_for_interrupt` |
| --- | --- |
| Arm Cortex-M (`thumbv6m`, `thumbv7m`, `thumbv7em`, `thumbv8m*` with OS `none`) | PRIMASK; `wfi`. |
| RISC-V machine mode (`riscv32`, `riscv64` with OS `none`) | `mstatus.MIE`; `wfi`. |
| Linux, Windows, macOS, BSDs, WASI | No-op: a hosted program has no interrupts to mask. |
| Other freestanding targets | Compile error if a reachable function uses them. |

Because hosted targets accept critical sections, code that uses them still
builds and tests on a desktop. Packages that contain a critical section but
never call it on a reachable path stay portable to every target.

## Tasks and the executor

A main loop that polls everything in turn works for blinking an LED. It stops
working once a program does several things at their own pace: read a sensor
every two seconds, answer a serial console, debounce a button, keep a radio
link alive. `std/embedded/executor` lets each activity be a **task** of its own.
The executor polls a task only when something it waits for has happened, a
time arriving or an interrupt, and puts the core to sleep while none has work.

A task is any value with a `poll` method. Most tasks are easiest to write as
[async functions](#async-tasks), which the compiler turns into such values;
underneath, a task is a struct that owns the peripherals it uses and
remembers where it is in its work:

```dodo
struct Blinker<L> {
    led: &mut L
    ticker: executor.Ticker

    pub fn poll(&mut self, cx: &mut executor.Context) -> executor.Poll {
        if self.ticker.ready(cx) {
            self.led.toggle()
        }
        return executor.Poll.Pending
    }
}
```

`poll` does what it can right now, never waiting, and returns
`executor.Poll.Pending` to be polled again later or `executor.Poll.Ready`
when the task is done. Before it returns Pending it says what should poll it
again: a time, through the context `cx` or a `Ticker` or `Timer`, or a
`Signal` it took. Each poll states this anew, and a task that arranges
nothing is not polled again. A task can also be polled when nothing it waits
for has happened, and then simply checks again.

Tasks run one after another and are never interrupted by each other, so they
share nothing that needs a critical section. A task that computes for long,
or busy-waits, delays every other task: split long work into steps with
`cx.yield_now()`.

### Running tasks on a board

Put references to the tasks in an array of `&mut dyn executor.Task` and hand
it to `executor.run` with a clock. On the RP2040 and RP2350 the clock is a
timer alarm, `b.timer.alarm(n)`:

```dodo
fn main() {
    b := board.take()!
    clock := b.timer.alarm(0)!
    blinker := Blinker { led: &mut b.led, ticker: executor.Ticker.every_ms(250) }
    console := Console.new(b.pins.uart(board.UART_TX_PIN, some(board.UART_RX_PIN), 115_200)!)
    tasks: [2]&mut dyn executor.Task = [&mut blinker, &mut console]
    executor.run(&mut tasks, &mut clock)
}
```

`run` polls every task once, then only the tasks that are due, and returns
when all of them are Ready; firmware whose tasks run forever stays in it.
When no task is due, the executor masks interrupts, checks once more that no
interrupt has woken a task, and calls the clock's `sleep_until` with the
earliest time any task asked for. The alarm arms itself for that time and
sleeps with `wfi`. An interrupt that arrives, the alarm's own included, ends
the sleep; the alarm clears its interrupt before interrupts are unmasked, so
it needs no handler, and other interrupts' handlers run as the executor lifts
the mask. Nothing can slip in between the check and the sleep.

| To wait for | Use | Notes |
| --- | --- | --- |
| A period | `executor.Ticker.every(us)`, `every_ms(ms)`; `ticker.ready(cx)` | True once per period, first one period after the first call. Keeps its schedule; missed periods count once. `reset()` starts over. |
| A one-shot deadline | `executor.Timer.new()`; `start(cx, us)`, `start_ms`, `start_at`; `expired(cx)` | True once when the time has passed. `stop()`, `is_running()`. |
| A time | `cx.wake_at(time)`, `wake_in(us)`, `wake_in_ms(ms)` | Clock microseconds; the earliest request counts. `cx.now_us()` is the time the poll started. |
| The other due tasks | `cx.yield_now()` | Polled again right after them, without sleeping. |
| An event | `SIGNAL.take(cx)`, `take_count(cx)`, `receive(cx)` | See below. |

An `executor.Executor` value does the same as `run` and keeps its state
between calls: `run_until(tasks, clock, end_us)` returns false when the clock
reaches `end_us` before every task is done, so a program or test can advance
in steps. `polls()`, `sleeps()`, and `is_finished(index)` report what it did.
`executor.Context.new(now_us)` makes a context for calling a task's `poll`
directly, and `cx.wake_time()` shows what the task asked for.

### Async tasks

Written by hand, a task keeps where it is in its fields and returns to the
executor at every step. An [`async fn`](async.md) writes the same task as
straight-line code: each `await` returns to the executor until what it
waits for has happened, and the next poll continues after it. Calling the
function makes the task, a future, which goes into the task list like any
other:

```dodo
const BUTTON: executor.Signal = executor.Signal { id: 1 }
const PRESSES: executor.Signal = executor.Signal { id: 2 }

async fn blink<L: hal.OutputPin>(led: &mut L, period_ms: u32) {
    ticker := executor.Ticker.every_ms(period_ms)
    for {
        led.set_high()
        await ticker.tick()
        led.set_low()
        await ticker.tick()
    }
}

// Counts presses, ignoring the contacts' bounce.
async fn count_presses(pin: &mut chip.Pin) {
    presses := 0u32
    for {
        cortex_m.enable(chip.IO_IRQ_BANK0)
        await BUTTON.wait()
        if (pin.events() & chip.RISING_EDGE) != 0 {
            pin.clear_events(chip.RISING_EDGE)
            presses += 1
            PRESSES.send(presses)
            await executor.sleep_ms(50)
            pin.clear_events(chip.RISING_EDGE)
        }
    }
}

fn main() {
    b := board.take()!
    clock := b.timer.alarm(0)!
    pin := b.pins.input(15)!
    pin.listen(chip.RISING_EDGE)
    blinker := blink(&mut b.led, 250)
    button := count_presses(&mut pin)
    console := Console { port: b.pins.uart(board.UART_TX_PIN, some(board.UART_RX_PIN), 115_200)! }
    tasks: [3]&mut dyn executor.Task = [&mut blinker, &mut button, &mut console]
    executor.run(&mut tasks, &mut clock)
}
```

Here `console` is a hand-written task; async and hand-written tasks mix
freely, since a future has the task's `poll` method. The repository's
[`examples/async_tasks`](https://github.com/Jotrorox/dodo/blob/main/examples/async_tasks/main.dodo)
writes all three tasks of `examples/tasks` as async fns. A future keeps its
locals between polls, and lives where its owner put it: on `main`'s stack
above, or in a `static mut` for a large one. Once started, it must stay in
place, as a reference into it may be held across an `await`. The
[async functions](async.md) chapter describes `await` and what it accepts:
`executor.sleep_ms`, `ticker.tick()`, `SIGNAL.wait()`, `SIGNAL.next_value()`,
`executor.yield_now()`, `executor.now()`, other async fns, and `executor.select` and
`executor.join` to wait for either or both of two futures.

### Waking tasks from interrupts

A **signal** carries an event from an interrupt handler, or from another task,
to the tasks waiting for it. A program numbers its signals from 0 to 31 with
constants that handlers and tasks both see:

```dodo
const BUTTON: executor.Signal = executor.Signal { id: 1 }

@interrupt(IO_IRQ_BANK0)
fn gpio_interrupt() {
    // Hold the interrupt back until the task has acknowledged the pin.
    cortex_m.disable(chip.IO_IRQ_BANK0)
    BUTTON.raise()
}

struct Button {
    pin: chip.Pin
    presses: u32

    pub fn poll(&mut self, cx: &mut executor.Context) -> executor.Poll {
        BUTTON.take(cx)
        if (self.pin.events() & chip.RISING_EDGE) != 0 {
            self.pin.clear_events(chip.RISING_EDGE)
            self.presses += 1
        }
        cortex_m.enable(chip.IO_IRQ_BANK0)
        return executor.Poll.Pending
    }
}
```

`raise()` counts the event and wakes every task that took the signal since
it was last raised. `take(cx)` reports whether it was raised since it was last
taken, takes it, and in either case makes the polled task wait for the next
raise, so a task that takes its signal on every poll never misses one.
`take_count(cx)` returns how many raises there were. `send(value)` raises
with a `u32` and `receive(cx)` returns the latest value, for example to
pass a reading or a count from one task to another. `is_raised()` looks
without taking and `reset()` forgets everything. Raising and taking are safe
in handlers and tasks alike; they use a critical section.

The handler above does the least it can: it holds its interrupt back in the
interrupt controller and raises the signal. The task that owns the pin then
acknowledges the event and lets the interrupt through again. Peripherals thus
stay owned by one task, need no `static mut`, and a handler never has to
share a driver with a task. The same pattern serves a serial port: listen for
`pl011.RX_INTERRUPT | pl011.RX_TIMEOUT_INTERRUPT`, and let the task drain the
port with `poll_read` before it enables `chip.UART0_IRQ` again. Read the port
at least every 2.8 ms while bytes arrive at 115200 baud, as its FIFO holds
32 bytes, or give it a [DMA ring](#serial-ports-by-dma), which holds as
much as its size and lets the task poll at its own pace. The repository's
[`examples/tasks`](https://github.com/Jotrorox/dodo/blob/main/examples/tasks/main.dodo)
runs a blinking LED, a debounced button, and such a serial console together.

| Interrupt source | Raise it | Acknowledge it |
| --- | --- | --- |
| GPIO pin (RP2040, RP2350) | `pin.listen(events)` with `chip.RISING_EDGE`, `FALLING_EDGE`, `LEVEL_HIGH`, `LEVEL_LOW`; all pins share `IO_IRQ_BANK0` (`pin.irq()`) | `pin.events()` shows what is raising it; `pin.clear_events(edges)` clears latched edges. Levels last as long as the level. `ignore(events)` stops. |
| Serial port | `port.listen(sources)` | Reading drains the receive sources; `clear_interrupts` the errors. |
| Timer alarm | `alarm.listen()` | `alarm.clear()`. An alarm used as the executor's clock is busy. |
| USB serial port | `serial.set_signal(&SIGNAL)`: the port's own interrupt handler raises it | Nothing to acknowledge: read with `poll_read` until it is Pending. |
| DMA channel | `channel.set_signal(&SIGNAL)`: the runtime's `DMA_IRQ_0` handler raises it when a transfer ends | Nothing to acknowledge: check the transfer, for example with `is_busy()`. |
| Polled I2C transaction | `bus.listen()`: `I2C0_IRQ` or `I2C1_IRQ` whenever a poll can move bytes or the transaction has ended | The next `poll_*` call, which deals with the condition and sets the mask for the next one. |

Several pins share `IO_IRQ_BANK0`, so give them one signal and let each
task check its own pin's `events()`: raising wakes every waiting task.

### Testing tasks on a desktop

The executor and its signals compile for desktops too. `fake.Clock` from
`std/embedded/hal/fake` is a simulated clock: time stands still while tasks
run and jumps ahead when the executor sleeps, and `interrupt_at(time, &signal)`
raises a signal at a set time as a handler would. A test runs in no time and
checks exactly when things happened:

```dodo test
package blinker

import "std/embedded/executor"
import "std/embedded/hal"
import "std/embedded/hal/fake"

// Turns an LED on for `on_ms` whenever the button signal is raised.
pub struct Flash<P: hal.OutputPin> {
    led: P
    on_ms: u32
    timer: executor.Timer

    pub fn new(led: P, on_ms: u32) -> Flash<P> {
        return Flash<P> { led, on_ms, timer: executor.Timer.new() }
    }

    pub fn poll(&mut self, cx: &mut executor.Context) -> executor.Poll {
        if PRESSED.take(cx) {
            self.led.set_high()
            self.timer.start_ms(cx, self.on_ms)
        }
        if self.timer.expired(cx) {
            self.led.set_low()
        }
        return executor.Poll.Pending
    }
}

pub const PRESSED: executor.Signal = executor.Signal { id: 0 }

@test
fn flashes_once_per_press() {
    PRESSED.reset()
    clock := fake.Clock.new()
    clock.interrupt_at(1_000, &PRESSED)
    clock.interrupt_at(5_000, &PRESSED)
    flash := Flash.new(fake.Pin.new(), 2)
    runner := executor.Executor.new()
    {
        tasks: [1]&mut dyn executor.Task = [&mut flash]
        assert(!runner.run_until(&mut tasks, &mut clock, 10_000))
    }
    assert_eq(flash.led.changes(), 4u32)
    assert(!flash.led.is_high())
    // Polled at the start, at each press, and when each flash ends.
    assert_eq(runner.polls(), 5u64)
    assert_eq(clock.idle_us(), 10_000u64)
}
```

A sleep that nothing could end traps instead of hanging the test.
`clock.advance(us)` lets time pass as if a task were busy, and `sleeps()` and
`idle_us()` show how often and how long the executor slept. `fake.Serial` has
the serial ports' `poll_read`, which returns Pending rather than end of input
while nothing has been fed.

### Rules and limits

- One executor runs at a time on a core, with up to 32 tasks
  (`executor.MAX_TASKS`) and 32 signals (`executor.MAX_SIGNALS`). Signal
  numbers are one set for each core's program, so libraries that raise
  signals take their numbers from the application. On the RP2040 and RP2350
  each core can run an executor of its own; see
  [two cores](#two-cores-on-the-rp2040-and-rp2350).
- A task does each step without waiting: an `async fn` returns to the
  executor at each `await`, a hand-written task at the end of each `poll`.
  Code between them runs to completion and delays every other task.
- A task owns its peripherals. Tasks that share a bus are one task, or one
  task owns the bus and the others send it requests with signals.
- The blocking bus methods (`write`, `read`, `transfer`) wait for the bus
  inside `poll`; they are short at usual speeds. For long transfers, or to
  keep other tasks running, use [SPI by DMA](#spi-transfers-by-dma), a
  [serial port by DMA](#serial-ports-by-dma), and
  [queued I2C transactions](#transactions-that-run-in-the-background), which
  return at once and finish while the task waits.
- On a desktop, where a critical section excludes nothing, use the executor
  and its signals from one thread.

## Persistent storage

Data that must outlive a restart, such as settings, calibration values,
counters, or credentials, goes in flash. `hal.Flash` is the protocol for
memory that behaves like NOR flash: erasing sets whole blocks of
`erase_size()` bytes to `0xFF` (`hal.FLASH_ERASED`), and writing then
programs bytes, in units of `write_size()`, only once until their block is
erased again. Out-of-range or misaligned requests return
`hal.Error.InvalidInput`. A board's program flash implements it through
regions, and so can a driver for an external SPI flash chip.

`std/embedded/storage` builds on any `hal.Flash`:

- `storage.Store` is a key-value store. Keys are text of 1 to 64 bytes
  (`storage.MAX_KEY_BYTES`); values are any bytes up to `max_value_len`,
  about one erase block.
- `storage.Region` is a window of a flash device, itself a `hal.Flash`, for
  giving a store part of a device.

```dodo
import "std/embedded/board"
import "std/embedded/storage"

fn main() {
    b := board.take()!
    settings := storage.Store.open(b.flash.last(64 * 1024)!)!
    boots := 0u32
    match settings.get_u32("boots") {
        ok(some(count)) => {
            boots = count
        }
        ok(none) => {}
        err(_) => {
            settings.clear()!
        }
    }
    settings.set_u32("boots", boots + 1)!
}
```

`get(key, buffer)` copies a value into a buffer and returns its length, or
`none` for a missing key; `len_of` and `contains` look without copying, and
`set`, `remove`, and `clear` change the store. `get_u32` and `set_u32` store
numbers as four little-endian bytes. `next_key` with a `storage.Cursor` lists
the keys. Errors are `storage.Error`: `Flash` and `Busy` from the device,
`Full`, `TooLarge` for an overlong key or value, `InvalidInput` for an empty
key or an unusable device, and `BufferTooSmall`.

The store is a log over the device's erase blocks, of which it needs 2 to 64
(`storage.MAX_BLOCKS`), each of at least 256 bytes. Every `set` or `remove`
appends a record to the newest block, and the latest record of a key wins; a
`set` of the value a key already has writes nothing. When the newest block
is full, the store moves on to the next one and compacts the oldest block in
use into it, copying the records still current there, then erases it. Erases
thus go round all blocks evenly, and one block always stays erased, so a
store holds somewhat less than its size minus one block. Replacing a value
needs room for the new record while the old one is still there; `remove`
works even in a full store.

Every record has a CRC-32 and is read back after it is written, and the store
writes only to erased bytes, so it also works on flash that cannot write a
byte twice. A power cut at any moment, even during a compaction, loses at most
the `set` or `remove` in progress: `Store.open` finds the last intact record
and finishes or undoes an interrupted compaction. After a failed write,
the store reads the flash again before its next operation.

Lookups scan the blocks in use, so the store suits small data, up to a few
hundred kilobytes, that changes now and then, not logs or fast-changing
values. A compaction holds the device busy for one erase and the copies.

`fake.Flash` from `std/embedded/hal/fake` is flash in a buffer of your own for
desktop tests. It checks the protocol strictly, refusing misaligned requests
and writes over bytes that are not erased, counts erases and writes, fails an
operation on request with `fail_next`, and cuts the power partway through an
operation with `cut_power_after(bytes)` or just after one with
`cut_power_after_operations(count)`, until `restore_power`:

```dodo
@test
fn settings_survive_a_power_cut() {
    memory := [0xFFu8; 4096]
    flash := fake.Flash.new(&mut memory, 1024, 4)
    store := storage.Store.open(flash)!
    store.set_u32("volume", 7)!
    store.flash().cut_power_after(10)
    match store.set_u32("volume", 8) {
        ok() => {
            core.assert(false)
        }
        err(error) => {
            core.assert(error == storage.Error.Flash)
        }
    }
    store.flash().restore_power()
    flash = store.release()
    store = storage.Store.open(flash)!
    match store.get_u32("volume")! {
        some(volume) => {
            core.assert(volume == 7)
        }
        none => {
            core.assert(false)
        }
    }
}
```

The repository's `tests/stdlib/storage_checks.dodo` cuts the power at every
erase and write, and every few bytes in between, of a long run of updates,
and checks that each key keeps its old or new value.

## Low power

Battery-powered firmware spends most of its time waiting.
`std/embedded/power` decides how much of the chip keeps running meanwhile.
Every chip package implements its `power.Power` protocol, every board hands it
out as `b.power`, and `std/embedded/hal/fake.Power` implements it on a
desktop. Power-aware code is therefore written and tested once, like a driver.

There are three levels, from the lightest to the deepest:

| Level | What stops | What wakes it | How |
| --- | --- | --- | --- |
| Sleep | The core | Any enabled interrupt | `power.Mode.Sleep`, the mode at reset |
| Deep sleep | The core and every clock outside the sleep set | An interrupt from a peripheral whose clock still runs | `power.Mode.DeepSleep` with `set_sleep_clocks` |
| Dormant | Every oscillator, and with them every clock | A pin event in a `power.Wake` | `dormant(&wake)` |

### Sleep modes

The mode says what every wait does from now on: `b.power.sleep()`,
`cpu.wait_for_interrupt()`, a timer alarm's `sleep_until`, and the
[executor](#tasks-and-the-executor) all honour it. An executor therefore
deep-sleeps between its tasks without being changed:

```dodo
import "std/embedded/board"
import "std/embedded/chip"
import "std/embedded/executor"
import "std/embedded/power"

fn main() {
    b := board.take()!
    // Only the timer runs while the core sleeps, so its alarm can wake it.
    b.power.set_sleep_clocks(chip.ALARM_CLOCKS)
    b.power.set_mode(power.Mode.DeepSleep)
    clock := b.timer.alarm(0)!
    // ... tasks as before ...
    executor.run(&mut tasks, &mut clock)
}
```

The sleep set must keep the clocks of whatever should wake the core:
`chip.ALARM_CLOCKS` for a timer alarm, `chip.CLOCK_GPIO` for a pin interrupt,
and a peripheral's own constant, such as `chip.CLOCK_UART0`, for its
interrupts. Add `chip.CLOCK_MEMORY` and `chip.CLOCK_DMA` for DMA that runs
during the sleep. Every clock is in the sleep set at reset, so a deep sleep
stops nothing more than a light one until the program says otherwise. When
the core wakes, the stopped clocks start again before its handler runs.
`power.deep_sleep(&mut b.power, keep)` deep-sleeps once and then restores the
previous mode and sleep set.

To wait for something an interrupt handler records, check it inside a
`hal.Critical` section and sleep there. An interrupt that becomes pending
between the check and the sleep still ends the sleep, and its handler runs
once the section ends. The executor does this for you.

### Clock gating

Apart from sleeping, the chip stops the clocks of peripherals that nothing
uses. Clocks are a `power.Clocks` set, a `u64` with one bit per clock gate.
The chip package names the gates, such as `chip.CLOCK_UART1` or
`chip.CLOCK_PIO0`, and they combine with `|`:

```dodo
stopped := b.power.disable_unused_clocks()   // every unclaimed peripheral
b.power.disable_clocks(chip.CLOCK_JTAG)!    // one the chip does not claim
serial := b.pins.uart(4, none, 115_200)!     // UART1's clocks start again
```

`disable_unused_clocks` stops the clocks of every peripheral the chip claims
and releases that is not claimed now, and returns them. Claiming one of those
peripherals starts its clocks again, so calling it once at start-up is safe.
`disable_clocks` refuses with `InvalidInput` the clocks the program cannot run
without (`chip.ESSENTIAL_CLOCKS`: memory, flash, the boot ROM, the bus fabric,
GPIO, and the clock, reset, and oscillator controls), and refuses with `Busy`
the clocks of a claimed peripheral. Peripherals that are not claimed through
`Pins`, such as the timer (`chip.CLOCK_TIMER`) and DMA (`chip.CLOCK_DMA`),
are the program's to stop: their registers stop answering until
`enable_clocks` starts them again.

### Dormant

`dormant` stops every oscillator. The chip keeps its memory and pin levels
and draws the least it can until one of the pin events in a `power.Wake`
happens. The chip then restarts its clocks as they were, and `dormant`
returns the pins that woke it, bit n for pin n:

```dodo
button := b.pins.input(15)!
// Wait for a new press, not one latched earlier.
button.clear_events(chip.RISING_EDGE)
wake := power.Wake.new()
wake.on(15, power.RISING_EDGE)
woken := b.power.dormant(&wake)!
```

`power.dormant_until(&mut b.power, pin, events)` is the one-pin shorthand.
The events are `power.LEVEL_LOW`, `LEVEL_HIGH`, `FALLING_EDGE`, and
`RISING_EDGE`, numbered as for `Pin.listen`. Claim each wake pin as an input
first, with the pull it needs, so its level is defined while nothing runs.
An edge or level that is present already ends the dormant state at once.

On the RP2040 and RP2350, `dormant` runs clk_sys from the crystal, stops
PLL_SYS, the ring oscillator, and the crystal, and afterwards starts PLL_SYS
again. Both cores stop. The microsecond timer pauses meanwhile, so `now_us`
does not count the time spent dormant. `dormant` returns `Busy` while the USB
serial port or an ADC input runs, because their 48 MHz clock would stop.
Wait until serial ports have sent their last bytes before calling it. A
complete example that blinks, deep-sleeps, and goes dormant until a button
press is `examples/low_power.dodo`.

### Testing power-aware code

`fake.Power.new(essential, managed)` starts with every clock running. It
refuses to disable `essential` clocks and treats `managed` clocks as
peripherals that `disable_unused_clocks` stops. Tests can `claim` and
`release` clocks as a chip does, and choose with `wake_next_dormant(pins)`
which pins end the next dormant state. A dormant state that nothing could end
traps instead of hanging the test. The fake counts `light_sleeps`,
`deep_sleeps`, and `dormant_states`, and reports the `requested_events` of
each pin.

### Sleep controls of the Cortex-M core

`std/embedded/cortex_m` also exposes the system control register directly.
`set_deep_sleep(bool)` sets SLEEPDEEP, which the chips' `set_mode` uses.
`set_sleep_on_exit(bool)` makes the core sleep again whenever its last
handler returns, for firmware that works only in interrupt handlers.
`set_event_on_pend(bool)` lets a pending but disabled interrupt end a
`wait_for_event`. Each core has its own register.

## Building firmware

### Boards

Select a board in `dodo.toml` and import `std/embedded/board`. Install an ELF linker
once (`brew install lld`, or `apt install lld`), then save this as `main.dodo`:

```dodo
package main

import "std/embedded/board"

fn main() {
    b := board.take()!
    for {
        b.led.toggle()
        b.timer.delay_ms(500)
    }
}
```

and this as `dodo.toml` in the same folder:

```toml
schema = 1

[targets.blink]
entry = "main.dodo"
board = "pico"
```

Hold the Pico's BOOTSEL button while connecting it over USB, so it appears as
the `RPI-RP2` drive (`RP2350` for a Pico 2 or Pico 2 W with `board = "pico2"`
or `board = "pico2_w"`), then run:

```sh
dodo run
```

Dodo builds the firmware, copies it to the drive, and the board restarts into
it: the LED blinks once a second. Firmware that opens the board's
[USB serial port](#usb-serial-on-the-rp2040-and-rp2350) with `b.usb.serial()`
spares you the button from then on: when no bootloader drive is mounted,
`dodo run` finds the board's port, restarts the board into its bootloader
through it, waits for the drive, and flashes. `dodo build` writes
`build/dev/thumbv6m-none-eabi/blink.elf` for debuggers and `blink.uf2` next to
it for drag-and-drop flashing (`thumbv8m.main-none-eabi` for the Pico 2).
Without a manifest, pass `--board pico` to
`dodo build`, `dodo run`, or `dodo check`. Editors use the manifest's board
too.

The `board` setting selects the chip's instruction set, its startup code and
linker script, and the package `std/embedded/board` resolves to. That package is
`std/embedded/board/pico` here, and every board package offers the same entry points,
so changing the board in `dodo.toml` is the only change a program needs:

| Entry point | Purpose |
| --- | --- |
| `board.NAME` | The board's name. |
| `board.take() -> Board!hal.Error` | Start the chip once: clocks, resets, LED. `Busy` on a second call. |
| `b.led` | The user LED: an output pin, on when high, with `toggle()`. |
| `b.pins` | The other GPIOs: `output(n)` and `input(n)` return pins that implement the [pin protocols](#device-protocols), or `Busy` if taken. |
| `b.pins.i2c(sda, scl, hz)` | An I2C bus on two pins, implementing the I2C bus protocol. |
| `b.pins.spi(clock, tx, rx, mode, hz)` | An SPI bus, implementing the SPI bus protocol; `rx` is `some(pin)` or `none`. |
| `b.pins.uart(tx, rx, baud)` | A [serial port](#serial-ports-on-the-rp2040-and-rp2350), implementing the `std/io` Reader and Writer; `rx` is `some(pin)` or `none`. |
| `b.pins.pwm(pin, hz)` | A [PWM output](#pwm-and-the-adc-on-the-rp2040-and-rp2350), implementing the PWM output protocol. |
| `b.pins.analog(pin)`, `b.pins.temperature()` | An ADC input on a pin, or the chip's temperature sensor, implementing the analog input protocol. |
| `board.I2C_SDA_PIN`, `board.I2C_SCL_PIN` | The board's usual I2C pins. |
| `board.SPI_CLOCK_PIN`, `board.SPI_TX_PIN`, `board.SPI_RX_PIN`, `board.SPI_SELECT_PIN` | The board's usual SPI pins, and a free pin for one chip select. |
| `board.UART_TX_PIN`, `board.UART_RX_PIN` | The board's usual serial port pins. |
| `b.timer` | The delay and counter protocols, plus `delay_ms(ms)`; `alarm(n)` claims an alarm with the alarm protocol, which can also run the [executor](#tasks-and-the-executor). |
| `b.rtc` | The chip's [real-time clock](#real-time-clocks-on-the-rp2040-and-rp2350), implementing the clock and alarm protocols. |
| `b.rng` | The chip's [random number generator](#random-numbers-on-the-rp2040-and-rp2350), implementing the random number generator protocol. |
| `b.watchdog` | The [watchdog](#the-watchdog-on-the-rp2040-and-rp2350): `start(timeout_ms)`, then `feed()` in time or the board resets; `reset_reason()` tells whether it did. Implements `hal.Watchdog`. |
| `b.dma` | The [DMA channels](#dma-on-the-rp2040-and-rp2350): `channel()` claims one, `release(channel)` gives it back. Buses and serial ports take them for transfers that run while the program does other work. |
| `b.usb.serial()` | A [USB serial port](#usb-serial-on-the-rp2040-and-rp2350) on the board's USB connector, implementing the `std/io` Reader and Writer, for printing over the USB cable; `b.usb.serial_with(identity)` sets its [USB identity](#usb-identity), and `b.usb.release_serial(port)` gives it back. |
| `board.reboot_to_bootloader()` | Restart into the board's flashing mode, for example to run `dodo run` again without pressing a button. |
| `board.report_panic(check, file, line, column)` | Unsafe. Write a failed check to the serial ports and the USB serial port, as the [default panic hook](#panics-on-the-serial-port) does; for panic hooks of your own. |

Board packages also export what is specific to their board, such as
`pico.LED_PIN`. Drivers written against the protocols take `b.pins` values
directly. Programs that build without a board, such as desktop tests, cannot
import `std/embedded/board`; keep that code in drivers and test them with fakes.

| Board | Chip | Package |
| --- | --- | --- |
| `pico` | `rp2040` | `std/embedded/board/pico`: Raspberry Pi Pico, 2 MiB flash, LED on GP25. |
| `pico2` | `rp2350` | `std/embedded/board/pico2`: Raspberry Pi Pico 2, 4 MiB flash, LED on GP25. |
| `pico2_w` | `rp2350` | `std/embedded/board/pico2_w`: Raspberry Pi Pico 2 W, 4 MiB flash, LED on the CYW43439 wireless chip. |

### The RP2040 and Raspberry Pi Pico

The `rp2040` chip compiles for `thumbv6m-none-eabi` and the Cortex-M0+. The
compiler adds what a desktop operating system would otherwise provide:

| Part | What it does |
| --- | --- |
| Boot stage 2 | Configures the flash interface for execute-in-place with the standard `03h` read command. The compiler adds the checksum the boot ROM verifies. |
| Vector table | Initial stack and reset handler. Every exception and interrupt is a weak default that an `@interrupt(NAME)` handler replaces. The default `USBCTRL_IRQ` serves the [USB serial port](#usb-serial-on-the-rp2040-and-rp2350) while one is open. |
| Reset handler | Copies initialized statics to RAM, zeroes the rest, and calls `main`. If `main` returns, the core sleeps. |
| Panic hook | `dodo_board_panic` masks interrupts, writes the failure to the program's [serial ports](#panics-on-the-serial-port) and USB serial port, if it opened any, and halts, keeping an open USB serial port served. Define `extern "C" fn dodo_board_panic(check: *const u8, file: *const u8, line: u32, column: u32)` to report failures differently; it must not return. |
| Runtime helpers | `memcpy`, `memmove`, `memset`, `memcmp`, the 32- and 64-bit division, 64-bit multiplication, and shift routines the Cortex-M0+ lacks in hardware, and software `f32` and `f64` arithmetic, comparisons, conversions, and `%`, since it has no floating-point unit. Results are correctly rounded IEEE 754, to nearest with ties to even, subnormals included. Addition, subtraction, and multiplication of normal numbers run in hand-written assembly, and division multiplies by a reciprocal. Typical cycles per call, as the RP2040 emulator counts them: `f32` addition 85, multiplication 90, division 280, comparison 50; `f64` addition 130, multiplication 195, division 570, comparison 80. The helpers are optimized whatever the program's `-O` level. |
| Linker script | Flash at `0x10000000` (size from the board), 264 KiB of RAM, and the stack at the top of RAM; `.ramfunc` and `.noinit` [sections](#sections-and-alignment). |

`std/embedded/chip/rp2040` is the chip support. `rp2040.take(config)` starts the
crystal, runs the CPU at 125 MHz from the system PLL, takes GPIO and the timer
out of reset, and returns the peripherals once. Claiming an
[I2C or SPI bus](#i2c-and-spi-on-the-rp2040-and-rp2350) takes its controller
out of reset. `rp2040.Config` holds what the
circuit board decides, the crystal frequency; a crystal the clock tree cannot
use is `InvalidInput`. Pins, the timer, and `reboot_to_bootloader(led)` are
the types and functions the Pico board package exposes. Interrupt handlers use
the datasheet names: `SysTick_Handler`, `HardFault_Handler`, `TIMER_IRQ_0` to
`TIMER_IRQ_3`, `UART0_IRQ`, `IO_IRQ_BANK0`, and so on; the package defines each
IRQ's number under the same name, such as `rp2040.UART0_IRQ`, for
[`std/embedded/cortex_m`](#the-cortex-m-core).

### The RP2350 and Raspberry Pi Pico 2

The `rp2350` chip runs on the RP2350's Arm cores and compiles for
`thumbv8m.main-none-eabi` and the Cortex-M33. Its boot ROM sets up
execute-in-place flash itself, so the compiler's runtime differs from the
RP2040's:

| Part | What it does |
| --- | --- |
| Vector table | At the start of flash, where the boot ROM enters Arm images. Weak defaults for the Cortex-M33 exceptions (including `SecureFault_Handler`) and all 52 interrupts. |
| Image definition | The `IMAGE_DEF` block, in the first 4 KiB of flash, that marks the image as a secure Arm RP2350 executable. The boot ROM runs nothing without it. |
| Reset handler | Clears the stack limit, installs the vector table, enables the FPU and the double-precision coprocessor, initializes RAM, and calls `main`. |
| Panic hook and helpers | As on the RP2040. The Cortex-M33 divides 32-bit integers in hardware, so only the 64-bit helpers are linked. Its FPU does single precision only, so `f32` arithmetic runs in hardware. `f64` addition, subtraction, multiplication, and division, and conversions between `f64` and `f32` or 32-bit integers, run on the RP2350's double-precision coprocessor (DCP): an `f64` division takes about 90 cycles instead of 340 in software, an addition about 45 instead of 150. The results are the software helpers' bit for bit: subnormal operands and results, which the coprocessor flushes to zero, are computed in software, and NaN results are the default NaN. The helpers save and restore the coprocessor's state when they interrupt one another, so `f64` works in interrupt handlers. The coprocessor sequences come from the Pico SDK, under its BSD-3-Clause license (`src/hardware/rp2350/LICENSE.pico-sdk`). Comparisons, `%`, conversion from `f32`, and conversions to and from 64-bit integers use the software helpers. |
| Linker script | Flash at `0x10000000` (size from the board), 520 KiB of RAM, and the stack at the top of RAM; `.ramfunc` and `.noinit` [sections](#sections-and-alignment). |

`std/embedded/chip/rp2350` mirrors the RP2040 package: `rp2350.take(config)`
starts the crystal, runs the CPU at 150 MHz from the system PLL, starts the
1 MHz ticks for TIMER0 and the watchdog, and hands out pins and the timer
once. Pins are released from pad isolation when claimed, as the RP2350
requires. `reboot_to_bootloader(led)` uses the boot ROM's reboot function;
the activity LED is skipped on chip revision A2, whose boot ROM mishandles it.
Interrupt handlers and IRQ number constants use the datasheet names, for
example `TIMER0_IRQ_0` and `IO_IRQ_BANK0`. GPIO 0 to 29 are supported, all of the RP2350A; the RP2350B's
extra pins are not yet.

Unlike the RP2040, the RP2350 boot ROM clears RAM when it starts, so nothing
in RAM survives a reboot into the bootloader.

### I2C and SPI on the RP2040 and RP2350

Both chips have two I2C controllers (DesignWare) and two SPI controllers
(PL022), each usable on fixed sets of pins. `pins.i2c` and `pins.spi` work
out the controller from the pins, take it out of reset, and route the pins
to it:

| Bus | Pins (GPIO numbers) |
| --- | --- |
| I2C0 | SDA 0, 4, 8, 12, 16, 20, 24, 28; SCL 1, 5, 9, 13, 17, 21, 25, 29 |
| I2C1 | SDA 2, 6, 10, 14, 18, 22, 26; SCL 3, 7, 11, 15, 19, 23, 27 |
| SPI0 | clock 2, 6, 18, 22; TX 3, 7, 19, 23; RX 0, 4, 16, 20; chip select 1, 5, 17, 21 |
| SPI1 | clock 10, 14, 26; TX 11, 15, 27; RX 8, 12, 24, 28; chip select 9, 13, 25, 29 |

The pins of one bus must belong to the same controller; otherwise, or for a
frequency the controller cannot make, the claim is `InvalidInput`. A pin or
controller that is already taken is `Busy`. Every board in this guide uses
I2C0 on GP4 and GP5 and SPI0 on GP16, GP18, and GP19, with GP17 as a chip
select, as in Raspberry Pi's pinout diagrams.

| | I2C | SPI |
| --- | --- | --- |
| Speed | Up to 1 MHz (`hal.I2C_FAST_PLUS_HZ`); `frequency_hz()` reports the result, the request rounded to whole clock cycles | The fastest rate up to the request, at most half the system clock: 62.5 MHz on the RP2040, 75 MHz on the RP2350 |
| Changing it | `set_frequency(hz)` between transactions | `set_frequency(hz)` and `set_mode(mode)` between transfers |
| Addresses and frames | 7-bit addresses through the `hal` protocol; `write_ten_bit`, `read_ten_bit`, and `write_read_ten_bit` for 10-bit addresses (0 to `0x3FF`) | 8-bit frames at first; `set_frame_bits(bits)` chooses 4 to 16. Frames above 8 bits move with `transfer_words` and `write_words` on `u16` words; the byte methods then return `Unsupported`. |
| Formats | Standard, fast, and fast-plus mode. High-speed mode (3.4 MHz) is not available: both chips leave its timing registers out of their controllers. | Motorola SPI in the four modes, and with `set_frame_format(pl022.FrameFormat.TexasInstruments)` or `.Microwire` the TI synchronous serial and National Microwire formats |
| Pins | Pull-ups enabled in the pads (about 50 kΩ): enough for an idle bus, too weak for 400 kHz. Most sensor boards bring 4.7 kΩ pull-ups; a bare bus needs them. | `pins.spi` leaves chip select to an output pin and `hal.SpiDevice`, because the controller's own pulses between frames in modes 0 and 2. `pins.spi_with_select(clock, tx, rx, select, mode, hz)` routes the controller's chip select as well, which the TI and Microwire formats need. |
| Errors | `NoAcknowledge` for a missing device or refused byte, `Bus` for lost arbitration, `Timeout` for a bus held low for around a second, `InvalidInput` for empty transfers, which the controller cannot send; `probe(address)` finds devices with a one-byte read instead | `Overrun` only if received bytes were lost; transfers cannot time out |
| Extra | | `set_loopback(true)` connects TX to RX inside the chip, for self-tests |
| Giving it back | `pins.release_i2c(bus)` holds the controller in reset and disconnects the pins, which stay pulled up | `pins.release_spi(bus)` holds the controller in reset and disconnects the pins |

Released pins can be claimed again, for a bus or as GPIOs, and
`pins.release(pin)` gives back a GPIO. This frees a device that holds SDA
low after an interrupted transfer, which makes every transaction `Timeout`:
release the bus, claim SCL as an output and SDA as an input, pulse SCL
until SDA reads high (at most nine times), release both pins, and claim the
bus again.

#### Transactions that run in the background

The blocking methods return when the transaction is over. Both buses also
implement the [polling forms](#polling-instead-of-waiting): an I2C bus
`hal.I2cPoll` and an SPI bus `hal.SpiPoll`, which run the same transactions,
of any length, while the program does other work. Each call moves the bytes
received so far into the buffer and refills the controller's FIFO, and
returns false while the transaction runs and true once it has ended:

```dodo
register: [1]u8 = [0x0F]
reply: [32]u8 = [0; 32]
for !i2c.poll_write_read(0x48, &register, &mut reply)! {
    // other work
}
```

Between polls an I2C controller holds SCL low, which every device accepts as
clock stretching, so a slow program loses nothing, but a transaction that is
never polled again keeps the bus. `is_busy()` is true while one runs, and
blocking calls and `set_frequency` on the bus are `Busy` meanwhile. A
transaction never ends while a device holds SCL low: bound the wait with an
`executor.Timer` and call `cancel()`, which aborts it and disables the
controller as a stuck blocking transfer does; `hal.BlockingI2c` does the
same after a number of polls. Devices with 10-bit addresses have
`poll_write_ten_bit`, `poll_read_ten_bit`, and `poll_write_read_ten_bit`. An
SPI bus moves at most a FIFO's worth of frames per poll and the clock simply
stops in between; its `cancel()` waits for the frames already queued.

A task waits with the I2C controller's interrupt. After `listen()`,
`I2C0_IRQ` or `I2C1_IRQ` is raised whenever the next poll can make progress:
the transmit FIFO is half empty while bytes remain to send, the receive FIFO
is half full, or the transaction ended with a STOP or an abort. Each poll
sets the controller's mask for what it waits on next, and an idle bus raises
nothing. The condition lasts until a poll deals with it, so a handler masks
the interrupt in the NVIC and raises a signal, and the task, once woken,
polls and enables it again, as in
[waking tasks from interrupts](#waking-tasks-from-interrupts).

#### Answering another master

Either chip can also be the device: `pins.i2c_target(sda, scl, address,
ten_bit, hz)` answers a master at a 7-bit address (or a 10-bit one with
`ten_bit`), and `pins.spi_target(clock, tx, rx, select, mode)` lets a master
clock and select the controller. Both are polled. An I2C target reports
what the master did, in order: the bytes it wrote, a STOP, and reads, which
hold the master up (SCL low) until the program answers:

```dodo
package main

import "std/embedded/board"
import "std/embedded/hal"

// A register device at 0x42: the first byte of a write picks the
// register, later bytes fill it, and reads continue from there.
fn main() {
    b := board.take()!
    target := b.pins.i2c_target(board.I2C_SDA_PIN, board.I2C_SCL_PIN, 0x42, false, hal.I2C_FAST_HZ)!
    memory: [16]u8 = [0; 16]
    pointer := 0usize
    buffer: [16]u8 = [0; 16]
    for {
        match target.poll(&mut buffer)! {
            hal.I2cTargetEvent.Write(count, start) => {
                first := 0usize
                if start {
                    pointer = buffer[0] as usize % 16
                    first = 1
                }
                for index in first..count {
                    memory[pointer] = buffer[index]
                    pointer = (pointer + 1) % 16
                }
            }
            hal.I2cTargetEvent.Read => {
                target.respond(&memory[pointer..pointer + 1])!
                pointer = (pointer + 1) % 16
            }
            _ => {}
        }
    }
}
```

The targets implement [`hal.I2cTarget` and
`hal.SpiTarget`](#being-the-device-targets), so a device emulation written
against them runs on any chip and against fakes. The controller holds SCL
low while its receive FIFO is full, so written bytes are never lost and
`poll` does not fail in practice; it would report `Overrun` if bytes were
dropped. `respond` can queue several bytes at once; what the master does not
read is dropped when it next reads. Addresses that the I2C specification reserves
(7-bit `0x00` to `0x07` and `0x78` to `0x7F`) are `InvalidInput`, and general
calls are not acknowledged.

An SPI target follows a clock of up to a twelfth of the system clock
(10.4 MHz on the RP2040, 12.5 MHz on the RP2350). `queue` loads replies
before the master clocks them, `read` collects what it sent, and `transfer`
does both, waiting as long as the master takes. A frame clocked while
nothing is queued repeats the last one, which the controller cannot detect.
In modes 0 and 2 the master must raise chip select between frames; masters
that keep it low for a whole transfer need modes 1 or 3. `set_output(false)`
keeps a listening target off the data line. Both targets are given back with
`pins.release_i2c_target` and `pins.release_spi_target`.

### Serial ports on the RP2040 and RP2350

Both chips have two UARTs (Arm PL011). `pins.uart(tx, rx, baud)` claims a
transmit pin, and a receive pin or `none` to only send, and returns a
`pl011.Port` sending 8N1 frames (eight data bits, no parity, one stop bit)
at the rate closest to `baud`. A port is a `std/io` Reader and Writer, so
[`std/fmt`](formatting.md) prints to it and `std/io` helpers such as
`io.read_line` read from it:

```dodo
package main

import "std/embedded/board"
import "std/fmt"
import "std/io"

fn main() {
    b := board.take()!
    serial := b.pins.uart(board.UART_TX_PIN, some(board.UART_RX_PIN), 115_200)!
    serial.set_crlf(true)
    fmt.printf(&mut serial, "Hello from {}!\n", board.NAME)!
    line: [64]u8 = [0; 64]
    for {
        got := io.read_line(&mut serial, &mut line)!
        b.led.toggle()
        io.write_all(&mut serial, &line[..got.count])!
    }
}
```

Every board in this guide uses UART0 on GP0 (TX) and GP1 (RX), where the
Raspberry Pi Debug Probe's UART connects. With a USB-to-serial adapter,
connect the board's TX to the adapter's RX, its RX to the adapter's TX, and
ground to ground, then open a terminal at the same rate, for example
`picocom -b 115200 /dev/ttyACM0` on Linux or
`screen /dev/tty.usbmodem* 115200` on macOS.
[`examples/serial_console.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/serial_console.dodo)
is a small command line with echo and backspace.

| UART | TX pins (GPIO numbers) | RX pins |
| --- | --- | --- |
| UART0 | 0, 12, 16, 28 | 1, 13, 17, 29 |
| UART1 | 4, 8, 20, 24 | 5, 9, 21, 25 |

On the RP2350, TX also works on the pins listed for CTS below and RX on
those for RTS (its UART_AUX function): UART0 TX on 2, 14, and 18 and RX on
3, 15, and 19, UART1 TX on 6, 10, 22, and 26 and RX on 7, 11, 23, and 27.
The two pins must belong to the same UART; otherwise, or for a rate out of
range, the claim is `InvalidInput`. A pin or UART that is already taken is
`Busy`.

`pins.uart_flow(tx, rx, cts, rts, baud)` claims a port with hardware flow
control. CTS is an input: the port sends only while the other end holds it
low. RTS is an output: the port holds it low while its receive FIFO has
room, and raises it from 24 bytes on, so a sender that honours it never
overruns the port, even while the program is busy. Unconnected, CTS is
pulled up and nothing is sent, and `flush()` waits as long.

| UART | CTS pins | RTS pins |
| --- | --- | --- |
| UART0 | 2, 14, 18 | 3, 15, 19 |
| UART1 | 6, 10, 22, 26 | 7, 11, 23, 27 |

| | Serial port |
| --- | --- |
| Speed | From 120 baud (144 on the RP2350) up to a sixteenth of the system clock: 7.8 Mbaud on the RP2040, 9.4 Mbaud on the RP2350. `baud()` reports the rate the divider makes, within 0.1 % up to 921600 baud (`115_207` for `115_200`). |
| Changing it | `set_baud(baud)`, and `set_format(data_bits, parity, stop_bits)` for 5 to 8 data bits, `pl011.Parity.None`, `Even`, or `Odd`, and 1 or 2 stop bits. Both wait until everything written is out. |
| Writing | `write` waits while the 32-byte transmit FIFO is full and returns once the last byte is queued; `flush()` waits until it has left. `poll_write` queues what fits and `poll_flush()` tells whether the last byte has left, both without waiting; ports implement `serial.Port` and `serial.PollPort`. A write or `flush()` that makes no progress for `pl011.DEFAULT_POLL_LIMIT` status polls (around a second), as when flow control holds the transmitter, fails with `TimedOut` and the bytes queued before it; `set_poll_limit(polls)` changes the limit, `pl011.NO_POLL_LIMIT` removes it. |
| Reading | `read` waits for the first byte, as long as it takes unless `set_read_poll_limit(polls)` bounds it (then `TimedOut`), then returns what has arrived; a serial port never reaches end of input. `poll_read` returns at once, `Pending` while nothing has arrived. Nothing buffers beyond the 32-byte receive FIFO, so read at least every 32 bytes' time (2.8 ms at 115200 baud) while data arrives. |
| Errors | A read fails with an `io.Error` of kind `Other` whose `code` holds `pl011.FRAMING_ERROR`, `PARITY_ERROR`, `BREAK`, or `OVERRUN`, after the good bytes before it (`transferred`). Bytes with framing or parity errors, and breaks, are dropped; an overrun means bytes were lost because the receive FIFO was full. The next read continues. |
| Terminals | `set_crlf(true)` writes `"\n"` as `"\r\n"` and reads the CR that Enter sends, alone or followed by LF, as one `"\n"`, so `io.read_line` ends at Enter. Off at first, so binary protocols get their bytes unchanged. Terminals do not show what is typed unless the program echoes it. |
| Pins | Pulled up in the pads, so an unconnected receiver and released pins idle high, as the line does between frames, instead of reading as a break. |
| Flow control and breaks | `set_flow_control(cts, rts)` turns each half of hardware flow control on or off, on a port claimed with `uart_flow`. `set_break(true)` holds the line low once everything written is out, until `set_break(false)`; keep it on for at least two frame times (174 µs at 115200 baud, 8N1). |
| Extra | `set_loopback(true)` connects TX to RX inside the chip, for self-tests. |
| Giving it back | `pins.release_uart(port)` waits until everything written is out, holds the UART in reset, and disconnects the pins, which stay pulled up. |

#### Panics on the serial port

A failed check (an overflow, an index out of bounds, `core.assert`, or `!` on
an error) calls the board runtime's panic hook. If the program has claimed a
serial port, the hook first writes which check failed and where to every
port it still holds, on a line of its own, the line a desktop program prints:

```text
dodo: index bounds check failed at /home/me/console/main.dodo:41:46
```

Then it masks interrupts and halts, as without a port. The message is out
before the board halts, at the port's own rate and format; nothing needs to
be configured. An open [USB serial port](#usb-serial-on-the-rp2040-and-rp2350)
gets the report too, and stays served after the halt: a terminal opened later
still shows the report, and `dodo run` can still restart the board into its
bootloader. A panic hook of your own replaces all of this. To keep the
report and do something else afterwards, call `board.report_panic` from it,
for example to restart into the bootloader so `dodo run` works again without
pressing BOOTSEL:

```dodo
extern "C" fn dodo_board_panic(check: *const u8, file: *const u8, line: u32, column: u32) {
    unsafe {
        board.report_panic(check, file, line, column)
    }
    board.reboot_to_bootloader()
}
```

### USB serial on the RP2040 and RP2350

Both chips have a USB controller on the board's USB connector, the one
`dodo run` flashes through. `b.usb.serial()` makes the board a USB serial
port (CDC-ACM), so it prints and reads over that same cable, with no adapter
and no wiring. The host needs no driver: the port is `/dev/ttyACM0` on
Linux, `/dev/cu.usbmodem...` on macOS, and a COM port on Windows. The port
is a `std/io` Reader and Writer, like a UART's:

```dodo
package main

import "std/embedded/board"
import "std/fmt"

fn main() {
    b := board.take()!
    usb := b.usb.serial()!
    count := 0u32
    for {
        fmt.printf(&mut usb, "tick {}\n", count)!
        count += 1
        b.led.toggle()
        b.timer.delay_ms(500)
    }
}
```

Open a terminal on the port, `picocom /dev/ttyACM0` or
`screen /dev/cu.usbmodem*`, at any baud rate: over USB the rate means
nothing. The port is served from its interrupt, `USBCTRL_IRQ`, so it keeps
answering the host while the program busy-waits, as in `delay_ms` above.
[`examples/usb_serial.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/usb_serial.dodo)
is a small command line over USB.

| | USB serial port |
| --- | --- |
| Writing | `write` queues its bytes and returns. While no terminal has the port open, the first 1024 bytes (`rp_usb.TX_CAPACITY`) wait for one and the rest are dropped, so the start of the output, such as a banner printed at startup, is shown when a terminal opens it. While one has it open, a full queue waits for the host, and output the host does not take within 100 ms is dropped. Printing never stops the program and never fails. `flush()` waits until the host has taken everything, within the same limit; `poll_write` queues what fits and `poll_flush()` tells whether the host has taken it all. The port implements `serial.Port` and `serial.PollPort`. |
| Reading | `read` waits for the first byte, then returns what has arrived; `poll_read` returns at once, `Pending` while nothing has. Up to 256 bytes (`rp_usb.RX_CAPACITY`) are kept; beyond that the host waits until the program reads, so no input is lost. `available()` counts what is waiting. |
| Terminals | Newline translation is on at first, unlike on a UART: `"\n"` goes out as `"\r\n"`, and the CR that Enter sends, alone or followed by LF, reads as `"\n"`. `set_crlf(false)` passes binary data unchanged. |
| Connection | `is_connected()` is true while a program on the host has the port open (DTR set, as terminals do); `is_configured()` while the host has enumerated the board. `line_state()` returns `rp_usb.DTR` and `rp_usb.RTS`, and `baud()` the rate the host set, for a bridge to a UART. |
| Tasks | `set_signal(&SIGNAL)` raises an [executor signal](#waking-tasks-from-interrupts) whenever bytes arrive, output leaves, or the host opens, closes, or suspends the port. |
| Restarting into the bootloader | A host that sets the port to 1200 baud restarts the board into its USB bootloader once the request is answered, as with the Pico SDK and Arduino. `dodo run` does this when no bootloader drive is mounted, so after the first flash the board needs no BOOTSEL press. `set_bootloader_reset(false)` turns it off, for example for a bridge to a UART that may really run at 1200 baud. |
| Panics | Until the port is released, a panic writes its report to it, and the default panic hook keeps serving the port after it halts: a terminal opened later still receives the report, and `dodo run` still works. |
| Identity | By default, Raspberry Pi's vendor ID `0x2E8A` with the Pico SDK's USB serial product ID `0x000A`, manufacturer "Dodo", and the board's name as the product; `b.usb.serial_with(identity)` sets your own, [below](#usb-identity). `b.usb.serial()` claims the port once (`Busy` otherwise); `b.usb.release_serial(port)` disconnects from the host and stops the controller. |
| Clocks | The controller runs at 48 MHz from PLL_USB, which the ADC shares: the PLL runs while either uses it. |
| Limits | One serial port, in device mode only: no host mode and no other USB classes. The queues are fixed at 256 bytes in (`rp_usb.RX_CAPACITY`) and 1 KiB out (`rp_usb.TX_CAPACITY`). |

#### USB identity

A USB vendor and product ID identify a device and its USB firmware, not the
library it was built with, so Dodo has no product ID of its own. The default
identity is meant for development: the board behaves like the Pico SDK's USB
serial port (CDC UART) and uses its IDs, so hosts need no driver and
`dodo run` finds the board to restart it into the bootloader. Hosts and tools
take it for a Pico running Pico SDK firmware, though, and may treat it like
one, or confuse it with another Pico plugged in at the same time.

A product you distribute with its own USB identity passes it to
`b.usb.serial_with`:

```dodo
usb := b.usb.serial_with(rp_usb.Identity {
    vendor_id: 0x1209,
    product_id: 0x0001,
    manufacturer: "Example Maker",
    product: "Plant Monitor",
})!
```

Use a vendor ID you own, or a product ID assigned to your product under
someone else's vendor ID, such as one from a vendor's sublicensing program
or the pid.codes registry (vendor ID `0x1209`, whose `0x0001` above is a test
ID). Do not pick an unassigned product ID under Raspberry Pi's vendor ID
`0x2E8A`. Strings are ASCII, cut at 32 bytes (`rp_usb.MAX_STRING_LENGTH`).
`rp_usb.default_identity(name)` is the default with another product name.
`dodo run` looks only for the default IDs, so a board with its own needs
BOOTSEL held while it is connected to flash it.

A task woken by the port reads until nothing is left:

```dodo
const USB_INPUT: executor.Signal = executor.Signal { id: 4 }

struct Echo {
    usb: rp_usb.Serial

    pub fn poll(&mut self, cx: &mut executor.Context) -> executor.Poll {
        USB_INPUT.take(cx)
        buffer := [0u8; 64]
        for {
            got := self.usb.poll_read(&mut buffer)!
            if got.transferred == 0 {
                break
            }
            self.usb.write(&buffer[..got.transferred])!
        }
        return executor.Poll.Pending
    }
}
```

with `usb.set_signal(&USB_INPUT)` before the task starts. Import
`std/embedded/chip/rp_usb` to name the `rp_usb.Serial` type.

The driver, `std/embedded/chip/rp_usb`, is a `Device` over any
`hal.Registers`, generic like the other drivers: the repository's desktop
tests run it against a model of the controller and a simulated host that
enumerates it, opens the port, and moves data both ways. On the chip, the
runtime's default `USBCTRL_IRQ` handler serves it; a program's own
`@interrupt(USBCTRL_IRQ)` replaces that handler, and with it the port, for
example to run a USB stack of its own. A custom board that uses
`std/embedded/chip` directly calls `p.usb.serial()`, named after the chip,
or `p.usb.serial_with(identity, activity_led)`.

### PWM and the ADC on the RP2040 and RP2350

`pins.pwm(pin, frequency_hz)` claims any GPIO as a PWM output and returns an
`rp_pwm.Pwm`, which implements `hal.PwmOutput`. It starts low; set its duty
cycle directly or with the [helpers](#helpers):

```dodo
package main

import "std/embedded/board"
import "std/embedded/hal"

fn main() {
    b := board.take()!
    led := b.pins.pwm(15, 1000)!
    hal.set_duty_percent(&mut led, 25)!
    // A hobby servo on GP16: 50 Hz, centred by 1.5 ms pulses.
    servo := b.pins.pwm(16, 50)!
    hal.set_pulse_us(&mut servo, 1500, 20_000)!
}
```

The PWM block has eight slices, each a 16-bit counter with two outputs.
GPIO n is output A (n even) or B (n odd) of slice (n / 2) % 8, so GPIO 0 and 1
are slice 0, GPIO 14 and 15 slice 7, and GPIO 16 and 17 slice 0 again. Two
pins on the same output, such as GPIO 0 and 16, would carry the same signal,
so the second claim is `Busy`. The two outputs of a slice share its
frequency: the second may join at the frequency the slice already runs at,
and is `Busy` at another.

A slice counts clk_sys (125 MHz on the RP2040, 150 MHz on the RP2350) through
a divider from 1 to 255 15/16 and wraps after TOP. `pins.pwm` picks the
smallest divider whose period fits in 65535 counts, so the output has as many
duty steps as the frequency allows: `max_duty()` is TOP + 1, 64516 at 1 kHz on
the RP2040. Frequencies run from 8 Hz (9 Hz on the RP2350) up to half of
clk_sys. For a divider and TOP of your own,
`pins.pwm_divided(pin, divider, top)` takes the divider in sixteenths, from
16 to 4095, and TOP, up to 65534.

| `rp_pwm.Pwm` | Purpose |
| --- | --- |
| `max_duty()`, `set_duty(duty)`, `duty()` | The PWM output protocol, and the duty last set. A new duty starts with the next period. |
| `set_frequency(hz)`, `set_timing(timing)` | Change the frequency, or the divider and TOP of `rp_pwm.divided`, keeping the duty cycle. `Busy` while the slice's other output is claimed. |
| `frequency_hz()`, `timing()` | The frequency, and the divider and TOP. |
| `set_inverted(on)` | High while the count is at or above the compare value instead of below it. |
| `pin()`, `slice()`, `output()`, `count()` | The GPIO, slice, output (0 for A), and the counter's value. |

`pins.release_pwm(output)` drives the pin low, disconnects it, and stops the
slice once neither output is in use. The two outputs of a slice write only
their own bits of the registers they share, so one can be kept by an
interrupt handler and the other by the main program.

`pins.pwm_input(pin, window_us)` measures the signal on an odd GPIO, input B
of PWM slice (n / 2) % 8: its frequency, duty cycle, and pulse length, as for
a servo or RC receiver signal, a fan's tachometer, or another board's PWM
output. It returns an `rp_pwm.PwmCapture`, which implements
[`hal.PwmInput`](#measuring-pulses). The input uses the slice's counter, so
the slice's other pin cannot be a PWM output meanwhile: `pwm_input` is
`Busy` if either output is in use, and outputs on that slice are `Busy` while
the input exists. Even pins are `InvalidInput`. `release_pwm_input` gives the
pin and slice back.

```dodo
input := b.pins.pwm_input(15, 100_000)!   // 100 ms windows
servo := hal.capture(&mut input, hal.DEFAULT_POLL_LIMIT)!
pulse := servo.high_us()                  // about 1000 to 2000
```

A slice cannot timestamp edges, so it counts over windows of `window_us`,
two per measurement, one right after the other: first rising edges, then
clk_sys cycles while the pin is high, over a whole number of the periods it
just measured. Polls note when the edge count changes, so a program that
polls often gets the period to within a few microseconds over the whole
window; one that polls once per window gets it to within one period per
window. Choose a window spanning at least a few periods of the slowest
signal: 10 ms for 25 kHz fan PWM or 1 kHz, 50 ms for a 100 Hz tachometer,
100 ms for a 50 Hz servo. Windows reach 134 ms on the RP2040 and 111 ms on
the RP2350 (`rp_pwm.max_window_us`). Fewer than two rising edges in a
window, or a line that does not change, reads as a steady line: `cycles` is
0 and `duty` shows the share of time it was high. Fast signals are counted
through a prescaler that adapts by itself, so a sudden jump in frequency
costs a window or two; `poll_capture` returns `Overrun` for a count that
wrapped beyond recovery, a signal near half of clk_sys or polls more than
about three windows apart, and the next measurement then starts. The pin is
pulled down, so an unconnected input reads as a steady low line; a fan's
open-collector tachometer needs a pull-up of 10 kΩ or less to 3.3 V.

`pins.analog(pin)` claims GPIO 26 to 29, ADC inputs 0 to 3, and returns an
`rp_adc.Analog`, which implements `hal.AnalogInput`: each `read()` converts
once, 0 to 4095 for 0 V to the ADC's reference, the 3.3 V supply on the Pico
boards. The pin's digital input and pulls are turned off, so they do not
disturb the voltage. `pins.temperature()` claims the on-chip temperature
sensor, ADC input 4, and `rp_adc.millicelsius(value, 3300)` converts its
readings to thousandths of a degree Celsius:

```dodo
package main

import "std/embedded/board"
import "std/embedded/chip/rp_adc"
import "std/embedded/hal"

fn main() {
    b := board.take()!
    knob := b.pins.analog(26)!
    sensor := b.pins.temperature()!
    millivolts := hal.read_millivolts(&mut knob, 3300)!
    celsius := rp_adc.millicelsius(sensor.read()!, 3300) / 1000
}
```

The ADC is one converter shared by its inputs. A read selects its input and
converts in about 2 us with interrupts masked, so inputs read by an interrupt
handler and by the main program never mix up their conversions. A conversion
that does not finish is `Timeout`, and one the converter flags as failed is
`Bus`. The ADC needs a 48 MHz clock, so its first input starts PLL_USB, and
releasing the last with `pins.release_analog(input)` stops both. The sensor
is accurate to a few degrees and depends on the reference voltage; on the
Pico and Pico 2, GP29 reads a third of the supply voltage VSYS.

### Random numbers on the RP2040 and RP2350

`b.rng`, or `Peripherals.rng` from the chip package, is each chip's best
generator; both implement [`hal.Rng`](#random-numbers), and
`std/embedded/chip/rp_rng` holds the drivers:

| Chip | `b.rng` | Source | Speed | Fit for keys |
| --- | --- | --- | --- | --- |
| RP2040 | `rp_rng.Rosc` | The ring oscillator's RANDOMBIT, sampled every 128 reads (about 10 us) and debiased with von Neumann's method: of two samples that differ, the first is kept; pairs that agree are dropped. | About 3 KB a second | No |
| RP2350 | `rp_rng.Trng` | The TRNG, with its hardware debiasing and all three health tests on, sampling its oscillator every 200 clk_sys cycles. | About 20 KB a second | Yes |

The ring oscillator was not designed or tested as an entropy source, and
the RP2040 has nothing better: use its bytes for seeds, random delays,
identifiers, and games. `take()` moves the clocks to the crystal and the
PLL but leaves the oscillator running, so its jitter against clk_sys is
what the samples see. An
oscillator that has stopped makes a fill `Timeout`, and 32 equal bits in a
row, which a source locked to the system clock gives, make it `Bus`.

The RP2350's TRNG collects 192 bits at a time in about 1 ms. A fill keeps
the bytes it does not use for the next, so `hal.random_u32` mostly returns
at once. Bits that fail the repetition tests are discarded, and four failed
autocorrelation tests in a row, which stop the TRNG, restart it; after 100
restarts for one block the fill reports `Bus`. The TRNG runs only during a
fill. Its events are unmasked meanwhile and raise `TRNG_IRQ`; while that
interrupt is disabled in the NVIC, as it is unless the program enables it,
the fill clears the pend they leave.

Both generators are generic over `hal.Registers`, as `rp_rng.RoscSource` and
`rp_rng.TrngSource`, so they run against simulated registers in
[desktop tests](#testing-with-fakes).

### PIO on the RP2040 and RP2350

PIO, programmable I/O, is the RP chips' set of small state machines for
protocols no other peripheral speaks: WS2812 LEDs, extra serial ports,
parallel buses, and the Pico 2 W's wireless bus. The RP2040 has two PIO blocks
and the RP2350 three. Each block has four state machines, which run programs
from 32 shared instruction slots, exchange 32-bit words with the processor
through a 4-word FIFO in each direction, drive and read GPIOs, and signal
each other through eight IRQ flags. Every instruction takes one cycle of the
state machine's clock, up to clk_sys, so timing is exact.

Programs are written in the Pico SDK's PIO assembly language and assembled
when the firmware is compiled. `rp_pio.assemble` takes the program as string
literals, one line each, and returns an `rp_pio.Program`, usually as a
constant:

```dodo
package main

import "std/embedded/board"
import "std/embedded/chip/rp_pio"

// Each word from the program: the LED on GP15 stays on for that many
// cycles of the state machine, then off until the next word.
const PULSE: rp_pio.Program = rp_pio.assemble(
    ".side_set 1",
    "    pull block    side 0",
    "    out x, 32     side 0",
    "on:",
    "    jmp x-- on    side 1",
)

fn main() {
    b := board.take()!
    pio := b.pins.pio(0)!
    b.pins.pio_pins(&pio, 15, 1)!
    loaded := pio.load(&PULSE)!
    config := loaded.config()
    config.set_sideset_pins(15)!
    // One instruction a microsecond.
    config.set_frequency(1_000_000)!
    sm := pio.state_machine(0)!
    sm.init(&config)
    sm.set_pindirs(15, 1, 1)
    sm.set_enabled(true)
    for {
        sm.put(250_000)
        b.timer.delay_ms(1000)
    }
}
```

The assembler accepts the language of the SDK's `pioasm`, so `.pio` files
from the SDK and its examples work unchanged: labels, `public` labels,
`.define`, integer expressions in parentheses, `side` and `[delay]` on any
instruction, `;` and `//` comments, and every instruction (`jmp`, `wait`,
`in`, `out`, `push`, `pull`, `mov`, `irq`, `set`, `nop`). Directives set up
the program and its state machine: `.side_set`, `.wrap_target`, `.wrap`,
`.origin`, `.word`, `.pio_version`, and `.clock_div`, `.fifo`, `.mov_status`,
`.in`, `.out`, and `.set`, whose settings `loaded.config()` starts from.
`.lang_opt` lines and `% c-sdk { ... %}` blocks are ignored. Write one
program per call; a single literal with `\n` between lines works too.

A mistake in a program is a compile error at its line, such as
`invalid PIO program: the SET value must be 0 to 31, not 32`. `assemble`
exists only in firmware for the RP2040 and RP2350, and checks the program
against the chip's PIO version: the RP2350's version 1 adds `mov pindirs`,
`mov rxfifo[...]` to and from RX FIFO registers, `irq` and `wait irq` on the
`prev` and `next` blocks' flags, `wait jmppin`, the `.fifo` choices `txput`,
`txget`, and `putget`, `.mov_status irq`, and IN pin counts below 32. Using
one in an RP2040 build is an error naming the instruction. Hosted builds
cannot assemble programs; desktop tests build an `rp_pio.Program` from its
instruction words instead.

`pins.pio(n)` claims block `n` and returns an `rp_pio.Pio`. State machines
drive only pins connected to their block: `pins.pio_pins(&pio, first, count)`
connects GPIO `first` and the `count - 1` after it, which are then taken like
any claimed pin. A state machine reads any pin whose input is enabled, so a
pin it only reads can also be claimed with `pins.input(n)`.

| `rp_pio.Pio` | Purpose |
| --- | --- |
| `load(&program)`, `unload(loaded)` | Load a program at its `.origin` or the highest free slots it fits, relocating its jumps; `Busy` if it does not fit. Returns an `rp_pio.Loaded`, whose `offset()` is its first instruction's address and whose `config()` is a `Config` with the program's settings. |
| `state_machine(index)`, `free_state_machine()`, `release_state_machine(sm)` | Claim state machine 0 to 3, or the first free one, stopped; give it back. |
| `enable_in_sync(machines)` | Start several state machines with their clock dividers in step. |
| `flags()`, `clear_flags(flags)`, `force_flags(flags)` | The IRQ flags, one bit each. Clearing releases state machines waiting on `irq wait`. |
| `listen(line, sources)`, `ignore(line, sources)`, `interrupts(line)`, `irq(line)` | Raise interrupt line 0 or 1 for `rp_pio.rx_not_empty(sm)`, `rp_pio.tx_not_full(sm)`, and `rp_pio.flag(n)` sources; `irq(line)` is its number for `cortex_m.enable` and `@interrupt`, such as `PIO0_IRQ_0`. |
| `bypass_input_sync(pin, on)` | Read a pin without the two-cycle input synchronizer. |

| `rp_pio.Config` | Purpose |
| --- | --- |
| `set_frequency(hz)`, `set_divider(divider)` | Instructions per second, from clk_sys down to 1/65536 of it, or the divider of clk_sys in 256ths. |
| `set_out_pins(first, count)`, `set_set_pins(first, count)`, `set_sideset_pins(first)`, `set_in_pins(first)`, `set_jmp_pin(pin)` | The pin mappings of `out`, `set`, side-set, `in` and `wait pin`, and `jmp pin`. |
| `set_out_shift(right, autopull, bits)`, `set_in_shift(right, autopush, bits)` | Shift direction, and automatic `pull` and `push` after so many bits. |
| `set_fifo_join(join)` | `rp_pio.FIFO_TX` or `FIFO_RX` give one direction both FIFOs, 8 words; `FIFO_TXPUT`, `FIFO_TXGET`, and `FIFO_PUTGET` (RP2350) turn the RX FIFO into four registers. |
| `set_wrap(target, wrap)`, `set_start(address)`, `set_mov_status(select, n)`, `set_out_special(sticky, enable_pin)`, `set_in_count(count)` | The rest of the state machine's settings. |

| `rp_pio.StateMachine` | Purpose |
| --- | --- |
| `init(&config)` | Stop, apply the settings, empty the FIFOs, reset the internal state, and jump to the start. |
| `set_enabled(on)`, `restart()`, `restart_clock()` | Run or pause the program. |
| `put(word)`, `try_put(word)`, `get()`, `try_get()` | Words to and from the program. `put` and `get` wait while the FIFO is full or empty. |
| `tx_level()`, `rx_level()`, `is_tx_full()`, `is_rx_empty()`, ..., `clear_fifos()`, `drain_tx()`, `take_tx_stall()` | FIFO state; `take_tx_stall()` tells when the program has run out of words. |
| `exec(instruction)`, `exec_wait(instruction)`, `jump(address)`, `address()` | Run one instruction now, such as a word of an assembled constant: `sm.exec(SET_X.instructions[0])`. |
| `set_pins(first, count, levels)`, `set_pindirs(first, count, outputs)` | Set pin levels and directions through the state machine, before its program starts. |
| `rx_register(index)`, `set_rx_register(index, value)` | The RP2350's RX FIFO registers. |
| `set_frequency(hz)`, `set_divider(divider)` | Change the clock while running. |

`pins.release_pio(pio)`, once its state machines have been given back,
disconnects the block's pins and holds it in reset. State machines change
only their own bits of the registers they share, so one can be kept by an
interrupt handler and another by the main program.
[`examples/ws2812.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/ws2812.dodo)
drives a strip of WS2812 LEDs this way.

### I2S audio on the RP2040 and RP2350

Neither chip has an I2S peripheral, so `std/embedded/chip/rp_i2s` makes one
from a [PIO](#pio-on-the-rp2040-and-rp2350) state machine. An
`rp_i2s.Output` plays samples to a DAC or amplifier such as the PCM5102 or
MAX98357A and implements [`hal.I2sOutput`](#i2s-audio); an `rp_i2s.Input`
records an I2S microphone or ADC such as the INMP441 and implements
`hal.I2sInput`. Both drive the bit clock (BCLK) and word select (LRCLK):
BCLK is GPIO `clock` and LRCLK the GPIO after it, and DATA any other pin.
Give all three to the block first:

```dodo
pio := b.pins.pio(0)!
b.pins.pio_pins(&pio, 9, 3)!             // DATA 9, BCLK 10, LRCLK 11
config := rp_i2s.Config.new(48_000, 16, 2, hal.I2sFormat.Philips)
output := rp_i2s.Output.new(&mut pio, 9, 10, &config)!
hal.write_samples(&mut output, &samples, hal.NO_POLL_LIMIT)!
```

A frame always has two slots on the wire, left then right: a mono output
sends each sample in both, and a mono input keeps the slot `config.channel`
names. Slots are 32 bit clocks, or 16 with `config.slot_bits = 16` for
16-bit samples, so BCLK runs at `sample_rate × 2 × slot_bits`. The state
machine spends four cycles per bit, divided down from clk_sys in 1/256
steps; `output.timing()` reports the rate actually produced and its error in
parts per million. At 125 MHz, 48 kHz comes out 64 ppm fast; a 150 MHz
RP2350 makes 48 kHz exactly. Philips, left-justified, and right-justified
framing are supported; a 24-bit sample sits in the top bits of its 32-bit
slot, or the bottom bits when right-justified.

The joined FIFO holds eight words: four frames in 32-bit slots, eight in
16-bit slots. `poll_write` queues as many whole frames as fit, so a task
tops it up, or a buffer plays without the processor: `output.pack(&samples,
buffer)` fills a word-aligned buffer in the wire format and
`output.dma(&mut channel, buffer)` returns a
[`hal.DmaStream`](#transfers-in-the-background-dma) paced by the state
machine. When the queue runs dry the output keeps its clocks running and
sends silence a whole frame at a time, so left and right never swap. The
input instead pauses its clocks when the program falls behind; the next
`poll_read` reports `Overrun` once and drops what was queued, and some
microphones need a few milliseconds to wake after their clock stops.

An output takes 17 instruction slots (10 with 16-bit slots) and an input 8,
so both fit in one block. `release(&mut pio)` stops the stream, gives back
its state machine, and frees the program's slots; the pins stay with the
block until `pins.release_pio`.

### DMA on the RP2040 and RP2350

Both chips have a DMA controller, 12 channels on the RP2040 and 16 on the
RP2350, that moves data between memory and peripherals while the processor
does other work. `b.dma` hands the channels out, each once:

```dodo
b := board.take()!
channel := b.dma.channel()!
source: [256]u8 = [7; 256]
copy: [256]u8 = [0; 256]
channel.copy(&mut copy, &source)!
b.dma.release(channel)
```

| `b.dma` | Purpose |
| --- | --- |
| `channel()` | Claim the free channel with the lowest number; `Busy` if all are taken. |
| `claim(number)` | Claim a given channel, for a program that also drives channels through registers of its own; `Busy` if taken, `InvalidInput` beyond the chip's channels. |
| `release(channel)` | Abort its transfer, stop its signal, and take it back. |
| `channels()`, `is_claimed(number)` | The number of channels; whether one is claimed. |

A channel, an `rp_dma.Channel`, does little by itself: `copy(destination,
source)` copies a buffer and waits, a word at a time when both are
word-aligned, at about a word per clock cycle. Its main use is to be lent to
a bus or a serial port, which starts transfers on it and paces them by the
peripheral's data requests, so a FIFO never overflows or runs dry:

| Peripheral | DMA session | What runs in the background |
| --- | --- | --- |
| SPI bus | `bus.dma(&mut tx, &mut rx, &mut buffer)`, a `pl022.DmaBus` | Transfers to and from a buffer, without gaps between frames at any clock rate. |
| Serial port | `port.dma(&mut rx, &mut ring, &mut tx, &mut output)`, a `pl011.DmaPort` | Reception into a ring buffer of up to 32 KiB, and writes. |

A session borrows the peripheral, its channels, and its buffers until it is
dropped, which stops its transfers and gives them back: the borrow checker
then keeps the program away from memory a channel is moving, and from the
peripheral. A task keeps the session in a field.

`channel.set_signal(&SIGNAL)` raises an [executor signal](#waking-tasks-from-interrupts)
each time a transfer on the channel ends, from the chip runtime's default
`DMA_IRQ_0` handler, so a task returns Pending until then instead of waiting
for the bus. `clear_signal()` stops it. A program's own
`@interrupt(DMA_IRQ_0)` replaces the default handler, and the signals with
it; `listen()`, `finished()`, and `clear()` serve such a handler, and
`DMA_IRQ_1` is left free. If the program panics, the panic report aborts
every claimed channel before it writes to the serial ports.

#### SPI transfers by DMA

`bus.dma(&mut tx, &mut rx, &mut buffer)` returns a `pl022.DmaBus` that
sends from and receives into `buffer`, `tx` feeding the transmit FIFO and
`rx` emptying the receive FIFO. It implements
[`hal.SpiDma`](#transfers-in-the-background-dma), so device drivers written
against that protocol use it unchanged:

```dodo
spi := b.pins.spi(board.SPI_CLOCK_PIN, board.SPI_TX_PIN, some(board.SPI_RX_PIN), hal.SpiMode.Mode0, 8_000_000)!
tx := b.dma.channel()!
rx := b.dma.channel()!
buffer: [512]u8 = [0; 512]
flash := spi.dma(&mut tx, &mut rx, &mut buffer)!
command := flash.buffer()!
command[0] = 0x03
flash.start_transfer(512)!
// ... other work ...
flash.finish()!
data := flash.buffer()!
```

| `pl022.DmaBus` | Purpose |
| --- | --- |
| `buffer()` | The buffer, to fill before a transfer or read after one; `Busy` from a start until `finish`. |
| `start_transfer(count)` | Send `buffer[..count]` and replace each byte with the one received at the same time; returns at once. |
| `start_write(count)` | Send `buffer[..count]` and discard what arrives; the buffer stays as it is. |
| `is_busy()` | True until the last byte has been received, which is when `rx`'s signal is raised. |
| `poll_finish()` | Without waiting: false while a transfer runs, true once none does, ending one that is done; errors as for `finish`. |
| `finish()` | Wait for the running transfer, if any, and end it: `Overrun` if replies were lost, `Bus` on a bus error. |
| `abort()` | Stop a running transfer; the device saw only part of it. |
| `transfer(count)`, `write(count)` | Start and finish, for code that waits anyway but wants no gaps between frames. |
| `capacity()` | The buffer's length, the most one transfer moves. |

Drive a chip select around the transfer with an output pin, as with the
polled bus: low before `start_transfer`, high after `finish`. Dropping the
session aborts a running transfer, so the device sees only part of it; the
bus then works as before. Frames above 8 bits are `Unsupported`.

#### Serial ports by DMA

`port.dma(&mut rx, &mut ring, &mut tx, &mut output)` returns a
`pl011.DmaPort`, a serial port with the `std/io` Reader and Writer
protocols, `serial.Port`, and `serial.PollPort`, like `pl011.Port`, but with
two differences:

- `rx` moves every byte that arrives into `ring`, round and round, so input
  waits there and not just in the 32-byte FIFO: a 1 KiB ring holds 89 ms at
  115200 baud. Bytes not read when the ring comes round again are lost, and
  the next read reports `OVERRUN`.
- A write copies its bytes into `output` and returns once `tx` is sending
  them; it waits only while the previous write is still in `output`. Larger
  writes go out in `output`-sized chunks.

```dodo
port := b.pins.uart(board.UART_TX_PIN, some(board.UART_RX_PIN), 115_200)!
port.set_crlf(true)
rx := b.dma.channel()!
tx := b.dma.channel()!
ring: [1024]u8 = [0; 1024]
output: [128]u8 = [0; 128]
serial := port.dma(&mut rx, &mut ring, &mut tx, &mut output)!
fmt.printf(&mut serial, "ready\n")!
```

The ring is the largest block of `ring` whose size is a power of two and
whose address is a multiple of its size, as the controller needs to wrap
around it: `ring_size()` tells. Any buffer of 2N bytes holds an N-byte ring;
a static with `@align(N)` holds one of all its N bytes. Below 32 bytes is
`InvalidInput`. `available()` counts the bytes waiting, and `read`,
`poll_read`, `write`, `poll_write`, `flush`, `set_crlf`, the poll limits,
and the interrupt methods work as on `pl011.Port`. Bytes with framing or
parity errors, and breaks, arrive in the ring like the rest; a read reports
the error after the bytes it returns, without telling which byte it was.
Dropping the session stops both channels: input still in the ring is lost
and output not yet sent is cut short.

A task polls the port on a ticker, at whatever pace the ring allows, or
listens for `pl011.RX_TIMEOUT_INTERRUPT` to hear when a message has ended.
The repository's
[`examples/dma.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/dma.dodo)
runs such a console next to a task that streams SPI frames by DMA.

#### DMA in drivers

The peripheral drivers start their transfers through `hal.DmaChannel`, so
they name no chip, and their desktop tests run them against test channels
that move real memory. A transfer is a `hal.DmaTransfer`: a source and a
destination address, a count of elements of `hal.DmaWidth.Byte`,
`HalfWord`, or `Word`, which addresses increment, what paces it
(`hal.DmaPace.Unpaced`, or the `Destination` or `Source` peripheral), and an
optional `hal.DmaRing`. `start` is unsafe, as the channel then writes memory
behind the borrow checker's back; `is_busy`, `remaining`, `max_count`,
`abort`, and `take_error` are safe. A driver keeps its buffers borrowed until
the transfer is done or aborted, and aborts it when dropped, as the DMA
sessions above do. `hal.address_of(bytes)` gives a buffer's address.
[`hal.DmaStream`](#transfers-in-the-background-dma) does all of that for a
buffer moved to or from one peripheral data register, so a new DMA-backed
driver only turns its peripheral's requests on and off around it, and
`fake.DmaChannel` runs such code on a desktop.

An `rp_dma.Channel` finds the data request (DREQ) from the peripheral
register a transfer reads or writes: the UARTs' and SPI controllers' data
registers, the I2C controllers' `DATA_CMD`, the ADC's FIFO, and the PIO
state machines' FIFOs; `rp_dma.request(chip, address, transmit)` looks one
up. `start_request(transfer, dreq)` paces by any other request, or by
`rp_dma.TIMER0` to `TIMER3`. The driver, `std/embedded/chip/rp_dma`, is a
`Driver` over any `hal.Registers`, tested against a model of both chips'
controllers.

### Flash on the RP2040 and RP2350

The board's flash chip holds the program from its start and is otherwise
free: `b.flash`, a `rp_flash.Flash`, hands out regions of it above the
program, each a `rp_flash.Region` with the `hal.Flash` protocol, for
[persistent storage](#persistent-storage). `last(length)` takes the last
`length` bytes, `region(start, length)` any sectors from `program_end()` on,
and `free()` all of them; `release(region)` gives one back. A region below
the program or past the board's flash size (2 MiB on the Pico, 4 MiB on the
Pico 2) is `InvalidInput`, and one that overlaps a region still out is
`Busy`. `b.flash.read(offset, buffer)` reads anywhere, the program included.

Flashing a new program through the bootloader rewrites only the program's
own sectors, so data in a region stays, as long as the program does not grow
into it. Regions erase 4 KiB sectors, about 50 ms each, and write any bytes,
filling the rest of each 256-byte page with `0xFF`, which leaves it as it
was.

Both go through the boot ROM, which takes the flash out of execute-in-place
mode meanwhile, so nothing may run from or read flash: a region masks
interrupts on the core that calls it while it works, and on the RP2040 runs
that part from SRAM. Interrupt handlers wait for up to one sector erase. A
USB serial port stays connected, as its controller answers the host by itself.
Core 1 runs from flash too, so a region refuses to erase or write with
`Busy` on core 1 and while core 1 runs a task: stop it first. DMA transfers
must not read from flash while a region erases or writes.

### Real-time clocks on the RP2040 and RP2350

`b.rtc` is each chip's [real-time clock](#real-time-clocks), handed out by
`take()` with `rp2040.Rtc` and `rp2350.Rtc` as its types. Both implement
`hal.Rtc` and `hal.RtcAlarm`, and the register-level drivers behind them,
generic over a register block for desktop tests, are in
`std/embedded/chip/rp_rtc`:

- The **RP2040's RTC** is a calendar of year, month, day, hours, minutes,
  and seconds. `take()` clocks it at 31.25 kHz from the crystal and leaves it
  unset; a reset stops it, so every boot starts unset. It counts leap years
  every fourth year only, so a clock running from 2099 into 2100 needs its
  time set again after February 2100; setting a time in such a year tells it.
  Its alarm compares every field with the time.
- The **RP2350's always-on timer** is the power manager's 64-bit millisecond
  counter, ticking from the crystal. It sits in the always-on power domain:
  once set, it keeps counting through the resets that leave that domain
  running, such as the watchdog's, and a later `take()` finds it still set.
  It stops when the chip loses power.

The alarm raises the clock's interrupt while `listen`ing: `RTC_IRQ` on the
RP2040, during the alarm's second, and `POWMAN_IRQ_TIMER` on the RP2350,
until `clear_alarm`. `rtc.irq()` gives its number for
[`std/embedded/cortex_m`](#the-cortex-m-core). To sleep until the alarm,
enable the interrupt inside a critical section: its arrival ends `wfi` while
interrupts are masked, so no handler runs and the same code works on both
chips:

```dodo
b.rtc.listen()
hal.rtc_alarm_in(&mut b.rtc, 60_000)!
irq := b.rtc.irq()
section := hal.Critical.enter()
cortex_m.enable(irq)
for !b.rtc.alarm_fired()! {
    cpu.wait_for_interrupt()
}
b.rtc.clear_alarm()!
cortex_m.disable(irq)
cortex_m.unpend(irq)
core.drop(section)
```

An `@interrupt(RTC_IRQ)` or `@interrupt(POWMAN_IRQ_TIMER)` handler instead
has to call `clear_alarm`, or the interrupt stays raised; move the clock into
a `static mut Option<rp2040.Rtc>` for it, as for a [timer
alarm](#an-interrupt-driven-driver).

With [low power](#low-power), the RP2040's RTC keeps counting in deep
sleep only if its clock is in the sleep set, as in
`b.power.set_sleep_clocks(rp2040.ALARM_CLOCKS | rp2040.CLOCK_RTC)`; its alarm
then wakes the core like a timer alarm. The RP2350's always-on timer needs no
gate and counts through deep sleep. Both tick from the crystal, which
`b.power.dormant` stops, so neither counts while the chip is dormant, nor can
its alarm end a dormant state; set the clock again from another source
afterwards if the time matters.

### Two cores on the RP2040 and RP2350

Both chips have two Arm cores that run the same program from the same flash
and share all memory and peripherals. `main` runs on core 0; core 1 waits in
the boot ROM until the program starts it with a task of its own. The board
hands out three parts from `std/embedded/chip/rp_multicore`:

| Part | What it does |
| --- | --- |
| `b.core1` | `start(&mut stack, task)` runs `task.run()` on core 1 and returns a `Running` handle. |
| `b.fifos` | `claim()` hands out a core's end of the FIFOs between the cores, twice: one for core 0 and one to move into core 1's task. |
| `b.spinlocks` | `claim(n)` and `claim_free()` hand out the hardware locks both cores see. |
| `b.doorbells` | On the RP2350, `claim(n)` and `claim_free()` hand out the doorbells that interrupt the other core. The Pico's RP2040 has none. |

A task for core 1 is a value with a public `run(self)` method, like a
[`std/thread`](threads.md) task. It moves to core 1, so it may own
peripherals, FIFO ends, and spinlocks, but holds no references: `start`
rejects a task type that could not cross a thread boundary. This program
answers each word core 0 sends with the next number:

```dodo
package main

import "std/embedded/board"
import "std/embedded/chip/rp_multicore"

struct Echo {
    fifo: rp_multicore.Fifo

    pub fn run(self) {
        for {
            word := self.fifo.read()
            self.fifo.write(word + 1)
        }
    }
}

fn main() {
    b := board.take()!
    fifo := b.fifos.claim()!
    stack := [0u64; 512]
    core1 := b.core1.start(&mut stack, Echo { fifo: b.fifos.claim()! })!
    fifo.write(41)
    core.assert(fifo.read() == 42)
    for {
        b.led.toggle()
        b.timer.delay_ms(500)
    }
}
```

The task and the three words the runtime's start-up code needs go at the top
of `stack`, and core 1's stack grows down from below them. `start` asks for
`rp_multicore.MIN_STACK_BYTES` (256) beyond the task; give it room for what
`run` calls as well, a few KiB for an executor. On the RP2350 the stack's
bottom becomes core 1's stack limit, so an overflow faults and the panic
hook reports it instead of overwriting memory; the RP2040's Cortex-M0+
cannot detect one. A stack in a `static mut` keeps it off core 0's stack.

Core 1 runs until `Running` is dropped or `stop()`ped, which holds core 1 in
reset, so it never outlives its stack: keep the handle for as long as core 1
should run, typically in `main`, whose loop never ends. `is_finished()` reports
that `run` returned, after which core 1 sleeps. `start` always restarts core 1
first, so a stopped core starts again the same way, and a program that a
debugger restarted finds core 1 in a known state. A stopped task is not
dropped: peripherals it owned stay claimed, and a spinlock it held stays
locked. `start` must be called on core 0; it returns `InvalidInput` for a
stack that is too small, and `Timeout` or `Bus` if core 1's boot ROM does not
answer the launch sequence.

| `rp_multicore.Fifo` | Purpose |
| --- | --- |
| `write(word)`, `try_write(word)` | Send a 32-bit word to the other core, waiting while the FIFO is full, or false then. Wakes the other core from `read`. |
| `read()`, `try_read()` | The next word the other core sent, sleeping (`wfe`) until there is one, or none then. |
| `can_read()`, `can_write()`, `drain()` | Whether a word waits or there is room, and discarding the waiting words. |
| `set_signal(&signal)`, `clear_signal()` | Raise an executor signal on this core whenever the other core has written. |
| `irq()` | This core's FIFO interrupt: `SIO_IRQ_PROC0` or `SIO_IRQ_PROC1` on the RP2040, `SIO_IRQ_FIFO` on the RP2350. |

Each direction holds 8 words on the RP2040 and 4 on the RP2350. A `Fifo`
always reaches the end of the core that uses it, so the two from `claim()`
are alike: keep one on each core.

Each core has its own interrupt mask and NVIC: `hal.Critical` and
`cortex_m.enable` act on the core that calls them, and a handler runs on the
core that enabled its interrupt. So each core can run its own
[executor](#tasks-and-the-executor), with its own alarm as the clock. Its
signals belong to that core: an id raised on core 0 does not wake a task on
core 1. To wake a task on the other core, write to the FIFO: after
`fifo.set_signal(&WORDS)`, the chip runtime's handler of this core's FIFO
interrupt raises `WORDS` and holds the interrupt back until the next
`try_read`, `read`, or `drain`. The waiting task takes the signal and reads
until `try_read` returns none:

```dodo
const WORDS: executor.Signal = executor.Signal { id: 0 }

struct Doubler {
    fifo: rp_multicore.Fifo

    pub fn poll(&mut self, cx: &mut executor.Context) -> executor.Poll {
        WORDS.take(cx)
        for {
            match self.fifo.try_read() {
                some(word) => self.fifo.write(word * 2),
                none => break,
            }
        }
        return executor.Poll.Pending
    }
}
```

A program's own `@interrupt` handler for the FIFO interrupt replaces the
runtime's, and `set_signal` with it. GPIO events reach core 0 only: `Pin.listen`
enables them for core 0's `IO_IRQ_BANK0`. The USB serial port's interrupt runs
on the core that opened the port.

Statics are shared by both cores, and `hal.Critical` does not keep the other
core out. A [`sync.Mutex`](#sharing-values-with-a-mutex) does: `start`
installs hardware spinlock 31 as the lock of `std/embedded/sync`'s sections,
so from then on a mutex locked on one core makes the other core wait as
well as masking interrupts, and the same static works across handlers,
tasks, and cores:

```dodo
static PRIMES: sync.Mutex<Found> = sync.Mutex.new(Found { count: 0, largest: 0 })

// On core 1:
PRIMES.with(fn(found: &mut Found) {
    found.count += 1
    found.largest = candidate
})

// On core 0:
guard := PRIMES.lock()
report(guard.get().count)
```

While one core holds a mutex, the other spins when it locks any mutex, so
keep sections short. Stopping core 1 releases a section it held. A program
of its own can also guard state with a spinlock:

```dodo
static mut TOTAL: u32 = 0

guard := lock.lock()
unsafe {
    TOTAL += 1
}
core.drop(guard)
```

`lock()` masks interrupts on this core, then waits while the other core holds
the lock; dropping the guard releases it and unmasks them again. `try_lock()`
does not wait, and `is_locked()` reports either core's hold. Claim a lock once,
then give the other core its own copy with `clone()`. The RP2040 has 32 locks,
of which lock 31 belongs to `std/embedded/sync`. On the RP2350, writes to some
other SIO registers unlock the locks just below them (erratum RP2350-E2), so
locks 0 to 4, 8, 9, and 12 to 17 are never handed out either; locks 18 to 30
work on both chips. On the RP2350 a single word can also be
shared with [atomics](#sharing-state-with-atomics), whose exclusive loads and
stores work across cores; the RP2040's Cortex-M0+ only loads and stores
atomically.

The RP2350 also has eight **doorbells**, which interrupt the other core
without passing data, so they wake the other core's executor without using
the FIFO. Claim one, give core 1 a `clone()`, and let the core that waits
raise a signal from it:

```dodo
const RUNG: executor.Signal = executor.Signal { id: 4 }

// Core 0, before starting core 1 with `bell.clone()` in its task:
bell := b.doorbells.claim(0)!
bell.set_signal(&RUNG)

// In an async task of core 0's executor:
await RUNG.wait()

// On core 1, when there is news:
bell.ring()
```

| `rp_multicore.Doorbell` | Purpose |
| --- | --- |
| `ring()`, `withdraw()` | Ring the doorbell on the other core, or take back a ring it has not acknowledged. Rings before an acknowledgement are one. |
| `is_ringing()`, `answer()` | Whether the other core rang it, and acknowledging that, true if it was ringing. |
| `set_signal(&signal)`, `clear_signal()` | Raise a signal on this core when the other core rings: the RP2350 runtime's `SIO_IRQ_BELL` handler acknowledges the ring and raises it. |
| `irq()`, `clone()` | `SIO_IRQ_BELL`, which each core has, and the same doorbell for the other core. |

A program's own `@interrupt(SIO_IRQ_BELL)` handler replaces the runtime's.

`examples/two_cores.dodo` searches for primes on core 1 and reports progress
over USB serial from an executor on core 0, woken through the FIFO, with a
spinlock around the results both cores read. The ADC and the USB serial port
share the bookkeeping that starts and stops PLL_USB, so claim and release them
from one core.

### The watchdog on the RP2040 and RP2350

A watchdog restarts firmware that hangs. Once started, it counts down, and
unless the program calls `feed()` before the count runs out, it resets the
chip. `b.watchdog` is stopped after `board.take()`:

```dodo
package main

import "std/embedded/board"
import "std/embedded/hal"

fn main() {
    b := board.take()!
    if b.watchdog.reset_reason() == hal.ResetReason.Timeout {
        // The last run hung: count it in a word that survived the reset.
        hangs := b.watchdog.scratch(0)
        b.watchdog.set_scratch(0, hangs + 1)
    }
    b.watchdog.start(500)!
    for {
        b.led.toggle()
        b.timer.delay_ms(100)
        b.watchdog.feed()
    }
}
```

| `rp_watchdog.Watchdog` | Purpose |
| --- | --- |
| `start(timeout_ms)` | Count down from the timeout, from 1 ms up to `max_timeout_ms()`: 8388 ms on the RP2040, which counts twice per tick (erratum RP2040-E1), and 16777 ms on the RP2350. `InvalidInput` outside that range. Starting a running watchdog changes its timeout. |
| `feed()` | Count down from the timeout again; the `hal.Watchdog` protocol, so drivers and tasks can feed it without naming the chip. |
| `stop()`, `is_running()` | Stop counting, so the chip is not reset, until `start` again. |
| `timeout_ms()`, `remaining_us()` | The timeout last started, and the microseconds left before the reset. |
| `reset_reason()` | Why the chip last started: `hal.ResetReason.Timeout` if the watchdog ran out, `Restart` after `restart()`, and `Other` after power-up, the RUN pin, a debugger, or the boot ROM. |
| `scratch(n)`, `set_scratch(n, value)` | Two words, `n` 0 or 1, that survive a watchdog reset but not power-up, for a note to the next run. |
| `restart()` | Reset the chip through the watchdog now. Does not return. |

The reset restarts both cores, the peripherals, and RAM, but not the
oscillators. The count pauses while a debugger halts a core. A panic halts
the program, so with the watchdog running the default panic hook's report is
followed by a restart, which `reset_reason()` reads as `Timeout`.

The boot ROM resets through the same watchdog when it restarts into the
bootloader and when it starts newly flashed firmware. As in the Pico SDK,
`start` leaves a mark in a scratch register that the boot ROM overwrites, and
only a reset with that mark counts as the watchdog's.
`reboot_to_bootloader()` clears the mark, so the first run after `dodo run`
reads `Other`. `board.take()` reads the reason once and clears the mark.

### The Raspberry Pi Pico 2 W

The Pico 2 W is a Pico 2 with an Infineon CYW43439 Wi-Fi and Bluetooth chip.
Its LED is wired to the wireless chip's GPIO 0, not to an RP2350 pin, so
`std/embedded/board/pico2_w` reaches it through the wireless chip. The same
blink program works unchanged with `board = "pico2_w"`:

| Part | What it does |
| --- | --- |
| `board.take()` | Starts the RP2350 as on the Pico 2, then power-cycles the wireless chip and brings up its gSPI bus. This takes about 270 ms, most of it the chip's power-up time. A chip that does not answer is `Timeout`. |
| `b.led` | `set_high`, `set_low`, `toggle`, and `is_high`, like a pin. Each change is one gSPI write of about 20 µs at `-O2`. `read_back()` reads the level from the wireless chip. |
| `b.led.usb_powered()` | True while USB power is present, from the wireless chip's VBUS sense input on its GPIO 2. |
| `b.pins` | GP23 (wireless power), GP24 (gSPI data), GP25 (gSPI chip select), and GP29 (gSPI clock) belong to the wireless chip and return `Busy`. GP0 to GP22 and GP26 to GP28 are on the header. |
| `board.reboot_to_bootloader()` | As on the Pico 2, but without an activity LED: GP25 is the wireless chip's select line. |

`std/embedded/wireless/cyw43` is the driver. It is portable: `cyw43.Spi`
bit-bangs the chip's half-duplex gSPI bus on any three pins, using
`set_as_output` and `set_as_input` on the data pin to turn the line around,
and `cyw43.Device` speaks the chip protocol over any bus with the same methods.
`Device.init` checks the bus test patterns and the chip ID, starts the chip's
backplane clock, and `set_gpio` and `gpio_is_high` drive and read the chip's
own pins.
It does not load the wireless firmware, so Wi-Fi and Bluetooth are not
available yet.

### Custom boards

For a circuit board of your own, set `chip` instead of `board` and configure
the chip yourself through `std/embedded/chip`:

```dodo
package main

import "std/embedded/chip"

fn main() {
    p := chip.take(chip.Config { crystal_hz: 12_000_000 })!
    led := p.pins.output(15)!
    for {
        led.toggle()
        p.timer.delay_ms(250)
    }
}
```

```toml
[targets.firmware]
entry = "main.dodo"
chip = "rp2040"
```

A chip alone assumes 2 MiB of flash (4 MiB for `chip = "rp2350"`). To give the board a name that
`std/embedded/board` resolves to, add it to the compiler (see below).

### Adding chips and boards

Board and chip support is split the same way in the compiler and the library,
so each new board or chip is a small, self-contained addition:

- **A board** on an existing chip is one entry in `BOARDS` in
  `src/hardware.rs` (name, chip, flash size) and a `std/embedded/board/NAME` package
  that configures the chip and provides the entry points above.
- **A chip** is a `src/hardware/NAME.rs` descriptor (target triple, CPU,
  memory map, image format, its PIO version if it has PIO, and image fixups
  such as boot checksums), its
  startup runtime as LLVM IR and linker script, an entry in `CHIPS`, and a
  `std/embedded/chip/NAME` package with its peripherals.
- **A peripheral** that `std/embedded/peripheral` already drives needs only
  its base address and input clock: the chip package hands the driver a
  `hal.RegisterBlock`. A new controller design is a new package there, with a
  simulated controller to test it on a desktop.

The compiler compiles and optimizes the runtime itself with the selected
target machine, whatever the program's `-O` level, so no assembler or C
toolchain is involved. `scripts/test_pico.py` (with
`--board pico`, `--board pico2`, or `--board pico2_w`) shows how to check a chip
on real hardware, and `scripts/test_rp2350_float.py` checks the RP2350's
coprocessor `f64` helpers against the software ones on a board. Without a
board, `scripts/test_emulator.py` runs the self-test in `tests/emulator` on
emulated RP2040 and RP2350 chips, as CI does; on the RP2040 it also checks
the assembly floating-point helpers against the software ones.

### Unsupported chips

For microcontrollers without a chip descriptor, compile to an object for the chip and link it with
the board's startup code and linker script using the chip vendor's or the
Arm/RISC-V C toolchain:

```sh
dodo build firmware.dodo --emit obj --target thumbv7em-none-eabihf \
  --cpu cortex-m4 -O 2 --panic-hook board_panic -o build/firmware.o
```

- `--target` and `--cpu` select the instruction set; see
  [targets and linking](command-line.md#targets-and-linking).
- `--panic-hook` names a C function the board supplies to report a failed
  check (overflow, bounds, assertion) and halt or reset. See
  [debugging and failure configuration](command-line.md).
- The startup code initializes RAM, installs the vector table that names your
  `@interrupt` handlers, and calls your entry function.
- The compiler may call `memcpy`, `memmove`, and `memset`; freestanding links
  must provide them, as described in [core utilities](core.md).
- Portable standard-library packages (bytes, text, JSON, collections, math,
  formatting, `std/io`) work in firmware too. Hosted packages such as
  `std/console` and `std/fs` do not.

## Current limits

- The boards are `pico`, `pico2`, and `pico2_w`, on the `rp2040` and `rp2350`
  chips. The chip packages cover clocks, GPIO, I2C, SPI, UART, PWM, the ADC,
  PIO, the timer, the real-time clock, the watchdog, DMA, a USB serial port,
  the random number generators, low power, and the second core; other USB
  devices (HID, mass storage, several functions at once) or USB host mode
  are not bundled
  yet. Write them with `hal.Reg` as above.
- PIO state machines move words through their FIFOs by polling or their
  interrupts. DMA can pace transfers by their FIFOs' data requests
  (`rp_dma.request` with `sm.tx_fifo_address()`), and `hal.DmaStream` runs
  a buffer through one, as `rp_i2s.Output.dma` does. Their pin mappings
  reach GPIO 0 to 31; the
  RP2350B's GPIO 32 to 47 and the RP2350's GPIOBASE register are not
  supported. `rp_pio.assemble` takes one program per call.
- PWM inputs measure by counting over windows, as the slices cannot
  timestamp edges; the PWM wrap interrupt is not exposed. ADC reads are
  polled one at a time, without the round-robin sampler, FIFO, or DMA.
- A serial port keeps no more input than its 32-byte FIFO unless it
  receives [by DMA](#serial-ports-by-dma), a program serves its interrupts,
  as in [an interrupt-driven driver](#an-interrupt-driven-driver), or the
  other end honours RTS. An I2C master is the bus's only master, without high-speed
  mode, which the chips' controllers lack, and cannot send empty transfers.
  Neither chip has a CAN controller: CAN goes through an external MCP2515
  on SPI, and I2S through PIO, as master only (the program drives BCLK and
  LRCLK).
  The Pico 2 W's wireless chip drives only its LED and senses USB power;
  Wi-Fi and Bluetooth need its firmware, which is not loaded yet.
- The software floating-point helpers raise no floating-point exception
  flags and return the default quiet NaN for every NaN result.
- Core 1 runs one task, started from core 0, and the cores share the
  vector table. Executor tasks on either core, async or hand-written, are
  cooperative: there is no preemptive scheduler. GPIO events interrupt
  core 0 only, and the RP2350's RISC-V cores are not supported. A signal
  belongs to one core; the FIFO or a doorbell carries a wake-up across.
- An async fn's future must stay in place once started, cannot await
  itself, and is as large as everything its body keeps across an `await`;
  `await` begins its statement. See [async functions](async.md).
- `sync.Mutex` masks interrupts while held, and with core 1 running also
  holds off the other core; on a hosted target it excludes nothing, like
  the executor's signals, so use both from one thread there.
- On the Cortex-M0+ of the RP2040, atomics only load and store; see
  [sharing state with atomics](#sharing-state-with-atomics).
- A naked function or global assembly can replace a vector's default
  handler or add code the runtime lacks, but the boot sequence and the
  vector table itself still come from the chip runtime.
- Low power covers sleep, deep sleep, clock gating, and the dormant state
  woken by GPIO 0 to 29. The RP2350's power manager states, which switch
  power domains off, its always-on timer, and waking a dormant chip by time
  (the RP2040's RTC from an external clock, the RP2350's AON timer) are not
  supported. Neither is lowering the system clock to save power. On a dual-core chip,
  deep sleep stops the sleep set's other clocks only while both cores
  deep-sleep.
- The timer protocols count microseconds. Counters at other rates, a
  timer's capture and compare channels, and periodic hardware timers have no
  shared protocol; use the chip's own methods, as for the RP2040's and
  RP2350's alarms, whose interrupts `listen` turns on.
- Flash regions erase and write only from core 0, with interrupts masked and
  core 1 stopped; there is no lockout that pauses core 1 in SRAM meanwhile.
  External SPI flash chips have no bundled driver yet, but any driver with
  the `hal.Flash` methods holds a `storage.Store`.
- Real-time clocks tick from the crystal: the RP2040's RTC is not clocked
  from a GPIO, nor the RP2350's always-on timer from its low-power
  oscillator or a GPIO, so they stop while the chip is dormant and cannot
  wake it, and the RP2350's power-down states are not supported.
- SPI by DMA and polled SPI transfers move frames of up to 8 bits; wider
  frames use the word methods, which wait. The ADC is not read by DMA yet,
  and channel signals come only from `DMA_IRQ_0`.

For raw register and pointer contracts, see [memory and foreign calls](memory-and-ffi.md).
