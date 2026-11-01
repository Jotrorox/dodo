; AVR runtime helpers for the ATmega chip runtimes. The compiler appends this
; file to a chip's startup runtime (see `src/hardware/CHIP.rs`), so it is
; compiled for that chip's CPU, with the data layout the runtime declares.
;
; memcpy, memmove, memset, memcmp and bcmp, with AVR's 16-bit size_t and
; int; the integer division, multiplication and overflow helpers LLVM calls
; under libgcc's and compiler-rt's names; and the __sync atomic helpers for
; the operations the AVR backend does not expand inline. All are weak and
; --gc-sections drops the unused ones. There is no avr-libc or libgcc.
;
; The 8-, 16- and 32-bit division helpers use the AVR builtin calling
; convention (calling convention 87 in LLVM IR), as libgcc's do: dividend
; and divisor in r24/r22 (8-bit), r25:r24/r23:r22 (16-bit) or r25-r22 /
; r21-r18 (32-bit), and a { quotient, remainder } result in r24 and r25,
; r23:r22 and r25:r24, or r21-r18 and r25-r22. Everything else follows the
; ordinary AVR calling convention.
;
; This module is not optimized by the IR pass pipeline, so the helpers below
; are never rewritten into calls to themselves: they multiply only 8-bit
; values (the MUL instruction) or call a narrower helper, and divide by
; shifting and subtracting.

attributes #10 = { nounwind "no-builtins" }

; ---------------------------------------------------------------------------
; Memory

define weak ptr @memcpy(ptr %dest, ptr %src, i16 %n) #10 {
entry:
  br label %test
test:
  %i = phi i16 [ 0, %entry ], [ %i.next, %body ]
  %done = icmp eq i16 %i, %n
  br i1 %done, label %exit, label %body
body:
  %s = getelementptr i8, ptr %src, i16 %i
  %d = getelementptr i8, ptr %dest, i16 %i
  %v = load i8, ptr %s, align 1
  store i8 %v, ptr %d, align 1
  %i.next = add i16 %i, 1
  br label %test
exit:
  ret ptr %dest
}

define weak ptr @memmove(ptr %dest, ptr %src, i16 %n) #10 {
entry:
  %d = ptrtoint ptr %dest to i16
  %s = ptrtoint ptr %src to i16
  %forward = icmp ule i16 %d, %s
  br i1 %forward, label %copy, label %backward.test
copy:
  %r = call ptr @memcpy(ptr %dest, ptr %src, i16 %n)
  ret ptr %dest
backward.test:
  %i = phi i16 [ %n, %entry ], [ %i.next, %backward ]
  %done = icmp eq i16 %i, 0
  br i1 %done, label %exit, label %backward
backward:
  %i.next = sub i16 %i, 1
  %bsrc = getelementptr i8, ptr %src, i16 %i.next
  %bdst = getelementptr i8, ptr %dest, i16 %i.next
  %v = load i8, ptr %bsrc, align 1
  store i8 %v, ptr %bdst, align 1
  br label %backward.test
exit:
  ret ptr %dest
}

define weak ptr @memset(ptr %dest, i16 %c, i16 %n) #10 {
entry:
  %byte = trunc i16 %c to i8
  br label %test
test:
  %i = phi i16 [ 0, %entry ], [ %i.next, %body ]
  %done = icmp eq i16 %i, %n
  br i1 %done, label %exit, label %body
body:
  %p = getelementptr i8, ptr %dest, i16 %i
  store i8 %byte, ptr %p, align 1
  %i.next = add i16 %i, 1
  br label %test
exit:
  ret ptr %dest
}

define weak i16 @memcmp(ptr %a, ptr %b, i16 %n) #10 {
entry:
  br label %test
test:
  %i = phi i16 [ 0, %entry ], [ %i.next, %same ]
  %done = icmp eq i16 %i, %n
  br i1 %done, label %equal, label %body
