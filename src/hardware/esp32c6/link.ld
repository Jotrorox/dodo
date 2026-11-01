/* Espressif ESP32-C6 memory layout for Dodo firmware. The compiler prepends
   the platform's MEMORY block (the board's flash size) before this script.

   FLASH is the flash chip as the boot ROM's direct boot maps it: offset 0
   of the chip at 0x42000000, through the cache, for both instructions and
   constants. RAM is the high-performance SRAM up to where the boot ROM
   keeps its own data; the ROM functions the firmware calls still use that. */

ENTRY(Reset_Handler)

/* The part of LP SRAM for statics that survive hibernation; see .lp_noinit. */
MEMORY
{
    LP_NOINIT (rw) : ORIGIN = 0x50003400, LENGTH = 0xBE0
}

SECTIONS
{
    /* The boot ROM jumps to flash + 8 when the first two words of flash are
       the direct boot marker. */
    .boot ORIGIN(FLASH) :
    {
        KEEP(*(.boot_header))
        KEEP(*(.text.Reset_Handler))
    } > FLASH
    ASSERT(Reset_Handler == ORIGIN(FLASH) + 8, "ESP32-C6 reset handler must follow the direct boot marker")

    .text :
    {
        KEEP(*(.trap_vector))
        *(.text .text.*)
    } > FLASH

    .rodata : ALIGN(4)
    {
        *(.srodata .srodata.*)
        *(.rodata .rodata.*)
        /* Addresses of weak symbols, resolved at link time. */
        *(.got .got.*)
        . = ALIGN(4);
    } > FLASH

    /* The LP core's program, when the firmware embeds one (lp-core =
       "PATH"): the image esp32c6.LpCore copies to LP SRAM. Empty otherwise. */
    .lp_image : ALIGN(4)
    {
        __lp_image_start = .;
        KEEP(*(.lp_image))
        __lp_image_end = .;
    } > FLASH

    /* Initialized statics, and functions placed with @section(".ramfunc")
       to run from SRAM, copied from flash at reset. */
    .data : ALIGN(4)
    {
        __data_start = .;
        *(.ramfunc .ramfunc.*)
        *(.sdata .sdata.* .data .data.*)
        . = ALIGN(4);
        __data_end = .;
    } > RAM AT > FLASH
    __data_load = LOADADDR(.data);
    /* For std/embedded/chip/esp_flash: the flash above the image is free. */
    __flash_image_end = LOADADDR(.data) + SIZEOF(.data);
    __flash_end = ORIGIN(FLASH) + LENGTH(FLASH);

    .bss (NOLOAD) : ALIGN(4)
    {
        __bss_start = .;
        *(.sbss .sbss.* .bss .bss.* COMMON)
        . = ALIGN(4);
        __bss_end = .;
    } > RAM

    /* Statics placed with @section(".noinit"): neither loaded nor zeroed, so
       they keep their contents across a reset that leaves SRAM powered. */
    .noinit (NOLOAD) : ALIGN(4)
    {
        *(.noinit .noinit.*)
        . = ALIGN(4);
        __noinit_end = .;
    } > RAM

    /* LP SRAM, 16 KiB at 0x50000000, which keeps its contents through
       hibernation and every reset but a power cycle: the LP core's program
       in the first 12 KiB (copied there from .lp_image by esp32c6.LpCore),
       1 KiB the HP and LP programs share (std/embedded/chip/esp_lp_shared),
       statics placed with @section(".lp_noinit"), neither loaded nor
       zeroed, and esp_rtc's record in the last 32 bytes. */
    .lp_noinit (NOLOAD) : ALIGN(4)
    {
        *(.lp_noinit .lp_noinit.*)
        . = ALIGN(4);
    } > LP_NOINIT

    /* The full-descending stack starts at the top of RAM, 16-byte aligned
       as the RISC-V calling convention requires. */
    __stack_top = (ORIGIN(RAM) + LENGTH(RAM)) & ~15;
    ASSERT(__noinit_end <= __stack_top - 4K, "ESP32-C6 RAM: less than 4 KiB left for the stack")

    /DISCARD/ : { *(.note .note.*) *(.eh_frame .eh_frame_hdr) }
}

/* Boot ROM functions, at their entries in the ROM's jump table, which stay
   put across chip revisions. */
PROVIDE(ets_delay_us = 0x40000040);
PROVIDE(ets_update_cpu_frequency = 0x40000048);
PROVIDE(rtc_get_reset_reason = 0x40000018);
PROVIDE(software_reset = 0x40000090);
PROVIDE(software_reset_cpu = 0x40000094);
PROVIDE(esp_rom_spiflash_erase_sector = 0x40000144);
PROVIDE(esp_rom_spiflash_write = 0x4000014c);
PROVIDE(esp_rom_spiflash_read = 0x40000150);
PROVIDE(esp_rom_spiflash_unlock = 0x40000154);
PROVIDE(esp_rom_spiflash_config_clk = 0x40000178);
PROVIDE(esp_rom_spiflash_read_status = 0x40000180);
PROVIDE(esp_rom_spiflash_config_readmode = 0x4000017c);
PROVIDE(spi_common_set_dummy_output = 0x40000190);
PROVIDE(spi_dummy_len_fix = 0x400001d0);
PROVIDE(Cache_Suspend_ICache = 0x40000698);
PROVIDE(Cache_Resume_ICache = 0x4000069c);
PROVIDE(Cache_Invalidate_ICache_All = 0x4000064c);
PROVIDE(Cache_Invalidate_Addr = 0x40000648);
PROVIDE(rom_i2c_readReg = 0x400012bc);
PROVIDE(rom_i2c_readReg_Mask = 0x400012c8);
PROVIDE(rom_i2c_writeReg = 0x400012c4);
PROVIDE(rom_i2c_writeReg_Mask = 0x400012cc);

/* Boot ROM data: a pointer to the ROM's flash chip record. */
PROVIDE(rom_spiflash_legacy_data = 0x4087ffec);
