//! Microchip ATtiny85: an 8-bit AVR core (`avr25`, without MUL) at up to
//! 20 MHz, 8 KiB of flash, 512 bytes of SRAM, and 512 bytes of EEPROM, as on
//! the Digispark. Boards flash it through the micronucleus USB bootloader,
//! which the compiler drives itself (`super::micronucleus`). The matching
//! peripheral support is `std/embedded/chip/attiny85`.
use super::{Chip, ImageFormat, InterruptAbi, Platform, Region, avr, image::Segment};

pub static CHIP: Chip = Chip {
    name: "attiny85",
    description: "Microchip ATtiny85",
    triple: "avr-none",
    cpu: "attiny85",
    features: "",
    flash_origin: 0,
    // The whole flash; boards subtract their bootloader's section.
    default_flash_size: 8 * 1024,
    // SRAM from data-space address 0x60, after the registers, in AVR ELF's
    // data region at 0x800000.
    ram: Region {
        origin: 0x0080_0060,
        length: 512,
    },
    runtime: || {
        avr::runtime(
            include_str!("attiny85/runtime.ll"),
            avr::Multiplier::Software,
        )
    },
    linker_script: include_str!("attiny85/link.ld"),
    panic_hook: "dodo_board_panic",
    interrupts: &[
        // Vectors 1 to 14, after reset.
        "INT0",
        "PCINT0",
        "TIMER1_COMPA",
        "TIMER1_OVF",
        "TIMER0_OVF",
        "EE_RDY",
        "ANA_COMP",
        "ADC",
        "TIMER1_COMPB",
        "TIMER0_COMPA",
        "TIMER0_COMPB",
        "WDT",
        "USI_START",
        "USI_OVF",
    ],
    opt_level: 2,
    interrupt_abi: InterruptAbi::Vector,
    linker: "ld.lld",
    package: "std/embedded/chip/attiny85",
    image: ImageFormat::Micronucleus {
        signature: [0x1E, 0x93, 0x0B],
        page_size: 64,
    },
    pio_version: None,
    usb_serial: None,
    fixup: relative_vectors_first,
};

/// The vector table's size: 15 one-word `rjmp`s.
const VECTOR_BYTES: usize = 30;

/// The reset vector must be the first word of flash, and like the PCINT0
/// vector (word 2), which micronucleus also relocates, an `rjmp`.
fn relative_vectors_first(
    platform: &Platform,
    elf: &mut [u8],
    segments: &[Segment],
) -> Result<(), String> {
    let first = segments
        .first()
        .filter(|segment| {
            segment.address == platform.flash().origin && segment.size >= VECTOR_BYTES
        })
        .ok_or("firmware has no interrupt vector table at the start of flash")?;
    for (word, name) in [(0, "reset"), (2, "PCINT0")] {
        let at = first.offset + word * 2;
        // `rjmp` is 0xC000 | a 12-bit word offset.
        if u16::from_le_bytes([elf[at], elf[at + 1]]) & 0xF000 != 0xC000 {
            return Err(format!(
                "firmware's {name} vector is not an rjmp, which the bootloader needs"
            ));
        }
    }
    Ok(())
}