body:
  %pa = getelementptr i8, ptr %a, i16 %i
  %pb = getelementptr i8, ptr %b, i16 %i
  %va = load i8, ptr %pa, align 1
  %vb = load i8, ptr %pb, align 1
  %eq = icmp eq i8 %va, %vb
  br i1 %eq, label %same, label %differ
same:
  %i.next = add i16 %i, 1
  br label %test
differ:
  %wa = zext i8 %va to i16
  %wb = zext i8 %vb to i16
  %diff = sub i16 %wa, %wb
  ret i16 %diff
equal:
  ret i16 0
}

@bcmp = weak alias i16 (ptr, ptr, i16), ptr addrspace(1) @memcmp

; ---------------------------------------------------------------------------
; Division. Restoring shift-subtract division, one quotient bit per step;
; `carry` handles remainders that overflow when the divisor has its top bit
; set. Signed results: the quotient is negative when the signs differ, and
; the remainder takes the dividend's sign. Dodo checks for zero divisors and
; signed overflow before dividing. 32- and 64-bit operands that fit in half
; the width, as most do, take the narrower helper's half as many steps.

define internal { i8, i8 } @avr_udivmod8(i8 %n, i8 %d) #10 {
entry:
  br label %loop
loop:
  %i = phi i8 [ 8, %entry ], [ %i.next, %loop ]
  %num = phi i8 [ %n, %entry ], [ %num.next, %loop ]
  %q = phi i8 [ 0, %entry ], [ %q.next, %loop ]
  %r = phi i8 [ 0, %entry ], [ %r.next, %loop ]
  %top = lshr i8 %num, 7
  %num.next = shl i8 %num, 1
  %carry = icmp slt i8 %r, 0
  %r.shift = shl i8 %r, 1
  %r.in = or i8 %r.shift, %top
  %fits = icmp uge i8 %r.in, %d
  %take = or i1 %carry, %fits
  %r.sub = sub i8 %r.in, %d
  %r.next = select i1 %take, i8 %r.sub, i8 %r.in
  %bit = zext i1 %take to i8
  %q.shift = shl i8 %q, 1
  %q.next = or i8 %q.shift, %bit
  %i.next = sub i8 %i, 1
  %done = icmp eq i8 %i.next, 0
  br i1 %done, label %exit, label %loop
exit:
  %result.q = insertvalue { i8, i8 } poison, i8 %q.next, 0
  %result = insertvalue { i8, i8 } %result.q, i8 %r.next, 1
  ret { i8, i8 } %result
}

define internal { i16, i16 } @avr_udivmod16(i16 %n, i16 %d) #10 {
entry:
  br label %loop
loop:
  %i = phi i8 [ 16, %entry ], [ %i.next, %loop ]
  %num = phi i16 [ %n, %entry ], [ %num.next, %loop ]
  %q = phi i16 [ 0, %entry ], [ %q.next, %loop ]
  %r = phi i16 [ 0, %entry ], [ %r.next, %loop ]
  %top = lshr i16 %num, 15
  %num.next = shl i16 %num, 1
  %carry = icmp slt i16 %r, 0
  %r.shift = shl i16 %r, 1
  %r.in = or i16 %r.shift, %top
  %fits = icmp uge i16 %r.in, %d
  %take = or i1 %carry, %fits
  %r.sub = sub i16 %r.in, %d
  %r.next = select i1 %take, i16 %r.sub, i16 %r.in
  %bit = zext i1 %take to i16
  %q.shift = shl i16 %q, 1
  %q.next = or i16 %q.shift, %bit
  %i.next = sub i8 %i, 1
  %done = icmp eq i8 %i.next, 0
  br i1 %done, label %exit, label %loop
exit:
  %result.q = insertvalue { i16, i16 } poison, i16 %q.next, 0
  %result = insertvalue { i16, i16 } %result.q, i16 %r.next, 1
  ret { i16, i16 } %result
}

define internal { i32, i32 } @avr_udivmod32(i32 %n, i32 %d) #10 {
entry:
  %both = or i32 %n, %d
  %small = icmp ult i32 %both, 65536
  br i1 %small, label %narrow, label %wide
