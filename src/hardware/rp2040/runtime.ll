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
;   dodo_board_panic
;                  The panic hook: masks interrupts and halts. Weak, so a
;                  program can define its own to report the failure.
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

module asm ".section .text.dodo_board_panic, \22ax\22, %progbits"
module asm ".weak dodo_board_panic"
module asm ".type dodo_board_panic, %function"
module asm ".thumb_func"
module asm "dodo_board_panic:"
module asm "    cpsid i"
module asm "1:  b 1b"

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
module asm ".thumb_set USBCTRL_IRQ, DefaultHandler"
module asm ".thumb_set XIP_IRQ, DefaultHandler"
module asm ".thumb_set PIO0_IRQ_0, DefaultHandler"
module asm ".thumb_set PIO0_IRQ_1, DefaultHandler"
module asm ".thumb_set PIO1_IRQ_0, DefaultHandler"
module asm ".thumb_set PIO1_IRQ_1, DefaultHandler"
module asm ".thumb_set DMA_IRQ_0, DefaultHandler"
module asm ".thumb_set DMA_IRQ_1, DefaultHandler"
module asm ".thumb_set IO_IRQ_BANK0, DefaultHandler"
module asm ".thumb_set IO_IRQ_QSPI, DefaultHandler"
module asm ".thumb_set SIO_IRQ_PROC0, DefaultHandler"
module asm ".thumb_set SIO_IRQ_PROC1, DefaultHandler"
module asm ".thumb_set CLOCKS_IRQ, DefaultHandler"
module asm ".thumb_set SPI0_IRQ, DefaultHandler"
module asm ".thumb_set SPI1_IRQ, DefaultHandler"
module asm ".thumb_set UART0_IRQ, DefaultHandler"
module asm ".thumb_set UART1_IRQ, DefaultHandler"
module asm ".thumb_set ADC_IRQ_FIFO, DefaultHandler"
module asm ".thumb_set I2C0_IRQ, DefaultHandler"
module asm ".thumb_set I2C1_IRQ, DefaultHandler"
module asm ".thumb_set RTC_IRQ, DefaultHandler"
module asm ".text"

