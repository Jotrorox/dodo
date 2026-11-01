; Raspberry Pi RP2350 firmware runtime for the Arm (Cortex-M33) cores,
; compiled by the Dodo compiler for every RP2350 executable (`chip = "rp2350"`
; or a board using it) and linked before the program object.
;
; Unlike the RP2040, the RP2350 boot ROM sets up execute-in-place flash access
; itself, so there is no second-stage bootloader. Instead it looks for an
; IMAGE_DEF block in the first 4 KiB of flash that marks the image as a secure
; Arm executable, then enters it through the vector table at the start of
; flash.
;
; It provides, in order of boot:
;   .vector_table  Initial stack pointer, reset handler, Cortex-M33
;                  exceptions, and the 52 RP2350 interrupts. Every handler is
;                  a weak alias of DefaultHandler, so `extern "C" fn
;                  SysTick_Handler()` or `extern "C" fn TIMER0_IRQ_0()` in Dodo
;                  replaces it.
;   .picobin_block The IMAGE_DEF block the boot ROM requires (RP2350
;                  datasheet 5.9.5, minimum viable image metadata).
;   Reset_Handler  Clears the stack limit, points VTOR at the vector table,
;                  enables the FPU and the double-precision coprocessor,
;                  copies .data from flash, zeroes .bss, and calls `main`.
;   dodo_rp2350_rom_call
;                  Calls a boot ROM function by its two-letter code, for
;                  example reboot ('RB') to restart into the USB bootloader.
;   dodo_flash_erase, dodo_flash_program
;                  Erase or program the flash through the boot ROM, for
;                  std/embedded/chip/rp_flash.
;   dodo_flash_image_end, dodo_flash_end
;                  Where the program image and the board's flash end.
;   IO_IRQ_QSPI    dodo_lockout_handler, from SRAM: parks this core while the
;                  other core erases or writes the flash, for
;                  std/embedded/chip/rp_multicore's lockout.
;   USBCTRL_IRQ    Calls dodo_usb_interrupt if the program defines it:
;                  std/embedded/chip/rp_usb does, for the USB serial port.
;   DMA_IRQ_0      Calls dodo_dma_interrupt if the program defines it:
;                  std/embedded/chip/rp_dma does, to raise the signals of
;                  finished DMA channels.
;   SIO_IRQ_FIFO   Calls dodo_fifo_interrupt if the program defines it:
;                  std/embedded/chip/rp_multicore does, to wake a task when
;                  the other core writes to the FIFO between the cores.
;   SIO_IRQ_BELL   Calls dodo_doorbell_interrupt if the program defines it:
;                  std/embedded/chip/rp_multicore does, to wake a task when
;                  the other core rings one of the doorbells.
;   dodo_core_id   The executing core's number, 0 or 1, from SIO CPUID; the
;                  compiler calls it for `cpu.core_id()`.
;   dodo_core1_trampoline
;                  Where core 1 starts, launched by
;                  std/embedded/chip/rp_multicore: limits the stack, enables
;                  the FPU and the double coprocessor as Reset_Handler does
;                  on core 0, and calls the program's entry point.
;   dodo_board_panic
;                  The panic hook: masks interrupts, passes its arguments to
;                  dodo_panic_report if the program defines it, and halts.
;                  std/embedded/chip/rp_pins defines that reporter, which
;                  writes the failure to the serial ports the program
;                  claimed. Weak, so a program can define its own hook.
;
; The memory and integer helpers LLVM calls follow from
; src/hardware/arm_eabi.ll, which the compiler appends to this file.

module asm ".syntax unified"
module asm ".cpu cortex-m33"
module asm ".thumb"

