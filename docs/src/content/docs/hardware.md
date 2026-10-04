---
title: "Hardware and embedded"
description: "Typed registers, interrupts, portable device drivers, and desktop testing with std/hal."
section: "Standard library"
order: 161
---

Dodo programs can talk to hardware directly, with no separate embedded
dialect. The same language, compiler, and test runner build a desktop tool and
a microcontroller firmware image. Hardware support is an ordinary library,
`std/hal`, on top of two small compiler packages:

| Layer | Import | What it gives you |
| --- | --- | --- |
| Device protocols | `std/hal` | Pin, delay, I2C, SPI, and serial interfaces that drivers are written against. |
| Typed registers | `std/hal` | `hal.Reg<T>` handles and mask constants for memory-mapped registers. |
| Interrupts | `std/hal` | `hal.Critical`, a section that masks interrupts until it is dropped. |
| Desktop testing | `std/hal/fake` | In-memory pins, delays, and buses that record what a driver did. |
| Raw access | `core/mmio`, `core/cpu` | Volatile loads/stores, barriers, interrupt masking, `wfi`. |

Most code needs only `std/hal`. Three kinds of code meet here:

- **Chip support** knows datasheet addresses. It turns registers into values
  that implement the protocols, for example a `Pin` with `set_high`.
- **Drivers** know a device, such as a sensor. They are generic over any value
  with the protocol's methods and never mention a chip.
- **Applications** take the chip's values and hand them to drivers.

Drivers therefore run unchanged on any chip, and on a desktop with fakes.

## Quickstart: a driver and its tests

Save this as `blink.dodo` and run `dodo test blink.dodo`. No hardware is needed:

```dodo test
package blink

import "std/hal"
import "std/hal/fake"

// Works with any output pin and any delay.
pub fn blink<P, D>(led: &mut P, delay: &mut D, times: u32) {
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

`blink` names no chip. `<P, D>` are type parameters: any pin with `set_high`
and `set_low`, and any delay with `delay_us`, satisfy it. The compiler checks
each use and generates code specialized for the real types, so the protocol
costs nothing at run time. See [generics](generics.md#reusable-behavior-through-public-methods)
for how these method protocols work.

## Device protocols

A protocol is a set of method signatures. Any struct with those public methods
can be passed where a driver expects it:

| Device | Methods |
| --- | --- |
| Output pin | `set_high(&mut self)`, `set_low(&mut self)` |
| Input pin | `is_high(&self) -> bool` |
| Delay | `delay_us(&mut self, microseconds: u32)` |
| I2C bus | `write(&mut self, address: u8, bytes: &[u8]) -> void!hal.Error`<br>`read(&mut self, address: u8, buffer: &mut[u8]) -> void!hal.Error`<br>`write_read(&mut self, address: u8, bytes: &[u8], buffer: &mut[u8]) -> void!hal.Error` |
| SPI bus | `transfer(&mut self, bytes: &mut[u8]) -> void!hal.Error`<br>`write(&mut self, bytes: &[u8]) -> void!hal.Error` |
| Serial port | The [`std/io`](io.md) Reader and Writer protocols. |

Pins and delays cannot fail. Bus operations return `hal.Error`, one shared
enum: `Timeout`, `NoAcknowledge`, `Bus`, `Overrun`, `Busy`, `InvalidInput`, and
`Unsupported`. Every driver and chip uses the same error type, so `?` passes
errors through layers without conversions, and `==` compares them. Use
`hal.error_name(error)` for a log message.

A serial port is just a byte stream. Implement `std/io`'s `write` and `read`
for a UART and every existing helper works with it, including `io.write_all` and
[`std/fmt`](formatting.md) output.

### Helpers

| Helper | Purpose |
| --- | --- |
| `hal.write_pin(&mut pin, high)` | Drive a pin from a `bool`. |
| `hal.delay_ms(&mut delay, ms)` | Millisecond delay from any `delay_us`. |
| `hal.read_register(&mut bus, address, register)` | Read one numbered 8-bit I2C register. |
| `hal.read_registers(&mut bus, address, first, buffer)` | Read consecutive I2C registers. |
| `hal.write_register(&mut bus, address, register, value)` | Write one I2C register. |
| `hal.SpiDevice<P>` | One SPI device and its chip-select pin; see below. |

### Several devices on one SPI bus

A `hal.SpiDevice` owns a device's chip-select pin and drives it around each
transaction. The bus itself is passed to each call instead of being stored, so
any number of devices share it, and the borrow checker guarantees that two
transactions never overlap:

```dodo test
package spi_example

