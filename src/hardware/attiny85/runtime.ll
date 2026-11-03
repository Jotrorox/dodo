; Microchip ATtiny85 firmware runtime, compiled by the Dodo compiler for
; every ATtiny85 executable (`chip = "attiny85"` or a board using it, such
; as the Digispark) and linked before the program object.
;
; The data layout comes first so that every function, here and in the
; helper files the compiler appends, is in program memory (address space 1).
;
; The ATtiny85 is an `avr25` core: no MUL instruction (src/hardware/
; avr_mul_soft.ll multiplies), and no JMP or CALL, so the vectors and calls
; are relative. With 8 KiB of flash, a relative jump reaches every address,
; as the program counter wraps around.
;
; It provides, in order of boot:
;   .vectors       The 15 interrupt vectors, one `rjmp` each: reset, then
;                  the chip's interrupts in vector order. Every handler is
;                  weak, so an `@interrupt(PCINT0)` function in Dodo
;                  replaces it; unhandled ones go to __bad_interrupt, which
;                  halts. The micronucleus bootloader of the Digispark moves
;                  the reset and PCINT0 vectors, which must therefore be
;                  rjmp instructions, into a table of its own and keeps them
;                  working.
;   __reset        Clears r1 (the compiler's zero register) and SREG, sets
;                  the stack pointer to the end of SRAM, records and clears
;                  the reset flags (MCUSR) and turns off a watchdog left
;                  running, copies .data from flash, zeroes .bss, enables
;                  interrupts as the other runtimes leave them, and calls
;                  `main`. If main returns, it sleeps with interrupts off.
;                  __do_copy_data and __do_clear_bss, which the AVR backend
;                  references wherever there is data, name the two loops.
;   TIMER0_OVF     The time base: adds the microseconds of each Timer 0
;                  overflow to the 56-bit dodo_avr_micros_count, with a
;                  fraction in 1/256 us, once std/embedded/chip/attiny85
;                  starts the timer and sets the step for its clock with
;                  dodo_avr_set_clock. Weak, so a program can take the
;                  vector, at the cost of the chip package's clock. Counting
;                  microseconds here spares programs the 64-bit
;                  multiplication of a tick count, which costs hundreds of
;                  bytes on a chip without MUL.
;   dodo_avr_micros
;                  The microsecond count plus Timer 0's progress since the
;                  last overflow, read atomically.
;   dodo_avr_delay_loops, dodo_avr_delay_loops32
;                  Busy-wait 4 cycles per count (1 to 65535, or any 32-bit
;                  count, 0 included).
;   dodo_avr_soft_tx
;                  Sends one byte on a pin of port B as a serial frame, 8N1,
;                  timed by counting cycles: the chip has no UART.
;   dodo_avr_eeprom_write, dodo_avr_watchdog, dodo_avr_watchdog_feed
;                  The EEPROM and watchdog sequences that must finish within
;                  four cycles, which compiled code cannot promise.
;   dodo_avr_flash_byte
;                  Reads one byte of program memory with LPM, such as a panic
;                  message, which the compiler places there.
;   dodo_avr_reset_cause
;                  MCUSR as found at reset. Bootloaders that run first, such
;                  as the Digispark's, may have cleared it.
;   dodo_avr_jump_to_zero
;                  Puts the peripherals the chip package uses back to their
;                  reset state and jumps to address 0 with interrupts off:
;                  into a bootloader that took over the reset vector, such
;                  as micronucleus, otherwise to the program's own start.
;   dodo_board_panic
;                  The panic hook: masks interrupts, passes its arguments to
;                  dodo_panic_report if the program defines it, and halts.
;                  The check and file strings are in program memory.
;                  std/embedded/chip/attiny85 defines that reporter, which
;                  writes the failure to the serial port if the program
;                  opened it. Weak, so a program can define its own hook.
;   abort          What LLVM calls for a trap: halts with interrupts masked.
;
; The integer, memory and atomic helpers LLVM calls on AVR follow from
; src/hardware/avr_builtins.ll and avr_mul_soft.ll, and the floating-point
; ones from src/hardware/avr_float.ll with the shared soft-float code.

target datalayout = "e-P1-p:16:8-i8:8-i16:8-i32:8-i64:8-f32:8-f64:8-n8:16-a:8"
target triple = "avr-none"

module asm ".section .vectors, \22ax\22, @progbits"
module asm ".global __vectors"
module asm "__vectors:"
module asm "    rjmp __reset"
module asm "    rjmp INT0"
module asm "    rjmp PCINT0"
module asm "    rjmp TIMER1_COMPA"
module asm "    rjmp TIMER1_OVF"
module asm "    rjmp TIMER0_OVF"
module asm "    rjmp EE_RDY"
module asm "    rjmp ANA_COMP"
module asm "    rjmp ADC"
module asm "    rjmp TIMER1_COMPB"
module asm "    rjmp TIMER0_COMPA"
module asm "    rjmp TIMER0_COMPB"
module asm "    rjmp WDT"
module asm "    rjmp USI_START"
module asm "    rjmp USI_OVF"

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
module asm "    out 0x21, r24"                          ; WDTCR: timed sequence,
module asm "    out 0x21, r1"                           ;   watchdog off
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
module asm "    rcall main"
module asm "    cli"
module asm "5:  sleep"                                  ; main returned
module asm "    rjmp 5b"

module asm ".section .text.__bad_interrupt, \22ax\22, @progbits"
module asm ".global __bad_interrupt"
module asm "__bad_interrupt:"
module asm "    rjmp __bad_interrupt"

; Timer 0 overflows every 256 ticks, 64 cycles each: 992.97 us at
; 16.5 MHz, which the step dodo_avr_overflow_us (992) and its fraction
; dodo_avr_overflow_fraction (248/256) represent. The 56-bit count lasts
; for millennia.
module asm ".section .text.TIMER0_OVF, \22ax\22, @progbits"
module asm "TIMER0_OVF:"
module asm "    push r24"
module asm "    push r25"
module asm "    in r24, 0x3f"
module asm "    push r24"
module asm "    lds r24, dodo_avr_micros_fraction"
module asm "    lds r25, dodo_avr_overflow_fraction"
module asm "    add r24, r25"
module asm "    sts dodo_avr_micros_fraction, r24"
module asm "    lds r24, dodo_avr_micros_count"
module asm "    lds r25, dodo_avr_overflow_us"
module asm "    adc r24, r25"
module asm "    sts dodo_avr_micros_count, r24"
module asm "    lds r24, dodo_avr_micros_count+1"
module asm "    lds r25, dodo_avr_overflow_us+1"
module asm "    adc r24, r25"
module asm "    sts dodo_avr_micros_count+1, r24"
module asm "    brcc 1f"
module asm "    lds r24, dodo_avr_micros_count+2"
module asm "    inc r24"
module asm "    sts dodo_avr_micros_count+2, r24"
module asm "    brne 1f"
module asm "    lds r24, dodo_avr_micros_count+3"
module asm "    inc r24"
module asm "    sts dodo_avr_micros_count+3, r24"
module asm "    brne 1f"
module asm "    lds r24, dodo_avr_micros_count+4"
module asm "    inc r24"
module asm "    sts dodo_avr_micros_count+4, r24"
module asm "    brne 1f"
module asm "    lds r24, dodo_avr_micros_count+5"
module asm "    inc r24"
module asm "    sts dodo_avr_micros_count+5, r24"
module asm "    brne 1f"
module asm "    lds r24, dodo_avr_micros_count+6"
module asm "    inc r24"
module asm "    sts dodo_avr_micros_count+6, r24"
module asm "1:  pop r24"
module asm "    out 0x3f, r24"
module asm "    pop r25"
module asm "    pop r24"
module asm "    reti"

; dodo_avr_set_clock(overflow_us: u16, fraction: u8, tick_scale: u16): the
; microseconds per Timer 0 overflow, in whole and 1/256 us, and per tick in
; 1/256 us.
module asm ".section .text.dodo_avr_set_clock, \22ax\22, @progbits"
module asm ".global dodo_avr_set_clock"
module asm "dodo_avr_set_clock:"
module asm "    in r0, 0x3f"
module asm "    cli"
module asm "    sts dodo_avr_overflow_us, r24"
module asm "    sts dodo_avr_overflow_us+1, r25"
module asm "    sts dodo_avr_overflow_fraction, r22"
module asm "    sts dodo_avr_tick_scale, r20"
module asm "    sts dodo_avr_tick_scale+1, r21"
module asm "    out 0x3f, r0"
module asm "    ret"

; dodo_avr_micros() -> u64: the count, plus TCNT0 * tick_scale / 256. An
; overflow that is pending, because interrupts are masked, counts once
; TCNT0 has wrapped, as in the Arduino core's micros(). The product is
; formed from TCNT0's lowest bit up, halving the sum each step; a set bit
; shifted in on top ends the loop after eight.
module asm ".section .text.dodo_avr_micros, \22ax\22, @progbits"
module asm ".global dodo_avr_micros"
module asm "dodo_avr_micros:"
module asm "    in r0, 0x3f"
module asm "    cli"
module asm "    lds r18, dodo_avr_micros_count"
module asm "    lds r19, dodo_avr_micros_count+1"
module asm "    lds r20, dodo_avr_micros_count+2"
module asm "    lds r21, dodo_avr_micros_count+3"
module asm "    lds r22, dodo_avr_micros_count+4"
module asm "    lds r23, dodo_avr_micros_count+5"
module asm "    lds r24, dodo_avr_micros_count+6"
module asm "    clr r25"
module asm "    in r26, 0x32"                           ; TCNT0
module asm "    sbis 0x38, 1"                           ; TIFR.TOV0
module asm "    rjmp 1f"
module asm "    cpi r26, 255"
module asm "    breq 1f"
module asm "    lds r30, dodo_avr_overflow_us"
module asm "    lds r31, dodo_avr_overflow_us+1"
module asm "    add r18, r30"
module asm "    adc r19, r31"
module asm "    adc r20, r1"
module asm "    adc r21, r1"
module asm "    adc r22, r1"
module asm "    adc r23, r1"
module asm "    adc r24, r1"
module asm "1:  out 0x3f, r0"
module asm "    lds r30, dodo_avr_tick_scale"
module asm "    lds r31, dodo_avr_tick_scale+1"
module asm "    clr r0"
module asm "    clr r27"
module asm "    sec"
module asm "    ror r26"
module asm "2:  brcc 3f"
module asm "    add r0, r30"
module asm "    adc r27, r31"
module asm "3:  ror r27"
module asm "    ror r0"
module asm "    lsr r26"
module asm "    brne 2b"
module asm "    add r18, r0"
module asm "    adc r19, r27"
module asm "    adc r20, r1"
module asm "    adc r21, r1"
module asm "    adc r22, r1"
module asm "    adc r23, r1"
module asm "    adc r24, r1"
module asm "    clr r0"
module asm "    ret"

; dodo_avr_delay_loops(count: u16): 4 cycles per count, plus the call.
module asm ".section .text.dodo_avr_delay_loops, \22ax\22, @progbits"
module asm ".global dodo_avr_delay_loops"
module asm "dodo_avr_delay_loops:"
module asm "1:  sbiw r24, 1"
module asm "    brne 1b"
module asm "    ret"

; dodo_avr_delay_loops32(count: u32): 4 cycles per count, plus a few: the
; low half's loops, then 65536 for each unit of the high half.
module asm ".section .text.dodo_avr_delay_loops32, \22ax\22, @progbits"
module asm ".global dodo_avr_delay_loops32"
module asm "dodo_avr_delay_loops32:"
module asm "    movw r30, r22"
module asm "    adiw r30, 0"
module asm "    breq 2f"
module asm "1:  sbiw r30, 1"
module asm "    brne 1b"
module asm "2:  adiw r24, 0"
module asm "    breq 4f"
module asm "3:  sbiw r30, 1"
module asm "    brne 3b"
module asm "    sbiw r24, 1"
module asm "    brne 3b"
module asm "4:  ret"

; dodo_avr_soft_tx(byte: u8, mask: u8, loops: u16): send `byte` on the
; port B pins in `mask` (one, configured as an output, idle high): a start
; bit, eight data bits from the lowest, and a stop bit, each 4 * loops + 13
; cycles long. Interrupts are masked from the start bit until the stop bit
; begins, which an interrupt handler may then lengthen harmlessly; the
; other port B outputs keep their levels from the start of the frame.
module asm ".section .text.dodo_avr_soft_tx, \22ax\22, @progbits"
module asm ".global dodo_avr_soft_tx"
module asm "dodo_avr_soft_tx:"
module asm "    in r0, 0x3f"
module asm "    cli"
module asm "    in r18, 0x18"                           ; PORTB
module asm "    mov r19, r22"
module asm "    com r19"
module asm "    ldi r25, 1"                             ; frame: stop, data, start
module asm "    lsl r24"
module asm "    rol r25"
module asm "    ldi r23, 10"
module asm "1:  mov r26, r18"
module asm "    and r26, r19"
module asm "    sbrc r24, 0"
module asm "    or r26, r22"
module asm "    out 0x18, r26"                          ; the bit starts
module asm "    cpi r23, 1"
module asm "    brne 2f"
module asm "    out 0x3f, r0"                           ; the stop bit: interrupts as before
module asm "2:  movw r30, r20"
module asm "3:  sbiw r30, 1"
module asm "    brne 3b"
module asm "    lsr r25"
module asm "    ror r24"
module asm "    dec r23"
module asm "    brne 1b"
module asm "    ret"

; dodo_avr_eeprom_write(address: u16, value: u8, mode: u8): wait for the
; previous write, then start one in `mode` (EECR's EEPM bits: 0x00 erase
; and write, 0x10 erase only, 0x20 write only). EEPE must follow EEMPE
; within four cycles, so interrupts are masked in between.
module asm ".section .text.dodo_avr_eeprom_write, \22ax\22, @progbits"
module asm ".global dodo_avr_eeprom_write"
module asm "dodo_avr_eeprom_write:"
module asm "1:  sbic 0x1c, 1"                           ; EECR.EEPE
module asm "    rjmp 1b"
module asm "    out 0x1f, r25"                          ; EEARH
module asm "    out 0x1e, r24"                          ; EEARL
module asm "    out 0x1d, r22"                          ; EEDR
module asm "    in r0, 0x3f"
module asm "    cli"
module asm "    out 0x1c, r20"                          ; EECR: mode
module asm "    sbi 0x1c, 2"                            ; EEMPE
module asm "    sbi 0x1c, 1"                            ; EEPE
module asm "    out 0x3f, r0"
module asm "    ret"

; dodo_avr_watchdog(config: u8): set WDTCR to `config` through the timed
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
module asm "    in r25, 0x21"                           ; WDTCR
module asm "    ori r25, 0x18"                          ; WDCE | WDE
module asm "    out 0x21, r25"
module asm "    out 0x21, r24"
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

; dodo_avr_jump_to_zero(): with interrupts off, stop the watchdog, the
; timers, the ADC, the USI, the pin change and external interrupts and
; their pending flags, release every pin, and jump to address 0, so that
; what runs there finds the chip much as after a reset. Does not return.
module asm ".section .text.dodo_avr_jump_to_zero, \22ax\22, @progbits"
module asm ".global dodo_avr_jump_to_zero"
module asm "dodo_avr_jump_to_zero:"
module asm "    cli"
module asm "    clr r1"
module asm "    ldi r24, 0"
module asm "    rcall dodo_avr_watchdog"
module asm "    out 0x39, r1"                           ; TIMSK
module asm "    out 0x3b, r1"                           ; GIMSK
module asm "    out 0x15, r1"                           ; PCMSK
module asm "    out 0x35, r1"                           ; MCUCR
module asm "    out 0x2a, r1"                           ; TCCR0A
module asm "    out 0x33, r1"                           ; TCCR0B
module asm "    out 0x32, r1"                           ; TCNT0
module asm "    out 0x30, r1"                           ; TCCR1
module asm "    out 0x2c, r1"                           ; GTCCR
module asm "    out 0x2f, r1"                           ; TCNT1
module asm "    out 0x06, r1"                           ; ADCSRA
module asm "    out 0x07, r1"                           ; ADMUX
module asm "    out 0x0d, r1"                           ; USICR
module asm "    out 0x17, r1"                           ; DDRB
module asm "    out 0x18, r1"                           ; PORTB
module asm "    out 0x20, r1"                           ; PRR
module asm "    ldi r24, 0xff"
module asm "    out 0x38, r24"                          ; TIFR: clear pending flags
module asm "    out 0x3a, r24"                          ; GIFR
module asm "    ldi r24, 0xf0"
module asm "    out 0x0e, r24"                          ; USISR
module asm "    rjmp __vectors"

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

module asm ".section .bss.dodo_avr_micros, \22aw\22, @nobits"
module asm ".global dodo_avr_micros_count"
module asm "dodo_avr_micros_count:"
module asm "    .zero 7"
module asm "dodo_avr_micros_fraction:"
module asm "    .zero 1"
module asm "dodo_avr_overflow_us:"
module asm "    .zero 2"
module asm "dodo_avr_overflow_fraction:"
module asm "    .zero 1"
module asm "dodo_avr_tick_scale:"
module asm "    .zero 2"
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

module asm ".weak INT0, PCINT0, TIMER1_COMPA, TIMER1_OVF, TIMER0_OVF, EE_RDY, ANA_COMP, ADC"
module asm ".weak TIMER1_COMPB, TIMER0_COMPA, TIMER0_COMPB, WDT, USI_START, USI_OVF"
module asm ".set INT0, __bad_interrupt"
module asm ".set PCINT0, __bad_interrupt"
module asm ".set TIMER1_COMPA, __bad_interrupt"
module asm ".set TIMER1_OVF, __bad_interrupt"
module asm ".set EE_RDY, __bad_interrupt"
module asm ".set ANA_COMP, __bad_interrupt"
module asm ".set ADC, __bad_interrupt"
module asm ".set TIMER1_COMPB, __bad_interrupt"
module asm ".set TIMER0_COMPA, __bad_interrupt"
module asm ".set TIMER0_COMPB, __bad_interrupt"
module asm ".set WDT, __bad_interrupt"
module asm ".set USI_START, __bad_interrupt"
module asm ".set USI_OVF, __bad_interrupt"
