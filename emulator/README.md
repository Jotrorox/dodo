# Firmware emulator runner

`dodo-emulator` runs Dodo firmware for the Raspberry Pi RP2040 (`--board pico`)
and RP2350 (`--board pico2`) on emulated chips, so CI can execute the startup
runtime, interrupts, the executor, and the second core without a board. It
wraps the [picoem](https://github.com/0x4D44/picoem) emulator crates
`rp2040-emu` and `rp2350-emu` (MIT or Apache-2.0), with the real Raspberry Pi
boot ROMs mapped.

`scripts/test_emulator.py` is the way to run it: it builds the self-test in
`tests/emulator` for each board at -O0 and -O2, downloads and verifies the
boot ROMs and the emulator crates, applies `patches/`, builds this runner when
its sources change, and runs each firmware image:

```sh
cargo build                          # the compiler, target/debug/dodo
python3 scripts/test_emulator.py     # or: --board pico2 -O 3
```

It needs `ld.lld` and `patch`, and `cargo` to build the runner.
`emulator/target` keeps the downloads, the patched crates, the runner, and
the firmware images. A plain `cargo build` here works only after the script
has unpacked the patched crates.

## What a run reports

The firmware boots from its vector table in flash as the boot ROM would start
it; the emulators do not model the flash interface that a full boot through
the ROM needs. The firmware records its progress in a static whose symbol ends
in `.EMULATOR_RESULT` (see `tests/emulator/selftest.dodo`), and the run ends
when that record says it finished or panicked, when a core lands in the
runtime's `DefaultHandler` (an unhandled fault or interrupt), or after a
cycle budget. Failures name the source line of the check, or the faulting
function and its caller, or where each core was when it hung.

## What the emulators do not model

The self-test only checks what both emulators model; the hardware self-tests
in `tests/hardware` (`scripts/test_pico.py`) cover the rest on real boards:

- Nothing outside the chip: no I2C or SPI devices, USB host, or wireless chip.
  `pico2_w` is not emulated.
- GPIO interrupts, and the RP2040's UART loopback.
- Erasing and writing the flash through the boot ROM, so the lockout that
  parks the other core is checked without a flash write.
- Timing against the CPU: the RP2350 emulator counts the timer in `clk_sys`
  cycles rather than `clk_ref`, so its microseconds pass about 12 times too
  quickly for the code running between them.
- The RP2350's double-precision coprocessor (DCP): its instructions compute
  wrong results, so `f64` arithmetic is checked on the RP2040 only.

## Patches

`patches/` fixes bugs and gaps in the pinned crate versions that the self-test
ran into, each checked against the architecture or datasheet. They are
candidates for upstream.

`rp2040-emu-0.1.5.patch`:

- The PSM resets core 1 (`FRCE_OFF.PROC1`), and core 1's boot ROM sends a 0
  through the FIFO when it leaves reset, as multicore launch code expects.
- `SIO_IRQ_PROC0` and `SIO_IRQ_PROC1` follow each core's FIFO (VLD, WOF, ROE).
- `ICSR.VECTACTIVE` reads the running exception's number.
- `IO_IRQ_QSPI` is raised on the core whose IO_QSPI `PROCn_INTF` forces it,
  as rp_multicore's lockout does.

`rp2350-emu-0.2.6.patch`:

- `CPSID`/`CPSIE` decoded the I and F bits swapped, so `cpsid i` set
  FAULTMASK and a critical section's `msr primask` restore never unmasked
  interrupts again.
- The ARMv8-M load-acquire and store-release instructions (`LDA`, `STL`, and
  their byte, halfword, and exclusive forms) fell through to `LDRD` and loaded
  the program counter.
- A 16-bit instruction in an IT block that returned from an exception (`bx lr`)
  had the handler's flags and IT state written over the restored ones.
- `WFI` woke only for interrupts that peripherals raised, not ones pended
  through `NVIC_ISPR`.
- The PSM resets core 1, which then restarts in the boot ROM.
- `SIO_IRQ_FIFO` follows each core's FIFO (VLD, WOF, ROE).
- `ICSR.VECTACTIVE` reads the running exception's number.
- The secure `IO_IRQ_QSPI` is raised on the core whose IO_QSPI `PROCn_INTF`
  forces it, as rp_multicore's lockout does.

The runner also seeds the redundancy coprocessor salt that core 0's boot path
would, since core 1 waits in the boot ROM until it has one.

To change a patch, edit the unpacked crate in `target/patched/`, diff it
against a fresh copy of the `.crate` file in `target/cache/`, and save the
diff as the patch; the script re-applies it when the patch changes.
