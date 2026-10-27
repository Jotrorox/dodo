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
| Interrupts | `std/embedded/hal` | `hal.Critical`, a section that masks interrupts until it is dropped; `hal.Handler`, a slot for a handler chosen at run time. |
| Desktop testing | `std/embedded/hal/fake` | In-memory pins, delays, buses, and serial ports that record what a driver did. |
| Boards | `std/embedded/board` | The selected board: one `take()` and the same LED, pin, and timer fields on every board. |
| Chip support | `std/embedded/chip` | The selected microcontroller's peripherals, for custom boards. |
| Wireless chip | `std/embedded/wireless/cyw43` | The CYW43439 on the Pico 2 W: power-up, its gSPI bus, and its GPIO pins. |
| Raw access | `core/mmio`, `core/cpu` | Volatile loads/stores, barriers, interrupt masking, `wfi`. |

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
| Serial port | [`io.Reader` and `io.Writer`](io.md) from `std/io`. |
| `hal.Registers` | `read(&mut self, offset: usize) -> u32`<br>`write(&mut self, offset: usize, value: u32)` |

Write drivers against bounded parameters, `fn probe<B: hal.I2c>(bus: &mut B)`,
for zero-cost static dispatch. Use `&mut dyn hal.I2c` (or any other interface)
when the device is chosen at run time, or to keep different devices in one
array, such as a list of sensors behind one interface of your own.

