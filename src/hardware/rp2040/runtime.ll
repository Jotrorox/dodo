; Raspberry Pi RP2040 firmware runtime, compiled by the Dodo compiler for
; every RP2040 executable (`chip = "rp2040"` or a board using it) and linked
; before the program object.
;
; It provides, in order of boot:
;   .boot2         Second-stage bootloader. The boot ROM copies these 256 bytes
;                  to SRAM, checks the CRC32 in the last 4 bytes, and runs them.
;                  They configure the flash interface for execute-in-place with
;                  the standard 03h read command, which every SPI flash chip
;                  supports, then jump through the vector table. The compiler
;                  fills in the CRC after linking.
;   .vector_table  Initial stack pointer, reset handler, Cortex-M0+ exceptions,
;                  and the 32 RP2040 interrupts. Every handler is a weak alias
;                  of DefaultHandler, so `extern "C" fn SysTick_Handler()` or
;                  `extern "C" fn TIMER_IRQ_0()` in Dodo replaces it.
;   Reset_Handler  Copies .data from flash, zeroes .bss, and calls `main`.
;   dodo_rp2040_reset_usb_boot
;                  Restarts into the boot ROM's USB bootloader.
;   dodo_flash_erase, dodo_flash_program
;                  Erase or program the flash through the boot ROM, from
;                  SRAM, for std/embedded/chip/rp_flash.
;   dodo_flash_image_end, dodo_flash_end
;                  Where the program image and the board's flash end.
;   USBCTRL_IRQ    Calls dodo_usb_interrupt if the program defines it:
;                  std/embedded/chip/rp_usb does, for the USB serial port.
;   DMA_IRQ_0      Calls dodo_dma_interrupt if the program defines it:
;                  std/embedded/chip/rp_dma does, to raise the signals of
;                  finished DMA channels.
;   ADC_IRQ_FIFO   Calls dodo_adc_interrupt if the program defines it:
;                  std/embedded/chip/rp_adc does, to raise the signal of
;                  an ADC sampler.
;   SIO_IRQ_PROC0, SIO_IRQ_PROC1
;                  Each core's FIFO interrupt: calls dodo_fifo_interrupt if
;                  the program defines it. std/embedded/chip/rp_multicore
;                  does, to wake a task when the other core writes to the
;                  FIFO between the cores.
;   dodo_core_id   The executing core's number, 0 or 1, from SIO CPUID; the
;                  compiler calls it for `cpu.core_id()`.
;   dodo_core1_trampoline
;                  Where core 1 starts, launched by
;                  std/embedded/chip/rp_multicore: calls the program's entry
;                  point.
;   dodo_board_panic
;                  The panic hook: masks interrupts, passes its arguments to
;                  dodo_panic_report if the program defines it, and halts.
;                  std/embedded/chip/rp_pins defines that reporter, which
;                  writes the failure to the serial ports the program
;                  claimed. Weak, so a program can define its own hook.
;
; The memory and integer helpers LLVM calls on Armv6-M follow from
; src/hardware/arm_eabi.ll, which the compiler appends to this file.

module asm ".syntax unified"
module asm ".cpu cortex-m0plus"
module asm ".thumb"

; Stage 2 bootloader, 252 bytes of position-independent code plus the CRC.
; Register offsets are from the SSI chapter of the RP2040 datasheet (4.10).
module asm ".section .boot2, \22ax\22, %progbits"
module asm ".global __boot2_start"
module asm "__boot2_start:"
module asm "    push {lr}"
module asm "    ldr r3, =0x18000000"                    ; XIP_SSI_BASE
module asm "    movs r1, #0"
module asm "    str r1, [r3, #0x08]"                    ; SSIENR: disable
module asm "    movs r1, #4"
module asm "    str r1, [r3, #0x14]"                    ; BAUDR: clk_sys / 4
module asm "    ldr r1, =0x001f0300"
module asm "    str r1, [r3, #0x00]"                    ; CTRLR0: 32-bit frames, EEPROM read
module asm "    ldr r0, =0x180000f4"
module asm "    ldr r1, =0x03000218"
module asm "    str r1, [r0]"                           ; SPI_CTRLR0: cmd 03h, 8-bit instr, 24-bit addr
module asm "    movs r1, #0"
module asm "    str r1, [r3, #0x04]"                    ; CTRLR1: one data frame
module asm "    movs r1, #1"
module asm "    str r1, [r3, #0x08]"                    ; SSIENR: enable
module asm "    pop {r0}"
module asm "    cmp r0, #0"
module asm "    beq 1f"
module asm "    bx r0"                                  ; called as a function
module asm "1:"
module asm "    ldr r0, =__vector_table"                ; entered from the boot ROM
module asm "    ldr r1, =0xe000ed08"                    ; VTOR
module asm "    str r0, [r1]"
module asm "    ldmia r0, {r0, r1}"
module asm "    msr msp, r0"
module asm "    bx r1"
module asm "    .ltorg"
module asm "    .org 252"
module asm "    .word 0"                                ; CRC32, patched after linking

