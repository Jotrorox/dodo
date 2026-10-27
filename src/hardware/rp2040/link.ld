/* Raspberry Pi RP2040 memory layout for Dodo firmware. The compiler prepends
   the platform's MEMORY block (the board's flash size) before this script. */

ENTRY(Reset_Handler)

SECTIONS
{
    /* The boot ROM loads the first 256 bytes of flash as stage 2. */
    .boot2 ORIGIN(FLASH) :
    {
        KEEP(*(.boot2))
    } > FLASH
    ASSERT(SIZEOF(.boot2) == 256, "RP2040 boot stage 2 must be exactly 256 bytes")

    /* Stage 2 jumps through the vector table at flash + 0x100. */
    .vector_table ORIGIN(FLASH) + 0x100 :
    {
        KEEP(*(.vector_table))
    } > FLASH

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

    /* The full-descending stack starts at the top of the 264 KiB of SRAM,
       striped banks and both scratch banks, as in the Pico SDK. */
    __stack_top = ORIGIN(RAM) + LENGTH(RAM);
    ASSERT(__noinit_end <= __stack_top - 4K, "RP2040 RAM: less than 4 KiB left for the stack")

    /DISCARD/ : { *(.note .note.*) }
}
