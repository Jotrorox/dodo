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
;   memcpy, memmove, memset, memcmp, bcmp and the integer division,
;   multiplication, and shift helpers that LLVM calls on Armv6-M, under
;   both their GNU and Arm EABI names. All are weak.
;
; There is no libc or compiler-rt: floating point is not supported yet.
; This module is not optimized by the IR pass pipeline, so the helpers below
; are never rewritten into calls to themselves.

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

attributes #0 = { nounwind "no-builtins" }

; ---------------------------------------------------------------------------
; Memory

define weak ptr @memcpy(ptr %dest, ptr %src, i32 %n) #0 {
entry:
  %d = ptrtoint ptr %dest to i32
  %s = ptrtoint ptr %src to i32
  %either = or i32 %d, %s
  %misaligned = and i32 %either, 3
  %aligned = icmp eq i32 %misaligned, 0
  br i1 %aligned, label %words, label %bytes
words:
  %wi = phi i32 [ 0, %entry ], [ %wi.next, %word ]
  %wleft = sub i32 %n, %wi
  %more = icmp uge i32 %wleft, 4
  br i1 %more, label %word, label %bytes
word:
  %wsrc = getelementptr i8, ptr %src, i32 %wi
  %wdst = getelementptr i8, ptr %dest, i32 %wi
  %wv = load i32, ptr %wsrc, align 4
  store i32 %wv, ptr %wdst, align 4
  %wi.next = add i32 %wi, 4
  br label %words
bytes:
  %bstart = phi i32 [ 0, %entry ], [ %wi, %words ]
  br label %byte.test
byte.test:
  %bi = phi i32 [ %bstart, %bytes ], [ %bi.next, %byte ]
  %bdone = icmp eq i32 %bi, %n
  br i1 %bdone, label %exit, label %byte
byte:
  %bsrc = getelementptr i8, ptr %src, i32 %bi
  %bdst = getelementptr i8, ptr %dest, i32 %bi
  %bv = load i8, ptr %bsrc, align 1
  store i8 %bv, ptr %bdst, align 1
  %bi.next = add i32 %bi, 1
  br label %byte.test
exit:
  ret ptr %dest
}

define weak ptr @memmove(ptr %dest, ptr %src, i32 %n) #0 {
entry:
  %d = ptrtoint ptr %dest to i32
  %s = ptrtoint ptr %src to i32
  %forward = icmp ule i32 %d, %s
  br i1 %forward, label %copy, label %backward.test
copy:
  %r = call ptr @memcpy(ptr %dest, ptr %src, i32 %n)
  ret ptr %dest
backward.test:
  %i = phi i32 [ %n, %entry ], [ %i.next, %backward ]
  %done = icmp eq i32 %i, 0
  br i1 %done, label %exit, label %backward
backward:
  %i.next = sub i32 %i, 1
  %bsrc = getelementptr i8, ptr %src, i32 %i.next
  %bdst = getelementptr i8, ptr %dest, i32 %i.next
  %v = load i8, ptr %bsrc, align 1
  store i8 %v, ptr %bdst, align 1
  br label %backward.test
exit:
  ret ptr %dest
}

define weak ptr @memset(ptr %dest, i32 %c, i32 %n) #0 {
entry:
  %byte = trunc i32 %c to i8
  br label %test
test:
  %i = phi i32 [ 0, %entry ], [ %i.next, %body ]
  %done = icmp eq i32 %i, %n
  br i1 %done, label %exit, label %body
body:
  %p = getelementptr i8, ptr %dest, i32 %i
  store i8 %byte, ptr %p, align 1
  %i.next = add i32 %i, 1
  br label %test
exit:
  ret ptr %dest
}

define weak i32 @memcmp(ptr %a, ptr %b, i32 %n) #0 {
entry:
  br label %test
test:
  %i = phi i32 [ 0, %entry ], [ %i.next, %same ]
  %done = icmp eq i32 %i, %n
  br i1 %done, label %equal, label %body
