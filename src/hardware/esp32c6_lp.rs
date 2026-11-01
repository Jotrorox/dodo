//! The ESP32-C6's low-power (LP) core: a second, slower RISC-V core
//! (RV32IMAC, clocked from the 17.5 MHz fast RC oscillator) in the chip's
//! always-on LP system, which keeps running while the main core hibernates
//! and can wake it. Its program lives in the first 12 KiB of the 16 KiB LP
//! SRAM at `0x50000000`. It is built for `chip = "esp32c6_lp"`, which
//! produces the raw image for that SRAM, and an ESP32-C6 firmware embeds it
//! with `lp-core = "PATH"`, so `esp32c6.LpCore` can load and start it. The
//! matching peripheral support is `std/embedded/chip/esp32c6_lp`.
use std::borrow::Cow;

use super::{Chip, ImageFormat, InterruptAbi, Platform, Region, image::Segment};

/// Where the LP core's program goes in LP SRAM, and how much of it.
pub const ORIGIN: u32 = 0x5000_0000;
pub const LENGTH: u32 = 0x3000;

pub static CHIP: Chip = Chip {
    name: "esp32c6_lp",
    description: "Espressif ESP32-C6 low-power core",
    triple: "riscv32-unknown-none-elf",
    cpu: "generic-rv32",
    features: "+m,+a,+c",
    // The whole program, code and data, lives in LP SRAM; the image is that
    // memory's contents.
    flash_origin: ORIGIN,
    default_flash_size: LENGTH,
    ram: Region {
        origin: ORIGIN,
        length: LENGTH,
    },
    runtime: || {
        Cow::Borrowed(concat!(
            include_str!("esp32c6_lp/runtime.ll"),
            include_str!("arm_eabi.ll"),
            include_str!("arm_float.ll"),
            include_str!("arm_float_soft.ll")
        ))
    },
    linker_script: include_str!("esp32c6_lp/link.ld"),
    panic_hook: "dodo_lp_panic",
    interrupts: &["Exception_Handler"],
    interrupt_abi: InterruptAbi::Dispatched,
    // 12 KiB holds few unoptimized programs.
    opt_level: 2,
    linker: "ld.lld",
    package: "std/embedded/chip/esp32c6_lp",
    image: ImageFormat::Coprocessor { host: "esp32c6" },
    pio_version: None,
    usb_serial: None,
    fixup: vector_check,
};

/// The LP core starts at offset 0x80 of LP SRAM, after its trap table.
fn vector_check(_: &Platform, elf: &mut [u8], segments: &[Segment]) -> Result<(), String> {
    let first = segments.first().ok_or("the LP core program is empty")?;
    if first.address != ORIGIN || first.size < 0x84 {
        return Err("the LP core program does not start with its trap table at 0x50000000".into());
    }
    let _ = elf;
    Ok(())
}
