//! Raspberry Pi RP2040: dual Cortex-M0+, 264 KiB SRAM, external QSPI flash
//! executed in place, and a boot ROM with a UF2 USB bootloader. The matching
//! peripheral support is `std/embedded/chip/rp2040`.
use std::borrow::Cow;

use super::{Chip, ImageFormat, InterruptAbi, Platform, Region, image::Segment, serial::UsbId};

pub static CHIP: Chip = Chip {
    name: "rp2040",
    description: "Raspberry Pi RP2040",
    triple: "thumbv6m-none-eabi",
    cpu: "cortex-m0plus",
    features: "",
    flash_origin: 0x1000_0000,
    default_flash_size: 2 * 1024 * 1024,
    ram: Region {
        origin: 0x2000_0000,
        length: 264 * 1024,
    },
    runtime: || {
        Cow::Borrowed(concat!(
            include_str!("rp2040/runtime.ll"),
            include_str!("arm_eabi.ll"),
            include_str!("arm_float.ll"),
            include_str!("rp2040/float.ll")
        ))
    },
    linker_script: include_str!("rp2040/link.ld"),
    panic_hook: "dodo_board_panic",
    interrupts: &[
        "NMI_Handler",
        "HardFault_Handler",
        "SVC_Handler",
        "PendSV_Handler",
        "SysTick_Handler",
        // IRQ 0 to 25.
        "TIMER_IRQ_0",
        "TIMER_IRQ_1",
        "TIMER_IRQ_2",
        "TIMER_IRQ_3",
        "PWM_IRQ_WRAP",
        "USBCTRL_IRQ",
        "XIP_IRQ",
        "PIO0_IRQ_0",
        "PIO0_IRQ_1",
        "PIO1_IRQ_0",
        "PIO1_IRQ_1",
        "DMA_IRQ_0",
        "DMA_IRQ_1",
        "IO_IRQ_BANK0",
        "IO_IRQ_QSPI",
        "SIO_IRQ_PROC0",
        "SIO_IRQ_PROC1",
        "CLOCKS_IRQ",
        "SPI0_IRQ",
        "SPI1_IRQ",
        "UART0_IRQ",
        "UART1_IRQ",
        "ADC_IRQ_FIFO",
        "I2C0_IRQ",
        "I2C1_IRQ",
        "RTC_IRQ",
    ],
    interrupt_abi: InterruptAbi::Vector,
    opt_level: 0,
    linker: "ld.lld",
    package: "std/embedded/chip/rp2040",
    image: ImageFormat::Uf2 {
        family: 0xE48B_FF56,
        drive_id: "RPI-RP2",
    },
    pio_version: Some(0),
    // std/embedded/chip/rp_usb's DEFAULT_VENDOR_ID and DEFAULT_PRODUCT_ID.
    usb_serial: Some(UsbId {
        vendor: 0x2E8A,
        product: 0x000A,
    }),
    fixup: boot2_checksum,
};

/// The boot ROM copies the first 256 bytes of flash (boot stage 2) to SRAM and
/// runs them only if their last word is the CRC of the first 252 bytes.
fn boot2_checksum(platform: &Platform, elf: &mut [u8], segments: &[Segment]) -> Result<(), String> {
    let boot2 = segments
        .iter()
        .find(|s| s.address == platform.flash().origin && s.size >= 256)
        .ok_or("firmware has no 256-byte boot stage at the start of flash")?;
    let bytes = &mut elf[boot2.offset..boot2.offset + 256];
    let crc = crc32_mpeg2(&bytes[..252]);
    bytes[252..].copy_from_slice(&crc.to_le_bytes());
    Ok(())
}

/// CRC-32 with polynomial 0x04C11DB7, initial value 0xFFFFFFFF, no bit
/// reflection and no final XOR, as checked by the boot ROM.
pub fn crc32_mpeg2(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= (byte as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc_matches_the_boot_rom_algorithm() {
        // CRC-32/MPEG-2 check value.
        assert_eq!(super::crc32_mpeg2(b"123456789"), 0x0376_E6E7);
    }
}
