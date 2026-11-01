/* Raspberry Pi RP2350 memory layout for Dodo firmware. The compiler prepends
   the platform's MEMORY block (the board's flash size) before this script. */

ENTRY(Reset_Handler)

SECTIONS
{
    /* The boot ROM enters Arm images through the vector table at the start
       of flash: initial stack pointer, then reset handler. */
    .vector_table ORIGIN(FLASH) :
    {
        KEEP(*(.vector_table))
    } > FLASH

    /* The boot ROM only boots an image whose IMAGE_DEF block lies in the
       first 4 KiB of flash. */
    .picobin_block : ALIGN(4)
    {
        KEEP(*(.picobin_block))
    } > FLASH
    ASSERT(SIZEOF(.picobin_block) == 20, "RP2350 image needs its IMAGE_DEF block")
    ASSERT(ADDR(.picobin_block) + SIZEOF(.picobin_block) <= ORIGIN(FLASH) + 4K,
           "RP2350 IMAGE_DEF block must be in the first 4 KiB of flash")

    .text :
    {
        *(.text .text.*)
    } > FLASH

    .rodata : ALIGN(4)
    {
        *(.rodata .rodata.*)
        *(.got .got.*)
        . = ALIGN(4);
    } > FLASH

    .ARM.extab : { *(.ARM.extab* .gnu.linkonce.armextab.*) } > FLASH
    .ARM.exidx : { *(.ARM.exidx* .gnu.linkonce.armexidx.*) } > FLASH

    /* Initialized statics, and functions placed with @section(".ramfunc")
       to run from SRAM, copied from flash at reset. */
    .data : ALIGN(4)
    {
        __data_start = .;
        *(.ramfunc .ramfunc.*)
        *(.data .data.*)
        . = ALIGN(4);
        __data_end = .;
    } > RAM AT > FLASH
    __data_load = LOADADDR(.data);

    /* Where the program image ends in flash, and where the board's flash
       ends, for std/embedded/chip/rp_flash: storage goes in between. */
    __flash_image_end = LOADADDR(.data) + SIZEOF(.data);
    __flash_end = ORIGIN(FLASH) + LENGTH(FLASH);

    .bss (NOLOAD) : ALIGN(4)
    {
        __bss_start = .;
        *(.bss .bss.* COMMON)
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

    /* The full-descending stack starts at the top of the 520 KiB of SRAM,
       striped banks and both scratch banks, as in the Pico SDK. */
    __stack_top = ORIGIN(RAM) + LENGTH(RAM);
    ASSERT(__noinit_end <= __stack_top - 4K, "RP2350 RAM: less than 4 KiB left for the stack")

    /DISCARD/ : { *(.note .note.*) }
}