narrow:
  %n16 = trunc i32 %n to i16
  %d16 = trunc i32 %d to i16
  %u16 = call { i16, i16 } @avr_udivmod16(i16 %n16, i16 %d16)
  %q16 = extractvalue { i16, i16 } %u16, 0
  %r16 = extractvalue { i16, i16 } %u16, 1
  %q.narrow = zext i16 %q16 to i32
  %r.narrow = zext i16 %r16 to i32
  %narrow.q = insertvalue { i32, i32 } poison, i32 %q.narrow, 0
  %narrow.result = insertvalue { i32, i32 } %narrow.q, i32 %r.narrow, 1
  ret { i32, i32 } %narrow.result
wide:
  %u = call { i32, i32 } @avr_udivmod32_loop(i32 %n, i32 %d)
  ret { i32, i32 } %u
}

define internal { i32, i32 } @avr_udivmod32_loop(i32 %n, i32 %d) #10 {
entry:
  br label %loop
loop:
  %i = phi i8 [ 32, %entry ], [ %i.next, %loop ]
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
  %i.next = sub i8 %i, 1
  %done = icmp eq i8 %i.next, 0
  br i1 %done, label %exit, label %loop
exit:
  %result.q = insertvalue { i32, i32 } poison, i32 %q.next, 0
  %result = insertvalue { i32, i32 } %result.q, i32 %r.next, 1
  ret { i32, i32 } %result
}

define internal { i64, i64 } @avr_udivmod64(i64 %n, i64 %d) #10 {
entry:
  %both = or i64 %n, %d
  %small = icmp ult i64 %both, 4294967296
  br i1 %small, label %narrow, label %wide
narrow:
  %n32 = trunc i64 %n to i32
  %d32 = trunc i64 %d to i32
  %u32 = call { i32, i32 } @avr_udivmod32(i32 %n32, i32 %d32)
  %q32 = extractvalue { i32, i32 } %u32, 0
  %r32 = extractvalue { i32, i32 } %u32, 1
  %q.narrow = zext i32 %q32 to i64
  %r.narrow = zext i32 %r32 to i64
  %narrow.q = insertvalue { i64, i64 } poison, i64 %q.narrow, 0
  %narrow.result = insertvalue { i64, i64 } %narrow.q, i64 %r.narrow, 1
  ret { i64, i64 } %narrow.result
wide:
  %u = call { i64, i64 } @avr_udivmod64_loop(i64 %n, i64 %d)
  ret { i64, i64 } %u
}

define internal { i64, i64 } @avr_udivmod64_loop(i64 %n, i64 %d) #10 {
entry:
  br label %loop
loop:
  %i = phi i8 [ 64, %entry ], [ %i.next, %loop ]
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
  %i.next = sub i8 %i, 1
  %done = icmp eq i8 %i.next, 0
  br i1 %done, label %exit, label %loop
exit:
  %result.q = insertvalue { i64, i64 } poison, i64 %q.next, 0
  %result = insertvalue { i64, i64 } %result.q, i64 %r.next, 1
  ret { i64, i64 } %result
}

define internal { i8, i8 } @avr_divmod8(i8 %a, i8 %b) #10 {
  %sa = ashr i8 %a, 7
  %sb = ashr i8 %b, 7
  %xa = xor i8 %a, %sa
  %ua = sub i8 %xa, %sa
  %xb = xor i8 %b, %sb
  %ub = sub i8 %xb, %sb
  %u = call { i8, i8 } @avr_udivmod8(i8 %ua, i8 %ub)
  %uq = extractvalue { i8, i8 } %u, 0
  %ur = extractvalue { i8, i8 } %u, 1
  %sq = xor i8 %sa, %sb
  %xq = xor i8 %uq, %sq
  %q = sub i8 %xq, %sq
  %xr = xor i8 %ur, %sa
  %r = sub i8 %xr, %sa
  %result.q = insertvalue { i8, i8 } poison, i8 %q, 0
  %result = insertvalue { i8, i8 } %result.q, i8 %r, 1
  ret { i8, i8 } %result
}

