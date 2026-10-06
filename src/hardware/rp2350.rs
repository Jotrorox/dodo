//! Raspberry Pi RP2350, Arm cores: dual Cortex-M33 with FPU, 520 KiB SRAM,
//! external QSPI flash executed in place, and a boot ROM with a UF2 USB
//! bootloader. The matching peripheral support is `std/embedded/chip/rp2350`.
//!
//! The RP2350 can also boot its Hazard3 RISC-V cores; Dodo builds for the Arm
//! cores, the default.
use super::{Chip, ImageFormat, Platform, Region, image::Segment};

pub static CHIP: Chip = Chip {
    name: "rp2350",
    description: "Raspberry Pi RP2350",
    triple: "thumbv8m.main-none-eabi",
    cpu: "cortex-m33",
    flash_origin: 0x1000_0000,
    default_flash_size: 4 * 1024 * 1024,
    ram: Region {
        origin: 0x2000_0000,
        length: 520 * 1024,
    },
    runtime: concat!(
        include_str!("rp2350/runtime.ll"),
        include_str!("arm_eabi.ll")
    ),
    linker_script: include_str!("rp2350/link.ld"),
    panic_hook: "dodo_board_panic",
    linker: "ld.lld",
    package: "std/embedded/chip/rp2350",
    image: ImageFormat::Uf2 {
        // rp2350-arm-s: a secure Arm executable.
        family: 0xE48B_FF59,
        drive_id: "RP2350",
    },
    fixup: image_def_check,
};

/// The RP2350 boot ROM needs no checksum, but it only boots an image whose
/// vector table starts flash and whose IMAGE_DEF block lies in its first
/// 4 KiB. The linker script places both; this catches a custom script that
/// does not.
fn image_def_check(
    platform: &Platform,
    elf: &mut [u8],
    segments: &[Segment],
) -> Result<(), String> {
    const START: [u8; 4] = 0xFFFF_DED3u32.to_le_bytes();
    let origin = platform.flash().origin;
    let first = segments
        .iter()
        .find(|s| s.address == origin)
        .ok_or("firmware has no vector table at the start of flash")?;
    let head = &elf[first.offset..first.offset + first.size.min(4096)];
    if head.as_chunks::<4>().0.contains(&START) {
        Ok(())
    } else {
        Err("firmware has no RP2350 IMAGE_DEF block in the first 4 KiB of flash".into())
    }
}