module asm ".section .vector_table, \22a\22, %progbits"
module asm ".balign 256"
module asm ".global __vector_table"
module asm "__vector_table:"
module asm "    .word __stack_top"
module asm "    .word Reset_Handler"
module asm "    .word NMI_Handler"
module asm "    .word HardFault_Handler"
module asm "    .word 0, 0, 0, 0, 0, 0, 0"
module asm "    .word SVC_Handler"
module asm "    .word 0, 0"
module asm "    .word PendSV_Handler"
module asm "    .word SysTick_Handler"
module asm "    .word TIMER_IRQ_0, TIMER_IRQ_1, TIMER_IRQ_2, TIMER_IRQ_3"
module asm "    .word PWM_IRQ_WRAP, USBCTRL_IRQ, XIP_IRQ"
module asm "    .word PIO0_IRQ_0, PIO0_IRQ_1, PIO1_IRQ_0, PIO1_IRQ_1"
module asm "    .word DMA_IRQ_0, DMA_IRQ_1, IO_IRQ_BANK0, IO_IRQ_QSPI"
module asm "    .word SIO_IRQ_PROC0, SIO_IRQ_PROC1, CLOCKS_IRQ"
module asm "    .word SPI0_IRQ, SPI1_IRQ, UART0_IRQ, UART1_IRQ"
module asm "    .word ADC_IRQ_FIFO, I2C0_IRQ, I2C1_IRQ, RTC_IRQ"
module asm "    .word DefaultHandler, DefaultHandler, DefaultHandler"
module asm "    .word DefaultHandler, DefaultHandler, DefaultHandler"

module asm ".section .text.Reset_Handler, \22ax\22, %progbits"
module asm ".global Reset_Handler"
module asm ".type Reset_Handler, %function"
module asm ".thumb_func"
module asm "Reset_Handler:"
module asm "    ldr r0, =__data_load"
module asm "    ldr r1, =__data_start"
module asm "    ldr r2, =__data_end"
module asm "    b 2f"
module asm "1:  ldm r0!, {r3}"
module asm "    stm r1!, {r3}"
module asm "2:  cmp r1, r2"
module asm "    blo 1b"
module asm "    ldr r1, =__bss_start"
module asm "    ldr r2, =__bss_end"
module asm "    movs r3, #0"
module asm "    b 4f"
module asm "3:  stm r1!, {r3}"
module asm "4:  cmp r1, r2"
module asm "    blo 3b"
module asm "    bl main"
module asm "5:  wfi"                                    ; main returned: sleep
module asm "    b 5b"
module asm "    .ltorg"

module asm ".section .text.DefaultHandler, \22ax\22, %progbits"
module asm ".global DefaultHandler"
module asm ".type DefaultHandler, %function"
module asm ".thumb_func"
module asm "DefaultHandler:"
module asm "    b DefaultHandler"

; USBCTRL_IRQ, weak like the other handlers, tail-calls the weak
; dodo_usb_interrupt if the program defines it, and is DefaultHandler
; otherwise. std/embedded/chip/rp_usb defines it to serve the USB serial
; port while one is open.
module asm ".section .text.USBCTRL_IRQ, \22ax\22, %progbits"
module asm ".weak dodo_usb_interrupt"
module asm ".type USBCTRL_IRQ, %function"
module asm ".thumb_func"
module asm "USBCTRL_IRQ:"
module asm "    ldr r0, =dodo_usb_interrupt"
module asm "    cmp r0, #0"
module asm "    bne 1f"
module asm "    ldr r0, =DefaultHandler"
module asm "1:  bx r0"                                  ; lr still holds EXC_RETURN
module asm "    .ltorg"

; DMA_IRQ_0 does the same with dodo_dma_interrupt, which
; std/embedded/chip/rp_dma defines to raise the signals of finished
; channels.
module asm ".section .text.DMA_IRQ_0, \22ax\22, %progbits"
module asm ".weak dodo_dma_interrupt"
module asm ".type DMA_IRQ_0, %function"
module asm ".thumb_func"
module asm "DMA_IRQ_0:"
module asm "    ldr r0, =dodo_dma_interrupt"
module asm "    cmp r0, #0"
module asm "    bne 1f"
module asm "    ldr r0, =DefaultHandler"
module asm "1:  bx r0"
module asm "    .ltorg"

