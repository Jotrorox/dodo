/* Memory layout of a program for the ESP32-C6's LP core. The compiler
   prepends the platform's MEMORY block, whose FLASH and RAM regions are
   both the 12 KiB of LP SRAM the program owns: the image is that memory's
   contents, which esp32c6.LpCore copies there from the main firmware.

   The LP core takes traps at 0x50000000 (mtvec, vectored) and starts at
   0x50000080. Initialized statics stay where the image put them, and the
   loader zeroes the rest of the region, so .bss is zero at the first start
   and statics keep their values from one wake of the core to the next. */

ENTRY(Reset_Handler)

SECTIONS
{
    .text ORIGIN(FLASH) :
    {
        KEEP(*(.lp_vectors))
        . = ORIGIN(FLASH) + 0x80;
        KEEP(*(.text.Reset_Handler))
        *(.text .text.*)
    } > FLASH
    ASSERT(Reset_Handler == ORIGIN(FLASH) + 0x80, "the LP core's reset handler must be at 0x50000080")

    .rodata : ALIGN(4)
    {
        *(.srodata .srodata.*)
        *(.rodata .rodata.*)
        *(.got .got.*)
        . = ALIGN(4);
    } > FLASH

    .data : ALIGN(4)
    {
        *(.ramfunc .ramfunc.*)
        *(.sdata .sdata.* .data .data.*)
        . = ALIGN(4);
    } > FLASH

    /* Zeroed by the loader, not by the runtime, so it survives from one
       wake to the next. Allocated in the image's file only as far as
       needed: the loader zeroes everything past the image. */
    .bss (NOLOAD) : ALIGN(4)
    {
        __bss_start = .;
        *(.sbss .sbss.* .bss .bss.* COMMON .noinit .noinit.*)
        . = ALIGN(4);
        __bss_end = .;
    } > FLASH

    __stack_top = (ORIGIN(RAM) + LENGTH(RAM)) & ~15;
    ASSERT(__bss_end <= __stack_top - 1K, "LP core program: less than 1 KiB of its 12 KiB of LP SRAM left for the stack")

    /DISCARD/ : { *(.note .note.*) *(.eh_frame .eh_frame_hdr) }
}