define internal { i16, i16 } @avr_divmod16(i16 %a, i16 %b) #10 {
  %sa = ashr i16 %a, 15
  %sb = ashr i16 %b, 15
  %xa = xor i16 %a, %sa
  %ua = sub i16 %xa, %sa
  %xb = xor i16 %b, %sb
  %ub = sub i16 %xb, %sb
  %u = call { i16, i16 } @avr_udivmod16(i16 %ua, i16 %ub)
  %uq = extractvalue { i16, i16 } %u, 0
  %ur = extractvalue { i16, i16 } %u, 1
  %sq = xor i16 %sa, %sb
  %xq = xor i16 %uq, %sq
  %q = sub i16 %xq, %sq
  %xr = xor i16 %ur, %sa
  %r = sub i16 %xr, %sa
  %result.q = insertvalue { i16, i16 } poison, i16 %q, 0
  %result = insertvalue { i16, i16 } %result.q, i16 %r, 1
  ret { i16, i16 } %result
}

define internal { i32, i32 } @avr_divmod32(i32 %a, i32 %b) #10 {
  %sa = ashr i32 %a, 31
  %sb = ashr i32 %b, 31
  %xa = xor i32 %a, %sa
  %ua = sub i32 %xa, %sa
  %xb = xor i32 %b, %sb
  %ub = sub i32 %xb, %sb
  %u = call { i32, i32 } @avr_udivmod32(i32 %ua, i32 %ub)
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

define internal { i64, i64 } @avr_divmod64(i64 %a, i64 %b) #10 {
  %sa = ashr i64 %a, 63
  %sb = ashr i64 %b, 63
  %xa = xor i64 %a, %sa
  %ua = sub i64 %xa, %sa
  %xb = xor i64 %b, %sb
  %ub = sub i64 %xb, %sb
  %u = call { i64, i64 } @avr_udivmod64(i64 %ua, i64 %ub)
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

define weak cc 87 { i8, i8 } @__udivmodqi4(i8 %n, i8 %d) #10 {
  %u = call { i8, i8 } @avr_udivmod8(i8 %n, i8 %d)
  ret { i8, i8 } %u
}
define weak cc 87 { i8, i8 } @__divmodqi4(i8 %n, i8 %d) #10 {
  %u = call { i8, i8 } @avr_divmod8(i8 %n, i8 %d)
  ret { i8, i8 } %u
}
define weak cc 87 { i16, i16 } @__udivmodhi4(i16 %n, i16 %d) #10 {
  %u = call { i16, i16 } @avr_udivmod16(i16 %n, i16 %d)
  ret { i16, i16 } %u
}
define weak cc 87 { i16, i16 } @__divmodhi4(i16 %n, i16 %d) #10 {
  %u = call { i16, i16 } @avr_divmod16(i16 %n, i16 %d)
  ret { i16, i16 } %u
}
define weak cc 87 { i32, i32 } @__udivmodsi4(i32 %n, i32 %d) #10 {
  %u = call { i32, i32 } @avr_udivmod32(i32 %n, i32 %d)
  ret { i32, i32 } %u
}
define weak cc 87 { i32, i32 } @__divmodsi4(i32 %n, i32 %d) #10 {
  %u = call { i32, i32 } @avr_divmod32(i32 %n, i32 %d)
  ret { i32, i32 } %u
}

define weak i64 @__udivdi3(i64 %n, i64 %d) #10 {
  %u = call { i64, i64 } @avr_udivmod64(i64 %n, i64 %d)
  %v = extractvalue { i64, i64 } %u, 0
  ret i64 %v
}
define weak i64 @__umoddi3(i64 %n, i64 %d) #10 {
  %u = call { i64, i64 } @avr_udivmod64(i64 %n, i64 %d)
  %v = extractvalue { i64, i64 } %u, 1
  ret i64 %v
}
define weak i64 @__divdi3(i64 %n, i64 %d) #10 {
  %u = call { i64, i64 } @avr_divmod64(i64 %n, i64 %d)
  %v = extractvalue { i64, i64 } %u, 0
  ret i64 %v
}
define weak i64 @__moddi3(i64 %n, i64 %d) #10 {
  %u = call { i64, i64 } @avr_divmod64(i64 %n, i64 %d)
  %v = extractvalue { i64, i64 } %u, 1
  ret i64 %v
}
define weak i64 @__udivmoddi4(i64 %n, i64 %d, ptr %remainder) #10 {
  %u = call { i64, i64 } @avr_udivmod64(i64 %n, i64 %d)
  %q = extractvalue { i64, i64 } %u, 0
  %r = extractvalue { i64, i64 } %u, 1
  %some = icmp ne ptr %remainder, null
  br i1 %some, label %store, label %done
