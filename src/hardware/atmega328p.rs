//! Microchip ATmega328P: an 8-bit AVR core at up to 20 MHz, 32 KiB of flash,
//! 2 KiB of SRAM, and 1 KiB of EEPROM, as on the Arduino Uno. Boards flash it
//! through a serial bootloader that speaks STK500 version 1, such as
//! Optiboot, which the compiler drives itself (`super::stk500`). The matching
//! peripheral support is `std/embedded/chip/atmega328p`.
use super::{Chip, ImageFormat, InterruptAbi, Platform, Region, avr, image::Segment};

pub static CHIP: Chip = Chip {
    name: "atmega328p",
    description: "Microchip ATmega328P",
    triple: "avr-none",
    cpu: "atmega328p",
    features: "",
    flash_origin: 0,
    // The whole flash; boards subtract their bootloader's section.
    default_flash_size: 32 * 1024,
    // SRAM from data-space address 0x100, after the registers, in AVR ELF's
    // data region at 0x800000.
    ram: Region {
        origin: 0x0080_0100,
        length: 2 * 1024,
    },
    runtime: || avr::runtime(include_str!("atmega328p/runtime.ll")),
    linker_script: include_str!("atmega328p/link.ld"),
    panic_hook: "dodo_board_panic",
    interrupts: &[
        // Vectors 1 to 25, after reset.
        "INT0",
        "INT1",
        "PCINT0",
        "PCINT1",
        "PCINT2",
        "WDT",
        "TIMER2_COMPA",
        "TIMER2_COMPB",
        "TIMER2_OVF",
        "TIMER1_CAPT",
        "TIMER1_COMPA",
        "TIMER1_COMPB",
        "TIMER1_OVF",
        "TIMER0_COMPA",
        "TIMER0_COMPB",
        "TIMER0_OVF",
        "SPI_STC",
        "USART_RX",
        "USART_UDRE",
        "USART_TX",
        "ADC",
        "EE_READY",
        "ANALOG_COMP",
        "TWI",
        "SPM_READY",
    ],
    opt_level: 2,
    interrupt_abi: InterruptAbi::Vector,
    linker: "ld.lld",
    package: "std/embedded/chip/atmega328p",
    image: ImageFormat::Stk500 {
        signature: [0x1E, 0x95, 0x0F],
        page_size: 128,
        baud: 115_200,
    },
    pio_version: None,
    usb_serial: None,
    fixup: vectors_first,
};

/// The reset vector must be the first word of flash.
fn vectors_first(platform: &Platform, elf: &mut [u8], segments: &[Segment]) -> Result<(), String> {
    let first = segments
        .first()
        .filter(|segment| segment.address == platform.flash().origin && segment.size >= 104)
        .ok_or("firmware has no interrupt vector table at the start of flash")?;
    // `jmp` is 0x940C | address bits; little-endian, its first byte is 0x0C.
    let word = u16::from_le_bytes([elf[first.offset], elf[first.offset + 1]]);
    if word & 0xFE0E != 0x940C {
        return Err("firmware does not start with the reset vector's jmp".into());
    }
    Ok(())
}