A bus is configured when it is created: its frequency, and for SPI a
`hal.SpiMode` (`Mode0` to `Mode3`, the clock polarity and phase numbering of
device datasheets). The protocols then only move bytes, so a driver works
whatever speed the application chose. `hal.I2C_STANDARD_HZ`,
`hal.I2C_FAST_HZ`, and `hal.I2C_FAST_PLUS_HZ` name the usual I2C speeds.
The register block protocol is for [peripheral drivers](#register-blocks)
rather than device drivers.

Pins and delays cannot fail. Bus operations return `hal.Error`, one shared
enum: `Timeout`, `NoAcknowledge`, `Bus`, `Overrun`, `Busy`, `InvalidInput`, and
`Unsupported`. Every driver and chip uses the same error type, so `?` passes
errors through layers without conversions, and `==` compares them. Use
`hal.error_name(error)` for a log message.

A serial port is just a byte stream. Implement `std/io`'s `write` and `read`
for a UART and every existing helper works with it, including `io.write_all` and
[`std/fmt`](formatting.md) output. The RP2040 and RP2350
[serial ports](#serial-ports-on-the-rp2040-and-rp2350) do exactly that.

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
| `fake.Serial` | A serial port, the `std/io` Reader and Writer: it records what is written and reads back bytes given to `feed`, then reports end of input. | `sent()`, `clear()`, `feed(bytes)` |

`fail_next` makes the next I2C transaction fail, so error paths are as easy to
test as success. A driver for a device on a serial line, such as a GPS or
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

An interrupt handler is an exported C-ABI function with the name your chip's
startup code puts in the vector table. Firmware entry points work the same way:

```dodo
package firmware

import "core/cpu"
import "std/embedded/hal"

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

### Handlers registered at run time

A vector-table entry is fixed when the firmware is linked. To choose or change
the work an interrupt does while the program runs, keep a function pointer in a
`hal.Handler` slot and call it from the entry point:

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

extern "C" fn TIMER_IRQ_0() {
    unsafe {
        TIMER.run()
    }
}

extern "C" fn firmware_main() {
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
`[4]Option<fn(u32)>`.

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
it: the LED blinks once a second. `dodo build` writes
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
| `board.I2C_SDA_PIN`, `board.I2C_SCL_PIN` | The board's usual I2C pins. |
| `board.SPI_CLOCK_PIN`, `board.SPI_TX_PIN`, `board.SPI_RX_PIN`, `board.SPI_SELECT_PIN` | The board's usual SPI pins, and a free pin for one chip select. |
| `board.UART_TX_PIN`, `board.UART_RX_PIN` | The board's usual serial port pins. |
| `b.timer` | The delay protocol, plus `delay_ms(ms)` and `now_us() -> u64`. |
| `board.reboot_to_bootloader()` | Restart into the board's flashing mode, for example to run `dodo run` again without pressing a button. |
| `board.report_panic(check, file, line, column)` | Unsafe. Write a failed check to the serial ports, as the [default panic hook](#panics-on-the-serial-port) does; for panic hooks of your own. |

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
| Vector table | Initial stack and reset handler. Every exception and interrupt is a weak default that an `extern "C"` function of the same name replaces. |
| Reset handler | Copies initialized statics to RAM, zeroes the rest, and calls `main`. If `main` returns, the core sleeps. |
| Panic hook | `dodo_board_panic` masks interrupts, writes the failure to the program's [serial ports](#panics-on-the-serial-port), if it claimed any, and halts. Define `extern "C" fn dodo_board_panic(check: *const u8, file: *const u8, line: u32, column: u32)` to report failures differently; it must not return. |
| Runtime helpers | `memcpy`, `memmove`, `memset`, `memcmp`, the 32- and 64-bit division, 64-bit multiplication, and shift routines the Cortex-M0+ lacks in hardware, and software `f32` and `f64` arithmetic, comparisons, conversions, and `%`, since it has no floating-point unit. Results are correctly rounded IEEE 754, to nearest with ties to even, subnormals included. |
| Linker script | Flash at `0x10000000` (size from the board), 264 KiB of RAM, and the stack at the top of RAM. |

`std/embedded/chip/rp2040` is the chip support. `rp2040.take(config)` starts the
crystal, runs the CPU at 125 MHz from the system PLL, takes GPIO and the timer
out of reset, and returns the peripherals once. Claiming an
[I2C or SPI bus](#i2c-and-spi-on-the-rp2040-and-rp2350) takes its controller
out of reset. `rp2040.Config` holds what the
circuit board decides, the crystal frequency; a crystal the clock tree cannot
use is `InvalidInput`. Pins, the timer, and `reboot_to_bootloader(led)` are
the types and functions the Pico board package exposes. Interrupt handlers use
the datasheet names: `SysTick_Handler`, `HardFault_Handler`, `TIMER_IRQ_0` to
`TIMER_IRQ_3`, `UART0_IRQ`, `IO_IRQ_BANK0`, and so on.

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
| Linker script | Flash at `0x10000000` (size from the board), 520 KiB of RAM, and the stack at the top of RAM. |

`std/embedded/chip/rp2350` mirrors the RP2040 package: `rp2350.take(config)`
starts the crystal, runs the CPU at 150 MHz from the system PLL, starts the
1 MHz ticks for TIMER0 and the watchdog, and hands out pins and the timer
once. Pins are released from pad isolation when claimed, as the RP2350
requires. `reboot_to_bootloader(led)` uses the boot ROM's reboot function;
the activity LED is skipped on chip revision A2, whose boot ROM mishandles it.
Interrupt handlers use the datasheet names, for example `TIMER0_IRQ_0` and
`IO_IRQ_BANK0`. GPIO 0 to 29 are supported, all of the RP2350A; the RP2350B's
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
be configured. A panic hook of your own replaces all of this. To keep the
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
  `extern "C"` handlers, and calls your entry function.
- The compiler may call `memcpy`, `memmove`, and `memset`; freestanding links
  must provide them, as described in [core utilities](core.md).
- Portable standard-library packages (bytes, text, JSON, collections, math,
  formatting, `std/io`) work in firmware too. Hosted packages such as
  `std/console` and `std/fs` do not.

## Current limits

- The boards are `pico`, `pico2`, and `pico2_w`, on the `rp2040` and `rp2350`
  chips. The chip packages cover clocks, GPIO, I2C, SPI, UART, and the timer;
  PWM, ADC, and USB are not bundled yet. Write them with `hal.Reg` as above.
- I2C, SPI, and UART transfers are polled, without interrupts or DMA, so a
  serial port keeps no more input than its 32-byte FIFO unless the other end
  honours RTS. An I2C master is the bus's only master, without high-speed
  mode, which the chips' controllers lack, and cannot send empty transfers.
  The Pico 2 W's wireless chip drives only its LED and senses USB power;
  Wi-Fi and Bluetooth need its firmware, which is not loaded yet.
- The software floating-point helpers are written for correctness, not speed:
  division, for example, computes one quotient bit per step, and `f32`
  arithmetic on the Cortex-M0+ goes through `f64`. They raise no
  floating-point exception flags and return the default quiet NaN for every
  NaN result.
- Only core 0 runs; core 1 stays in the boot ROM. The RP2350's RISC-V cores
  are not supported.
- `std/sync/atomic` currently supports x86-64 and AArch64 only. On
  microcontrollers, share state with interrupt handlers through a critical
  section.
- There is no inline assembly, `@interrupt` attribute, or linker-section
  attribute. Handlers use the C ABI, which is what Cortex-M vector tables
  expect; RISC-V handlers need a small assembly trampoline in the startup code.
- ADC, PWM, timers, and DMA have no shared protocol yet. Expose them as methods
  on your chip's types.

For raw register and pointer contracts, see [memory and foreign calls](memory-and-ffi.md).