store:
  store i64 %r, ptr %remainder, align 1
  br label %done
done:
  ret i64 %q
}

; ---------------------------------------------------------------------------
; Multiplication. The ATmega's MUL multiplies two bytes into 16 bits, which
; LLVM uses for 8- and 16-bit products; wider ones are built from those.

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

; The full 64-bit product of two 32-bit values; its 32-bit products call
; __mulsi3.
define internal i64 @avr_umul32x32(i32 %x, i32 %y) #10 {
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

define weak i64 @__muldi3(i64 %a, i64 %b) #10 {
  %al = trunc i64 %a to i32
  %ah64 = lshr i64 %a, 32
  %ah = trunc i64 %ah64 to i32
  %bl = trunc i64 %b to i32
  %bh64 = lshr i64 %b, 32
  %bh = trunc i64 %bh64 to i32
  %low = call i64 @avr_umul32x32(i32 %al, i32 %bl)
  %c1 = mul i32 %al, %bh
  %c2 = mul i32 %ah, %bl
  %cross = add i32 %c1, %c2
  %cross64 = zext i32 %cross to i64
  %high = shl i64 %cross64, 32
  %result = add i64 %low, %high
  ret i64 %result
}

; The full 128-bit product of two 64-bit values, as { low, high }.
define internal { i64, i64 } @avr_umul64x64(i64 %a, i64 %b) #10 {
  %a0 = trunc i64 %a to i32
  %a1.w = lshr i64 %a, 32
  %a1 = trunc i64 %a1.w to i32
  %b0 = trunc i64 %b to i32
  %b1.w = lshr i64 %b, 32
  %b1 = trunc i64 %b1.w to i32
  %p00 = call i64 @avr_umul32x32(i32 %a0, i32 %b0)
  %p01 = call i64 @avr_umul32x32(i32 %a0, i32 %b1)
  %p10 = call i64 @avr_umul32x32(i32 %a1, i32 %b0)
  %p11 = call i64 @avr_umul32x32(i32 %a1, i32 %b1)
  ; middle = (p00 >> 32) + low32(p01) + low32(p10), at most 34 bits
  %p00.hi = lshr i64 %p00, 32
  %p01.lo = and i64 %p01, 4294967295
  %p10.lo = and i64 %p10, 4294967295
  %m1 = add i64 %p00.hi, %p01.lo
  %middle = add i64 %m1, %p10.lo
  %p00.lo = and i64 %p00, 4294967295
  %middle.lo = shl i64 %middle, 32
  %low = or i64 %middle.lo, %p00.lo
  %middle.hi = lshr i64 %middle, 32
  %p01.hi = lshr i64 %p01, 32
  %p10.hi = lshr i64 %p10, 32
  %h1 = add i64 %p11, %p01.hi
  %h2 = add i64 %h1, %p10.hi
  %high = add i64 %h2, %middle.hi
  %r.lo = insertvalue { i64, i64 } poison, i64 %low, 0
  %r = insertvalue { i64, i64 } %r.lo, i64 %high, 1
  ret { i64, i64 } %r
}