body:
  %pa = getelementptr i8, ptr %a, i32 %i
  %pb = getelementptr i8, ptr %b, i32 %i
  %va = load i8, ptr %pa, align 1
  %vb = load i8, ptr %pb, align 1
  %eq = icmp eq i8 %va, %vb
  br i1 %eq, label %same, label %differ
same:
  %i.next = add i32 %i, 1
  br label %test
differ:
  %wa = zext i8 %va to i32
  %wb = zext i8 %vb to i32
  %diff = sub i32 %wa, %wb
  ret i32 %diff
equal:
  ret i32 0
}

@bcmp = weak alias i32 (ptr, ptr, i32), ptr @memcmp

define weak void @__aeabi_memcpy(ptr %dest, ptr %src, i32 %n) #0 {
  %r = call ptr @memcpy(ptr %dest, ptr %src, i32 %n)
  ret void
}
@__aeabi_memcpy4 = weak alias void (ptr, ptr, i32), ptr @__aeabi_memcpy
@__aeabi_memcpy8 = weak alias void (ptr, ptr, i32), ptr @__aeabi_memcpy

define weak void @__aeabi_memmove(ptr %dest, ptr %src, i32 %n) #0 {
  %r = call ptr @memmove(ptr %dest, ptr %src, i32 %n)
  ret void
}
@__aeabi_memmove4 = weak alias void (ptr, ptr, i32), ptr @__aeabi_memmove
@__aeabi_memmove8 = weak alias void (ptr, ptr, i32), ptr @__aeabi_memmove

; The EABI variant takes the length before the fill byte.
define weak void @__aeabi_memset(ptr %dest, i32 %n, i32 %c) #0 {
  %r = call ptr @memset(ptr %dest, i32 %c, i32 %n)
  ret void
}
@__aeabi_memset4 = weak alias void (ptr, i32, i32), ptr @__aeabi_memset
@__aeabi_memset8 = weak alias void (ptr, i32, i32), ptr @__aeabi_memset

define weak void @__aeabi_memclr(ptr %dest, i32 %n) #0 {
  %r = call ptr @memset(ptr %dest, i32 0, i32 %n)
  ret void
}
@__aeabi_memclr4 = weak alias void (ptr, i32), ptr @__aeabi_memclr
@__aeabi_memclr8 = weak alias void (ptr, i32), ptr @__aeabi_memclr

; ---------------------------------------------------------------------------
; 32-bit division. Armv6-M has no divide instruction. Restoring shift-subtract
; division, one quotient bit per step; `carry` handles remainders that
; overflow 32 bits when the divisor is at least 2^31. Dodo checks for zero
; divisors and signed overflow before dividing.

define internal { i32, i32 } @udivmod32(i32 %n, i32 %d) #0 {
entry:
  br label %loop
loop:
  %i = phi i32 [ 32, %entry ], [ %i.next, %loop ]
  %num = phi i32 [ %n, %entry ], [ %num.next, %loop ]
  %q = phi i32 [ 0, %entry ], [ %q.next, %loop ]
  %r = phi i32 [ 0, %entry ], [ %r.next, %loop ]
  %top = lshr i32 %num, 31
  %num.next = shl i32 %num, 1
  %carry = icmp slt i32 %r, 0
  %r.shift = shl i32 %r, 1
  %r.in = or i32 %r.shift, %top
  %fits = icmp uge i32 %r.in, %d
  %take = or i1 %carry, %fits
  %r.sub = sub i32 %r.in, %d
  %r.next = select i1 %take, i32 %r.sub, i32 %r.in
  %bit = zext i1 %take to i32
  %q.shift = shl i32 %q, 1
  %q.next = or i32 %q.shift, %bit
  %i.next = sub i32 %i, 1
  %done = icmp eq i32 %i.next, 0
  br i1 %done, label %exit, label %loop
exit:
  %result.q = insertvalue { i32, i32 } poison, i32 %q.next, 0
  %result = insertvalue { i32, i32 } %result.q, i32 %r.next, 1
  ret { i32, i32 } %result
}

