; Runtime for programs on the ESP32-C6's low-power (LP) RISC-V core
; (RV32IMAC), compiled by the Dodo compiler for every `chip = "esp32c6_lp"`
; program and linked before the program object. Such a program is the LP
; core's: an ESP32-C6 firmware embeds its image (`lp-core = "PATH"`) and
; esp32c6.LpCore copies it to LP SRAM at 0x50000000 and starts the core.
;
; The core starts at 0x50000080 every time the PMU wakes it: at the start
; esp32c6.LpCore asks for, and at each wake of its LP timer period. Its
; statics keep their values between wakes, as nothing reinitializes them:
; the loader put .data in place and zeroed the rest.
;
; It provides:
;   .lp_vectors    The vectored trap table mtvec points at, at 0x50000000:
;                  every exception and interrupt enters dodo_lp_trap.
;   Reset_Handler  Masks interrupts, sets the stack pointer and mtvec,
;                  acknowledges what woke the core (the main core's
;                  trigger and the LP timer, levels that would wake it again
;                  at once), counts the start in the shared area's RUNS
;                  word, and
;                  calls `main`. When `main` returns, the core halts
;                  (dodo_lp_halt) until its next wake.
;   dodo_lp_halt   Arms the LP timer's target 1 for the next wake, if the
;                  shared area's PERIOD words ask for one (LP timer counts,
;                  low word first), and asks the PMU to put the core to
;                  sleep (PMU LP_CPU_PWR1.SLEEP_REQ). Does not return: the
;                  next wake restarts the core at Reset_Handler.
;   dodo_lp_trap   Calls Exception_Handler(mcause, mepc, mtval); its weak
;                  default reports through the panic hook as `processor
;                  exception check failed at mcause:mepc:CAUSE:PC`.
;   dodo_lp_panic  The panic hook, weak: records its arguments in the shared
;                  area (the PANIC words), stops the core's wakeups, and
;                  halts it. The strings stay in LP SRAM, where the main
;                  core reads them (esp32c6.LpCore.panic_message).
;
; The shared area is the 1 KiB of LP SRAM at 0x50003000 that both cores
; use; std/embedded/chip/esp_lp_shared lays it out. The memory, integer,
; and floating-point helpers follow from src/hardware/arm_eabi.ll,
; arm_float.ll, and arm_float_soft.ll, as on the main core.

module asm ".section .lp_vectors, \22ax\22, @progbits"
module asm ".global __lp_vector_table"
module asm "__lp_vector_table:"
module asm ".option push"
module asm ".option norvc"
module asm "    .rept 32"
module asm "    j dodo_lp_trap"
module asm "    .endr"
module asm ".option pop"

module asm ".section .text.Reset_Handler, \22ax\22, @progbits"
module asm ".global Reset_Handler"
module asm ".type Reset_Handler, @function"
module asm "Reset_Handler:"
module asm "    csrci mstatus, 8"                         ; mask interrupts
module asm "    csrw mie, zero"
module asm "    lui sp, %hi(__stack_top)"
module asm "    addi sp, sp, %lo(__stack_top)"
module asm "    lui t0, %hi(__lp_vector_table)"
module asm "    addi t0, t0, %lo(__lp_vector_table)"
module asm "    ori t0, t0, 1"                            ; vectored mode
module asm "    csrw mtvec, t0"
module asm "    li t0, 0x600b0000"                        ; PMU LP_INT_CLR.HP_SW_TRIGGER:
module asm "    li t1, 0x80000000"                        ; the main core's start, a
module asm "    sw t1, 0x178(t0)"                         ; level that would wake again
module asm "    li t0, 0x600b0c00"                        ; LP_TIMER LP_INT_CLR.MAIN_TIMER:
module asm "    sw t1, 0x44(t0)"                          ; the period's wake, likewise
module asm "    li t0, 0x50003000"                        ; shared area: RUNS += 1
module asm "    lw t1, 0(t0)"
module asm "    addi t1, t1, 1"
module asm "    sw t1, 0(t0)"
module asm "    call main"
module asm "    tail dodo_lp_halt"

module asm ".section .text.dodo_lp_halt, \22ax\22, @progbits"
module asm ".global dodo_lp_halt"
module asm ".type dodo_lp_halt, @function"
module asm "dodo_lp_halt:"
module asm "    li t0, 0x50003000"
module asm "    lw a0, 4(t0)"                             ; PERIOD, low
module asm "    lw a1, 8(t0)"                             ; PERIOD, high
module asm "    or t1, a0, a1"
module asm "    beqz t1, 2f"
module asm "    li t2, 0x600b0c00"                        ; LP_TIMER
module asm "    li t3, 0x10000000"                        ; UPDATE.MAIN_TIMER_UPDATE
module asm "    sw t3, 0x10(t2)"
module asm "    lw a2, 0x14(t2)"                          ; MAIN_BUF0_LOW
module asm "    lw a3, 0x18(t2)"                          ; MAIN_BUF0_HIGH
module asm "    lui t4, 0x10"
module asm "    addi t4, t4, -1"                          ; 0xffff
module asm "    and a3, a3, t4"
module asm "    add a4, a2, a0"                           ; target = count + period
module asm "    sltu t5, a4, a2"
module asm "    add a5, a3, a1"
module asm "    add a5, a5, t5"
module asm "    and a5, a5, t4"
module asm "    li t6, 0x80000000"
module asm "    sw t6, 0x44(t2)"                          ; LP_INT_CLR.MAIN_TIMER
module asm "    sw a4, 0x08(t2)"                          ; TAR1_LOW
module asm "    sw a5, 0x0c(t2)"                          ; TAR1_HIGH
module asm "    or a5, a5, t6"
module asm "    sw a5, 0x0c(t2)"                          ; ... compare
module asm "2:  li t0, 0x600b0000"                        ; PMU LP_CPU_PWR1.SLEEP_REQ
module asm "    lw t1, 0x180(t0)"
module asm "    li t2, 0x80000000"
module asm "    or t1, t1, t2"
module asm "    sw t1, 0x180(t0)"
module asm "3:  j 3b"