; SIO_IRQ_PROC0 and SIO_IRQ_PROC1, the cores' FIFO interrupts, likewise
; tail-call the weak dodo_fifo_interrupt.
module asm ".section .text.FifoHandler, \22ax\22, %progbits"
module asm ".weak dodo_fifo_interrupt"
module asm ".type FifoHandler, %function"
module asm ".thumb_func"
module asm "FifoHandler:"
module asm "    ldr r0, =dodo_fifo_interrupt"
module asm "    cmp r0, #0"
module asm "    bne 1f"
module asm "    ldr r0, =DefaultHandler"
module asm "1:  bx r0"
module asm "    .ltorg"

; ADC_IRQ_FIFO likewise tail-calls the weak dodo_adc_interrupt, which
; std/embedded/chip/rp_adc defines to raise a sampler's signal.
module asm ".section .text.AdcHandler, \22ax\22, %progbits"
module asm ".weak dodo_adc_interrupt"
module asm ".type AdcHandler, %function"
module asm ".thumb_func"
module asm "AdcHandler:"
module asm "    ldr r0, =dodo_adc_interrupt"
module asm "    cmp r0, #0"
module asm "    bne 1f"
module asm "    ldr r0, =DefaultHandler"
module asm "1:  bx r0"
module asm "    .ltorg"

module asm ".section .text.dodo_core_id, \22ax\22, %progbits"
module asm ".global dodo_core_id"
module asm ".type dodo_core_id, %function"
module asm ".thumb_func"
module asm "dodo_core_id:"
module asm "    ldr r0, =0xd0000000"                    ; SIO CPUID
module asm "    ldr r0, [r0]"
module asm "    bx lr"
module asm "    .ltorg"

; Core 1's boot ROM sets VTOR and the stack pointer from the launch sequence
; and enters here. The launching core left three words on top of the stack:
; the entry point's argument, the entry point (`unsafe fn(*mut u8)`), and the
; stack's lowest address, which the Cortex-M0+ cannot enforce. If the entry
; point returns, core 1 sleeps.
module asm ".section .text.dodo_core1_trampoline, \22ax\22, %progbits"
module asm ".global dodo_core1_trampoline"
module asm ".type dodo_core1_trampoline, %function"
module asm ".thumb_func"
module asm "dodo_core1_trampoline:"
module asm "    pop {r0, r1, r2}"
module asm "    blx r1"
module asm "1:  wfi"
module asm "    b 1b"

; dodo_core1_trampoline's address, with the Thumb bit, for the launch sequence.
module asm ".section .text.dodo_core1_entry, \22ax\22, %progbits"
module asm ".global dodo_core1_entry"
module asm ".type dodo_core1_entry, %function"
module asm ".thumb_func"
module asm "dodo_core1_entry:"
module asm "    ldr r0, =dodo_core1_trampoline"
module asm "    bx lr"
module asm "    .ltorg"

; A weak reference: without a definition the linker makes it 0, and a
; definition is kept by --gc-sections only while this hook is.
module asm ".section .text.dodo_board_panic, \22ax\22, %progbits"
module asm ".weak dodo_board_panic"
module asm ".weak dodo_panic_report"
module asm ".type dodo_board_panic, %function"
module asm ".thumb_func"
module asm "dodo_board_panic:"
module asm "    cpsid i"
module asm "    ldr r4, =dodo_panic_report"
module asm "    cmp r4, #0"
module asm "    beq 1f"
module asm "    blx r4"                                 ; r0 to r3 are the hook's arguments
module asm "1:  b 1b"
module asm "    .ltorg"

; dodo_rp2040_reset_usb_boot(activity_pins, disabled_interfaces): look up the
; boot ROM's reset_usb_boot (code 'UB') and jump to it. It never returns.
module asm ".section .text.dodo_rp2040_reset_usb_boot, \22ax\22, %progbits"
module asm ".global dodo_rp2040_reset_usb_boot"
module asm ".type dodo_rp2040_reset_usb_boot, %function"
module asm ".thumb_func"
module asm "dodo_rp2040_reset_usb_boot:"
module asm "    push {r0, r1}"
module asm "    movs r3, #0x14"
module asm "    ldrh r0, [r3, #0]"                      ; ROM function table
module asm "    ldrh r2, [r3, #4]"                      ; rom_table_lookup
module asm "    ldr r1, =0x4255"                        ; 'U' | 'B' << 8
module asm "    blx r2"
module asm "    mov r2, r0"
module asm "    pop {r0, r1}"
module asm "    bx r2"
module asm "    .ltorg"