; Signed results: the quotient is negative when the signs differ, and the
; remainder takes the dividend's sign.
define internal { i32, i32 } @divmod32(i32 %a, i32 %b) #0 {
  %sa = ashr i32 %a, 31
  %sb = ashr i32 %b, 31
  %xa = xor i32 %a, %sa
  %ua = sub i32 %xa, %sa
  %xb = xor i32 %b, %sb
  %ub = sub i32 %xb, %sb
  %u = call { i32, i32 } @udivmod32(i32 %ua, i32 %ub)
  %uq = extractvalue { i32, i32 } %u, 0
  %ur = extractvalue { i32, i32 } %u, 1
  %sq = xor i32 %sa, %sb
  %xq = xor i32 %uq, %sq
  %q = sub i32 %xq, %sq
  %xr = xor i32 %ur, %sa
  %r = sub i32 %xr, %sa
  %result.q = insertvalue { i32, i32 } poison, i32 %q, 0
  %result = insertvalue { i32, i32 } %result.q, i32 %r, 1
  ret { i32, i32 } %result
}

define weak i32 @__udivsi3(i32 %n, i32 %d) #0 {
  %u = call { i32, i32 } @udivmod32(i32 %n, i32 %d)
  %q = extractvalue { i32, i32 } %u, 0
  ret i32 %q
}
define weak i32 @__umodsi3(i32 %n, i32 %d) #0 {
  %u = call { i32, i32 } @udivmod32(i32 %n, i32 %d)
  %r = extractvalue { i32, i32 } %u, 1
  ret i32 %r
}
define weak i32 @__divsi3(i32 %n, i32 %d) #0 {
  %u = call { i32, i32 } @divmod32(i32 %n, i32 %d)
  %q = extractvalue { i32, i32 } %u, 0
  ret i32 %q
}
define weak i32 @__modsi3(i32 %n, i32 %d) #0 {
  %u = call { i32, i32 } @divmod32(i32 %n, i32 %d)
  %r = extractvalue { i32, i32 } %u, 1
  ret i32 %r
}
; The EABI divmod helpers return the quotient in r0 and remainder in r1.
define weak { i32, i32 } @__aeabi_uidivmod(i32 %n, i32 %d) #0 {
  %u = call { i32, i32 } @udivmod32(i32 %n, i32 %d)
  ret { i32, i32 } %u
}
define weak { i32, i32 } @__aeabi_idivmod(i32 %n, i32 %d) #0 {
  %u = call { i32, i32 } @divmod32(i32 %n, i32 %d)
  ret { i32, i32 } %u
}
@__aeabi_uidiv = weak alias i32 (i32, i32), ptr @__udivsi3
@__aeabi_idiv = weak alias i32 (i32, i32), ptr @__divsi3

; ---------------------------------------------------------------------------
; 64-bit division, the same algorithm. Only constant 64-bit shifts are used,
; which Armv6-M lowers inline.

define internal { i64, i64 } @udivmod64(i64 %n, i64 %d) #0 {
entry:
  br label %loop
loop:
  %i = phi i32 [ 64, %entry ], [ %i.next, %loop ]
  %num = phi i64 [ %n, %entry ], [ %num.next, %loop ]
  %q = phi i64 [ 0, %entry ], [ %q.next, %loop ]
  %r = phi i64 [ 0, %entry ], [ %r.next, %loop ]
  %top = lshr i64 %num, 63
  %num.next = shl i64 %num, 1
  %carry = icmp slt i64 %r, 0
  %r.shift = shl i64 %r, 1
  %r.in = or i64 %r.shift, %top
  %fits = icmp uge i64 %r.in, %d
  %take = or i1 %carry, %fits
  %r.sub = sub i64 %r.in, %d
  %r.next = select i1 %take, i64 %r.sub, i64 %r.in
  %bit = zext i1 %take to i64
  %q.shift = shl i64 %q, 1
  %q.next = or i64 %q.shift, %bit
  %i.next = sub i32 %i, 1
  %done = icmp eq i32 %i.next, 0
  br i1 %done, label %exit, label %loop
exit:
  %result.q = insertvalue { i64, i64 } poison, i64 %q.next, 0
  %result = insertvalue { i64, i64 } %result.q, i64 %r.next, 1
  ret { i64, i64 } %result
}