import "std/hal"
import "std/hal/fake"

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

import "std/hal"
import "std/hal/fake"

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

## Testing with fakes

`std/hal/fake` implements every protocol in memory:

| Fake | Behaves like | Inspect with |
| --- | --- | --- |
| `fake.Pin` | An input/output pin, initially low. | `is_high()`, `changes()` |
| `fake.Delay` | A delay that returns at once. | `elapsed_us()` |
| `fake.I2c` | One device at an address with 256 numbered registers and an auto-incrementing register pointer. Other addresses report `NoAcknowledge`. | `register(n)`, `set_register(n, v)`, `fail_next(error)` |
| `fake.Spi` | A bus that records sent bytes and answers from a programmed reply. | `sent()`, `clear()`, `reply_with(bytes)` |

`fail_next` makes the next I2C transaction fail, so error paths are as easy to
test as success. For serial ports use `io.MemoryReader` and `io.MemoryWriter`
from [`std/io`](io.md). A protocol is just methods, so writing your own fake for
a device with special behavior takes a few lines.

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

import "std/hal"
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

### Owning peripherals once

Chip support usually hands each peripheral out once. The
[GPIO example](https://github.com/Jotrorox/dodo/blob/main/examples/gpio.dodo)
shows the full pattern for the RP2040: an unsafe `Port.take()` at startup, and
safe `port.output(25)` calls that return `hal.Error.Busy` if a pin is claimed
twice. Its `Pin` implements the output and input protocols with single stores to
the chip's set/clear registers, and the whole abstraction compiles to the same
instructions as hand-written register code.

## Interrupts

An interrupt handler is an exported C-ABI function with the name your chip's
startup code puts in the vector table. Firmware entry points work the same way:

```dodo
package firmware

import "core/cpu"
import "std/hal"

static mut TICKS: u32 = 0

// Cortex-M SysTick exception.
extern "C" fn SysTick_Handler() {
    section := hal.Critical.enter()
    unsafe {
        TICKS += 1
    }
    core.drop(section)
}

fn ticks() -> u32 {
    section := hal.Critical.enter()
    value := unsafe { TICKS }
    core.drop(section)
    return value
}

// Called by the board's reset handler once RAM is initialized.
extern "C" fn firmware_main() {
    for ticks() < 1000 {
        cpu.wait_for_interrupt()
    }
}
```

`hal.Critical.enter()` masks interrupts on the current core and remembers
whether they were enabled; dropping it restores exactly that state, so sections
nest. Reading or writing `static mut` storage is unsafe in Dodo: the critical
section is what makes it correct, and the `unsafe` block marks the place where
you rely on it. Keep sections short, and remember they never exclude other
threads or other cores.

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

## Building firmware

Compile to an object for the chip and link it with the board's startup code and
linker script using the chip vendor's or the Arm/RISC-V C toolchain:

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
  `extern "C"` handlers, and calls your entry function.
- The compiler may call `memcpy`, `memmove`, and `memset`; freestanding links
  must provide them, as described in [core utilities](core.md).
- Portable standard-library packages (bytes, text, JSON, collections, math,
  formatting, `std/io`) work in firmware too. Hosted packages such as
  `std/console` and `std/fs` do not.

## Current limits

- Chip support packages for specific microcontrollers are not bundled yet;
  write them with `hal.Reg` as above, or start from the GPIO example.
- `std/sync/atomic` currently supports x86-64 and AArch64 only. On
  microcontrollers, share state with interrupt handlers through a critical
  section.
- There is no inline assembly, `@interrupt` attribute, or linker-section
  attribute. Handlers use the C ABI, which is what Cortex-M vector tables
  expect; RISC-V handlers need a small assembly trampoline in the startup code.
- ADC, PWM, timers, and DMA have no shared protocol yet. Expose them as methods
  on your chip's types.

For raw register and pointer contracts, see [memory and foreign calls](memory-and-ffi.md).
