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

    .data : ALIGN(4)
    {
        __data_start = .;
        *(.data .data.*)
        . = ALIGN(4);
        __data_end = .;
    } > RAM AT > FLASH
    __data_load = LOADADDR(.data);

    .bss (NOLOAD) : ALIGN(4)
    {
        __bss_start = .;
        *(.bss .bss.* COMMON)
        . = ALIGN(4);
        __bss_end = .;
    } > RAM

    /* The full-descending stack starts at the top of the 520 KiB of SRAM,
       striped banks and both scratch banks, as in the Pico SDK. */
    __stack_top = ORIGIN(RAM) + LENGTH(RAM);
    ASSERT(__bss_end <= __stack_top - 4K, "RP2350 RAM: less than 4 KiB left for the stack")

    /DISCARD/ : { *(.note .note.*) }
}