; dodo_flash_erase(offset, count) and dodo_flash_program(offset, data, count)
; erase `count` bytes of flash from `offset` (multiples of 4 KiB), or program
; them from `data` in SRAM (multiples of 256 bytes), and return 0. The flash
; cannot execute in place meanwhile, so this code runs from SRAM (.ramfunc)
; with interrupts masked, and calls only the boot ROM (datasheet 2.8.3):
; connect_internal_flash, flash_exit_xip, flash_range_erase (64 KiB block
; erases where aligned, 4 KiB sectors elsewhere) or flash_range_program, and
; flash_flush_cache. Then the copy of .boot2 taken first, which is position
; independent and returns when called, sets execute-in-place up again exactly
; as at boot.
module asm ".section .ramfunc.dodo_flash, \22ax\22, %progbits"
module asm ".balign 4"
module asm ".global dodo_flash_erase"
module asm ".type dodo_flash_erase, %function"
module asm ".thumb_func"
module asm "dodo_flash_erase:"
module asm "    mov r3, r1"                             ; count
module asm "    movs r2, #0"                            ; no data
module asm "    mov r1, r0"                             ; offset
module asm "    movs r0, #0"                            ; erase
module asm "    b 1f"
module asm ".global dodo_flash_program"
module asm ".type dodo_flash_program, %function"
module asm ".thumb_func"
module asm "dodo_flash_program:"
module asm "    mov r3, r2"                             ; count
module asm "    mov r2, r1"                             ; data
module asm "    mov r1, r0"                             ; offset
module asm "    movs r0, #1"                            ; program
module asm "1:  push {r4, r5, r6, r7, lr}"
module asm "    sub sp, #20"                            ; four routines, PRIMASK
module asm "    mov r4, r0"
module asm "    mov r5, r1"
module asm "    mov r6, r2"
module asm "    mov r7, r3"
module asm "    mrs r0, primask"
module asm "    str r0, [sp, #16]"
module asm "    cpsid i"
module asm "    ldr r1, =0x4649"                        ; 'I' 'F' connect_internal_flash
module asm "    bl 8f"
module asm "    str r0, [sp, #0]"
module asm "    ldr r1, =0x5845"                        ; 'E' 'X' flash_exit_xip
module asm "    bl 8f"
module asm "    str r0, [sp, #4]"
module asm "    ldr r1, =0x4552"                        ; 'R' 'E' flash_range_erase
module asm "    cmp r4, #0"
module asm "    beq 2f"
module asm "    ldr r1, =0x5052"                        ; 'R' 'P' flash_range_program
module asm "2:  bl 8f"
module asm "    str r0, [sp, #8]"
module asm "    ldr r1, =0x4346"                        ; 'F' 'C' flash_flush_cache
module asm "    bl 8f"
module asm "    str r0, [sp, #12]"
module asm "    ldr r0, =0x10000000"                    ; .boot2, the first 256 bytes
module asm "    ldr r1, =dodo_rp2040_boot2_copy"
module asm "    movs r2, #64"
module asm "3:  ldm r0!, {r3}"
module asm "    stm r1!, {r3}"
module asm "    subs r2, #1"
module asm "    bne 3b"
module asm "    ldr r0, [sp, #0]"                       ; no flash access from here
module asm "    blx r0"
module asm "    ldr r0, [sp, #4]"
module asm "    blx r0"
module asm "    cmp r4, #0"
module asm "    bne 4f"
module asm "    mov r0, r5"                             ; erase(offset, count,
module asm "    mov r1, r7"
module asm "    movs r2, #1"
module asm "    lsls r2, r2, #16"                       ;   64 KiB blocks,
module asm "    movs r3, #0xd8"                         ;   block erase command)
module asm "    b 5f"
module asm "4:  mov r0, r5"                             ; program(offset, data, count)
module asm "    mov r1, r6"
module asm "    mov r2, r7"
module asm "5:  ldr r4, [sp, #8]"
module asm "    blx r4"
module asm "    ldr r0, [sp, #12]"
module asm "    blx r0"                                 ; also releases chip select
module asm "    ldr r0, =dodo_rp2040_boot2_copy + 1"
module asm "    blx r0"                                 ; flash executes in place again
module asm "    ldr r0, [sp, #16]"
module asm "    msr primask, r0"
module asm "    movs r0, #0"
module asm "    add sp, #20"
module asm "    pop {r4, r5, r6, r7, pc}"
module asm "8:  movs r3, #0x14"                         ; rom_table_lookup(table, r1)
module asm "    ldrh r0, [r3, #0]"
module asm "    ldrh r2, [r3, #4]"
module asm "    bx r2"                                  ; returns to the caller
module asm "    .ltorg"