module asm ".section .vector_table, \22a\22, %progbits"
module asm ".balign 512"
module asm ".global __vector_table"
module asm "__vector_table:"
module asm "    .word __stack_top"
module asm "    .word Reset_Handler"
module asm "    .word NMI_Handler"
module asm "    .word HardFault_Handler"
module asm "    .word MemManage_Handler"
module asm "    .word BusFault_Handler"
module asm "    .word UsageFault_Handler"
module asm "    .word SecureFault_Handler"
module asm "    .word 0, 0, 0"
module asm "    .word SVC_Handler"
module asm "    .word DebugMon_Handler"
module asm "    .word 0"
module asm "    .word PendSV_Handler"
module asm "    .word SysTick_Handler"
module asm "    .word TIMER0_IRQ_0, TIMER0_IRQ_1, TIMER0_IRQ_2, TIMER0_IRQ_3"
module asm "    .word TIMER1_IRQ_0, TIMER1_IRQ_1, TIMER1_IRQ_2, TIMER1_IRQ_3"
module asm "    .word PWM_IRQ_WRAP_0, PWM_IRQ_WRAP_1"
module asm "    .word DMA_IRQ_0, DMA_IRQ_1, DMA_IRQ_2, DMA_IRQ_3, USBCTRL_IRQ"
module asm "    .word PIO0_IRQ_0, PIO0_IRQ_1, PIO1_IRQ_0, PIO1_IRQ_1, PIO2_IRQ_0, PIO2_IRQ_1"
module asm "    .word IO_IRQ_BANK0, IO_IRQ_BANK0_NS, IO_IRQ_QSPI, IO_IRQ_QSPI_NS"
module asm "    .word SIO_IRQ_FIFO, SIO_IRQ_BELL, SIO_IRQ_FIFO_NS, SIO_IRQ_BELL_NS"
module asm "    .word SIO_IRQ_MTIMECMP, CLOCKS_IRQ"
module asm "    .word SPI0_IRQ, SPI1_IRQ, UART0_IRQ, UART1_IRQ, ADC_IRQ_FIFO"
module asm "    .word I2C0_IRQ, I2C1_IRQ, OTP_IRQ, TRNG_IRQ"
module asm "    .word PROC0_IRQ_CTI, PROC1_IRQ_CTI, PLL_SYS_IRQ, PLL_USB_IRQ"
module asm "    .word POWMAN_IRQ_POW, POWMAN_IRQ_TIMER"
module asm "    .word DefaultHandler, DefaultHandler, DefaultHandler"
module asm "    .word DefaultHandler, DefaultHandler, DefaultHandler"

; IMAGE_DEF: one IMAGE_TYPE item (executable, secure, Arm, RP2350), the last
; item marker with the items' size in words, and a link back to itself, since
; this is the only block.
module asm ".section .picobin_block, \22a\22, %progbits"
module asm ".balign 4"
module asm ".global __picobin_block"
module asm "__picobin_block:"
module asm "    .word 0xffffded3"                       ; PICOBIN_BLOCK_MARKER_START
module asm "    .byte 0x42, 0x01"                       ; IMAGE_TYPE item, 1 word
module asm "    .hword 0x1021"                          ; EXE | SECURITY_S | CPU_ARM | CHIP_RP2350
module asm "    .byte 0xff"                             ; LAST item
module asm "    .hword 1"                               ; items before it: 1 word
module asm "    .byte 0"
module asm "    .word 0"                                ; next block: this one
module asm "    .word 0xab123579"                       ; PICOBIN_BLOCK_MARKER_END

module asm ".section .text.Reset_Handler, \22ax\22, %progbits"
module asm ".global Reset_Handler"
module asm ".type Reset_Handler, %function"
module asm ".thumb_func"
module asm "Reset_Handler:"
module asm "    movs r0, #0"
module asm "    msr msplim, r0"                         ; no stack limit
module asm "    ldr r0, =__vector_table"
module asm "    ldr r1, =0xe000ed08"                    ; VTOR
module asm "    str r0, [r1]"
module asm "    ldr r0, =0xe000ed88"                    ; CPACR: full access to
module asm "    ldr r1, [r0]"                           ; CP10 and CP11, the FPU,
module asm "    orr r1, r1, #0x00f00000"                ; and CP4, the
module asm "    orr r1, r1, #0x00000300"                ; double coprocessor
module asm "    str r1, [r0]"
module asm "    dsb"
module asm "    isb"
module asm "    ldr r0, =__data_load"
module asm "    ldr r1, =__data_start"
module asm "    ldr r2, =__data_end"
module asm "    b 2f"
module asm "1:  ldr r3, [r0], #4"
module asm "    str r3, [r1], #4"
module asm "2:  cmp r1, r2"
module asm "    blo 1b"
module asm "    ldr r1, =__bss_start"
module asm "    ldr r2, =__bss_end"
module asm "    movs r3, #0"
module asm "    b 4f"
module asm "3:  str r3, [r1], #4"
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

; SIO_IRQ_FIFO, each core's FIFO interrupt, likewise tail-calls the weak
; dodo_fifo_interrupt.
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

