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
| Device protocols | `std/embedded/hal` | Pin, delay, I2C, SPI, and serial interfaces that drivers are written against. |
| Peripheral drivers | `std/embedded/peripheral/...` | I2C, SPI, and UART controller drivers, shared by every chip that contains the controller. |
| Typed registers | `std/embedded/hal` | `hal.Reg<T>` and `hal.RegisterBlock` handles and mask constants for memory-mapped registers. |
| Interrupts | `std/embedded/hal`, `std/embedded/cortex_m` | `@interrupt` handlers; `hal.Critical`, a section that masks interrupts until it is dropped; `hal.Handler`, a slot for a handler chosen at run time; the Cortex-M interrupt controller and SysTick. |
| Tasks | `std/embedded/executor` | Several activities on one core, each woken on time or by an interrupt, with the core asleep in between. |
| Desktop testing | `std/embedded/hal/fake` | In-memory pins, delays, buses, serial ports, and a simulated clock that record what a driver did. |
| Boards | `std/embedded/board` | The selected board: one `take()` and the same LED, pin, and timer fields on every board. |
| Chip support | `std/embedded/chip` | The selected microcontroller's peripherals, for custom boards. |
| Wireless chip | `std/embedded/wireless/cyw43` | The CYW43439 on the Pico 2 W: power-up, its gSPI bus, and its GPIO pins. |
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
| `hal.I2c` | `write(&mut self, address: u8, bytes: &[u8]) -> void!hal.Error`<br>`read(&mut self, address: u8, buffer: &mut[u8]) -> void!hal.Error`<br>`write_read(&mut self, address: u8, bytes: &[u8], buffer: &mut[u8]) -> void!hal.Error` |
| `hal.Spi` | `transfer(&mut self, bytes: &mut[u8]) -> void!hal.Error`<br>`write(&mut self, bytes: &[u8]) -> void!hal.Error` |
| `hal.PwmOutput` | `max_duty(&self) -> u16`<br>`set_duty(&mut self, duty: u16) -> void!hal.Error` |
| `hal.AnalogInput` | `max_value(&self) -> u16`<br>`read(&mut self) -> u16!hal.Error` |
| Serial port | [`io.Reader` and `io.Writer`](io.md) from `std/io`. |
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
reference voltage. `hal.I2C_STANDARD_HZ`,
`hal.I2C_FAST_HZ`, and `hal.I2C_FAST_PLUS_HZ` name the usual I2C speeds.
The register block protocol is for [peripheral drivers](#register-blocks)
rather than device drivers.

Pins and delays cannot fail. Bus, PWM, and analog operations return
`hal.Error`, one shared enum, because such a device may itself sit on a bus,
as a PWM controller chip on I2C does: `Timeout`, `NoAcknowledge`, `Bus`,
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

### Helpers

| Helper | Purpose |
| --- | --- |
| `hal.write_pin(&mut pin, high)` | Drive a pin from a `bool`. |
| `hal.delay_ms(&mut delay, ms)` | Millisecond delay from any `delay_us`. |
| `hal.read_register(&mut bus, address, register)` | Read one numbered 8-bit I2C register. |
| `hal.read_registers(&mut bus, address, first, buffer)` | Read consecutive I2C registers. |
| `hal.write_register(&mut bus, address, register, value)` | Write one I2C register. |
| `hal.set_duty_percent(&mut output, percent)` | Set a PWM output high for 0 to 100% of each period. |
| `hal.set_duty_fraction(&mut output, numerator, denominator)` | Set a PWM duty cycle to a fraction, rounded to the nearest step. |
| `hal.set_pulse_us(&mut output, pulse_us, period_us)` | Set a PWM pulse length, as for a servo: `set_pulse_us(&mut servo, 1500, 20_000)` centres one on a 50 Hz output. |
| `hal.read_millivolts(&mut input, reference_mv)` | Read an analog input as millivolts. |
| `hal.SpiDevice<P>` | One SPI device and its chip-select pin; see below. |

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
| `fake.Delay` | A delay that returns at once. | `elapsed_us()` |
| `fake.I2c` | One device at an address with 256 numbered registers and an auto-incrementing register pointer. Other addresses report `NoAcknowledge`. | `register(n)`, `set_register(n, v)`, `fail_next(error)` |
| `fake.Spi` | A bus that records sent bytes and answers from a programmed reply. | `sent()`, `clear()`, `reply_with(bytes)` |
| `fake.Serial` | A serial port, the `std/io` Reader and Writer: it records what is written and reads back bytes given to `feed`, then reports end of input; `poll_read` reports Pending instead. | `sent()`, `clear()`, `feed(bytes)` |
| `fake.Pwm` | A PWM output with a given `max_duty`, initially at duty 0. | `duty()`, `changes()`, `fail_next(error)` |
| `fake.Analog` | An analog input with a given `max_value` that reads the value given to `set`, after any given to `queue`, one per read. | `set(value)`, `queue(values)`, `reads()`, `fail_next(error)` |
| `fake.Clock` | The [executor](#tasks-and-the-executor)'s clock: time jumps ahead when the executor sleeps, and simulated interrupts raise signals. | `now_us()`, `interrupt_at(time, &signal)`, `advance(us)`, `sleeps()`, `idle_us()` |

`fail_next` makes the next I2C transaction, duty change, or analog read fail,
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
through `static mut` storage. Reading or writing `static mut` storage is unsafe
in Dodo, and keeping both sides consistent is the program's job: `hal.Critical.enter()` masks interrupts on the
current core and remembers whether they were enabled; dropping it restores
exactly that state, so sections nest. A handler cannot be interrupted by
itself, so it needs no section for state only it and lower-priority code use.
Keep sections short, and remember they never exclude other threads or cores.

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

### An interrupt-driven driver

A driver can live in a `static mut Option<...>` that the handler serves. The
repository's
[`examples/interrupts`](https://github.com/Jotrorox/dodo/blob/main/examples/interrupts/main.dodo)
echoes a serial port this way: the UART's receive interrupt moves bytes into a
queue, SysTick counts milliseconds, and the main loop sleeps until either has
news:

```dodo
static mut SERIAL: Option<pl011.Port> = none

@interrupt(UART0_IRQ)
fn serial_received() {
    unsafe {
        match &mut SERIAL {
            some(port) => {
                buffer := [0u8; 32]
                match port.poll_read(&mut buffer) {
                    ok(progress) => {
                        enqueue(&buffer[..progress.transferred])
                    }
                    err(failure) => {
                        enqueue(&buffer[..failure.transferred])
                        port.clear_interrupts(pl011.ERROR_INTERRUPTS)
                    }
                }
            }
            none => {}
        }
    }
}

fn main() {
    b := board.take()!
    port := b.pins.uart(board.UART_TX_PIN, some(board.UART_RX_PIN), 115_200)!
    port.listen(pl011.RX_INTERRUPT | pl011.RX_TIMEOUT_INTERRUPT | pl011.ERROR_INTERRUPTS)
    unsafe {
        SERIAL = some(port)
    }
    cortex_m.enable(chip.UART0_IRQ)
    // ... sleep with cpu.wait_for_interrupt() and drain the queue
}
```

Three switches must all be on for a handler to run: the peripheral's own
interrupt enable (`port.listen`, `alarm.listen`), the interrupt's line in the
core's interrupt controller (`cortex_m.enable`), and the core's interrupt mask,
which is open at reset and closed only inside critical sections.

| Driver | Interrupt sources |
| --- | --- |
| `pl011.Port` and `Controller` | `listen(sources)`, `ignore(sources)`, `pending_interrupts()`, and `clear_interrupts(sources)` with `RX_INTERRUPT` (FIFO 3/4 full), `RX_TIMEOUT_INTERRUPT` (bytes waiting for 32 bit times), `TX_INTERRUPT`, and `ERROR_INTERRUPTS`. The receive sources end when reads drain the FIFO. |
| `Timer.alarm(n)` on the RP2040 and RP2350 | An `Alarm` that fires once at `schedule_at(time)` or `schedule_in(microseconds)`; `listen` raises `alarm.irq()`, and its handler calls `clear`. `release_alarm` gives it back. With `now_us()` and `sleep_until(deadline)` it is also the [executor](#tasks-and-the-executor)'s clock. |
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

### Sleeping and other targets

`cpu.wait_for_interrupt()` sleeps until an interrupt is pending, which is the
usual idle loop. `cpu.fence()` is a full hardware memory barrier, for example
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

A task is any value with a `poll` method, usually a struct that owns the
peripherals it uses and remembers where it is in its work:

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
32 bytes. The repository's
[`examples/tasks`](https://github.com/Jotrorox/dodo/blob/main/examples/tasks/main.dodo)
runs a blinking LED, a debounced button, and such a serial console together.

| Interrupt source | Raise it | Acknowledge it |
| --- | --- | --- |
| GPIO pin (RP2040, RP2350) | `pin.listen(events)` with `chip.RISING_EDGE`, `FALLING_EDGE`, `LEVEL_HIGH`, `LEVEL_LOW`; all pins share `IO_IRQ_BANK0` (`pin.irq()`) | `pin.events()` shows what is raising it; `pin.clear_events(edges)` clears latched edges. Levels last as long as the level. `ignore(events)` stops. |
| Serial port | `port.listen(sources)` | Reading drains the receive sources; `clear_interrupts` the errors. |
| Timer alarm | `alarm.listen()` | `alarm.clear()`. An alarm used as the executor's clock is busy. |
| USB serial port | `serial.set_signal(&SIGNAL)`: the port's own interrupt handler raises it | Nothing to acknowledge: read with `poll_read` until it is Pending. |

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
  numbers are one set for the whole program, so libraries that raise signals
  take their numbers from the application.
- Tasks are state machines written by hand: Dodo has no `async` functions
  that the compiler would turn into them. Keep the state in the task's
  fields, and do each step without waiting.
- A task owns its peripherals. Tasks that share a bus are one task, or one
  task owns the bus and the others send it requests with signals.
- I2C and SPI transfers still wait for the bus inside `poll`; they are short
  at usual speeds.
- On a desktop, where a critical section excludes nothing, use the executor
  and its signals from one thread.

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
| `b.timer` | The delay protocol, plus `delay_ms(ms)` and `now_us() -> u64`; `alarm(n)` claims an alarm, which can also run the [executor](#tasks-and-the-executor). |
| `b.usb.serial()` | A [USB serial port](#usb-serial-on-the-rp2040-and-rp2350) on the board's USB connector, implementing the `std/io` Reader and Writer, for printing over the USB cable; `b.usb.release_serial(port)` gives it back. |
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
| Runtime helpers | `memcpy`, `memmove`, `memset`, `memcmp`, the 32- and 64-bit division, 64-bit multiplication, and shift routines the Cortex-M0+ lacks in hardware, and software `f32` and `f64` arithmetic, comparisons, conversions, and `%`, since it has no floating-point unit. Results are correctly rounded IEEE 754, to nearest with ties to even, subnormals included. |
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
| Reset handler | Clears the stack limit, installs the vector table, enables the FPU, initializes RAM, and calls `main`. |
| Panic hook and helpers | As on the RP2040. The Cortex-M33 divides 32-bit integers in hardware, so only the 64-bit helpers are linked. Its FPU does single precision only: `f32` arithmetic runs in hardware, and `f64`, `f32` `%`, and conversions between `f32` and `f64` or 64-bit integers use the software helpers. |
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
import "std/embedded/peripheral/designware_i2c"

// A register device at 0x42: the first byte of a write picks the
// register, later bytes fill it, and reads continue from there.
fn main() {
    b := board.take()!
    target := b.pins.i2c_target(board.I2C_SDA_PIN, board.I2C_SCL_PIN, 0x42, false, hal.I2C_FAST_HZ)!
    memory: [16]u8 = [0; 16]
    pointer := 0usize
    buffer: [16]u8 = [0; 16]
    for {
        match target.poll(&mut buffer) {
            designware_i2c.TargetEvent.Write(count, start) => {
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
            designware_i2c.TargetEvent.Read => {
                target.respond(&memory[pointer..pointer + 1])
                pointer = (pointer + 1) % 16
            }
            _ => {}
        }
    }
}
```

`respond` can queue several bytes at once; what the master does not read is
dropped when it next reads. Addresses that the I2C specification reserves
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
| Writing | `write` waits while the 32-byte transmit FIFO is full and returns once the last byte is queued; `flush()` waits until it has left. `poll_write` queues what fits without waiting. A write or `flush()` that makes no progress for `pl011.DEFAULT_POLL_LIMIT` status polls (around a second), as when flow control holds the transmitter, fails with `TimedOut` and the bytes queued before it; `set_poll_limit(polls)` changes the limit, `pl011.NO_POLL_LIMIT` removes it. |
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
| Writing | `write` queues its bytes and returns. While no terminal has the port open, the first 1024 bytes (`rp_usb.TX_CAPACITY`) wait for one and the rest are dropped, so the start of the output, such as a banner printed at startup, is shown when a terminal opens it. While one has it open, a full queue waits for the host, and output the host does not take within 100 ms is dropped. Printing never stops the program and never fails. `flush()` waits until the host has taken everything, within the same limit; `poll_write` queues what fits. |
| Reading | `read` waits for the first byte, then returns what has arrived; `poll_read` returns at once, `Pending` while nothing has. Up to 256 bytes (`rp_usb.RX_CAPACITY`) are kept; beyond that the host waits until the program reads, so no input is lost. `available()` counts what is waiting. |
| Terminals | Newline translation is on at first, unlike on a UART: `"\n"` goes out as `"\r\n"`, and the CR that Enter sends, alone or followed by LF, reads as `"\n"`. `set_crlf(false)` passes binary data unchanged. |
| Connection | `is_connected()` is true while a program on the host has the port open (DTR set, as terminals do); `is_configured()` while the host has enumerated the board. `line_state()` returns `rp_usb.DTR` and `rp_usb.RTS`, and `baud()` the rate the host set, for a bridge to a UART. |
| Tasks | `set_signal(&SIGNAL)` raises an [executor signal](#waking-tasks-from-interrupts) whenever bytes arrive, output leaves, or the host opens, closes, or suspends the port. |
| Restarting into the bootloader | A host that sets the port to 1200 baud restarts the board into its USB bootloader once the request is answered, as with the Pico SDK and Arduino. `dodo run` does this when no bootloader drive is mounted, so after the first flash the board needs no BOOTSEL press. `set_bootloader_reset(false)` turns it off, for example for a bridge to a UART that may really run at 1200 baud. |
| Panics | Until the port is released, a panic writes its report to it, and the default panic hook keeps serving the port after it halts: a terminal opened later still receives the report, and `dodo run` still works. |
| Identity | Raspberry Pi's vendor ID `0x2E8A` with the Pico SDK's USB serial product ID `0x000A`, manufacturer "Dodo", and the board's name as the product. `b.usb.serial()` claims the port once (`Busy` otherwise); `b.usb.release_serial(port)` disconnects from the host and stops the controller. |
| Clocks | The controller runs at 48 MHz from PLL_USB, which the ADC shares: the PLL runs while either uses it. |

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
or `p.usb.serial_as(name, activity_led)`.

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
  memory map, image format, and image fixups such as boot checksums), its
  startup runtime as LLVM IR and linker script, an entry in `CHIPS`, and a
  `std/embedded/chip/NAME` package with its peripherals.
- **A peripheral** that `std/embedded/peripheral` already drives needs only
  its base address and input clock: the chip package hands the driver a
  `hal.RegisterBlock`. A new controller design is a new package there, with a
  simulated controller to test it on a desktop.

The compiler compiles the runtime itself with the selected target machine, so
no assembler or C toolchain is involved. `scripts/test_pico.py` (with
`--board pico`, `--board pico2`, or `--board pico2_w`) shows how to check a chip
on real hardware.

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
  the timer, and a USB serial port; PIO, DMA, and other USB devices (HID,
  mass storage, several functions at once) or USB host mode are not bundled
  yet. Write them with `hal.Reg` as above.
- PWM outputs only generate; the slices' input modes, which count or time a
  pin's pulses, and the PWM wrap interrupt are not exposed. ADC reads are
  polled one at a time, without the round-robin sampler, FIFO, or DMA.
- I2C and SPI transfers are polled, without interrupts or DMA. Serial ports
  are polled too unless a program serves their interrupts, as in
  [an interrupt-driven driver](#an-interrupt-driven-driver) or a
  [task woken by them](#waking-tasks-from-interrupts); without that a
  port keeps no more input than its 32-byte FIFO unless the other end honours
  RTS. An I2C master is the bus's only master, without high-speed
  mode, which the chips' controllers lack, and cannot send empty transfers.
  The Pico 2 W's wireless chip drives only its LED and senses USB power;
  Wi-Fi and Bluetooth need its firmware, which is not loaded yet.
- The software floating-point helpers are written for correctness, not speed:
  division, for example, computes one quotient bit per step, and `f32`
  arithmetic on the Cortex-M0+ goes through `f64`. They raise no
  floating-point exception flags and return the default quiet NaN for every
  NaN result.
- Only core 0 runs; core 1 stays in the boot ROM. Several activities share
  core 0 as [executor tasks](#tasks-and-the-executor), which are
  cooperative: there is no preemptive scheduler. The RP2350's RISC-V cores
  are not supported.
- On the Cortex-M0+ of the RP2040, atomics only load and store; see
  [sharing state with atomics](#sharing-state-with-atomics).
- Inline assembly lives inside functions; there is no global or naked
  assembly, so startup code and vector tables come from the chip runtime or a
  `@section`-placed static.
- Timers and DMA have no shared protocol yet. Expose them as methods on your
  chip's types.

For raw register and pointer contracts, see [memory and foreign calls](memory-and-ffi.md).