module asm ".section .bss.dodo_rp2040_boot2_copy, \22aw\22, %nobits"
module asm ".balign 4"
module asm "dodo_rp2040_boot2_copy:"
module asm "    .space 256"

; dodo_flash_image_end() and dodo_flash_end(): the linker script's addresses.
module asm ".section .text.dodo_flash_image_end, \22ax\22, %progbits"
module asm ".global dodo_flash_image_end"
module asm ".type dodo_flash_image_end, %function"
module asm ".thumb_func"
module asm "dodo_flash_image_end:"
module asm "    ldr r0, =__flash_image_end"
module asm "    bx lr"
module asm "    .ltorg"
module asm ".section .text.dodo_flash_end, \22ax\22, %progbits"
module asm ".global dodo_flash_end"
module asm ".type dodo_flash_end, %function"
module asm ".thumb_func"
module asm "dodo_flash_end:"
module asm "    ldr r0, =__flash_end"
module asm "    bx lr"
module asm "    .ltorg"

module asm ".weak NMI_Handler, HardFault_Handler, SVC_Handler, PendSV_Handler, SysTick_Handler"
module asm ".weak TIMER_IRQ_0, TIMER_IRQ_1, TIMER_IRQ_2, TIMER_IRQ_3, PWM_IRQ_WRAP, USBCTRL_IRQ"
module asm ".weak XIP_IRQ, PIO0_IRQ_0, PIO0_IRQ_1, PIO1_IRQ_0, PIO1_IRQ_1, DMA_IRQ_0, DMA_IRQ_1"
module asm ".weak IO_IRQ_BANK0, IO_IRQ_QSPI, SIO_IRQ_PROC0, SIO_IRQ_PROC1, CLOCKS_IRQ, SPI0_IRQ"
module asm ".weak SPI1_IRQ, UART0_IRQ, UART1_IRQ, ADC_IRQ_FIFO, I2C0_IRQ, I2C1_IRQ, RTC_IRQ"
module asm ".thumb_set NMI_Handler, DefaultHandler"
module asm ".thumb_set HardFault_Handler, DefaultHandler"
module asm ".thumb_set SVC_Handler, DefaultHandler"
module asm ".thumb_set PendSV_Handler, DefaultHandler"
module asm ".thumb_set SysTick_Handler, DefaultHandler"
module asm ".thumb_set TIMER_IRQ_0, DefaultHandler"
module asm ".thumb_set TIMER_IRQ_1, DefaultHandler"
module asm ".thumb_set TIMER_IRQ_2, DefaultHandler"
module asm ".thumb_set TIMER_IRQ_3, DefaultHandler"
module asm ".thumb_set PWM_IRQ_WRAP, DefaultHandler"
module asm ".thumb_set XIP_IRQ, DefaultHandler"
module asm ".thumb_set PIO0_IRQ_0, DefaultHandler"
module asm ".thumb_set PIO0_IRQ_1, DefaultHandler"
module asm ".thumb_set PIO1_IRQ_0, DefaultHandler"
module asm ".thumb_set PIO1_IRQ_1, DefaultHandler"
module asm ".thumb_set DMA_IRQ_1, DefaultHandler"
module asm ".thumb_set IO_IRQ_BANK0, DefaultHandler"
module asm ".thumb_set IO_IRQ_QSPI, DefaultHandler"
module asm ".thumb_set SIO_IRQ_PROC0, FifoHandler"
module asm ".thumb_set SIO_IRQ_PROC1, FifoHandler"
module asm ".thumb_set CLOCKS_IRQ, DefaultHandler"
module asm ".thumb_set SPI0_IRQ, DefaultHandler"
module asm ".thumb_set SPI1_IRQ, DefaultHandler"
module asm ".thumb_set UART0_IRQ, DefaultHandler"
module asm ".thumb_set UART1_IRQ, DefaultHandler"
module asm ".thumb_set ADC_IRQ_FIFO, AdcHandler"
module asm ".thumb_set I2C0_IRQ, DefaultHandler"
module asm ".thumb_set I2C1_IRQ, DefaultHandler"
module asm ".thumb_set RTC_IRQ, DefaultHandler"
module asm ".text"