; SIO_IRQ_BELL, each core's doorbell interrupt, likewise tail-calls the
; weak dodo_doorbell_interrupt.
module asm ".section .text.BellHandler, \22ax\22, %progbits"
module asm ".weak dodo_doorbell_interrupt"
module asm ".type BellHandler, %function"
module asm ".thumb_func"
module asm "BellHandler:"
module asm "    ldr r0, =dodo_doorbell_interrupt"
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
; stack's lowest address, which becomes the stack limit, so an overflow
; faults instead of overwriting memory below. If the entry point returns,
; core 1 sleeps.
module asm ".section .text.dodo_core1_trampoline, \22ax\22, %progbits"
module asm ".global dodo_core1_trampoline"
module asm ".type dodo_core1_trampoline, %function"
module asm ".thumb_func"
module asm "dodo_core1_trampoline:"
module asm "    pop {r0, r1, r2}"
module asm "    msr msplim, r2"
module asm "    ldr r3, =0xe000ed88"                    ; CPACR, per core
module asm "    ldr r2, [r3]"
module asm "    orr r2, r2, #0x00f00000"
module asm "    orr r2, r2, #0x00000300"
module asm "    str r2, [r3]"
module asm "    dsb"
module asm "    isb"
module asm "    blx r1"
module asm "1:  wfi"
module asm "    b 1b"
module asm "    .ltorg"

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

; dodo_rp2350_rom_call(code, a0, a1, a2, a3): look up the boot ROM function
; `code` (two ASCII characters, first in the low byte) for secure Arm callers
; and call it with up to four arguments, returning its result. The ROM table
; lookup function's address is the halfword at 0x16. Returns -1 if the ROM
; has no such function.
module asm ".section .text.dodo_rp2350_rom_call, \22ax\22, %progbits"
module asm ".global dodo_rp2350_rom_call"
module asm ".type dodo_rp2350_rom_call, %function"
module asm ".thumb_func"
module asm "dodo_rp2350_rom_call:"
module asm "    push {r4, r5, r6, lr}"                  ; a3 is now at [sp, #16]
module asm "    mov r4, r1"
module asm "    mov r5, r2"
module asm "    mov r6, r3"
module asm "    movs r1, #4"                            ; RT_FLAG_FUNC_ARM_SEC
module asm "    movs r3, #0x16"
module asm "    ldrh r3, [r3]"                          ; rom_table_lookup
module asm "    blx r3"
module asm "    cbz r0, 1f"
module asm "    mov r12, r0"
module asm "    mov r0, r4"
module asm "    mov r1, r5"
module asm "    mov r2, r6"
module asm "    ldr r3, [sp, #16]"
module asm "    blx r12"
module asm "    pop {r4, r5, r6, pc}"
module asm "1:  mov r0, #-1"
module asm "    pop {r4, r5, r6, pc}"

; dodo_flash_erase(offset, count) and dodo_flash_program(offset, data, count)
; erase `count` bytes of flash from `offset` (multiples of 4 KiB), or program
; them from `data` in SRAM (multiples of 256 bytes), through the boot ROM's
; flash_op (datasheet 5.4.8) at the flash's runtime address, as a secure
; caller, with interrupts masked. flash_op leaves execute-in-place mode,
; works, flushes the cache, and restores it, so this code may run from flash.
; Returns flash_op's result, 0 on success, or -1 if the ROM lacks it.
module asm ".section .text.dodo_flash, \22ax\22, %progbits"
module asm ".global dodo_flash_erase"
module asm ".type dodo_flash_erase, %function"
module asm ".thumb_func"
module asm "dodo_flash_erase:"
module asm "    mov r2, r1"                             ; size
module asm "    mov r1, r0"                             ; offset
module asm "    movs r3, #0"                            ; no buffer
module asm "    ldr r0, =0x00101"                       ; erase, secure, runtime address
module asm "    b 1f"
module asm ".global dodo_flash_program"
module asm ".type dodo_flash_program, %function"
module asm ".thumb_func"
module asm "dodo_flash_program:"
module asm "    mov r3, r1"                             ; buffer
module asm "    mov r1, r0"                             ; offset; r2 is the size
module asm "    ldr r0, =0x10101"                       ; program, secure, runtime address
module asm "1:  push {r4, r5, r6, r7, r8, lr}"
module asm "    mrs r8, primask"
module asm "    cpsid i"
module asm "    mov r4, r0"
module asm "    mov r5, r1"
module asm "    mov r6, r2"
module asm "    mov r7, r3"
module asm "    ldr r0, =0x4F46"                        ; 'F' 'O' flash_op
module asm "    movs r1, #4"                            ; RT_FLAG_FUNC_ARM_SEC
module asm "    movs r3, #0x16"
module asm "    ldrh r3, [r3]"                          ; rom_table_lookup
module asm "    blx r3"
module asm "    cbz r0, 2f"
module asm "    mov r12, r0"
module asm "    mov r0, r4"
module asm "    ldr r1, =0x10000000"                    ; XIP_BASE
module asm "    add r1, r5"
module asm "    mov r2, r6"
module asm "    mov r3, r7"
module asm "    blx r12"
module asm "    b 3f"
module asm "2:  mov r0, #-1"
module asm "3:  msr primask, r8"
module asm "    pop {r4, r5, r6, r7, r8, pc}"
module asm "    .ltorg"

; The multicore lockout (std/embedded/chip/rp_multicore). While one core
; erases or writes the flash, the other must not fetch from it, so the first
; parks the second here, in SRAM (.ramfunc), with its interrupts masked. The
; requesting core writes a nonzero request number to the mailbox's first
; word, then forces this core's IO_IRQ_QSPI, the secure one, as the QSPI
; pins are secure, through IO_QSPI's PROC0_INTF or PROC1_INTF (datasheet
; 9.11.2: INTS = INTR & INTE | INTF), so the vector is fetched while the
; flash still executes in place. This handler clears the forced bit (SCLK's
; LEVEL_LOW, bit 8), copies the request into the second word to acknowledge
; it, and spins until the first word changes. A request withdrawn before
; the handler ran reads as 0: it returns at once. The mailbox's address
; comes from dodo_lockout_words.
module asm ".section .ramfunc.dodo_lockout, \22ax\22, %progbits"
module asm ".balign 4"
module asm ".global dodo_lockout_handler"
module asm ".type dodo_lockout_handler, %function"
module asm ".thumb_func"
module asm "dodo_lockout_handler:"
module asm "    mrs r3, primask"
module asm "    cpsid i"
module asm "    ldr r0, =0xd0000000"                    ; SIO CPUID: 0 or 1
module asm "    ldr r0, [r0]"
module asm "    movs r1, #12"
module asm "    mul r1, r0, r1"
module asm "    ldr r2, =0x40033220"                    ; IO_QSPI PROC0_INTF, clear alias
module asm "    add r2, r2, r1"                         ; PROC1_INTF 12 bytes on
module asm "    mov r1, #0x100"
module asm "    str r1, [r2]"
module asm "    dsb"
module asm "    ldr r0, =dodo_lockout_mailbox"
module asm "    ldr r1, [r0]"                           ; the request
module asm "    cbz r1, 2f"
module asm "    dmb"
module asm "    str r1, [r0, #4]"                       ; acknowledged: parked
module asm "1:  ldr r2, [r0]"
module asm "    cmp r2, r1"
module asm "    beq 1b"
module asm "2:  dmb"
module asm "    msr primask, r3"
module asm "    bx lr"
module asm "    .ltorg"