define internal { i64, i64 } @divmod64(i64 %a, i64 %b) #0 {
  %sa = ashr i64 %a, 63
  %sb = ashr i64 %b, 63
  %xa = xor i64 %a, %sa
  %ua = sub i64 %xa, %sa
  %xb = xor i64 %b, %sb
  %ub = sub i64 %xb, %sb
  %u = call { i64, i64 } @udivmod64(i64 %ua, i64 %ub)
  %uq = extractvalue { i64, i64 } %u, 0
  %ur = extractvalue { i64, i64 } %u, 1
  %sq = xor i64 %sa, %sb
  %xq = xor i64 %uq, %sq
  %q = sub i64 %xq, %sq
  %xr = xor i64 %ur, %sa
  %r = sub i64 %xr, %sa
  %result.q = insertvalue { i64, i64 } poison, i64 %q, 0
  %result = insertvalue { i64, i64 } %result.q, i64 %r, 1
  ret { i64, i64 } %result
}

define weak i64 @__udivdi3(i64 %n, i64 %d) #0 {
  %u = call { i64, i64 } @udivmod64(i64 %n, i64 %d)
  %q = extractvalue { i64, i64 } %u, 0
  ret i64 %q
}
define weak i64 @__umoddi3(i64 %n, i64 %d) #0 {
  %u = call { i64, i64 } @udivmod64(i64 %n, i64 %d)
  %r = extractvalue { i64, i64 } %u, 1
  ret i64 %r
}
define weak i64 @__divdi3(i64 %n, i64 %d) #0 {
  %u = call { i64, i64 } @divmod64(i64 %n, i64 %d)
  %q = extractvalue { i64, i64 } %u, 0
  ret i64 %q
}
define weak i64 @__moddi3(i64 %n, i64 %d) #0 {
  %u = call { i64, i64 } @divmod64(i64 %n, i64 %d)
  %r = extractvalue { i64, i64 } %u, 1
  ret i64 %r
}
; Quotient in r0:r1, remainder in r2:r3.
define weak { i64, i64 } @__aeabi_uldivmod(i64 %n, i64 %d) #0 {
  %u = call { i64, i64 } @udivmod64(i64 %n, i64 %d)
  ret { i64, i64 } %u
}
define weak { i64, i64 } @__aeabi_ldivmod(i64 %n, i64 %d) #0 {
  %u = call { i64, i64 } @divmod64(i64 %n, i64 %d)
  ret { i64, i64 } %u
}

; ---------------------------------------------------------------------------
; 64-bit multiplication from 16x16-bit products, since Armv6-M's `muls`
; keeps only the low 32 bits.

define internal i64 @umul32x32(i32 %x, i32 %y) #0 {
  %x0 = and i32 %x, 65535
  %x1 = lshr i32 %x, 16
  %y0 = and i32 %y, 65535
  %y1 = lshr i32 %y, 16
  %p00 = mul i32 %x0, %y0
  %p01 = mul i32 %x0, %y1
  %p10 = mul i32 %x1, %y0
  %p11 = mul i32 %x1, %y1
  %w00 = zext i32 %p00 to i64
  %w01 = zext i32 %p01 to i64
  %w10 = zext i32 %p10 to i64
  %w11 = zext i32 %p11 to i64
  %s01 = shl i64 %w01, 16
  %s10 = shl i64 %w10, 16
  %s11 = shl i64 %w11, 32
  %a = add i64 %w00, %s01
  %b = add i64 %a, %s10
  %c = add i64 %b, %s11
  ret i64 %c
}

define weak i64 @__muldi3(i64 %a, i64 %b) #0 {
  %al = trunc i64 %a to i32
  %ah64 = lshr i64 %a, 32
  %ah = trunc i64 %ah64 to i32
  %bl = trunc i64 %b to i32
  %bh64 = lshr i64 %b, 32
  %bh = trunc i64 %bh64 to i32
  %low = call i64 @umul32x32(i32 %al, i32 %bl)
  %c1 = mul i32 %al, %bh
  %c2 = mul i32 %ah, %bl
  %cross = add i32 %c1, %c2
  %cross64 = zext i32 %cross to i64
  %high = shl i64 %cross64, 32
  %result = add i64 %low, %high
  ret i64 %result
}
@__aeabi_lmul = weak alias i64 (i64, i64), ptr @__muldi3