; The caller-saved registers in a 64-byte frame; if Exception_Handler
; returns, the faulting instruction runs again.
module asm ".section .text.dodo_lp_trap, \22ax\22, @progbits"
module asm ".global dodo_lp_trap"
module asm ".type dodo_lp_trap, @function"
module asm "dodo_lp_trap:"
module asm "    addi sp, sp, -64"
module asm "    sw ra, 0(sp)"
module asm "    sw t0, 4(sp)"
module asm "    sw t1, 8(sp)"
module asm "    sw t2, 12(sp)"
module asm "    sw a0, 16(sp)"
module asm "    sw a1, 20(sp)"
module asm "    sw a2, 24(sp)"
module asm "    sw a3, 28(sp)"
module asm "    sw a4, 32(sp)"
module asm "    sw a5, 36(sp)"
module asm "    sw a6, 40(sp)"
module asm "    sw a7, 44(sp)"
module asm "    sw t3, 48(sp)"
module asm "    sw t4, 52(sp)"
module asm "    sw t5, 56(sp)"
module asm "    sw t6, 60(sp)"
module asm "    csrr a0, mcause"
module asm "    csrr a1, mepc"
module asm "    csrr a2, mtval"
module asm "    call Exception_Handler"
module asm "    lw ra, 0(sp)"
module asm "    lw t0, 4(sp)"
module asm "    lw t1, 8(sp)"
module asm "    lw t2, 12(sp)"
module asm "    lw a0, 16(sp)"
module asm "    lw a1, 20(sp)"
module asm "    lw a2, 24(sp)"
module asm "    lw a3, 28(sp)"
module asm "    lw a4, 32(sp)"
module asm "    lw a5, 36(sp)"
module asm "    lw a6, 40(sp)"
module asm "    lw a7, 44(sp)"
module asm "    lw t3, 48(sp)"
module asm "    lw t4, 52(sp)"
module asm "    lw t5, 56(sp)"
module asm "    lw t6, 60(sp)"
module asm "    addi sp, sp, 64"
module asm "    mret"

module asm ".section .text.dodo_lp_exception, \22ax\22, @progbits"
module asm ".type dodo_lp_exception, @function"
module asm "dodo_lp_exception:"
module asm "    mv a3, a1"                                ; column: the faulting PC
module asm "    mv a2, a0"                                ; line: mcause
module asm "    lui a0, %hi(dodo_lp_exception_check)"
module asm "    addi a0, a0, %lo(dodo_lp_exception_check)"
module asm "    lui a1, %hi(dodo_lp_exception_file)"
module asm "    addi a1, a1, %lo(dodo_lp_exception_file)"
module asm "    tail dodo_lp_panic"
module asm ".section .rodata.dodo_lp_exception_text, \22a\22, @progbits"
module asm "dodo_lp_exception_check:"
module asm "    .asciz \22processor exception\22"
module asm "dodo_lp_exception_file:"
module asm "    .asciz \22mcause:mepc\22"
module asm ".weak Exception_Handler"
module asm ".set Exception_Handler, dodo_lp_exception"

; dodo_lp_panic(check, file, line, column): the PANIC words of the shared
; area, then no more wakes (LP_CPU_PWR1.WAKEUP_EN 0) and sleep.
module asm ".section .text.dodo_lp_panic, \22ax\22, @progbits"
module asm ".weak dodo_lp_panic"
module asm ".type dodo_lp_panic, @function"
module asm "dodo_lp_panic:"
module asm "    li t0, 0x50003000"
module asm "    sw a0, 0x14(t0)"                          ; PANIC_CHECK
module asm "    sw a1, 0x18(t0)"                          ; PANIC_FILE
module asm "    sw a2, 0x1c(t0)"                          ; PANIC_LINE
module asm "    sw a3, 0x20(t0)"                          ; PANIC_COLUMN
module asm "    li t1, 0x50414e43"                        ; PANIC: 'PANC'
module asm "    sw t1, 0x10(t0)"
module asm "    li t2, 0x600b0000"
module asm "    li t3, 0x80000000"
module asm "    sw t3, 0x180(t2)"                         ; LP_CPU_PWR1
module asm "1:  j 1b"
