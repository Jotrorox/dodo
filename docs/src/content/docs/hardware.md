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
| Typed registers | `std/embedded/hal` | `hal.Reg<T>` handles and mask constants for memory-mapped registers. |
| Interrupts | `std/embedded/hal` | `hal.Critical`, a section that masks interrupts until it is dropped. |
| Desktop testing | `std/embedded/hal/fake` | In-memory pins, delays, and buses that record what a driver did. |
| Boards | `std/embedded/board` | The selected board: one `take()` and the same LED, pin, and timer fields on every board. |
| Chip support | `std/embedded/chip` | The selected microcontroller's peripherals, for custom boards. |
| Wireless chip | `std/embedded/wireless/cyw43` | The CYW43439 on the Pico 2 W: power-up, its gSPI bus, and its GPIO pins. |
| Raw access | `core/mmio`, `core/cpu` | Volatile loads/stores, barriers, interrupt masking, `wfi`. |

Four kinds of code meet here:

- **Chip support** (`std/embedded/chip/NAME`) knows datasheet addresses. It turns
  registers into values that implement the protocols, for example a `Pin`
  with `set_high`.
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

## Testing with fakes

`std/embedded/hal/fake` implements every protocol in memory:

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
| `b.timer` | The delay protocol, plus `delay_ms(ms)` and `now_us() -> u64`. |
| `board.reboot_to_bootloader()` | Restart into the board's flashing mode, for example to run `dodo run` again without pressing a button. |

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
| Panic hook | `dodo_board_panic` masks interrupts and halts. Define `extern "C" fn dodo_board_panic(check: *const u8, file: *const u8, line: u32, column: u32)` to report failures differently; it must not return. |
| Runtime helpers | `memcpy`, `memmove`, `memset`, `memcmp`, the 32- and 64-bit division, 64-bit multiplication, and shift routines the Cortex-M0+ lacks in hardware, and software `f32` and `f64` arithmetic, comparisons, conversions, and `%`, since it has no floating-point unit. Results are correctly rounded IEEE 754, to nearest with ties to even, subnormals included. |
| Linker script | Flash at `0x10000000` (size from the board), 264 KiB of RAM, and the stack at the top of RAM. |

`std/embedded/chip/rp2040` is the chip support. `rp2040.take(config)` starts the
crystal, runs the CPU at 125 MHz from the system PLL, takes GPIO and the timer
out of reset, and returns the peripherals once. `rp2040.Config` holds what the
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
  chips. The chip packages cover clocks, GPIO, and the timer; UART, I2C, SPI,
  PWM, and USB are not bundled yet. Write them with `hal.Reg` as above. The
  Pico 2 W's wireless chip drives only its LED and senses USB power; Wi-Fi
  and Bluetooth need its firmware, which is not loaded yet.
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
