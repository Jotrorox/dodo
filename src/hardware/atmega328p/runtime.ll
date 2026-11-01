; Microchip ATmega328P firmware runtime, compiled by the Dodo compiler for
; every ATmega328P executable (`chip = "atmega328p"` or a board using it,
; such as the Arduino Uno) and linked before the program object.
;
; The data layout comes first so that every function, here and in the
; helper files the compiler appends, is in program memory (address space 1).
;
; It provides, in order of boot:
;   .vectors       The 26 interrupt vectors, one `jmp` each: reset, then the
;                  chip's interrupts in vector order. Every handler is weak,
;                  so an `@interrupt(USART_RX)` function in Dodo replaces it;
;                  unhandled ones go to __bad_interrupt, which halts.
;   __reset        Clears r1 (the compiler's zero register) and SREG, sets
;                  the stack pointer to the end of SRAM, records and clears
;                  the reset flags (MCUSR) and turns off a watchdog left
;                  running, copies .data from flash, zeroes .bss, enables
;                  interrupts as the Cortex-M runtimes leave them, and calls
;                  `main`. If main returns, it sleeps with interrupts off.
;                  __do_copy_data and __do_clear_bss, which the AVR backend
;                  references wherever there is data, name the two loops.
;   TIMER0_OVF     The time base: counts Timer 0 overflows in the 48-bit
;                  dodo_avr_overflows once std/embedded/chip/atmega328p
;                  starts the timer. Weak, so a program can take the vector,
;                  at the cost of the chip package's clock.
;   USART_RX       Moves each received byte into the 32-byte ring buffer
;                  dodo_avr_rx_buffer once std/embedded/chip/atmega328p
;                  enables the receive interrupt, recording framing, parity
;                  and overrun errors and dropped bytes; dodo_avr_rx_take,
;                  dodo_avr_rx_errors and dodo_avr_rx_clear serve the chip
;                  package's serial port. Weak, like TIMER0_OVF.
;   dodo_avr_ticks
;                  The overflow count and TCNT0 as one 56-bit tick count,
;                  read atomically.
;   dodo_avr_delay_loops
;                  Busy-waits 4 cycles per count (1 to 65535).
;   dodo_avr_eeprom_write, dodo_avr_watchdog, dodo_avr_watchdog_feed
;                  The EEPROM and watchdog sequences that must finish within
;                  four cycles, which compiled code cannot promise.
;   dodo_avr_flash_byte
;                  Reads one byte of program memory with LPM, such as a panic
;                  message, which the compiler places there.
;   dodo_avr_reset_cause
;                  MCUSR as found at reset. The Uno's bootloader clears it
;                  before starting the program, so there it reads 0.
;   dodo_board_panic
;                  The panic hook: masks interrupts, passes its arguments to
;                  dodo_panic_report if the program defines it, and halts.
;                  The check and file strings are in program memory.
;                  std/embedded/chip/atmega328p defines that reporter, which
;                  writes the failure to the serial port if the program
;                  opened it. Weak, so a program can define its own hook.
;   abort          What LLVM calls for a trap: halts with interrupts masked.
;
; The integer, memory and atomic helpers LLVM calls on AVR follow from
; src/hardware/avr_builtins.ll, and the floating-point ones from
; src/hardware/avr_float.ll with the shared soft-float code.

target datalayout = "e-P1-p:16:8-i8:8-i16:8-i32:8-i64:8-f32:8-f64:8-n8:16-a:8"
target triple = "avr-none"

module asm ".section .vectors, \22ax\22, @progbits"
module asm ".global __vectors"
module asm "__vectors:"
module asm "    jmp __reset"
module asm "    jmp INT0"
module asm "    jmp INT1"
module asm "    jmp PCINT0"
module asm "    jmp PCINT1"
module asm "    jmp PCINT2"
module asm "    jmp WDT"
module asm "    jmp TIMER2_COMPA"
module asm "    jmp TIMER2_COMPB"
module asm "    jmp TIMER2_OVF"
module asm "    jmp TIMER1_CAPT"
module asm "    jmp TIMER1_COMPA"
module asm "    jmp TIMER1_COMPB"
module asm "    jmp TIMER1_OVF"
module asm "    jmp TIMER0_COMPA"
module asm "    jmp TIMER0_COMPB"
module asm "    jmp TIMER0_OVF"
module asm "    jmp SPI_STC"
module asm "    jmp USART_RX"
module asm "    jmp USART_UDRE"
module asm "    jmp USART_TX"
module asm "    jmp ADC"
module asm "    jmp EE_READY"
module asm "    jmp ANALOG_COMP"
module asm "    jmp TWI"
module asm "    jmp SPM_READY"

module asm ".section .text.__reset, \22ax\22, @progbits"
module asm ".global __reset"
module asm "__reset:"
module asm "    clr r1"
module asm "    out 0x3f, r1"                           ; SREG
module asm "    ldi r28, lo8(__stack)"
module asm "    ldi r29, hi8(__stack)"
module asm "    out 0x3e, r29"                          ; SPH
module asm "    out 0x3d, r28"                          ; SPL
module asm "    wdr"
module asm "    in r2, 0x34"                            ; MCUSR, kept in r2
module asm "    out 0x34, r1"                           ; clears WDRF, which forces WDE on
module asm "    ldi r24, 0x18"                          ; WDCE | WDE
module asm "    sts 0x60, r24"                          ; WDTCSR: timed sequence,
module asm "    sts 0x60, r1"                           ;   watchdog off
module asm ".global __do_copy_data"
module asm "__do_copy_data:"
module asm "    ldi r30, lo8(__data_load)"
module asm "    ldi r31, hi8(__data_load)"
module asm "    ldi r26, lo8(__data_start)"
module asm "    ldi r27, hi8(__data_start)"
module asm "    ldi r24, lo8(__data_end)"
module asm "    ldi r25, hi8(__data_end)"
module asm "    rjmp 2f"
module asm "1:  lpm r0, Z+"
module asm "    st X+, r0"
module asm "2:  cp r26, r24"
module asm "    cpc r27, r25"
module asm "    brne 1b"
module asm ".global __do_clear_bss"
module asm "__do_clear_bss:"
module asm "    ldi r26, lo8(__bss_start)"
module asm "    ldi r27, hi8(__bss_start)"
module asm "    ldi r24, lo8(__bss_end)"
module asm "    ldi r25, hi8(__bss_end)"
module asm "    rjmp 4f"
module asm "3:  st X+, r1"
module asm "4:  cp r26, r24"
module asm "    cpc r27, r25"
module asm "    brne 3b"
module asm "    sts dodo_avr_reset_flags, r2"
module asm "    sei"
module asm "    call main"
module asm "    cli"
module asm "5:  sleep"                                  ; main returned
module asm "    rjmp 5b"

module asm ".section .text.__bad_interrupt, \22ax\22, @progbits"
module asm ".global __bad_interrupt"
module asm "__bad_interrupt:"
module asm "    rjmp __bad_interrupt"

; Timer 0 overflows every 256 ticks; the 48-bit count lasts for millennia.
module asm ".section .text.TIMER0_OVF, \22ax\22, @progbits"
module asm "TIMER0_OVF:"
module asm "    push r24"
module asm "    push r25"
module asm "    in r24, 0x3f"
module asm "    push r24"
module asm "    lds r24, dodo_avr_overflows"
module asm "    lds r25, dodo_avr_overflows+1"
module asm "    adiw r24, 1"
module asm "    sts dodo_avr_overflows, r24"
module asm "    sts dodo_avr_overflows+1, r25"
module asm "    brne 1f"
module asm "    lds r24, dodo_avr_overflows+2"
module asm "    lds r25, dodo_avr_overflows+3"
module asm "    adiw r24, 1"
module asm "    sts dodo_avr_overflows+2, r24"
module asm "    sts dodo_avr_overflows+3, r25"
module asm "    brne 1f"
module asm "    lds r24, dodo_avr_overflows+4"
module asm "    lds r25, dodo_avr_overflows+5"
module asm "    adiw r24, 1"
module asm "    sts dodo_avr_overflows+4, r24"
module asm "    sts dodo_avr_overflows+5, r25"
module asm "1:  pop r24"
module asm "    out 0x3f, r24"
module asm "    pop r25"
module asm "    pop r24"
module asm "    reti"

;
; A handler may interrupt code between a MUL and the instruction clearing
; r1 again, so it uses no zero register.
module asm ".section .text.USART_RX, \22ax\22, @progbits"
module asm "USART_RX:"
module asm "    push r24"
module asm "    in r24, 0x3f"
module asm "    push r24"
module asm "    push r25"
module asm "    push r26"
module asm "    push r27"
module asm "    push r30"
module asm "    push r31"
module asm "    lds r26, 0xc0"                          ; UCSR0A, before UDR0
module asm "    lds r24, 0xc6"                          ; UDR0
module asm "    andi r26, 0x1c"                         ; FE0 | DOR0 | UPE0
module asm "    lds r25, dodo_avr_rx_head"
module asm "    mov r27, r25"
module asm "    inc r27"
module asm "    andi r27, 31"
module asm "    lds r30, dodo_avr_rx_tail"
module asm "    cp r27, r30"
module asm "    brne 1f"
module asm "    ori r26, 0x01"                          ; full: the byte is dropped
module asm "    rjmp 2f"
module asm "1:  mov r30, r25"
module asm "    ldi r31, 0"
module asm "    subi r30, lo8(-(dodo_avr_rx_buffer))"
module asm "    sbci r31, hi8(-(dodo_avr_rx_buffer))"
module asm "    st Z, r24"
module asm "    sts dodo_avr_rx_head, r27"
module asm "2:  tst r26"
module asm "    breq 3f"
module asm "    lds r24, dodo_avr_rx_error_bits"
module asm "    or r24, r26"
module asm "    sts dodo_avr_rx_error_bits, r24"
module asm "3:  pop r31"
module asm "    pop r30"
module asm "    pop r27"
module asm "    pop r26"
module asm "    pop r25"
module asm "    pop r24"
module asm "    out 0x3f, r24"
module asm "    pop r24"
module asm "    reti"

; dodo_avr_rx_take() -> i16: the oldest received byte, or -1 if none. Only
; the handler moves the head and only this the tail, each one byte.
module asm ".section .text.dodo_avr_rx_take, \22ax\22, @progbits"
module asm ".global dodo_avr_rx_take"
module asm "dodo_avr_rx_take:"
module asm "    lds r30, dodo_avr_rx_tail"
module asm "    lds r24, dodo_avr_rx_head"
module asm "    cp r30, r24"
module asm "    breq 1f"
module asm "    mov r25, r30"
module asm "    ldi r31, 0"
module asm "    subi r30, lo8(-(dodo_avr_rx_buffer))"
module asm "    sbci r31, hi8(-(dodo_avr_rx_buffer))"
module asm "    ld r24, Z"
module asm "    inc r25"
module asm "    andi r25, 31"
module asm "    sts dodo_avr_rx_tail, r25"
module asm "    clr r25"
module asm "    ret"
module asm "1:  ldi r24, 0xff"
module asm "    ldi r25, 0xff"
module asm "    ret"

; dodo_avr_rx_errors() -> u8: the error bits since the last call (0x01 a
; dropped byte, 0x04 parity, 0x08 overrun, 0x10 framing), cleared.
module asm ".section .text.dodo_avr_rx_errors, \22ax\22, @progbits"
module asm ".global dodo_avr_rx_errors"
module asm "dodo_avr_rx_errors:"
module asm "    in r0, 0x3f"
module asm "    cli"
module asm "    lds r24, dodo_avr_rx_error_bits"
module asm "    sts dodo_avr_rx_error_bits, r1"
module asm "    out 0x3f, r0"
module asm "    ret"

; dodo_avr_rx_clear(): discard what the buffer holds, and its errors.
module asm ".section .text.dodo_avr_rx_clear, \22ax\22, @progbits"
module asm ".global dodo_avr_rx_clear"
module asm "dodo_avr_rx_clear:"
module asm "    in r0, 0x3f"
module asm "    cli"
module asm "    lds r24, dodo_avr_rx_head"
module asm "    sts dodo_avr_rx_tail, r24"
module asm "    sts dodo_avr_rx_error_bits, r1"
module asm "    out 0x3f, r0"
module asm "    ret"

module asm ".section .bss.dodo_avr_rx, \22aw\22, @nobits"
module asm "dodo_avr_rx_buffer:"
module asm "    .zero 32"
module asm "dodo_avr_rx_head:"
module asm "    .zero 1"
module asm "dodo_avr_rx_tail:"
module asm "    .zero 1"
module asm "dodo_avr_rx_error_bits:"
module asm "    .zero 1"

; dodo_avr_ticks() -> u64: overflows << 8 | TCNT0. An overflow that is
; pending, because interrupts are masked, counts once TCNT0 has wrapped,
; as in the Arduino core's micros().
module asm ".section .text.dodo_avr_ticks, \22ax\22, @progbits"
module asm ".global dodo_avr_ticks"
module asm "dodo_avr_ticks:"
module asm "    in r0, 0x3f"
module asm "    cli"
module asm "    lds r19, dodo_avr_overflows"
module asm "    lds r20, dodo_avr_overflows+1"
module asm "    lds r21, dodo_avr_overflows+2"
module asm "    lds r22, dodo_avr_overflows+3"
module asm "    lds r23, dodo_avr_overflows+4"
module asm "    lds r24, dodo_avr_overflows+5"
module asm "    in r18, 0x26"                           ; TCNT0
module asm "    sbis 0x15, 0"                           ; TIFR0.TOV0
module asm "    rjmp 1f"
module asm "    cpi r18, 255"
module asm "    breq 1f"
module asm "    ldi r25, 1"
module asm "    add r19, r25"
module asm "    adc r20, r1"
module asm "    adc r21, r1"
module asm "    adc r22, r1"
module asm "    adc r23, r1"
module asm "    adc r24, r1"
module asm "1:  out 0x3f, r0"
module asm "    clr r25"
module asm "    ret"

; dodo_avr_delay_loops(count: u16): 4 cycles per count, plus the call.
module asm ".section .text.dodo_avr_delay_loops, \22ax\22, @progbits"
module asm ".global dodo_avr_delay_loops"
module asm "dodo_avr_delay_loops:"
module asm "1:  sbiw r24, 1"
module asm "    brne 1b"
module asm "    ret"

; dodo_avr_eeprom_write(address: u16, value: u8, mode: u8): wait for the
; previous write, then start one in `mode` (EECR's EEPM bits: 0x00 erase
; and write, 0x10 erase only, 0x20 write only). EEPE must follow EEMPE
; within four cycles, so interrupts are masked in between.
module asm ".section .text.dodo_avr_eeprom_write, \22ax\22, @progbits"
module asm ".global dodo_avr_eeprom_write"
module asm "dodo_avr_eeprom_write:"
module asm "1:  sbic 0x1f, 1"                           ; EECR.EEPE
module asm "    rjmp 1b"
module asm "    out 0x22, r25"                          ; EEARH
module asm "    out 0x21, r24"                          ; EEARL
module asm "    out 0x20, r22"                          ; EEDR
module asm "    in r0, 0x3f"
module asm "    cli"
module asm "    out 0x1f, r20"                          ; EECR: mode
module asm "    sbi 0x1f, 2"                            ; EEMPE
module asm "    sbi 0x1f, 1"                            ; EEPE
module asm "    out 0x3f, r0"
module asm "    ret"

; dodo_avr_watchdog(config: u8): set WDTCSR to `config` through the timed
; sequence (WDCE and WDE, then the value within four cycles). A config
; without WDE and WDIE stops the watchdog; WDRF in MCUSR, which would keep
; it running, is cleared first.
module asm ".section .text.dodo_avr_watchdog, \22ax\22, @progbits"
module asm ".global dodo_avr_watchdog"
module asm "dodo_avr_watchdog:"
module asm "    in r0, 0x3f"
module asm "    cli"
module asm "    wdr"
module asm "    in r25, 0x34"                           ; MCUSR
module asm "    andi r25, 0xf7"                         ; clear WDRF
module asm "    out 0x34, r25"
module asm "    lds r25, 0x60"
module asm "    ori r25, 0x18"                          ; WDCE | WDE
module asm "    sts 0x60, r25"
module asm "    sts 0x60, r24"
module asm "    out 0x3f, r0"
module asm "    ret"

; dodo_avr_watchdog_feed()
module asm ".section .text.dodo_avr_watchdog_feed, \22ax\22, @progbits"
module asm ".global dodo_avr_watchdog_feed"
module asm "dodo_avr_watchdog_feed:"
module asm "    wdr"
module asm "    ret"

; dodo_avr_flash_byte(address: u16) -> u8
module asm ".section .text.dodo_avr_flash_byte, \22ax\22, @progbits"
module asm ".global dodo_avr_flash_byte"
module asm "dodo_avr_flash_byte:"
module asm "    movw r30, r24"
module asm "    lpm r24, Z"
module asm "    ret"

; dodo_board_panic(check, file: program memory, line, column: u32): the
; arguments stay in r25 to r14 for the reporter.
module asm ".section .text.dodo_board_panic, \22ax\22, @progbits"
module asm ".weak dodo_board_panic"
module asm ".weak dodo_panic_report"
module asm "dodo_board_panic:"
module asm "    cli"
module asm "    ldi r30, pm_lo8(dodo_panic_report)"
module asm "    ldi r31, pm_hi8(dodo_panic_report)"
module asm "    cp r30, r1"
module asm "    cpc r31, r1"
module asm "    breq 1f"
module asm "    icall"
module asm "    cli"
module asm "1:  rjmp 1b"

module asm ".section .text.abort, \22ax\22, @progbits"
module asm ".global abort"
module asm "abort:"
module asm "    cli"
module asm "1:  rjmp 1b"

module asm ".section .bss.dodo_avr_overflows, \22aw\22, @nobits"
module asm ".global dodo_avr_overflows"
module asm "dodo_avr_overflows:"
module asm "    .zero 6"
module asm ".section .bss.dodo_avr_reset_flags, \22aw\22, @nobits"
module asm ".global dodo_avr_reset_flags"
module asm "dodo_avr_reset_flags:"
module asm "    .zero 1"

; dodo_avr_reset_cause() -> u8, for the chip package.
module asm ".section .text.dodo_avr_reset_cause, \22ax\22, @progbits"
module asm ".global dodo_avr_reset_cause"
module asm "dodo_avr_reset_cause:"
module asm "    lds r24, dodo_avr_reset_flags"
module asm "    ret"

module asm ".weak INT0, INT1, PCINT0, PCINT1, PCINT2, WDT, TIMER2_COMPA, TIMER2_COMPB"
module asm ".weak TIMER2_OVF, TIMER1_CAPT, TIMER1_COMPA, TIMER1_COMPB, TIMER1_OVF, TIMER0_COMPA"
module asm ".weak TIMER0_COMPB, TIMER0_OVF, SPI_STC, USART_RX, USART_UDRE, USART_TX, ADC, EE_READY"
module asm ".weak ANALOG_COMP, TWI, SPM_READY"
module asm ".set INT0, __bad_interrupt"
module asm ".set INT1, __bad_interrupt"
module asm ".set PCINT0, __bad_interrupt"
module asm ".set PCINT1, __bad_interrupt"
module asm ".set PCINT2, __bad_interrupt"
module asm ".set WDT, __bad_interrupt"
module asm ".set TIMER2_COMPA, __bad_interrupt"
module asm ".set TIMER2_COMPB, __bad_interrupt"
module asm ".set TIMER2_OVF, __bad_interrupt"
module asm ".set TIMER1_CAPT, __bad_interrupt"
module asm ".set TIMER1_COMPA, __bad_interrupt"
module asm ".set TIMER1_COMPB, __bad_interrupt"
module asm ".set TIMER1_OVF, __bad_interrupt"
module asm ".set TIMER0_COMPA, __bad_interrupt"
module asm ".set TIMER0_COMPB, __bad_interrupt"
module asm ".set SPI_STC, __bad_interrupt"
module asm ".set USART_UDRE, __bad_interrupt"
module asm ".set USART_TX, __bad_interrupt"
module asm ".set ADC, __bad_interrupt"
module asm ".set EE_READY, __bad_interrupt"
module asm ".set ANALOG_COMP, __bad_interrupt"
module asm ".set TWI, __bad_interrupt"
module asm ".set SPM_READY, __bad_interrupt"
