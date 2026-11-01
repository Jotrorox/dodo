; RP2350 binary64 helpers on the double-precision coprocessor (DCP). The
; compiler appends this file after arm_float.ll for the RP2350 (see
; `src/hardware/CHIP.rs`), in place of arm_float_soft.ll, so `f64`
; arithmetic and conversions run on the coprocessor with no change to
; programs. Reset_Handler in runtime.ll enables coprocessor 4.
;
; Each Cortex-M33 core has its own DCP (RP2350 datasheet 3.6.2). An operation
; is a fixed ("canned") sequence of coprocessor instructions: write the
; operands, run a few steps, read the result. The sequences, and the state
; save and restore, are the Pico SDK's (hardware_dcp/dcp_canned.inc.S and
; pico_double/double_aeabi_dcp.S, copyright 2020 Raspberry Pi (Trading)
; Ltd., BSD-3-Clause; see LICENSE.pico-sdk next to this file); dmul and ddiv
; do their long multiplications on the CPU in between. The helpers
; are assembly, as the coprocessor instructions have no IR form. Cycles per
; call at 150 MHz with -O2 on everyday operands, measured on a board: dadd
; 43, dmul 62, ddiv 89, i2d 19, d2iz 27, d2f 25. The software helpers take
; about 150, 205, 340, 45, 30 and 50, as the emulator counts them (it was
; within 10% of the board for the software helpers' first version).
;
; The results are the software helpers' bit for bit. The DCP rounds to
; nearest, ties to even, like them, but flushes subnormal operands and
; results to zero and passes NaN payloads through. So a subnormal operand,
; or a result below 2^-1021 (where a flushed subnormal would show up), is
; computed again in software, and a NaN result becomes the default NaN.
; Conversions to integers return 0 for NaN, as in arm_float.ll. f2d stays in
; software, which is as fast.
;
; Interrupts: the DCP sets its "engaged" flag at the start of a sequence and
; clears it on the final read. A helper that finds it set has interrupted a
; sequence on the same core, so it saves the DCP's six words of state first
; and restores them, flag included, when it returns. The helpers are then
; safe in interrupt handlers, at the cost of one coprocessor read and a
; branch per call. tests/hardware/rp2350_float checks all of this on a
; board, against the software helpers.

; DCP instructions (RP2350 datasheet 3.6.2.5), as in the Pico SDK's
; hardware_dcp/dcp_instr.inc.S.
module asm ".macro WXMD rl, rh"
module asm "    mcrr p4, #0, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro WYMD rl, rh"
module asm "    mcrr p4, #0, \5Crl, \5Crh, c1"
module asm ".endm"
module asm ".macro WEFD rl, rh"
module asm "    mcrr p4, #0, \5Crl, \5Crh, c2"
module asm ".endm"
module asm ".macro WXUP rl, rh"
module asm "    mcrr p4, #1, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro WYUP rl, rh"
module asm "    mcrr p4, #1, \5Crl, \5Crh, c1"
module asm ".endm"
module asm ".macro WXMS rl, rh"
module asm "    mcrr p4, #2, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro WXMO rl, rh"
module asm "    mcrr p4, #3, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro WXDD rl, rh"
module asm "    mcrr p4, #4, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro WXUC rl, rh"
module asm "    mcrr p4, #6, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro WXIC rl, rh"
module asm "    mcrr p4, #7, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro WXDC rl, rh"
module asm "    mcrr p4, #8, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro ADD0"
module asm "    cdp p4, #0, c0, c0, c1, #0"
module asm ".endm"
module asm ".macro ADD1"
module asm "    cdp p4, #1, c0, c0, c1, #0"
module asm ".endm"
module asm ".macro SUB1"
module asm "    cdp p4, #1, c0, c0, c1, #1"
module asm ".endm"
module asm ".macro NRDF"
module asm "    cdp p4, #8, c0, c0, c2, #1"
module asm ".endm"
module asm ".macro NRDD"
module asm "    cdp p4, #8, c0, c0, c0, #1"
module asm ".endm"
module asm ".macro NTDC"
module asm "    cdp p4, #8, c0, c0, c0, #2"
module asm ".endm"
module asm ".macro RDFG rt"
module asm "    mrc p4, #0, \5Crt, c0, c2, #5"
module asm ".endm"
module asm ".macro RDIC rt"
module asm "    mrc p4, #0, \5Crt, c0, c3, #0"
module asm ".endm"
module asm ".macro RDUC rt"
module asm "    mrc p4, #0, \5Crt, c0, c3, #1"
module asm ".endm"
module asm ".macro REFD rl, rh"
module asm "    mrrc p4, #0, \5Crl, \5Crh, c10"
module asm ".endm"
module asm ".macro RXMS rl, rh, s"
module asm "    mrrc p4, #\5Cs, \5Crl, \5Crh, c4"
module asm ".endm"
module asm ".macro RYMS rl, rh, s"
module asm "    mrrc p4, #\5Cs, \5Crl, \5Crh, c5"
module asm ".endm"
module asm ".macro RYMR rl, rh"
module asm "    mrrc p4, #2, \5Crl, \5Crh, c1"
module asm ".endm"
module asm ".macro RDDA rl, rh"
module asm "    mrrc p4, #1, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro RDDS rl, rh"
module asm "    mrrc p4, #3, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro RDDM rl, rh"
module asm "    mrrc p4, #5, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro RDDD rl, rh"
module asm "    mrrc p4, #7, \5Crl, \5Crh, c0"
module asm ".endm"
module asm ".macro PCMP rt"
module asm "    mrc2 p4, #0, \5Crt, c0, c0, #1"
module asm ".endm"
module asm ".macro PXMD rl, rh"
module asm "    mrrc2 p4, #0, \5Crl, \5Crh, c8"
module asm ".endm"
module asm ".macro PYMD rl, rh"
module asm "    mrrc2 p4, #0, \5Crl, \5Crh, c9"
module asm ".endm"

; The start of a helper. The entry point checks the engaged flag (bit 31 of
; the status, into N); when it is set, the helper has interrupted a
; sequence on the same core, so it branches back to save the coprocessor
; state, which returns to the body with the return address pointing at the
; restore code.
module asm ".macro helper name"
module asm "    .section .text.\5Cname, \22ax\22, %progbits"
module asm "    .weak \5Cname"
module asm "    .type \5Cname, %function"
module asm "    .p2align 2"
module asm "1:  push {lr}"
module asm "    bl dodo_dcp_save"
module asm "    b 2f"
module asm "    .thumb_func"
module asm "\5Cname:"
module asm "    PCMP apsr_nzcv"
module asm "    bmi 1b"
module asm "2:"
module asm ".endm"

; Software for a subnormal operand: one whose high word, without the sign,
; is below 2^20 (exponent field 0) while the operand is not zero.
module asm ".macro check_operand lo, hi, soft"
module asm "    lsls r12, \5Chi, #1"
module asm "    cmp r12, #0x00200000"
module asm "    bhs 3f"
module asm "    orrs r12, r12, \5Clo"
module asm "    beq 3f"
module asm "    b \5Csoft"
module asm "3:"
module asm ".endm"

; Results whose exponent field is 2 to 2046 return; the others branch to
; \edge: 0 or 1 (perhaps a flushed subnormal) and 2047 (infinity or NaN).
module asm ".macro check_result hi, edge"
module asm "    lsls r12, \5Chi, #1"
module asm "    cmp r12, #0x00400000"
module asm "    blo \5Cedge"
module asm "    cmn r12, #0x00200000"
module asm "    bhs \5Cedge"
module asm ".endm"

; The rest of check_result: infinity stands, NaN becomes the default NaN,
; and tiny results branch to \tiny.
module asm ".macro edge_result tiny"
module asm "    lsls r12, r1, #1"
module asm "    cmp r12, #0x00400000"
module asm "    blo \5Ctiny"
module asm "    orrs r12, r0, r1, lsl #12"
module asm "    ittt ne"
module asm "    movne r0, #0"
module asm "    movwne r1, #0"
module asm "    movtne r1, #0x7ff8"
module asm ".endm"

; ---------------------------------------------------------------------------
; State save and restore for a helper that interrupted another, as in the
; Pico SDK's pico_double: the six words of state go on the stack, below the
; helper's return address, and the helper returns through the restore code.
module asm ".section .text.dodo_dcp_save, \22ax\22, %progbits"
module asm ".type dodo_dcp_save, %function"
module asm ".thumb_func"
module asm "dodo_dcp_save:"
module asm "    sub sp, #24"
module asm "    push {r0, r1}"
module asm "    PXMD r0, r1"
module asm "    strd r0, r1, [sp, #8]"
module asm "    PYMD r0, r1"
module asm "    strd r0, r1, [sp, #16]"
module asm "    REFD r0, r1"
module asm "    strd r0, r1, [sp, #24]"
module asm "    pop {r0, r1}"
module asm "    blx lr"
; The helper returns here.
module asm "    pop {r12, lr}"
module asm "    WXMD r12, lr"
module asm "    pop {r12, lr}"
module asm "    WYMD r12, lr"
module asm "    pop {r12, lr}"
module asm "    WEFD r12, lr"
module asm "    pop {pc}"

; ---------------------------------------------------------------------------
; Arithmetic. The operands stay on the stack for a result computed again in
; software.

module asm "helper __aeabi_dadd"
module asm "    check_operand r0, r1, dodo_soft_dadd"
module asm "    check_operand r2, r3, dodo_soft_dadd"
module asm "    push {r0, r1}"
module asm "    WXUP r0, r1"
module asm "    WYUP r2, r3"
module asm "    ADD0"
module asm "    ADD1"
module asm "    NRDD"
module asm "    RDDA r0, r1"
module asm "    check_result r1, 4f"
module asm "    add sp, #8"
module asm "    bx lr"
module asm "4:  edge_result 5f"
module asm "    add sp, #8"
module asm "    bx lr"
module asm "5:  pop {r0, r1}"
module asm "    b dodo_soft_dadd"

module asm "helper __aeabi_dsub"
module asm "    check_operand r0, r1, dodo_soft_dsub"
module asm "    check_operand r2, r3, dodo_soft_dsub"
module asm "    push {r0, r1}"
module asm "    WXUP r0, r1"
module asm "    WYUP r2, r3"
module asm "    ADD0"
module asm "    SUB1"
module asm "    NRDD"
module asm "    RDDS r0, r1"
module asm "    check_result r1, 4f"
module asm "    add sp, #8"
module asm "    bx lr"
module asm "4:  edge_result 5f"
module asm "    add sp, #8"
module asm "    bx lr"
module asm "5:  pop {r0, r1}"
module asm "    b dodo_soft_dsub"

; The mantissas come back to the CPU for their 128-bit product, which goes
; back as its high half and a sticky bit for the low half.
module asm "helper __aeabi_dmul"
module asm "    check_operand r0, r1, dodo_soft_dmul"
module asm "    check_operand r2, r3, dodo_soft_dmul"
module asm "    push {r0, r1, r2, r3, r4, lr}"
module asm "    WXUP r0, r1"
module asm "    WYUP r2, r3"
module asm "    RXMS r0, r1, 0"
module asm "    RYMS r2, r3, 0"
module asm "    umull r4, r12, r0, r2"
module asm "    movs lr, #0"
module asm "    umlal r12, lr, r0, r3"
module asm "    umlal r12, lr, r1, r2"
module asm "    WXMS r4, r12"
module asm "    movs r4, #0"
module asm "    umlal lr, r4, r1, r3"
module asm "    WXMO lr, r4"
module asm "    NRDD"
module asm "    RDDM r0, r1"
module asm "    check_result r1, 4f"
module asm "    add sp, #16"
module asm "    pop {r4, pc}"
module asm "4:  edge_result 5f"
module asm "    add sp, #16"
module asm "    pop {r4, pc}"
module asm "5:  pop {r0, r1, r2, r3, r4, lr}"
module asm "    b dodo_soft_dmul"

; A reciprocal estimate from the coprocessor, refined on the CPU, gives the
; quotient; its remainder corrects the last bit for exact rounding.
module asm "helper __aeabi_ddiv"
module asm "    check_operand r0, r1, dodo_soft_ddiv"
module asm "    check_operand r2, r3, dodo_soft_ddiv"
module asm "    push {r0, r1, r2, r3}"
module asm "    WXUP r0, r1"
module asm "    WYUP r2, r3"
module asm "    RYMR r0, r1"
module asm "    umull r1, r2, r0, r1"
module asm "    mvn r2, r2, lsl #2"
module asm "    smmlar r0, r2, r0, r0"
module asm "    smmulr r2, r2, r2"
module asm "    smmlar r0, r2, r0, r0"
module asm "    sub r12, r0, r0, lsr #31"
module asm "    RXMS r2, r3, 0"
module asm "    smmulr r1, r12, r3"
module asm "    RYMS r2, r3, 1"
module asm "    umull r2, r0, r1, r2"
module asm "    mla r0, r1, r3, r0"
module asm "    RXMS r2, r3, 4"
module asm "    sub r0, r2, r0"
module asm "    smmulr r2, r0, r12"
module asm "    mov r3, r1, lsr #4"
module asm "    adds r0, r2, r1, lsl #28"
module asm "    adc r1, r3, r2, asr #31"
module asm "    orr r0, r0, r1, lsr #21"
module asm "    bic r0, r0, r1, lsr #22"
module asm "    bic r0, r0, #7"
module asm "    RYMS r2, r3, 7"
module asm "    umull r3, r12, r0, r2"
module asm "    RYMS r2, r3, 7"
module asm "    mla r12, r0, r3, r12"
module asm "    mla r12, r1, r2, r12"
module asm "    RXMS r2, r3, 0"
module asm "    sub r12, r12, r2, lsl #18"
module asm "    orr r0, r0, r12, lsr #29"
module asm "    sub r0, r0, #1"
module asm "    WXDD r0, r1"
module asm "    NRDD"
module asm "    RDDD r0, r1"
module asm "    check_result r1, 4f"
module asm "    add sp, #16"
module asm "    bx lr"
module asm "4:  edge_result 5f"
module asm "    add sp, #16"
module asm "    bx lr"
module asm "5:  pop {r0, r1, r2, r3}"
module asm "    b dodo_soft_ddiv"

; ---------------------------------------------------------------------------
; Conversions. Every 32-bit integer is exact in binary64.

module asm "helper __aeabi_i2d"
module asm "    WXIC r0, r0"
module asm "    ADD0"
module asm "    SUB1"
module asm "    NRDD"
module asm "    RDDS r0, r1"
module asm "    bx lr"

module asm "helper __aeabi_ui2d"
module asm "    WXUC r0, r0"
module asm "    ADD0"
module asm "    SUB1"
module asm "    NRDD"
module asm "    RDDS r0, r1"
module asm "    bx lr"

; Truncation toward zero, saturating; NaN gives 0 (an exponent field of
; 2047 with a nonzero fraction).
module asm ".macro check_nan"
module asm "    lsls r12, r1, #1"
module asm "    cmn r12, #0x00200000"
module asm "    blo 3f"
module asm "    orrs r12, r0, r1, lsl #12"
module asm "    itt ne"
module asm "    movne r0, #0"
module asm "    bxne lr"
module asm "3:"
module asm ".endm"

module asm "helper __aeabi_d2iz"
module asm "    check_nan"
module asm "    WXDC r0, r1"
module asm "    ADD0"
module asm "    ADD1"
module asm "    NTDC"
module asm "    RDIC r0"
module asm "    bx lr"

module asm "helper __aeabi_d2uiz"
module asm "    check_nan"
module asm "    WXDC r0, r1"
module asm "    ADD0"
module asm "    ADD1"
module asm "    NTDC"
module asm "    RDUC r0"
module asm "    bx lr"

; To binary32, rounding to nearest: results with an exponent field of 2 to
; 254 return; infinity stands, NaN becomes the default NaN 0x7FC00000, and
; anything smaller is computed again in software.
module asm "helper __aeabi_d2f"
module asm "    WXUP r0, r1"
module asm "    NRDF"
module asm "    RDFG r2"
module asm "    lsls r12, r2, #1"
module asm "    cmp r12, #0x02000000"
module asm "    blo 4f"
module asm "    cmn r12, #0x01000000"
module asm "    bhs 5f"
module asm "    mov r0, r2"
module asm "    bx lr"
module asm "4:  b dodo_soft_d2f"
module asm "5:  lsls r12, r2, #9"
module asm "    itt ne"
module asm "    movne r2, #0x7f800000"
module asm "    orrne r2, r2, #0x00400000"
module asm "    mov r0, r2"
module asm "    bx lr"

; GNU names, which Dodo's own code generator calls.
module asm ".weak __adddf3"
module asm ".thumb_set __adddf3, __aeabi_dadd"
module asm ".weak __subdf3"
module asm ".thumb_set __subdf3, __aeabi_dsub"
module asm ".weak __muldf3"
module asm ".thumb_set __muldf3, __aeabi_dmul"
module asm ".weak __divdf3"
module asm ".thumb_set __divdf3, __aeabi_ddiv"
module asm ".weak __floatsidf"
module asm ".thumb_set __floatsidf, __aeabi_i2d"
module asm ".weak __floatunsidf"
module asm ".thumb_set __floatunsidf, __aeabi_ui2d"
module asm ".weak __fixdfsi"
module asm ".thumb_set __fixdfsi, __aeabi_d2iz"
module asm ".weak __fixunsdfsi"
module asm ".thumb_set __fixunsdfsi, __aeabi_d2uiz"
module asm ".weak __truncdfsf2"
module asm ".thumb_set __truncdfsf2, __aeabi_d2f"

@__aeabi_f2d = weak alias i64 (i32), ptr @dodo_soft_f2d
@__extendsfdf2 = weak alias i64 (i32), ptr @dodo_soft_f2d

; binary32 addition, subtraction and multiplication run on the FPU; these
; names are for other LLVM configurations.
@__aeabi_fadd = weak alias i32 (i32, i32), ptr @dodo_soft_fadd
@__aeabi_fsub = weak alias i32 (i32, i32), ptr @dodo_soft_fsub
@__aeabi_fmul = weak alias i32 (i32, i32), ptr @dodo_soft_fmul
@__addsf3 = weak alias i32 (i32, i32), ptr @dodo_soft_fadd
@__subsf3 = weak alias i32 (i32, i32), ptr @dodo_soft_fsub
@__mulsf3 = weak alias i32 (i32, i32), ptr @dodo_soft_fmul