; ---------------------------------------------------------------------------
; 64-bit shifts by a variable amount from 0 to 63, built from 32-bit shifts.
; Arm register shifts of 32 or more produce zero, but select keeps the IR
; free of poison regardless.

define weak i64 @__ashldi3(i64 %a, i32 %b) #0 {
  %lo = trunc i64 %a to i32
  %hi64 = lshr i64 %a, 32
  %hi = trunc i64 %hi64 to i32
  %big = icmp uge i32 %b, 32
  %zero = icmp eq i32 %b, 0
  %bm = sub i32 %b, 32
  %big.hi = shl i32 %lo, %bm
  %inv = sub i32 32, %b
  %carry = lshr i32 %lo, %inv
  %hi.shift = shl i32 %hi, %b
  %small.hi.raw = or i32 %hi.shift, %carry
  %small.hi = select i1 %zero, i32 %hi, i32 %small.hi.raw
  %small.lo = shl i32 %lo, %b
  %new.hi = select i1 %big, i32 %big.hi, i32 %small.hi
  %new.lo = select i1 %big, i32 0, i32 %small.lo
  %whi = zext i32 %new.hi to i64
  %wlo = zext i32 %new.lo to i64
  %shi = shl i64 %whi, 32
  %r = or i64 %shi, %wlo
  ret i64 %r
}

define weak i64 @__lshrdi3(i64 %a, i32 %b) #0 {
  %lo = trunc i64 %a to i32
  %hi64 = lshr i64 %a, 32
  %hi = trunc i64 %hi64 to i32
  %big = icmp uge i32 %b, 32
  %zero = icmp eq i32 %b, 0
  %bm = sub i32 %b, 32
  %big.lo = lshr i32 %hi, %bm
  %inv = sub i32 32, %b
  %carry = shl i32 %hi, %inv
  %lo.shift = lshr i32 %lo, %b
  %small.lo.raw = or i32 %lo.shift, %carry
  %small.lo = select i1 %zero, i32 %lo, i32 %small.lo.raw
  %small.hi = lshr i32 %hi, %b
  %new.lo = select i1 %big, i32 %big.lo, i32 %small.lo
  %new.hi = select i1 %big, i32 0, i32 %small.hi
  %whi = zext i32 %new.hi to i64
  %wlo = zext i32 %new.lo to i64
  %shi = shl i64 %whi, 32
  %r = or i64 %shi, %wlo
  ret i64 %r
}

define weak i64 @__ashrdi3(i64 %a, i32 %b) #0 {
  %lo = trunc i64 %a to i32
  %hi64 = lshr i64 %a, 32
  %hi = trunc i64 %hi64 to i32
  %sign = ashr i32 %hi, 31
  %big = icmp uge i32 %b, 32
  %zero = icmp eq i32 %b, 0
  %bm = sub i32 %b, 32
  %big.lo = ashr i32 %hi, %bm
  %inv = sub i32 32, %b
  %carry = shl i32 %hi, %inv
  %lo.shift = lshr i32 %lo, %b
  %small.lo.raw = or i32 %lo.shift, %carry
  %small.lo = select i1 %zero, i32 %lo, i32 %small.lo.raw
  %small.hi = ashr i32 %hi, %b
  %new.lo = select i1 %big, i32 %big.lo, i32 %small.lo
  %new.hi = select i1 %big, i32 %sign, i32 %small.hi
  %whi = zext i32 %new.hi to i64
  %wlo = zext i32 %new.lo to i64
  %shi = shl i64 %whi, 32
  %r = or i64 %shi, %wlo
  ret i64 %r
}
@__aeabi_llsl = weak alias i64 (i64, i32), ptr @__ashldi3
@__aeabi_llsr = weak alias i64 (i64, i32), ptr @__lshrdi3
@__aeabi_lasr = weak alias i64 (i64, i32), ptr @__ashrdi3