; Signed multiplication with overflow, for llvm.smul.with.overflow: the
; product, and 1 in the int (16 bits on AVR) at %overflow if it does not
; fit, else 0.
define weak i32 @__mulosi4(i32 %a, i32 %b, ptr %overflow) #10 {
  %wa = sext i32 %a to i64
  %wb = sext i32 %b to i64
  %p = mul i64 %wa, %wb
  %r = trunc i64 %p to i32
  %back = sext i32 %r to i64
  %differs = icmp ne i64 %back, %p
  %flag = zext i1 %differs to i16
  store i16 %flag, ptr %overflow, align 1
  ret i32 %r
}

define weak i64 @__mulodi4(i64 %a, i64 %b, ptr %overflow) #10 {
  %sa = ashr i64 %a, 63
  %sb = ashr i64 %b, 63
  %xa = xor i64 %a, %sa
  %ua = sub i64 %xa, %sa
  %xb = xor i64 %b, %sb
  %ub = sub i64 %xb, %sb
  %u = call { i64, i64 } @avr_umul64x64(i64 %ua, i64 %ub)
  %lo = extractvalue { i64, i64 } %u, 0
  %hi = extractvalue { i64, i64 } %u, 1
  %sign = xor i64 %sa, %sb
  %negative = icmp ne i64 %sign, 0
  %xr = xor i64 %lo, %sign
  %r = sub i64 %xr, %sign
  %high.set = icmp ne i64 %hi, 0
  ; |product| may reach 2^63 when negative, 2^63 - 1 otherwise.
  %limit = select i1 %negative, i64 -9223372036854775808, i64 9223372036854775807
  %too.big = icmp ugt i64 %lo, %limit
  %over = or i1 %high.set, %too.big
  %flag = zext i1 %over to i16
  store i16 %flag, ptr %overflow, align 1
  ret i64 %r
}

; ---------------------------------------------------------------------------
; Atomic helpers. The AVR backend expands 8- and 16-bit atomic loads,
; stores, additions, subtractions and bitwise operations inline, with
; interrupts masked, and calls these for exchanges, compare-and-swap,
; minimum and maximum, and every operation on wider values. A single core
; makes masking interrupts enough: each helper saves SREG, clears its
; interrupt flag, and restores it.

define internal i8 @avr_mask() #10 {
  %sreg = call i8 asm sideeffect "in $0, 0x3f\0Acli", "=r,~{memory}"()
  ret i8 %sreg
}

define internal void @avr_restore(i8 %sreg) #10 {
  call void asm sideeffect "out 0x3f, $0", "r,~{memory}"(i8 %sreg)
  ret void
}

define weak i8 @__sync_lock_test_and_set_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  store i8 %v, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_val_compare_and_swap_1(ptr %p, i8 %expected, i8 %new) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %same = icmp eq i8 %old, %expected
  %value = select i1 %same, i8 %new, i8 %old
  store i8 %value, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}


define weak i8 @__sync_fetch_and_add_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %new = add i8 %old, %v
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_fetch_and_sub_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %new = sub i8 %old, %v
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_fetch_and_and_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %new = and i8 %old, %v
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_fetch_and_or_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %new = or i8 %old, %v
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_fetch_and_xor_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %new = xor i8 %old, %v
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_fetch_and_nand_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %both = and i8 %old, %v
  %new = xor i8 %both, -1
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_fetch_and_max_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %keep = icmp sgt i8 %old, %v
  %new = select i1 %keep, i8 %old, i8 %v
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_fetch_and_min_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %keep = icmp slt i8 %old, %v
  %new = select i1 %keep, i8 %old, i8 %v
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_fetch_and_umax_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %keep = icmp ugt i8 %old, %v
  %new = select i1 %keep, i8 %old, i8 %v
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i8 @__sync_fetch_and_umin_1(ptr %p, i8 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i8, ptr %p, align 1
  %keep = icmp ult i8 %old, %v
  %new = select i1 %keep, i8 %old, i8 %v
  store i8 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i8 %old
}

define weak i16 @__sync_lock_test_and_set_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  store i16 %v, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_val_compare_and_swap_2(ptr %p, i16 %expected, i16 %new) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %same = icmp eq i16 %old, %expected
  %value = select i1 %same, i16 %new, i16 %old
  store i16 %value, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}