module asm ".section .bss.dodo_lockout_mailbox, \22aw\22, %nobits"
module asm ".balign 4"
module asm ".global dodo_lockout_mailbox"
module asm "dodo_lockout_mailbox:"
module asm "    .space 8"

module asm ".section .text.dodo_lockout_words, \22ax\22, %progbits"
module asm ".global dodo_lockout_words"
module asm ".type dodo_lockout_words, %function"
module asm ".thumb_func"
module asm "dodo_lockout_words:"
module asm "    ldr r0, =dodo_lockout_mailbox"
module asm "    bx lr"
module asm "    .ltorg"

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

module asm ".weak NMI_Handler, HardFault_Handler, MemManage_Handler, BusFault_Handler"
module asm ".weak UsageFault_Handler, SecureFault_Handler, SVC_Handler, DebugMon_Handler"
module asm ".weak PendSV_Handler, SysTick_Handler"
module asm ".weak TIMER0_IRQ_0, TIMER0_IRQ_1, TIMER0_IRQ_2, TIMER0_IRQ_3"
module asm ".weak TIMER1_IRQ_0, TIMER1_IRQ_1, TIMER1_IRQ_2, TIMER1_IRQ_3"
module asm ".weak PWM_IRQ_WRAP_0, PWM_IRQ_WRAP_1, DMA_IRQ_0, DMA_IRQ_1, DMA_IRQ_2, DMA_IRQ_3"
module asm ".weak USBCTRL_IRQ, PIO0_IRQ_0, PIO0_IRQ_1, PIO1_IRQ_0, PIO1_IRQ_1, PIO2_IRQ_0, PIO2_IRQ_1"
module asm ".weak IO_IRQ_BANK0, IO_IRQ_BANK0_NS, IO_IRQ_QSPI, IO_IRQ_QSPI_NS"
module asm ".weak SIO_IRQ_FIFO, SIO_IRQ_BELL, SIO_IRQ_FIFO_NS, SIO_IRQ_BELL_NS, SIO_IRQ_MTIMECMP"
module asm ".weak CLOCKS_IRQ, SPI0_IRQ, SPI1_IRQ, UART0_IRQ, UART1_IRQ, ADC_IRQ_FIFO"
module asm ".weak I2C0_IRQ, I2C1_IRQ, OTP_IRQ, TRNG_IRQ, PROC0_IRQ_CTI, PROC1_IRQ_CTI"
module asm ".weak PLL_SYS_IRQ, PLL_USB_IRQ, POWMAN_IRQ_POW, POWMAN_IRQ_TIMER"
module asm ".thumb_set NMI_Handler, DefaultHandler"
module asm ".thumb_set HardFault_Handler, DefaultHandler"
module asm ".thumb_set MemManage_Handler, DefaultHandler"
module asm ".thumb_set BusFault_Handler, DefaultHandler"
module asm ".thumb_set UsageFault_Handler, DefaultHandler"
module asm ".thumb_set SecureFault_Handler, DefaultHandler"
module asm ".thumb_set SVC_Handler, DefaultHandler"
module asm ".thumb_set DebugMon_Handler, DefaultHandler"
module asm ".thumb_set PendSV_Handler, DefaultHandler"
module asm ".thumb_set SysTick_Handler, DefaultHandler"
module asm ".thumb_set TIMER0_IRQ_0, DefaultHandler"
module asm ".thumb_set TIMER0_IRQ_1, DefaultHandler"
module asm ".thumb_set TIMER0_IRQ_2, DefaultHandler"
module asm ".thumb_set TIMER0_IRQ_3, DefaultHandler"
module asm ".thumb_set TIMER1_IRQ_0, DefaultHandler"
module asm ".thumb_set TIMER1_IRQ_1, DefaultHandler"
module asm ".thumb_set TIMER1_IRQ_2, DefaultHandler"
module asm ".thumb_set TIMER1_IRQ_3, DefaultHandler"
module asm ".thumb_set PWM_IRQ_WRAP_0, DefaultHandler"
module asm ".thumb_set PWM_IRQ_WRAP_1, DefaultHandler"
module asm ".thumb_set DMA_IRQ_1, DefaultHandler"
module asm ".thumb_set DMA_IRQ_2, DefaultHandler"
module asm ".thumb_set DMA_IRQ_3, DefaultHandler"
module asm ".thumb_set PIO0_IRQ_0, DefaultHandler"
module asm ".thumb_set PIO0_IRQ_1, DefaultHandler"
module asm ".thumb_set PIO1_IRQ_0, DefaultHandler"
module asm ".thumb_set PIO1_IRQ_1, DefaultHandler"
module asm ".thumb_set PIO2_IRQ_0, DefaultHandler"
module asm ".thumb_set PIO2_IRQ_1, DefaultHandler"
module asm ".thumb_set IO_IRQ_BANK0, DefaultHandler"
module asm ".thumb_set IO_IRQ_BANK0_NS, DefaultHandler"
module asm ".thumb_set IO_IRQ_QSPI, dodo_lockout_handler"
module asm ".thumb_set IO_IRQ_QSPI_NS, DefaultHandler"
module asm ".thumb_set SIO_IRQ_FIFO, FifoHandler"
module asm ".thumb_set SIO_IRQ_BELL, BellHandler"
module asm ".thumb_set SIO_IRQ_FIFO_NS, DefaultHandler"
module asm ".thumb_set SIO_IRQ_BELL_NS, DefaultHandler"
module asm ".thumb_set SIO_IRQ_MTIMECMP, DefaultHandler"
module asm ".thumb_set CLOCKS_IRQ, DefaultHandler"
module asm ".thumb_set SPI0_IRQ, DefaultHandler"
module asm ".thumb_set SPI1_IRQ, DefaultHandler"
module asm ".thumb_set UART0_IRQ, DefaultHandler"
module asm ".thumb_set UART1_IRQ, DefaultHandler"
module asm ".thumb_set ADC_IRQ_FIFO, DefaultHandler"
module asm ".thumb_set I2C0_IRQ, DefaultHandler"
module asm ".thumb_set I2C1_IRQ, DefaultHandler"
module asm ".thumb_set OTP_IRQ, DefaultHandler"
module asm ".thumb_set TRNG_IRQ, DefaultHandler"
module asm ".thumb_set PROC0_IRQ_CTI, DefaultHandler"
module asm ".thumb_set PROC1_IRQ_CTI, DefaultHandler"
module asm ".thumb_set PLL_SYS_IRQ, DefaultHandler"
module asm ".thumb_set PLL_USB_IRQ, DefaultHandler"
module asm ".thumb_set POWMAN_IRQ_POW, DefaultHandler"
module asm ".thumb_set POWMAN_IRQ_TIMER, DefaultHandler"
module asm ".text"

