/* Microchip ATmega328P memory layout for Dodo firmware. The compiler prepends
   the platform's MEMORY block: FLASH is program memory from 0, without the
   bootloader's section at its top, and RAM is the 2 KiB of SRAM, which AVR
   ELF files place at 0x800000 + its data-space address 0x100. */

ENTRY(__vectors)

SECTIONS
{
    .text :
    {
        KEEP(*(.vectors))
        /* Constants in program memory (address space 1), such as panic
           messages, read with LPM. */
        *(.progmem .progmem.*)
        . = ALIGN(2);
        *(.text .text.*)
        . = ALIGN(2);
    } > FLASH

    /* AVR loads read data memory only, so constants live in SRAM as well:
       .rodata and initialized statics are copied from flash at reset. */
    .data :
    {
        __data_start = .;
        *(.rodata .rodata.*)
        *(.data .data.*)
        __data_end = .;
    } > RAM AT > FLASH
    __data_load = LOADADDR(.data);

    .bss (NOLOAD) :
    {
        __bss_start = .;
        *(.bss .bss.* COMMON)
        __bss_end = .;
    } > RAM

    /* Statics placed with @section(".noinit"): neither loaded nor zeroed, so
       they keep their contents across a reset. */
    .noinit (NOLOAD) :
    {
        *(.noinit .noinit.*)
        __noinit_end = .;
    } > RAM

    /* The stack grows down from the last byte of SRAM. */
    __stack = ORIGIN(RAM) + LENGTH(RAM) - 1;
    ASSERT(__noinit_end <= __stack + 1 - 256, "ATmega328P RAM: statics and constants leave less than 256 bytes for the stack")

    /DISCARD/ : { *(.note .note.*) }
}
