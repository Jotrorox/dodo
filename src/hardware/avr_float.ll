; AVR names for the soft-float helpers. The compiler appends this file after
; the ATmega runtime, avr_builtins.ll, arm_float.ll with arm_float_soft.ll,
; and avr_float32.ll, which supply the IEEE 754 arithmetic as integer code
; (see src/hardware/avr.rs): binary32 arithmetic and integer conversions
; from avr_float32.ll, the rest from the shared code. Their Arm EABI and GNU aliases are left out
; there, as they name functions in the default address space and the GNU
; comparisons return a 32-bit int, while the AVR backend expects an 8-bit
; result in r24 from every comparison helper. The GNU names LLVM calls on
; AVR follow here, with AVR's address space for code.
;
; float is binary32 and double binary64, so f64 is exact on AVR too, unlike
; avr-gcc's default 32-bit double.

attributes #12 = { nounwind "no-builtins" }

; Arithmetic and conversions take and return the same bits as the helpers,
; in the same registers.
@__addsf3 = weak alias i32 (i32, i32), ptr addrspace(1) @dodo_f32_add
@__subsf3 = weak alias i32 (i32, i32), ptr addrspace(1) @dodo_f32_sub
@__mulsf3 = weak alias i32 (i32, i32), ptr addrspace(1) @dodo_f32_mul
@__divsf3 = weak alias i32 (i32, i32), ptr addrspace(1) @dodo_f32_div
@__adddf3 = weak alias i64 (i64, i64), ptr addrspace(1) @dodo_soft_dadd
@__subdf3 = weak alias i64 (i64, i64), ptr addrspace(1) @dodo_soft_dsub
@__muldf3 = weak alias i64 (i64, i64), ptr addrspace(1) @dodo_soft_dmul
@__divdf3 = weak alias i64 (i64, i64), ptr addrspace(1) @dodo_soft_ddiv
@__fixsfsi = weak alias i32 (i32), ptr addrspace(1) @dodo_f32_to_i32
@__fixunssfsi = weak alias i32 (i32), ptr addrspace(1) @dodo_f32_to_u32
@__fixsfdi = weak alias i64 (i32), ptr addrspace(1) @dodo_f32_to_i64
@__fixunssfdi = weak alias i64 (i32), ptr addrspace(1) @dodo_f32_to_u64
@__fixdfsi = weak alias i32 (i64), ptr addrspace(1) @dodo_soft_d2iz
@__fixunsdfsi = weak alias i32 (i64), ptr addrspace(1) @dodo_soft_d2uiz
@__fixdfdi = weak alias i64 (i64), ptr addrspace(1) @__aeabi_d2lz
@__fixunsdfdi = weak alias i64 (i64), ptr addrspace(1) @__aeabi_d2ulz
@__floatsisf = weak alias i32 (i32), ptr addrspace(1) @dodo_f32_from_i32
@__floatunsisf = weak alias i32 (i32), ptr addrspace(1) @dodo_f32_from_u32
@__floatdisf = weak alias i32 (i64), ptr addrspace(1) @dodo_f32_from_i64
@__floatundisf = weak alias i32 (i64), ptr addrspace(1) @dodo_f32_from_u64
@__floatsidf = weak alias i64 (i32), ptr addrspace(1) @dodo_soft_i2d
@__floatunsidf = weak alias i64 (i32), ptr addrspace(1) @dodo_soft_ui2d
@__floatdidf = weak alias i64 (i64), ptr addrspace(1) @__aeabi_l2d
@__floatundidf = weak alias i64 (i64), ptr addrspace(1) @__aeabi_ul2d
@__extendsfdf2 = weak alias i64 (i32), ptr addrspace(1) @dodo_soft_f2d
@__truncdfsf2 = weak alias i32 (i64), ptr addrspace(1) @dodo_soft_d2f

; Comparisons: -1, 0 or 1 as a is less than, equal to or greater than b in
; r24. Unordered operands give 1 for the less-than and equality family and
; -1 for the greater-than family, so that every ordered test is false.
define weak i8 @__lesf2(i32 %a, i32 %b) #12 {
  %r = call i32 @cmp32(i32 %a, i32 %b, i32 1)
  %n = trunc i32 %r to i8
  ret i8 %n
}
define weak i8 @__gesf2(i32 %a, i32 %b) #12 {
  %r = call i32 @cmp32(i32 %a, i32 %b, i32 -1)
  %n = trunc i32 %r to i8
  ret i8 %n
}
@__ltsf2 = weak alias i8 (i32, i32), ptr addrspace(1) @__lesf2
@__eqsf2 = weak alias i8 (i32, i32), ptr addrspace(1) @__lesf2
@__nesf2 = weak alias i8 (i32, i32), ptr addrspace(1) @__lesf2
@__cmpsf2 = weak alias i8 (i32, i32), ptr addrspace(1) @__lesf2
@__gtsf2 = weak alias i8 (i32, i32), ptr addrspace(1) @__gesf2
define weak i8 @__unordsf2(i32 %a, i32 %b) #12 {
  %r = call i32 @__aeabi_fcmpun(i32 %a, i32 %b)
  %n = trunc i32 %r to i8
  ret i8 %n
}

define weak i8 @__ledf2(i64 %a, i64 %b) #12 {
  %r = call i32 @cmp64(i64 %a, i64 %b, i32 1)
  %n = trunc i32 %r to i8
  ret i8 %n
}
define weak i8 @__gedf2(i64 %a, i64 %b) #12 {
  %r = call i32 @cmp64(i64 %a, i64 %b, i32 -1)
  %n = trunc i32 %r to i8
  ret i8 %n
}
@__ltdf2 = weak alias i8 (i64, i64), ptr addrspace(1) @__ledf2
@__eqdf2 = weak alias i8 (i64, i64), ptr addrspace(1) @__ledf2
@__nedf2 = weak alias i8 (i64, i64), ptr addrspace(1) @__ledf2
@__cmpdf2 = weak alias i8 (i64, i64), ptr addrspace(1) @__ledf2
@__gtdf2 = weak alias i8 (i64, i64), ptr addrspace(1) @__gedf2
define weak i8 @__unorddf2(i64 %a, i64 %b) #12 {
  %r = call i32 @__aeabi_dcmpun(i64 %a, i64 %b)
  %n = trunc i32 %r to i8
  ret i8 %n
}
