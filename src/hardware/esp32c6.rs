//! Espressif ESP32-C6: one 160 MHz RISC-V core (RV32IMAC), 512 KiB of SRAM,
//! flash executed in place through a cache, and a boot ROM that flashes
//! over a serial port, including the chip's own USB serial port. The
//! matching peripheral support is `std/embedded/chip/esp32c6`.
//!
//! The chip's low-power RISC-V core and its radios (Wi-Fi 6, Bluetooth LE,
//! IEEE 802.15.4) are not used.
use super::{
    Chip, ImageFormat, InterruptAbi, Platform, Region, esp_flash::DownloadFlag, image::Segment,
    serial::UsbId,
};

pub static CHIP: Chip = Chip {
    name: "esp32c6",
    description: "Espressif ESP32-C6",
    triple: "riscv32-unknown-none-elf",
    cpu: "generic-rv32",
    features: "+m,+a,+c",
    // The boot ROM's direct boot maps flash offset 0 here.
    flash_origin: 0x4200_0000,
    default_flash_size: 4 * 1024 * 1024,
    // The high-performance SRAM below the boot ROM's own data.
    ram: Region {
        origin: 0x4080_0000,
        length: 0x7_E610,
    },
    runtime: concat!(
        include_str!("esp32c6/runtime.ll"),
        include_str!("arm_eabi.ll"),
        include_str!("arm_float.ll"),
        include_str!("arm_float_soft.ll")
    ),
    linker_script: include_str!("esp32c6/link.ld"),
    panic_hook: "dodo_board_panic",
    interrupts: &[
        "Exception_Handler",
        // Interrupt sources 0 to 76.
        "WIFI_MAC",
        "WIFI_MAC_NMI",
        "WIFI_PWR",
        "WIFI_BB",
        "BT_MAC",
        "BT_BB",
        "BT_BB_NMI",
        "LP_TIMER",
        "COEX",
        "BLE_TIMER",
        "BLE_SEC",
        "I2C_MASTER",
        "ZB_MAC",
        "PMU",
        "EFUSE",
        "LP_RTC_TIMER",
        "LP_UART",
        "LP_I2C",
        "LP_WDT",
        "LP_PERI_TIMEOUT",
        "LP_APM_M0",
        "LP_APM_M1",
        "FROM_CPU_INTR0",
        "FROM_CPU_INTR1",
        "FROM_CPU_INTR2",
        "FROM_CPU_INTR3",
        "ASSIST_DEBUG",
        "TRACE",
        "CACHE",
        "CPU_PERI_TIMEOUT",
        "GPIO",
        "GPIO_NMI",
        "PAU",
        "HP_PERI_TIMEOUT",
        "MODEM_PERI_TIMEOUT",
        "HP_APM_M0",
        "HP_APM_M1",
        "HP_APM_M2",
        "HP_APM_M3",
        "LP_APM0",
        "MSPI",
        "I2S0",
        "UHCI0",
        "UART0",
        "UART1",
        "LEDC",
        "TWAI0",
        "TWAI1",
        "USB_DEVICE",
        "RMT",
        "I2C_EXT0",
        "TG0_T0_LEVEL",
        "TG0_T1_LEVEL",
        "TG0_WDT_LEVEL",
        "TG1_T0_LEVEL",
        "TG1_T1_LEVEL",
        "TG1_WDT_LEVEL",
        "SYSTIMER_TARGET0",
        "SYSTIMER_TARGET1",
        "SYSTIMER_TARGET2",
        "APB_SARADC",
        "MCPWM0",
        "PCNT",
        "PARL_IO",
        "SLC0",
        "SLC1",
        "DMA_IN_CH0",
        "DMA_IN_CH1",
        "DMA_IN_CH2",
        "DMA_OUT_CH0",
        "DMA_OUT_CH1",
        "DMA_OUT_CH2",
        "SPI2",
        "AES",
        "SHA",
        "RSA",
        "ECC",
    ],
    interrupt_abi: InterruptAbi::Dispatched,
    linker: "ld.lld",
    package: "std/embedded/chip/esp32c6",
    image: ImageFormat::EspFlash {
        chip_id: 13,
        usb: UsbId {
            vendor: 0x303A,
            product: 0x1001,
        },
        // LP_AON_SYS_CFG.FORCE_DOWNLOAD_BOOT, which reboot_to_bootloader in
        // std/embedded/chip/esp32c6 sets.
        download_flag: DownloadFlag {
            address: 0x600B_1034,
            mask: 1 << 30,
            // The RTC watchdog (LP_WDT) off, and the super watchdog fed by
            // itself, each unlocked with its write key.
            watchdogs_off: &[
                (0x600B_1C18, 0x50D8_3AA1, u32::MAX),
                (0x600B_1C00, 0, u32::MAX),
                (0x600B_1C18, 0, u32::MAX),
                (0x600B_1C20, 0x50D8_3AA1, u32::MAX),
                (0x600B_1C1C, 1 << 18, 1 << 18),
                (0x600B_1C20, 0, u32::MAX),
            ],
        },
    },
    pio_version: None,
    usb_serial: None,
    fixup: direct_boot_check,
};

/// The direct boot marker the boot ROM looks for at the start of flash.
pub const DIRECT_BOOT_MARKER: u32 = 0xAEDB_041D;

/// The boot ROM only direct-boots an image whose first two words are the
/// marker; otherwise it looks for a second-stage bootloader. The linker
/// script places the marker; this catches a custom script that does not.
fn direct_boot_check(
    platform: &Platform,
    elf: &mut [u8],
    segments: &[Segment],
) -> Result<(), String> {
    let origin = platform.flash().origin;
    let first = segments
        .iter()
        .find(|s| s.address == origin && s.size >= 8)
        .ok_or("firmware has no direct boot marker at the start of flash")?;
    let marker = DIRECT_BOOT_MARKER.to_le_bytes();
    if elf[first.offset..first.offset + 4] == marker
        && elf[first.offset + 4..first.offset + 8] == marker
    {
        Ok(())
    } else {
        Err("firmware does not start with the ESP32-C6 direct boot marker".into())
    }
}
