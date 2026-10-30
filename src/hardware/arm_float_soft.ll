; Names of the binary64 arithmetic and conversion helpers for chips without
; a double-precision accelerator, where each is the software implementation
; in arm_float.ll. The compiler appends this file after arm_float.ll (see
; `src/hardware/CHIP.rs`); the RP2350 appends rp2350/dcp.ll instead, which
; defines the same names.
;
; Dodo's own code generator calls the GNU names; other LLVM configurations
; call the Arm EABI names.

@__aeabi_dadd = weak alias i64 (i64, i64), ptr @dodo_soft_dadd
@__aeabi_dsub = weak alias i64 (i64, i64), ptr @dodo_soft_dsub
@__aeabi_dmul = weak alias i64 (i64, i64), ptr @dodo_soft_dmul
@__aeabi_ddiv = weak alias i64 (i64, i64), ptr @dodo_soft_ddiv
@__aeabi_i2d = weak alias i64 (i32), ptr @dodo_soft_i2d
@__aeabi_ui2d = weak alias i64 (i32), ptr @dodo_soft_ui2d
@__aeabi_d2iz = weak alias i32 (i64), ptr @dodo_soft_d2iz
@__aeabi_d2uiz = weak alias i32 (i64), ptr @dodo_soft_d2uiz
@__aeabi_f2d = weak alias i64 (i32), ptr @dodo_soft_f2d
@__aeabi_d2f = weak alias i32 (i64), ptr @dodo_soft_d2f

@__adddf3 = weak alias i64 (i64, i64), ptr @dodo_soft_dadd
@__subdf3 = weak alias i64 (i64, i64), ptr @dodo_soft_dsub
@__muldf3 = weak alias i64 (i64, i64), ptr @dodo_soft_dmul
@__divdf3 = weak alias i64 (i64, i64), ptr @dodo_soft_ddiv
@__floatsidf = weak alias i64 (i32), ptr @dodo_soft_i2d
@__floatunsidf = weak alias i64 (i32), ptr @dodo_soft_ui2d
@__fixdfsi = weak alias i32 (i64), ptr @dodo_soft_d2iz
@__fixunsdfsi = weak alias i32 (i64), ptr @dodo_soft_d2uiz
@__extendsfdf2 = weak alias i64 (i32), ptr @dodo_soft_f2d
@__truncdfsf2 = weak alias i32 (i64), ptr @dodo_soft_d2f