define weak i16 @__sync_fetch_and_add_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %new = add i16 %old, %v
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_fetch_and_sub_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %new = sub i16 %old, %v
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_fetch_and_and_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %new = and i16 %old, %v
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_fetch_and_or_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %new = or i16 %old, %v
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_fetch_and_xor_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %new = xor i16 %old, %v
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_fetch_and_nand_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %both = and i16 %old, %v
  %new = xor i16 %both, -1
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_fetch_and_max_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %keep = icmp sgt i16 %old, %v
  %new = select i1 %keep, i16 %old, i16 %v
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_fetch_and_min_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %keep = icmp slt i16 %old, %v
  %new = select i1 %keep, i16 %old, i16 %v
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_fetch_and_umax_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %keep = icmp ugt i16 %old, %v
  %new = select i1 %keep, i16 %old, i16 %v
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i16 @__sync_fetch_and_umin_2(ptr %p, i16 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i16, ptr %p, align 1
  %keep = icmp ult i16 %old, %v
  %new = select i1 %keep, i16 %old, i16 %v
  store i16 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i16 %old
}

define weak i32 @__sync_lock_test_and_set_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  store i32 %v, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_val_compare_and_swap_4(ptr %p, i32 %expected, i32 %new) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %same = icmp eq i32 %old, %expected
  %value = select i1 %same, i32 %new, i32 %old
  store i32 %value, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}


define weak i32 @__sync_fetch_and_add_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %new = add i32 %old, %v
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_fetch_and_sub_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %new = sub i32 %old, %v
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_fetch_and_and_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %new = and i32 %old, %v
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_fetch_and_or_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %new = or i32 %old, %v
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_fetch_and_xor_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %new = xor i32 %old, %v
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_fetch_and_nand_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %both = and i32 %old, %v
  %new = xor i32 %both, -1
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_fetch_and_max_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %keep = icmp sgt i32 %old, %v
  %new = select i1 %keep, i32 %old, i32 %v
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_fetch_and_min_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %keep = icmp slt i32 %old, %v
  %new = select i1 %keep, i32 %old, i32 %v
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_fetch_and_umax_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %keep = icmp ugt i32 %old, %v
  %new = select i1 %keep, i32 %old, i32 %v
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i32 @__sync_fetch_and_umin_4(ptr %p, i32 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i32, ptr %p, align 1
  %keep = icmp ult i32 %old, %v
  %new = select i1 %keep, i32 %old, i32 %v
  store i32 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i32 %old
}

define weak i64 @__sync_lock_test_and_set_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  store i64 %v, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_val_compare_and_swap_8(ptr %p, i64 %expected, i64 %new) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %same = icmp eq i64 %old, %expected
  %value = select i1 %same, i64 %new, i64 %old
  store i64 %value, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}


define weak i64 @__sync_fetch_and_add_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %new = add i64 %old, %v
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_fetch_and_sub_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %new = sub i64 %old, %v
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_fetch_and_and_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %new = and i64 %old, %v
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_fetch_and_or_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %new = or i64 %old, %v
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_fetch_and_xor_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %new = xor i64 %old, %v
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_fetch_and_nand_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %both = and i64 %old, %v
  %new = xor i64 %both, -1
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_fetch_and_max_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %keep = icmp sgt i64 %old, %v
  %new = select i1 %keep, i64 %old, i64 %v
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_fetch_and_min_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %keep = icmp slt i64 %old, %v
  %new = select i1 %keep, i64 %old, i64 %v
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_fetch_and_umax_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %keep = icmp ugt i64 %old, %v
  %new = select i1 %keep, i64 %old, i64 %v
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak i64 @__sync_fetch_and_umin_8(ptr %p, i64 %v) #10 {
  %s = call i8 @avr_mask()
  %old = load i64, ptr %p, align 1
  %keep = icmp ult i64 %old, %v
  %new = select i1 %keep, i64 %old, i64 %v
  store i64 %new, ptr %p, align 1
  call void @avr_restore(i8 %s)
  ret i64 %old
}

define weak void @__sync_synchronize() #10 {
  ret void
}
