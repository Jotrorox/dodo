; Helpers for AVR chips with the MUL instruction, such as the ATmega328P,
; which have flash to spare: MUL multiplies two bytes into 16 bits, which
; LLVM uses for 8- and 16-bit products, and __mulsi3 is built from those;
; the 32-bit division entry points use avr_builtins.ll's division, which
; takes a faster path for 16-bit operands. Appended to the runtime after
; avr_builtins.ll, whose attributes and internal helpers it uses.

; The full 32-bit product of two 16-bit values, from four byte products.
define internal i32 @avr_umul16x16(i16 %x, i16 %y) #10 {
  %x0.b = trunc i16 %x to i8
  %x1.w = lshr i16 %x, 8
  %x1.b = trunc i16 %x1.w to i8
  %y0.b = trunc i16 %y to i8
  %y1.w = lshr i16 %y, 8
  %y1.b = trunc i16 %y1.w to i8
  %x0 = zext i8 %x0.b to i16
  %x1 = zext i8 %x1.b to i16
  %y0 = zext i8 %y0.b to i16
  %y1 = zext i8 %y1.b to i16
  %p00 = mul i16 %x0, %y0
  %p01 = mul i16 %x0, %y1
  %p10 = mul i16 %x1, %y0
  %p11 = mul i16 %x1, %y1
  %w00 = zext i16 %p00 to i32
  %w01 = zext i16 %p01 to i32
  %w10 = zext i16 %p10 to i32
  %w11 = zext i16 %p11 to i32
  %s01 = shl i32 %w01, 8
  %s10 = shl i32 %w10, 8
  %s11 = shl i32 %w11, 16
  %a = add i32 %w00, %s01
  %b = add i32 %a, %s10
  %c = add i32 %b, %s11
  ret i32 %c
}

define weak i32 @__mulsi3(i32 %a, i32 %b) #10 {
  %al = trunc i32 %a to i16
  %ah.w = lshr i32 %a, 16
  %ah = trunc i32 %ah.w to i16
  %bl = trunc i32 %b to i16
  %bh.w = lshr i32 %b, 16
  %bh = trunc i32 %bh.w to i16
  %low = call i32 @avr_umul16x16(i16 %al, i16 %bl)
  %c1 = mul i16 %al, %bh
  %c2 = mul i16 %ah, %bl
  %cross = add i16 %c1, %c2
  %cross32 = zext i16 %cross to i32
  %high = shl i32 %cross32, 16
  %result = add i32 %low, %high
  ret i32 %result
}

; 32-bit division, in the AVR builtin calling convention.
define weak cc 87 { i32, i32 } @__udivmodsi4(i32 %n, i32 %d) #10 {
  %u = call { i32, i32 } @avr_udivmod32(i32 %n, i32 %d)
  ret { i32, i32 } %u
}
define weak cc 87 { i32, i32 } @__divmodsi4(i32 %n, i32 %d) #10 {
  %u = call { i32, i32 } @avr_divmod32(i32 %n, i32 %d)
  ret { i32, i32 } %u
}
