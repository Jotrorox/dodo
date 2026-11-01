; Arm EABI floating-point helpers shared by the Cortex-M chip runtimes. The
; compiler appends this file after arm_eabi.ll (see `src/hardware/CHIP.rs`).
;
; IEEE 754 binary32 and binary64 arithmetic, comparisons and conversions
; that LLVM calls on soft-float targets, under both their GNU and Arm EABI
; names, plus fmod and fmodf for `%`. Armv6-M (RP2040) has no FPU and calls all of them.
; The RP2350's Cortex-M33 has a single-precision FPU, so it calls only the
; binary64 helpers, the conversions between f32 and f64 or 64-bit integers,
; and fmodf. All are weak; --gc-sections drops the unused ones.
;
; binary64 addition, subtraction, multiplication and division, the
; conversions between binary64 and binary32 or 32-bit integers, and binary32
; addition, subtraction and multiplication are defined here as dodo_soft_*.
; The file the compiler appends next gives them their GNU and EABI names:
; arm_float_soft.ll for these software versions, rp2040/float.ll for the
; RP2040's assembly fast paths, or rp2350/dcp.ll for the RP2350's
; coprocessor. The latter two fall back to these.
;
; Results round to nearest, ties to even, and subnormals are kept. NaN
; results are the default quiet NaN; there are no exception flags.
; Out-of-range conversions to integers saturate and NaN converts to zero;
; Dodo checks the range before converting.
;
; Everything is integer code: a float is its bits in an i32 and a double
; its bits in an i64, which the soft-float base ABI passes in the same
; registers. The algorithms follow Berkeley SoftFloat 3, written for a 32-bit
; core without a 64-bit multiply or variable 64-bit shifts, both of which
; Armv6-M would otherwise call helpers for:
;
; - binary32 arithmetic and conversions work on 32-bit words throughout.
; - Multiplication builds wide products from 16x16-bit ones (mul32x32),
;   since Armv6-M's `muls` keeps only the low 32 bits of a product.
; - Variable 64-bit shifts are built from 32-bit ones (lshr64, shl64).
; - Division multiplies by a reciprocal of the divisor (recip32): binary32
;   takes one quotient digit, binary64 two. Each digit is then corrected
;   against its exact remainder, so the quotient and its sticky bit are exact
;   whatever the reciprocal's error, which only decides how rarely a
;   correction step runs.
;
; The compiler optimizes this module like a program at -O3, so the small
; internal functions are inlined. These helpers need only the integer
; helpers from arm_eabi.ll, and on a 64-bit host they need nothing, which is
; how tests/soft_float.rs checks them against hardware.

attributes #1 = { nounwind "no-builtins" }
attributes #2 = { nounwind "no-builtins" alwaysinline }

; ---------------------------------------------------------------------------
; Bit utilities. Armv6-M has no clz instruction, and LLVM would call
; __clzsi2 for llvm.ctlz.

define internal i32 @clz32(i32 %x) #2 {
  %c16 = icmp ult i32 %x, 65536
  %x16 = shl i32 %x, 16
  %a16 = select i1 %c16, i32 %x16, i32 %x
  %n16 = select i1 %c16, i32 16, i32 0
  %c8 = icmp ult i32 %a16, 16777216
  %x8 = shl i32 %a16, 8
  %a8 = select i1 %c8, i32 %x8, i32 %a16
  %d8 = select i1 %c8, i32 8, i32 0
  %n8 = add i32 %n16, %d8
  %c4 = icmp ult i32 %a8, 268435456
  %x4 = shl i32 %a8, 4
  %a4 = select i1 %c4, i32 %x4, i32 %a8
  %d4 = select i1 %c4, i32 4, i32 0
  %n4 = add i32 %n8, %d4
  %c2 = icmp ult i32 %a4, 1073741824
  %x2 = shl i32 %a4, 2
  %a2 = select i1 %c2, i32 %x2, i32 %a4
  %d2 = select i1 %c2, i32 2, i32 0
  %n2 = add i32 %n4, %d2
  %c1 = icmp sgt i32 %a2, -1
  %d1 = zext i1 %c1 to i32
  %n1 = add i32 %n2, %d1
  %zero = icmp eq i32 %x, 0
  %dz = zext i1 %zero to i32
  %n = add i32 %n1, %dz
  ret i32 %n
}

define internal i32 @clz64(i64 %x) #2 {
  %hi64 = lshr i64 %x, 32
  %hi = trunc i64 %hi64 to i32
  %lo = trunc i64 %x to i32
  %high = icmp ne i32 %hi, 0
  %word = select i1 %high, i32 %hi, i32 %lo
  %base = select i1 %high, i32 0, i32 32
  %n = call i32 @clz32(i32 %word)
  %r = add i32 %base, %n
  ret i32 %r
}

; a >> n and a << n for n from 0 to 63, from 32-bit shifts. Shifting the
; carried word by one and then by 31 - (n & 31) never shifts by 32.
define internal i64 @lshr64(i64 %a, i32 %n) #2 {
  %lo = trunc i64 %a to i32
  %h64 = lshr i64 %a, 32
  %hi = trunc i64 %h64 to i32
  %m = and i32 %n, 31
  %big = icmp ugt i32 %n, 31
  %hs = lshr i32 %hi, %m
  %ls = lshr i32 %lo, %m
  %h1 = shl i32 %hi, 1
  %back = sub i32 31, %m
  %carry = shl i32 %h1, %back
  %lc = or i32 %ls, %carry
  %rlo = select i1 %big, i32 %hs, i32 %lc
  %rhi = select i1 %big, i32 0, i32 %hs
  %wl = zext i32 %rlo to i64
  %wh = zext i32 %rhi to i64
  %sh = shl i64 %wh, 32
  %r = or i64 %sh, %wl
  ret i64 %r
}

define internal i64 @shl64(i64 %a, i32 %n) #2 {
  %lo = trunc i64 %a to i32
  %h64 = lshr i64 %a, 32
  %hi = trunc i64 %h64 to i32
  %m = and i32 %n, 31
  %big = icmp ugt i32 %n, 31
  %hs = shl i32 %hi, %m
  %ls = shl i32 %lo, %m
  %l1 = lshr i32 %lo, 1
  %back = sub i32 31, %m
  %carry = lshr i32 %l1, %back
  %hc = or i32 %hs, %carry
  %rhi = select i1 %big, i32 %ls, i32 %hc
  %rlo = select i1 %big, i32 0, i32 %ls
  %wl = zext i32 %rlo to i64
  %wh = zext i32 %rhi to i64
  %sh = shl i64 %wh, 32
  %r = or i64 %sh, %wl
  ret i64 %r
}

; Shift right by any distance, setting the lowest bit if any 1 bits were
; shifted out, so rounding still sees them. Thumb-1 has no conditional
; execution, so each case is its own branch.
define internal i64 @shr_jam64(i64 %a, i32 %dist) #1 {
entry:
  %lo = trunc i64 %a to i32
  %h64 = lshr i64 %a, 32
  %hi = trunc i64 %h64 to i32
  %short = icmp ult i32 %dist, 32
  br i1 %short, label %short.case, label %long.case
short.case:
  %none = icmp eq i32 %dist, 0
  br i1 %none, label %unshifted, label %shift
unshifted:
  ret i64 %a
shift:
  %back = sub i32 32, %dist
  %l1 = lshr i32 %lo, %dist
  %h1 = shl i32 %hi, %back
  %rlo = or i32 %l1, %h1
  %rhi = lshr i32 %hi, %dist
  %lost = shl i32 %lo, %back
  %j = icmp ne i32 %lost, 0
  %jw = zext i1 %j to i32
  %rlo.j = or i32 %rlo, %jw
  %wl = zext i32 %rlo.j to i64
  %wh = zext i32 %rhi to i64
  %sh = shl i64 %wh, 32
  %r = or i64 %sh, %wl
  ret i64 %r
long.case:
  %far = icmp ugt i32 %dist, 63
  br i1 %far, label %far.case, label %mid
far.case:
  %nz = icmp ne i64 %a, 0
  %nzw = zext i1 %nz to i64
  ret i64 %nzw
mid:
  %d2 = sub i32 %dist, 32
  %r2 = lshr i32 %hi, %d2
  %hd = shl i32 %hi, 1
  %b2 = sub i32 31, %d2
  %lost2 = shl i32 %hd, %b2
  %any = or i32 %lost2, %lo
  %j2 = icmp ne i32 %any, 0
  %j2w = zext i1 %j2 to i32
  %r2j = or i32 %r2, %j2w
  %w2 = zext i32 %r2j to i64
  ret i64 %w2
}

define internal i32 @shr_jam32(i32 %a, i32 %dist) #2 {
  %far = icmp ugt i32 %dist, 31
  %nz = icmp ne i32 %a, 0
  %nzw = zext i1 %nz to i32
  %x = select i1 %far, i32 %nzw, i32 %a
  %d = select i1 %far, i32 0, i32 %dist
  %r = lshr i32 %x, %d
  %back = shl i32 %r, %d
  %lost = icmp ne i32 %back, %x
  %j = zext i1 %lost to i32
  %out = or i32 %r, %j
  ret i32 %out
}

; The full 64-bit product of two 32-bit words, from four 16x16-bit products.
define internal i64 @mul32x32(i32 %a, i32 %b) #2 {
  %al = and i32 %a, 65535
  %ah = lshr i32 %a, 16
  %bl = and i32 %b, 65535
  %bh = lshr i32 %b, 16
  %ll = mul i32 %al, %bl
  %lh = mul i32 %al, %bh
  %hl = mul i32 %ah, %bl
  %hh = mul i32 %ah, %bh
  %wll = zext i32 %ll to i64
  %wlh = zext i32 %lh to i64
  %whl = zext i32 %hl to i64
  %whh = zext i32 %hh to i64
  %mid = add i64 %wlh, %whl
  %mids = shl i64 %mid, 16
  %high = shl i64 %whh, 32
  %s = add i64 %high, %wll
  %r = add i64 %s, %mids
  ret i64 %r
}

; The full 128-bit product as { high, low }.
define internal { i64, i64 } @mul64x64(i64 %a, i64 %b) #1 {
  %a0 = trunc i64 %a to i32
  %a1w = lshr i64 %a, 32
  %a1 = trunc i64 %a1w to i32
  %b0 = trunc i64 %b to i32
  %b1w = lshr i64 %b, 32
  %b1 = trunc i64 %b1w to i32
  %p00 = call i64 @mul32x32(i32 %a0, i32 %b0)
  %p01 = call i64 @mul32x32(i32 %a0, i32 %b1)
  %p10 = call i64 @mul32x32(i32 %a1, i32 %b0)
  %p11 = call i64 @mul32x32(i32 %a1, i32 %b1)
  %c00 = lshr i64 %p00, 32
  %l01 = and i64 %p01, 4294967295
  %l10 = and i64 %p10, 4294967295
  %m0 = add i64 %c00, %l01
  %mid = add i64 %m0, %l10
  %midlo = shl i64 %mid, 32
  %l00 = and i64 %p00, 4294967295
  %lo = or i64 %midlo, %l00
  %h01 = lshr i64 %p01, 32
  %h10 = lshr i64 %p10, 32
  %hmid = lshr i64 %mid, 32
  %h0 = add i64 %p11, %h01
  %h1 = add i64 %h0, %h10
  %hi = add i64 %h1, %hmid
  %result.hi = insertvalue { i64, i64 } poison, i64 %hi, 0
  %result = insertvalue { i64, i64 } %result.hi, i64 %lo, 1
  ret { i64, i64 } %result
}

; ---------------------------------------------------------------------------
; Reciprocals for division.

; Linear pieces k0 - k1 t of 1 / x on the 16 intervals [1/2 + i/32,
; 1/2 + (i + 1)/32), with t from 0 to 1 across each: k0 * 2^31 and k1 * 2^15.
; Each piece is the interval's chord, lowered by half its largest distance
; from 1 / x; all are within 2^-11 of it.
@recip.k0 = internal constant [16 x i32] [i32 -1914417, i32 -254249559, i32 -478576499, i32 -679312156, i32 -859991277, i32 -1023476165, i32 -1172109305, i32 -1307826225, i32 -1432240175, i32 -1546706523, i32 -1652372289, i32 -1750214701, i32 -1841071482, i32 -1925664886, i32 -2004620928, i32 -2078484895]
@recip.k1 = internal constant [16 x i16] [i16 3855, i16 3427, i16 3066, i16 2759, i16 2497, i16 2270, i16 2072, i16 1900, i16 1748, i16 1613, i16 1494, i16 1387, i16 1291, i16 1205, i16 1128, i16 1057]

; 2^63 / d, within 2, for d from 2^31 to 2^32 - 1, and at most 2^32 - 1, in
; nine 16x16-bit multiplications. Values are fixed point with a 1-bit integer
; part, so y = 2^63 / d is 1 / (d / 2^32). The table's estimate y0 is refined
; by two Newton-Raphson steps y' = y + y e, where e = 1 - (d / 2^32) y is the
; relative error: about 2^-11 before the first step, so that step works
; with y's top 16 bits, and 2^-21 before the second, so y e needs only the
; top 16 bits of y. Overflow cannot happen: |e| stays below 2^-10 and 2^-20.
define internal i32 @recip32(i32 %d) #1 {
  %i0 = lshr i32 %d, 27
  %i = and i32 %i0, 15
  %u0 = lshr i32 %d, 11
  %u = and i32 %u0, 65535
  %p0 = getelementptr [16 x i32], ptr @recip.k0, i32 0, i32 %i
  %k0 = load i32, ptr %p0
  %p1 = getelementptr [16 x i16], ptr @recip.k1, i32 0, i32 %i
  %k1h = load i16, ptr %p1
  %k1 = zext i16 %k1h to i32
  %slope = mul i32 %k1, %u
  %y0 = sub i32 %k0, %slope
  ; First step, from y0's top 16 bits: t = d y0 (a 32x16-bit product), e in
  ; units of 2^-31, and y1 = y0 + y0 e.
  %y16 = lshr i32 %y0, 16
  %dh = lshr i32 %d, 16
  %dl = and i32 %d, 65535
  %th = mul i32 %dh, %y16
  %tl0 = mul i32 %dl, %y16
  %tl = lshr i32 %tl0, 16
  %t = add i32 %th, %tl
  %e = sub i32 -2147483648, %t
  %e6 = ashr i32 %e, 6
  %c0 = mul i32 %y16, %e6
  %c = ashr i32 %c0, 9
  %yb = shl i32 %y16, 16
  %y1 = add i32 %yb, %c
  ; Second step: t2 = d y1 (the top half of a 32x32-bit product), and a
  ; correction small enough for a 16x16-bit product.
  %p = call i64 @mul32x32(i32 %d, i32 %y1)
  %t2w = lshr i64 %p, 32
  %t2 = trunc i64 %t2w to i32
  %e2 = sub i32 -2147483648, %t2
  %y1h = lshr i32 %y1, 16
  %c20 = mul i32 %y1h, %e2
  %c2 = ashr i32 %c20, 15
  %y1w = zext i32 %y1 to i64
  %c2w = sext i32 %c2 to i64
  %y2 = add i64 %y1w, %c2w
  %big = icmp ugt i64 %y2, 4294967295
  %y2t = trunc i64 %y2 to i32
  %out = select i1 %big, i32 -1, i32 %y2t
  ret i32 %out
}

; Corrects a quotient digit q, whose remainder r = n - q m may be negative or
; at least m, until 0 <= r < m. Returns { q, r }.
define internal { i32, i32 } @fix_digit32(i32 %q, i32 %r, i32 %m) #1 {
entry:
  br label %down
down:
  %qd = phi i32 [ %q, %entry ], [ %qd.next, %down.step ]
  %rd = phi i32 [ %r, %entry ], [ %rd.next, %down.step ]
  %neg = icmp slt i32 %rd, 0
  br i1 %neg, label %down.step, label %up
down.step:
  %qd.next = sub i32 %qd, 1
  %rd.next = add i32 %rd, %m
  br label %down
up:
  %qu = phi i32 [ %qd, %down ], [ %qu.next, %up.step ]
  %ru = phi i32 [ %rd, %down ], [ %ru.next, %up.step ]
  %big = icmp uge i32 %ru, %m
  br i1 %big, label %up.step, label %done
up.step:
  %qu.next = add i32 %qu, 1
  %ru.next = sub i32 %ru, %m
  br label %up
done:
  %res.q = insertvalue { i32, i32 } poison, i32 %qu, 0
  %res = insertvalue { i32, i32 } %res.q, i32 %ru, 1
  ret { i32, i32 } %res
}

define internal { i32, i64 } @fix_digit64(i32 %q, i64 %r, i64 %m) #1 {
entry:
  br label %down
down:
  %qd = phi i32 [ %q, %entry ], [ %qd.next, %down.step ]
  %rd = phi i64 [ %r, %entry ], [ %rd.next, %down.step ]
  %neg = icmp slt i64 %rd, 0
  br i1 %neg, label %down.step, label %up
down.step:
  %qd.next = sub i32 %qd, 1
  %rd.next = add i64 %rd, %m
  br label %down
up:
  %qu = phi i32 [ %qd, %down ], [ %qu.next, %up.step ]
  %ru = phi i64 [ %rd, %down ], [ %ru.next, %up.step ]
  %big = icmp uge i64 %ru, %m
  br i1 %big, label %up.step, label %done
up.step:
  %qu.next = add i32 %qu, 1
  %ru.next = sub i64 %ru, %m
  br label %up
done:
  %res.q = insertvalue { i32, i64 } poison, i32 %qu, 0
  %res = insertvalue { i32, i64 } %res.q, i64 %ru, 1
  ret { i32, i64 } %res
}

; The low 64 bits of q * m.
define internal i64 @mul_digit64(i32 %q, i64 %m) #2 {
  %lo = trunc i64 %m to i32
  %h = lshr i64 %m, 32
  %hi = trunc i64 %h to i32
  %pl = call i64 @mul32x32(i32 %q, i32 %lo)
  %ph = mul i32 %q, %hi
  %phw = zext i32 %ph to i64
  %phs = shl i64 %phw, 32
  %r = add i64 %pl, %phs
  ret i64 %r
}

; ---------------------------------------------------------------------------
; binary64 packing. Bit patterns: sign 0x8000000000000000 =
; -9223372036854775808, infinity 0x7FF0000000000000 = 9218868437227405312,
; default NaN 0x7FF8000000000000 = 9221120237041090560, magnitude mask
; 9223372036854775807, fraction mask 0x000FFFFFFFFFFFFF = 4503599627370495,
; hidden bit 2^52 = 4503599627370496.

define internal i32 @exp64(i64 %a) #2 {
  %s = lshr i64 %a, 52
  %t = trunc i64 %s to i32
  %e = and i32 %t, 2047
  ret i32 %e
}

define internal i1 @isnan64(i64 %a) #2 {
  %abs = and i64 %a, 9223372036854775807
  %nan = icmp ugt i64 %abs, 9218868437227405312
  ret i1 %nan
}

; Adds rather than ors the fields, so a significand carry into bit 52
; increments the exponent.
define internal i64 @pack64(i32 %sign, i32 %exp, i64 %sig) #2 {
  %sw = zext i32 %sign to i64
  %s = shl i64 %sw, 63
  %e = zext i32 %exp to i64
  %es = shl i64 %e, 52
  %a = add i64 %s, %es
  %r = add i64 %a, %sig
  ret i64 %r
}

; Rounds a significand with its leading 1 at bit 62 and ten bits below the
; result's last bit. `exp` is one less than the biased exponent; out of range
; it underflows to a subnormal or zero, or overflows to infinity. A
; subnormal's exponent is 0, which a carry out of rounding makes normal.
define internal i64 @round_pack64(i32 %sign, i32 %exp, i64 %sig) #1 {
entry:
  %edge = icmp uge i32 %exp, 2045
  br i1 %edge, label %edge.case, label %round
edge.case:
  %tiny = icmp slt i32 %exp, 0
  br i1 %tiny, label %subnormal, label %check
subnormal:
  %dist = sub i32 0, %exp
  %ssig = call i64 @shr_jam64(i64 %sig, i32 %dist)
  br label %round
check:
  %big = icmp sgt i32 %exp, 2045
  %inc = add i64 %sig, 512
  %carry = icmp slt i64 %inc, 0
  %over = or i1 %big, %carry
  br i1 %over, label %overflow, label %round
overflow:
  %sw = zext i32 %sign to i64
  %s = shl i64 %sw, 63
  %inf = or i64 %s, 9218868437227405312
  ret i64 %inf
round:
  %e = phi i32 [ %exp, %entry ], [ 0, %subnormal ], [ %exp, %check ]
  %m = phi i64 [ %sig, %entry ], [ %ssig, %subnormal ], [ %sig, %check ]
  %low = trunc i64 %m to i32
  %bits = and i32 %low, 1023
  %up = add i64 %m, 512
  %q = lshr i64 %up, 10
  %tie = icmp eq i32 %bits, 512
  %even = and i64 %q, -2
  %r = select i1 %tie, i64 %even, i64 %q
  %packed = call i64 @pack64(i32 %sign, i32 %e, i64 %r)
  ret i64 %packed
}

; round_pack64 for a significand that is not normalized.
define internal i64 @norm_round_pack64(i32 %sign, i32 %exp, i64 %sig) #1 {
entry:
  %lz = call i32 @clz64(i64 %sig)
  %dist = sub i32 %lz, 1
  %e = sub i32 %exp, %dist
  %wide = icmp uge i32 %dist, 10
  %inrange = icmp ult i32 %e, 2045
  %exact = and i1 %wide, %inrange
  br i1 %exact, label %exact.case, label %round
exact.case:
  %d10 = sub i32 %dist, 10
  %s = call i64 @shl64(i64 %sig, i32 %d10)
  %z = icmp eq i64 %sig, 0
  %ee = select i1 %z, i32 0, i32 %e
  %p = call i64 @pack64(i32 %sign, i32 %ee, i64 %s)
  ret i64 %p
round:
  %s2 = call i64 @shl64(i64 %sig, i32 %dist)
  %p2 = call i64 @round_pack64(i32 %sign, i32 %e, i64 %s2)
  ret i64 %p2
}

; The exponent and significand of a finite nonzero value, with the leading 1
; of the significand at bit 52. Subnormals get exponents below 1.
define internal { i32, i64 } @unpack64(i64 %a) #2 {
entry:
  %e = call i32 @exp64(i64 %a)
  %m = and i64 %a, 4503599627370495
  %sub = icmp eq i32 %e, 0
  br i1 %sub, label %subnormal, label %normal
normal:
  %h = or i64 %m, 4503599627370496
  %n.e = insertvalue { i32, i64 } poison, i32 %e, 0
  %n = insertvalue { i32, i64 } %n.e, i64 %h, 1
  ret { i32, i64 } %n
subnormal:
  %lz = call i32 @clz64(i64 %m)
  %d = sub i32 %lz, 11
  %ne = sub i32 1, %d
  %s = call i64 @shl64(i64 %m, i32 %d)
  %s.e = insertvalue { i32, i64 } poison, i32 %ne, 0
  %r = insertvalue { i32, i64 } %s.e, i64 %s, 1
  ret { i32, i64 } %r
}

; ---------------------------------------------------------------------------
; binary64 arithmetic

; |a| + |b| with the sign `sign`.
define internal i64 @add_mags64(i64 %a, i64 %b, i32 %sign) #1 {
entry:
  %ea = call i32 @exp64(i64 %a)
  %eb = call i32 @exp64(i64 %b)
  %ma = and i64 %a, 4503599627370495
  %mb = and i64 %b, 4503599627370495
  %diff = sub i32 %ea, %eb
  %same = icmp eq i32 %diff, 0
  br i1 %same, label %equal, label %unequal
equal:
  %tiny = icmp eq i32 %ea, 0
  br i1 %tiny, label %subnormals, label %equal.check
subnormals:
  ; The fraction sum carries into the exponent if it becomes normal.
  %sum = add i64 %a, %mb
  ret i64 %sum
equal.check:
  %einf = icmp eq i32 %ea, 2047
  br i1 %einf, label %equal.special, label %equal.normal
equal.special:
  %mab = or i64 %ma, %mb
  %enan = icmp ne i64 %mab, 0
  %er = select i1 %enan, i64 9221120237041090560, i64 %a
  ret i64 %er
equal.normal:
  %s0 = add i64 %ma, 9007199254740992
  %s1 = add i64 %s0, %mb
  %s2 = shl i64 %s1, 9
  br label %pack
unequal:
  %sa = shl i64 %ma, 9
  %sb = shl i64 %mb, 9
  %bbig = icmp slt i32 %diff, 0
  br i1 %bbig, label %b.larger, label %a.larger
b.larger:
  %binf = icmp eq i32 %eb, 2047
  br i1 %binf, label %b.special, label %b.finite
b.special:
  %bnan = icmp ne i64 %mb, 0
  %bsw = zext i32 %sign to i64
  %bs = shl i64 %bsw, 63
  %binfz = or i64 %bs, 9218868437227405312
  %br = select i1 %bnan, i64 9221120237041090560, i64 %binfz
  ret i64 %br
b.finite:
  %anorm = icmp ne i32 %ea, 0
  %sa.h = add i64 %sa, 2305843009213693952
  %sa.s = shl i64 %sa, 1
  %sa.x = select i1 %anorm, i64 %sa.h, i64 %sa.s
  %ndiff = sub i32 0, %diff
  %sa.j = call i64 @shr_jam64(i64 %sa.x, i32 %ndiff)
  br label %unequal.sum
a.larger:
  %ainf = icmp eq i32 %ea, 2047
  br i1 %ainf, label %a.special, label %a.finite
a.special:
  %anan = icmp ne i64 %ma, 0
  %ar = select i1 %anan, i64 9221120237041090560, i64 %a
  ret i64 %ar
a.finite:
  %bnorm = icmp ne i32 %eb, 0
  %sb.h = add i64 %sb, 2305843009213693952
  %sb.s = shl i64 %sb, 1
  %sb.x = select i1 %bnorm, i64 %sb.h, i64 %sb.s
  %sb.j = call i64 @shr_jam64(i64 %sb.x, i32 %diff)
  br label %unequal.sum
unequal.sum:
  %ez = phi i32 [ %eb, %b.finite ], [ %ea, %a.finite ]
  %x = phi i64 [ %sa.j, %b.finite ], [ %sa, %a.finite ]
  %y = phi i64 [ %sb, %b.finite ], [ %sb.j, %a.finite ]
  %t0 = add i64 %x, 2305843009213693952
  %t1 = add i64 %t0, %y
  %low = icmp ult i64 %t1, 4611686018427387904
  %t2 = shl i64 %t1, 1
  %ez1 = sub i32 %ez, 1
  %usig = select i1 %low, i64 %t2, i64 %t1
  %uexp = select i1 %low, i32 %ez1, i32 %ez
  br label %pack
pack:
  %pe = phi i32 [ %ea, %equal.normal ], [ %uexp, %unequal.sum ]
  %ps = phi i64 [ %s2, %equal.normal ], [ %usig, %unequal.sum ]
  %r = call i64 @round_pack64(i32 %sign, i32 %pe, i64 %ps)
  ret i64 %r
}

; |a| - |b| with the sign of a, flipped when |b| is larger.
define internal i64 @sub_mags64(i64 %a, i64 %b, i32 %sign) #1 {
entry:
  %ea = call i32 @exp64(i64 %a)
  %eb = call i32 @exp64(i64 %b)
  %ma = and i64 %a, 4503599627370495
  %mb = and i64 %b, 4503599627370495
  %diff = sub i32 %ea, %eb
  %same = icmp eq i32 %diff, 0
  br i1 %same, label %equal, label %unequal
equal:
  %einf = icmp eq i32 %ea, 2047
  br i1 %einf, label %nan, label %equal.finite
nan:
  ret i64 9221120237041090560
equal.finite:
  %d = sub i64 %ma, %mb
  %dz = icmp eq i64 %d, 0
  br i1 %dz, label %zero, label %equal.nonzero
zero:
  ret i64 0
equal.nonzero:
  %enorm = icmp ne i32 %ea, 0
  %ea1 = sub i32 %ea, 1
  %e = select i1 %enorm, i32 %ea1, i32 %ea
  %neg = icmp slt i64 %d, 0
  %flip = xor i32 %sign, 1
  %sz = select i1 %neg, i32 %flip, i32 %sign
  %nd = sub i64 0, %d
  %ad = select i1 %neg, i64 %nd, i64 %d
  %lz = call i32 @clz64(i64 %ad)
  %shift = sub i32 %lz, 11
  %ez = sub i32 %e, %shift
  %under = icmp slt i32 %ez, 0
  %shift.f = select i1 %under, i32 %e, i32 %shift
  %ez.f = select i1 %under, i32 0, i32 %ez
  %m = call i64 @shl64(i64 %ad, i32 %shift.f)
  %p = call i64 @pack64(i32 %sz, i32 %ez.f, i64 %m)
  ret i64 %p
unequal:
  %sa = shl i64 %ma, 10
  %sb = shl i64 %mb, 10
  %bbig = icmp slt i32 %diff, 0
  br i1 %bbig, label %b.larger, label %a.larger
b.larger:
  %bsign = xor i32 %sign, 1
  %binf = icmp eq i32 %eb, 2047
  br i1 %binf, label %b.special, label %b.finite
b.special:
  %bnan = icmp ne i64 %mb, 0
  %bsw = zext i32 %bsign to i64
  %bs = shl i64 %bsw, 63
  %binfz = or i64 %bs, 9218868437227405312
  %br = select i1 %bnan, i64 9221120237041090560, i64 %binfz
  ret i64 %br
b.finite:
  %anorm = icmp ne i32 %ea, 0
  %sa.h = add i64 %sa, 4611686018427387904
  %sa.s = shl i64 %sa, 1
  %sa.x = select i1 %anorm, i64 %sa.h, i64 %sa.s
  %ndiff = sub i32 0, %diff
  %sa.j = call i64 @shr_jam64(i64 %sa.x, i32 %ndiff)
  %sb.h = or i64 %sb, 4611686018427387904
  %bz = sub i64 %sb.h, %sa.j
  br label %norm
a.larger:
  %ainf = icmp eq i32 %ea, 2047
  br i1 %ainf, label %a.special, label %a.finite
a.special:
  %anan = icmp ne i64 %ma, 0
  %ar = select i1 %anan, i64 9221120237041090560, i64 %a
  ret i64 %ar
a.finite:
  %bnorm = icmp ne i32 %eb, 0
  %sb.n = add i64 %sb, 4611686018427387904
  %sb.s = shl i64 %sb, 1
  %sb.x = select i1 %bnorm, i64 %sb.n, i64 %sb.s
  %sb.j = call i64 @shr_jam64(i64 %sb.x, i32 %diff)
  %sa.n = or i64 %sa, 4611686018427387904
  %az = sub i64 %sa.n, %sb.j
  br label %norm
norm:
  %ns = phi i32 [ %bsign, %b.finite ], [ %sign, %a.finite ]
  %ne = phi i32 [ %eb, %b.finite ], [ %ea, %a.finite ]
  %nz = phi i64 [ %bz, %b.finite ], [ %az, %a.finite ]
  %ne1 = sub i32 %ne, 1
  %r = call i64 @norm_round_pack64(i32 %ns, i32 %ne1, i64 %nz)
  ret i64 %r
}

; Two normal operands take a shorter path than add_mags64 and sub_mags64:
; ordered by magnitude, the result has the larger one's sign and exponent
; before normalization, whether the magnitudes add or subtract.
define internal i64 @dadd64(i64 %a, i64 %b) #1 {
entry:
  %x = xor i64 %a, %b
  %same = icmp sgt i64 %x, -1
  %ea = call i32 @exp64(i64 %a)
  %eb = call i32 @exp64(i64 %b)
  %ea1 = sub i32 %ea, 1
  %eb1 = sub i32 %eb, 1
  %an = icmp ult i32 %ea1, 2046
  %bn = icmp ult i32 %eb1, 2046
  %normal = and i1 %an, %bn
  br i1 %normal, label %fast, label %slow
slow:
  %sw = lshr i64 %a, 63
  %sign = trunc i64 %sw to i32
  br i1 %same, label %add, label %sub
add:
  %sum = call i64 @add_mags64(i64 %a, i64 %b, i32 %sign)
  ret i64 %sum
sub:
  %difference = call i64 @sub_mags64(i64 %a, i64 %b, i32 %sign)
  ret i64 %difference
fast:
  %aabs = and i64 %a, 9223372036854775807
  %babs = and i64 %b, 9223372036854775807
  %swap = icmp ult i64 %aabs, %babs
  %big = select i1 %swap, i64 %b, i64 %a
  %small = select i1 %swap, i64 %a, i64 %b
  %eh = call i32 @exp64(i64 %big)
  %el = call i32 @exp64(i64 %small)
  %d = sub i32 %eh, %el
  %bigs = lshr i64 %big, 63
  %zs = trunc i64 %bigs to i32
  %fh = and i64 %big, 4503599627370495
  %mh = or i64 %fh, 4503599627370496
  %fl = and i64 %small, 4503599627370495
  %ml = or i64 %fl, 4503599627370496
  br i1 %same, label %fast.add, label %fast.sub
fast.add:
  ; The larger significand's leading 1 at bit 61, so that the sum's is at
  ; bit 61 or 62.
  %h9 = shl i64 %mh, 9
  %l9 = shl i64 %ml, 9
  %l9j = call i64 @shr_jam64(i64 %l9, i32 %d)
  %s = add i64 %h9, %l9j
  %low = icmp ult i64 %s, 4611686018427387904
  br i1 %low, label %fast.add.shift, label %fast.add.round
fast.add.shift:
  %s2 = shl i64 %s, 1
  %eh1 = sub i32 %eh, 1
  br label %fast.add.round
fast.add.round:
  %ssig = phi i64 [ %s, %fast.add ], [ %s2, %fast.add.shift ]
  %sexp = phi i32 [ %eh, %fast.add ], [ %eh1, %fast.add.shift ]
  %r = call i64 @round_pack64(i32 %zs, i32 %sexp, i64 %ssig)
  ret i64 %r
fast.sub:
  %h10 = shl i64 %mh, 10
  %l10 = shl i64 %ml, 10
  %l10j = call i64 @shr_jam64(i64 %l10, i32 %d)
  %z = sub i64 %h10, %l10j
  %cancel = icmp eq i64 %z, 0
  br i1 %cancel, label %fast.zero, label %fast.difference
fast.zero:
  ; Equal magnitudes: +0 when rounding to nearest.
  ret i64 0
fast.difference:
  %ez = sub i32 %eh, 1
  %n = call i64 @norm_round_pack64(i32 %zs, i32 %ez, i64 %z)
  ret i64 %n
}

define weak i64 @dodo_soft_dadd(i64 %a, i64 %b) #1 {
  %r = call i64 @dadd64(i64 %a, i64 %b)
  ret i64 %r
}

define weak i64 @dodo_soft_dsub(i64 %a, i64 %b) #1 {
  %nb = xor i64 %b, -9223372036854775808
  %r = call i64 @dadd64(i64 %a, i64 %nb)
  ret i64 %r
}

define weak i64 @dodo_soft_dmul(i64 %a, i64 %b) #1 {
entry:
  %x = xor i64 %a, %b
  %s63 = and i64 %x, -9223372036854775808
  %sw = lshr i64 %x, 63
  %sign = trunc i64 %sw to i32
  %aabs = and i64 %a, 9223372036854775807
  %babs = and i64 %b, 9223372036854775807
  %ainf = icmp uge i64 %aabs, 9218868437227405312
  %binf = icmp uge i64 %babs, 9218868437227405312
  %special = or i1 %ainf, %binf
  br i1 %special, label %special.case, label %finite
special.case:
  ; NaN for a NaN operand or infinity times zero, otherwise infinity.
  %anan = icmp ugt i64 %aabs, 9218868437227405312
  %bnan = icmp ugt i64 %babs, 9218868437227405312
  %azero.s = icmp eq i64 %aabs, 0
  %bzero.s = icmp eq i64 %babs, 0
  %n0 = or i1 %anan, %bnan
  %n1 = or i1 %n0, %azero.s
  %n2 = or i1 %n1, %bzero.s
  %inf = or i64 %s63, 9218868437227405312
  %sr = select i1 %n2, i64 9221120237041090560, i64 %inf
  ret i64 %sr
finite:
  %azero = icmp eq i64 %aabs, 0
  %bzero = icmp eq i64 %babs, 0
  %zero = or i1 %azero, %bzero
  br i1 %zero, label %zero.case, label %multiply
zero.case:
  ret i64 %s63
multiply:
  %ua = call { i32, i64 } @unpack64(i64 %a)
  %ub = call { i32, i64 } @unpack64(i64 %b)
  %ea = extractvalue { i32, i64 } %ua, 0
  %ma = extractvalue { i32, i64 } %ua, 1
  %eb = extractvalue { i32, i64 } %ub, 0
  %mb = extractvalue { i32, i64 } %ub, 1
  %eab = add i32 %ea, %eb
  %ez = sub i32 %eab, 1023
  %sa = shl i64 %ma, 10
  %sb = shl i64 %mb, 11
  %p = call { i64, i64 } @mul64x64(i64 %sa, i64 %sb)
  %hi = extractvalue { i64, i64 } %p, 0
  %lo = extractvalue { i64, i64 } %p, 1
  %sticky = icmp ne i64 %lo, 0
  %st = zext i1 %sticky to i64
  %z = or i64 %hi, %st
  %low = icmp ult i64 %z, 4611686018427387904
  %z2 = shl i64 %z, 1
  %ez1 = sub i32 %ez, 1
  %sig = select i1 %low, i64 %z2, i64 %z
  %e = select i1 %low, i32 %ez1, i32 %ez
  %r = call i64 @round_pack64(i32 %sign, i32 %e, i64 %sig)
  ret i64 %r
}

; The quotient's first 31 bits come from the dividend's top 32 bits times
; the reciprocal of the divisor's top 32 bits, the next 24 from the
; remainder's, each digit corrected against its exact remainder. The last
; remainder gives the sticky bit.
define weak i64 @dodo_soft_ddiv(i64 %a, i64 %b) #1 {
entry:
  %x = xor i64 %a, %b
  %s63 = and i64 %x, -9223372036854775808
  %sw = lshr i64 %x, 63
  %sign = trunc i64 %sw to i32
  %aabs = and i64 %a, 9223372036854775807
  %babs = and i64 %b, 9223372036854775807
  %anan = icmp ugt i64 %aabs, 9218868437227405312
  %bnan = icmp ugt i64 %babs, 9218868437227405312
  %ainf = icmp eq i64 %aabs, 9218868437227405312
  %binf = icmp eq i64 %babs, 9218868437227405312
  %azero = icmp eq i64 %aabs, 0
  %bzero = icmp eq i64 %babs, 0
  %n0 = or i1 %anan, %bnan
  %infs = and i1 %ainf, %binf
  %zeros = and i1 %azero, %bzero
  %n1 = or i1 %n0, %infs
  %invalid = or i1 %n1, %zeros
  br i1 %invalid, label %nan, label %check.inf
nan:
  ret i64 9221120237041090560
check.inf:
  %toinf = or i1 %ainf, %bzero
  br i1 %toinf, label %inf, label %check.zero
inf:
  %infz = or i64 %s63, 9218868437227405312
  ret i64 %infz
check.zero:
  %tozero = or i1 %azero, %binf
  br i1 %tozero, label %zero, label %divide
zero:
  ret i64 %s63
divide:
  %ua = call { i32, i64 } @unpack64(i64 %a)
  %ub = call { i32, i64 } @unpack64(i64 %b)
  %ea = extractvalue { i32, i64 } %ua, 0
  %ma = extractvalue { i32, i64 } %ua, 1
  %eb = extractvalue { i32, i64 } %ub, 0
  %mb = extractvalue { i32, i64 } %ub, 1
  %ed = sub i32 %ea, %eb
  %ez = add i32 %ed, 1022
  %less = icmp ult i64 %ma, %mb
  %ma2 = shl i64 %ma, 1
  %ez1 = sub i32 %ez, 1
  %num = select i1 %less, i64 %ma2, i64 %ma
  %e = select i1 %less, i32 %ez1, i32 %ez
  ; num / mb is from 1 to 2. x is about 2^84 / mb.
  %mbt = lshr i64 %mb, 21
  %d = trunc i64 %mbt to i32
  %xr = call i32 @recip32(i32 %d)
  ; q1 = floor(num * 2^30 / mb), from 2^30 to 2^31.
  %numt = lshr i64 %num, 22
  %a1 = trunc i64 %numt to i32
  %p1 = call i64 @mul32x32(i32 %a1, i32 %xr)
  %q1h = lshr i64 %p1, 32
  %q1e = trunc i64 %q1h to i32
  %n30 = shl i64 %num, 30
  %qm1 = call i64 @mul_digit64(i32 %q1e, i64 %mb)
  %r1e = sub i64 %n30, %qm1
  %c1 = call { i32, i64 } @fix_digit64(i32 %q1e, i64 %r1e, i64 %mb)
  %q1 = extractvalue { i32, i64 } %c1, 0
  %r1 = extractvalue { i32, i64 } %c1, 1
  ; q0 = floor(r1 * 2^24 / mb), below 2^24.
  %r1t = lshr i64 %r1, 21
  %a0 = trunc i64 %r1t to i32
  %p0 = call i64 @mul32x32(i32 %a0, i32 %xr)
  %q0h = lshr i64 %p0, 39
  %q0e = trunc i64 %q0h to i32
  %r24 = shl i64 %r1, 24
  %qm0 = call i64 @mul_digit64(i32 %q0e, i64 %mb)
  %r0e = sub i64 %r24, %qm0
  %c0 = call { i32, i64 } @fix_digit64(i32 %q0e, i64 %r0e, i64 %mb)
  %q0 = extractvalue { i32, i64 } %c0, 0
  %r0 = extractvalue { i32, i64 } %c0, 1
  ; floor(num * 2^54 / mb), with its leading 1 at bit 54.
  %q1w = zext i32 %q1 to i64
  %q1s = shl i64 %q1w, 24
  %q0w = zext i32 %q0 to i64
  %q = add i64 %q1s, %q0w
  %qs = shl i64 %q, 8
  %inexact = icmp ne i64 %r0, 0
  %st = zext i1 %inexact to i64
  %sig = or i64 %qs, %st
  %result = call i64 @round_pack64(i32 %sign, i32 %e, i64 %sig)
  ret i64 %result
}

; C fmod: the exact remainder of a / b truncated toward zero, with the sign
; of a. Shift-subtract over the exponent difference, as in musl.
define internal i64 @fmod64(i64 %x, i64 %y) #1 {
entry:
  %sx = and i64 %x, -9223372036854775808
  %ax = and i64 %x, 9223372036854775807
  %ay = and i64 %y, 9223372036854775807
  %ynan = icmp ugt i64 %ay, 9218868437227405312
  %yzero = icmp eq i64 %ay, 0
  %xbad = icmp uge i64 %ax, 9218868437227405312
  %b0 = or i1 %ynan, %yzero
  %bad = or i1 %b0, %xbad
  br i1 %bad, label %nan, label %check.small
nan:
  ret i64 9221120237041090560
check.small:
  %small = icmp ult i64 %ax, %ay
  br i1 %small, label %same, label %check.equal
same:
  ret i64 %x
check.equal:
  %equal = icmp eq i64 %ax, %ay
  br i1 %equal, label %zero, label %divide
zero:
  ret i64 %sx
divide:
  %ux = call { i32, i64 } @unpack64(i64 %x)
  %uy = call { i32, i64 } @unpack64(i64 %y)
  %ex = extractvalue { i32, i64 } %ux, 0
  %mx = extractvalue { i32, i64 } %ux, 1
  %ey = extractvalue { i32, i64 } %uy, 0
  %my = extractvalue { i32, i64 } %uy, 1
  br label %loop.test
loop.test:
  %e = phi i32 [ %ex, %divide ], [ %e.next, %loop.next ]
  %m = phi i64 [ %mx, %divide ], [ %m.next, %loop.next ]
  %d = sub i64 %m, %my
  %dz = icmp eq i64 %d, 0
  br i1 %dz, label %zero, label %loop.step
loop.step:
  %ge = icmp sge i64 %d, 0
  %m1 = select i1 %ge, i64 %d, i64 %m
  %more = icmp sgt i32 %e, %ey
  br i1 %more, label %loop.next, label %finish
loop.next:
  %m.next = shl i64 %m1, 1
  %e.next = sub i32 %e, 1
  br label %loop.test
finish:
  %lz = call i32 @clz64(i64 %m1)
  %sh = sub i32 %lz, 11
  %rn = call i64 @shl64(i64 %m1, i32 %sh)
  %er = sub i32 %e, %sh
  %normal = icmp sgt i32 %er, 0
  br i1 %normal, label %pack.normal, label %pack.subnormal
pack.normal:
  %frac = sub i64 %rn, 4503599627370496
  %erw = zext i32 %er to i64
  %es = shl i64 %erw, 52
  %v0 = or i64 %frac, %es
  %v = or i64 %v0, %sx
  ret i64 %v
pack.subnormal:
  %s = sub i32 1, %er
  %rs = call i64 @lshr64(i64 %rn, i32 %s)
  %vs = or i64 %rs, %sx
  ret i64 %vs
}

define weak i64 @fmod(i64 %x, i64 %y) #1 {
  %r = call i64 @fmod64(i64 %x, i64 %y)
  ret i64 %r
}

; ---------------------------------------------------------------------------
; binary64 comparisons. The EABI ones return 1 or 0; any NaN operand gives 0,
; except for __aeabi_dcmpun.

define internal i1 @unordered64(i64 %a, i64 %b) #2 {
  %an = call i1 @isnan64(i64 %a)
  %bn = call i1 @isnan64(i64 %b)
  %un = or i1 %an, %bn
  ret i1 %un
}

; -1, 0 or 1 as a is less than, equal to or greater than b, and `unordered`
; for a NaN. Unequal ordered values with a nonnegative one among them order
; like their bit patterns as signed integers; two negative ones the other way.
define internal i32 @cmp64(i64 %a, i64 %b, i32 %unordered) #1 {
entry:
  %un = call i1 @unordered64(i64 %a, i64 %b)
  br i1 %un, label %unordered.case, label %ordered
unordered.case:
  ret i32 %unordered
ordered:
  %eq = icmp eq i64 %a, %b
  %or = or i64 %a, %b
  %mag = and i64 %or, 9223372036854775807
  %zeros = icmp eq i64 %mag, 0
  %same = or i1 %eq, %zeros
  br i1 %same, label %equal, label %unequal
equal:
  ret i32 0
unequal:
  %both = and i64 %a, %b
  %neg = icmp slt i64 %both, 0
  %lt = icmp slt i64 %a, %b
  %less = xor i1 %lt, %neg
  %r = select i1 %less, i32 -1, i32 1
  ret i32 %r
}

define weak i32 @__aeabi_dcmpeq(i64 %a, i64 %b) #1 {
  %c = call i32 @cmp64(i64 %a, i64 %b, i32 1)
  %eq = icmp eq i32 %c, 0
  %r = zext i1 %eq to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmplt(i64 %a, i64 %b) #1 {
  %c = call i32 @cmp64(i64 %a, i64 %b, i32 1)
  %lt = icmp slt i32 %c, 0
  %r = zext i1 %lt to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmple(i64 %a, i64 %b) #1 {
  %c = call i32 @cmp64(i64 %a, i64 %b, i32 1)
  %le = icmp slt i32 %c, 1
  %r = zext i1 %le to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmpgt(i64 %a, i64 %b) #1 {
  %c = call i32 @cmp64(i64 %a, i64 %b, i32 -1)
  %gt = icmp sgt i32 %c, 0
  %r = zext i1 %gt to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmpge(i64 %a, i64 %b) #1 {
  %c = call i32 @cmp64(i64 %a, i64 %b, i32 -1)
  %ge = icmp sgt i32 %c, -1
  %r = zext i1 %ge to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmpun(i64 %a, i64 %b) #1 {
  %un = call i1 @unordered64(i64 %a, i64 %b)
  %r = zext i1 %un to i32
  ret i32 %r
}

; The GNU comparisons return cmp64's result, and the caller tests its sign:
; 1 for unordered operands in the less-than and equality family, -1 in the
; greater-than family, so that every ordered test is false.
define weak i32 @__ledf2(i64 %a, i64 %b) #1 {
  %r = call i32 @cmp64(i64 %a, i64 %b, i32 1)
  ret i32 %r
}
define weak i32 @__gedf2(i64 %a, i64 %b) #1 {
  %r = call i32 @cmp64(i64 %a, i64 %b, i32 -1)
  ret i32 %r
}
@__ltdf2 = weak alias i32 (i64, i64), ptr @__ledf2
@__eqdf2 = weak alias i32 (i64, i64), ptr @__ledf2
@__nedf2 = weak alias i32 (i64, i64), ptr @__ledf2
@__cmpdf2 = weak alias i32 (i64, i64), ptr @__ledf2
@__gtdf2 = weak alias i32 (i64, i64), ptr @__gedf2

; ---------------------------------------------------------------------------
; binary64 conversions

; Truncates toward zero. Negative values, NaN and values below 1 give 0;
; values from 2^64 up give the maximum.
define weak i64 @__aeabi_d2ulz(i64 %a) #1 {
entry:
  %e = call i32 @exp64(i64 %a)
  %small = icmp ult i32 %e, 1023
  %nan = call i1 @isnan64(i64 %a)
  %neg = icmp slt i64 %a, 0
  %z0 = or i1 %small, %nan
  %z = or i1 %z0, %neg
  br i1 %z, label %zero, label %check
zero:
  ret i64 0
check:
  %huge = icmp uge i32 %e, 1087
  br i1 %huge, label %max, label %convert
max:
  ret i64 -1
convert:
  ; The significand with its leading 1 at bit 63, shifted down to the units.
  %f = shl i64 %a, 11
  %top = or i64 %f, -9223372036854775808
  %sr = sub i32 1086, %e
  %v = call i64 @lshr64(i64 %top, i32 %sr)
  ret i64 %v
}

; Truncates toward zero. NaN gives 0; magnitudes from 2^63 up saturate.
define weak i64 @__aeabi_d2lz(i64 %a) #1 {
entry:
  %e = call i32 @exp64(i64 %a)
  %small = icmp ult i32 %e, 1023
  %nan = call i1 @isnan64(i64 %a)
  %z = or i1 %small, %nan
  br i1 %z, label %zero, label %check
zero:
  ret i64 0
check:
  %neg = icmp slt i64 %a, 0
  %huge = icmp uge i32 %e, 1086
  br i1 %huge, label %saturate, label %convert
saturate:
  %sat = select i1 %neg, i64 -9223372036854775808, i64 9223372036854775807
  ret i64 %sat
convert:
  %f = shl i64 %a, 11
  %top = or i64 %f, -9223372036854775808
  %sr = sub i32 1086, %e
  %v = call i64 @lshr64(i64 %top, i32 %sr)
  %nv = sub i64 0, %v
  %sv = select i1 %neg, i64 %nv, i64 %v
  ret i64 %sv
}

; The top 32 bits of a finite value's significand, leading 1 at bit 31.
define internal i32 @top32_64(i64 %a) #2 {
  %h = lshr i64 %a, 21
  %t = trunc i64 %h to i32
  %r = or i32 %t, -2147483648
  ret i32 %r
}

define weak i32 @dodo_soft_d2iz(i64 %a) #1 {
entry:
  %e = call i32 @exp64(i64 %a)
  %small = icmp ult i32 %e, 1023
  %nan = call i1 @isnan64(i64 %a)
  %z = or i1 %small, %nan
  br i1 %z, label %zero, label %check
zero:
  ret i32 0
check:
  %neg = icmp slt i64 %a, 0
  %huge = icmp uge i32 %e, 1054
  br i1 %huge, label %saturate, label %convert
saturate:
  %sat = select i1 %neg, i32 -2147483648, i32 2147483647
  ret i32 %sat
convert:
  %t = call i32 @top32_64(i64 %a)
  %sr = sub i32 1054, %e
  %v = lshr i32 %t, %sr
  %nv = sub i32 0, %v
  %sv = select i1 %neg, i32 %nv, i32 %v
  ret i32 %sv
}

define weak i32 @dodo_soft_d2uiz(i64 %a) #1 {
entry:
  %e = call i32 @exp64(i64 %a)
  %small = icmp ult i32 %e, 1023
  %nan = call i1 @isnan64(i64 %a)
  %neg = icmp slt i64 %a, 0
  %z0 = or i1 %small, %nan
  %z = or i1 %z0, %neg
  br i1 %z, label %zero, label %check
zero:
  ret i32 0
check:
  %huge = icmp uge i32 %e, 1055
  br i1 %huge, label %max, label %convert
max:
  ret i32 -1
convert:
  %t = call i32 @top32_64(i64 %a)
  %sr = sub i32 1054, %e
  %v = lshr i32 %t, %sr
  ret i32 %v
}

define internal i64 @u64_to_f64(i32 %sign, i64 %m) #1 {
entry:
  %z = icmp eq i64 %m, 0
  br i1 %z, label %zero, label %convert
zero:
  %sw = zext i32 %sign to i64
  %s = shl i64 %sw, 63
  ret i64 %s
convert:
  %lz = call i32 @clz64(i64 %m)
  %top = call i64 @shl64(i64 %m, i32 %lz)
  %lost = and i64 %top, 1
  %half = lshr i64 %top, 1
  %sig = or i64 %half, %lost
  %e = sub i32 1085, %lz
  %r = call i64 @round_pack64(i32 %sign, i32 %e, i64 %sig)
  ret i64 %r
}

define weak i64 @__aeabi_ul2d(i64 %x) #1 {
  %r = call i64 @u64_to_f64(i32 0, i64 %x)
  ret i64 %r
}
define weak i64 @__aeabi_l2d(i64 %x) #1 {
  %neg = icmp slt i64 %x, 0
  %nx = sub i64 0, %x
  %mag = select i1 %neg, i64 %nx, i64 %x
  %sign = zext i1 %neg to i32
  %r = call i64 @u64_to_f64(i32 %sign, i64 %mag)
  ret i64 %r
}

; A 32-bit integer is exact in binary64: no rounding, all in 32-bit words.
define internal i64 @u32_to_f64(i32 %sign, i32 %m) #2 {
entry:
  %z = icmp eq i32 %m, 0
  br i1 %z, label %zero, label %convert
zero:
  %sw = zext i32 %sign to i64
  %s = shl i64 %sw, 63
  ret i64 %s
convert:
  %lz = call i32 @clz32(i32 %m)
  %n = shl i32 %m, %lz
  %e = sub i32 1054, %lz
  %s31 = shl i32 %sign, 31
  %es = shl i32 %e, 20
  %f = shl i32 %n, 1
  %fh = lshr i32 %f, 12
  %h0 = or i32 %s31, %es
  %hi = or i32 %h0, %fh
  %lo = shl i32 %n, 21
  %hw = zext i32 %hi to i64
  %hs = shl i64 %hw, 32
  %lw = zext i32 %lo to i64
  %r = or i64 %hs, %lw
  ret i64 %r
}

define weak i64 @dodo_soft_ui2d(i32 %x) #1 {
  %r = call i64 @u32_to_f64(i32 0, i32 %x)
  ret i64 %r
}
define weak i64 @dodo_soft_i2d(i32 %x) #1 {
  %sign = lshr i32 %x, 31
  %nx = sub i32 0, %x
  %neg = icmp slt i32 %x, 0
  %mag = select i1 %neg, i32 %nx, i32 %x
  %r = call i64 @u32_to_f64(i32 %sign, i32 %mag)
  ret i64 %r
}

; ---------------------------------------------------------------------------
; binary32 packing and conversion to and from binary64. Bit patterns:
; sign 0x80000000 = -2147483648, infinity 0x7F800000 = 2139095040, default
; NaN 0x7FC00000 = 2143289344, magnitude mask 2147483647, fraction mask
; 0x007FFFFF = 8388607, hidden bit 2^23 = 8388608.

define internal i1 @isnan32(i32 %a) #2 {
  %abs = and i32 %a, 2147483647
  %nan = icmp ugt i32 %abs, 2139095040
  ret i1 %nan
}

define internal i32 @exp32(i32 %a) #2 {
  %s = lshr i32 %a, 23
  %e = and i32 %s, 255
  ret i32 %e
}

define internal i32 @pack32(i32 %sign, i32 %exp, i32 %sig) #2 {
  %ss = shl i32 %sign, 31
  %es = shl i32 %exp, 23
  %p0 = add i32 %ss, %es
  %p = add i32 %p0, %sig
  ret i32 %p
}

; round_pack64 for binary32: the leading 1 at bit 30 and seven rounding
; bits.
define internal i32 @round_pack32(i32 %sign, i32 %exp, i32 %sig) #1 {
entry:
  %edge = icmp uge i32 %exp, 253
  br i1 %edge, label %edge.case, label %round
edge.case:
  %tiny = icmp slt i32 %exp, 0
  br i1 %tiny, label %subnormal, label %check
subnormal:
  %dist = sub i32 0, %exp
  %ssig = call i32 @shr_jam32(i32 %sig, i32 %dist)
  br label %round
check:
  %big = icmp sgt i32 %exp, 253
  %inc = add i32 %sig, 64
  %carry = icmp slt i32 %inc, 0
  %over = or i1 %big, %carry
  br i1 %over, label %overflow, label %round
overflow:
  %s = shl i32 %sign, 31
  %inf = or i32 %s, 2139095040
  ret i32 %inf
round:
  %e = phi i32 [ %exp, %entry ], [ 0, %subnormal ], [ %exp, %check ]
  %m = phi i32 [ %sig, %entry ], [ %ssig, %subnormal ], [ %sig, %check ]
  %bits = and i32 %m, 127
  %up = add i32 %m, 64
  %q = lshr i32 %up, 7
  %tie = icmp eq i32 %bits, 64
  %even = and i32 %q, -2
  %r = select i1 %tie, i32 %even, i32 %q
  %p = call i32 @pack32(i32 %sign, i32 %e, i32 %r)
  ret i32 %p
}

; round_pack32 for a significand that is not normalized.
define internal i32 @norm_round_pack32(i32 %sign, i32 %exp, i32 %sig) #1 {
entry:
  %lz = call i32 @clz32(i32 %sig)
  %dist = sub i32 %lz, 1
  %e = sub i32 %exp, %dist
  %wide = icmp uge i32 %dist, 7
  %inrange = icmp ult i32 %e, 253
  %exact = and i1 %wide, %inrange
  br i1 %exact, label %exact.case, label %round
exact.case:
  %d7 = sub i32 %dist, 7
  %s = shl i32 %sig, %d7
  %z = icmp eq i32 %sig, 0
  %ee = select i1 %z, i32 0, i32 %e
  %p = call i32 @pack32(i32 %sign, i32 %ee, i32 %s)
  ret i32 %p
round:
  %s2 = shl i32 %sig, %dist
  %p2 = call i32 @round_pack32(i32 %sign, i32 %e, i32 %s2)
  ret i32 %p2
}

; unpack64 for binary32: the leading 1 at bit 23.
define internal { i32, i32 } @unpack32(i32 %a) #2 {
entry:
  %e = call i32 @exp32(i32 %a)
  %m = and i32 %a, 8388607
  %sub = icmp eq i32 %e, 0
  br i1 %sub, label %subnormal, label %normal
normal:
  %h = or i32 %m, 8388608
  %n.e = insertvalue { i32, i32 } poison, i32 %e, 0
  %n = insertvalue { i32, i32 } %n.e, i32 %h, 1
  ret { i32, i32 } %n
subnormal:
  %lz = call i32 @clz32(i32 %m)
  %d = sub i32 %lz, 8
  %ne = sub i32 1, %d
  %s = shl i32 %m, %d
  %s.e = insertvalue { i32, i32 } poison, i32 %ne, 0
  %r = insertvalue { i32, i32 } %s.e, i32 %s, 1
  ret { i32, i32 } %r
}

define weak i64 @dodo_soft_f2d(i32 %a) #1 {
entry:
  %top = lshr i32 %a, 31
  %sign = zext i32 %top to i64
  %s63 = shl i64 %sign, 63
  %e = call i32 @exp32(i32 %a)
  %m = and i32 %a, 8388607
  %special = icmp eq i32 %e, 255
  br i1 %special, label %special.case, label %finite
special.case:
  %mz = icmp eq i32 %m, 0
  %inf = or i64 %s63, 9218868437227405312
  %sr = select i1 %mz, i64 %inf, i64 9221120237041090560
  ret i64 %sr
finite:
  %tiny = icmp eq i32 %e, 0
  br i1 %tiny, label %small, label %pack
small:
  %zero = icmp eq i32 %m, 0
  br i1 %zero, label %zero.case, label %subnormal
zero.case:
  ret i64 %s63
subnormal:
  %lz = call i32 @clz32(i32 %m)
  %d = sub i32 %lz, 8
  %ne = sub i32 1, %d
  %nm = shl i32 %m, %d
  %nf = and i32 %nm, 8388607
  br label %pack
pack:
  %pe = phi i32 [ %e, %finite ], [ %ne, %subnormal ]
  %pm = phi i32 [ %m, %finite ], [ %nf, %subnormal ]
  %de = add i32 %pe, 896
  %dew = zext i32 %de to i64
  %des = shl i64 %dew, 52
  %pmw = zext i32 %pm to i64
  %pms = shl i64 %pmw, 29
  %r0 = or i64 %s63, %des
  %r = or i64 %r0, %pms
  ret i64 %r
}

define weak i32 @dodo_soft_d2f(i64 %a) #1 {
entry:
  %top = lshr i64 %a, 63
  %sign = trunc i64 %top to i32
  %e = call i32 @exp64(i64 %a)
  %m = and i64 %a, 4503599627370495
  %special = icmp eq i32 %e, 2047
  br i1 %special, label %special.case, label %finite
special.case:
  %mz = icmp eq i64 %m, 0
  %s = shl i32 %sign, 31
  %inf = or i32 %s, 2139095040
  %sr = select i1 %mz, i32 %inf, i32 2143289344
  ret i32 %sr
finite:
  %hi = lshr i64 %m, 22
  %low = and i64 %m, 4194303
  %lost = icmp ne i64 %low, 0
  %st = zext i1 %lost to i64
  %f = or i64 %hi, %st
  %f32 = trunc i64 %f to i32
  %ef = or i32 %e, %f32
  %zero = icmp eq i32 %ef, 0
  br i1 %zero, label %zero.case, label %round
zero.case:
  %sz = shl i32 %sign, 31
  ret i32 %sz
round:
  %sig = or i32 %f32, 1073741824
  %e2 = sub i32 %e, 897
  %r = call i32 @round_pack32(i32 %sign, i32 %e2, i32 %sig)
  ret i32 %r
}

; ---------------------------------------------------------------------------
; binary32 arithmetic, as for binary64.

define internal i32 @add_mags32(i32 %a, i32 %b, i32 %sign) #1 {
entry:
  %ea = call i32 @exp32(i32 %a)
  %eb = call i32 @exp32(i32 %b)
  %ma = and i32 %a, 8388607
  %mb = and i32 %b, 8388607
  %diff = sub i32 %ea, %eb
  %same = icmp eq i32 %diff, 0
  br i1 %same, label %equal, label %unequal
equal:
  %tiny = icmp eq i32 %ea, 0
  br i1 %tiny, label %subnormals, label %equal.check
subnormals:
  %sum = add i32 %a, %mb
  ret i32 %sum
equal.check:
  %einf = icmp eq i32 %ea, 255
  br i1 %einf, label %equal.special, label %equal.normal
equal.special:
  %mab = or i32 %ma, %mb
  %enan = icmp ne i32 %mab, 0
  %er = select i1 %enan, i32 2143289344, i32 %a
  ret i32 %er
equal.normal:
  %s0 = add i32 %ma, 16777216
  %s1 = add i32 %s0, %mb
  %s2 = shl i32 %s1, 6
  br label %pack
unequal:
  %sa = shl i32 %ma, 6
  %sb = shl i32 %mb, 6
  %bbig = icmp slt i32 %diff, 0
  br i1 %bbig, label %b.larger, label %a.larger
b.larger:
  %binf = icmp eq i32 %eb, 255
  br i1 %binf, label %b.special, label %b.finite
b.special:
  %bnan = icmp ne i32 %mb, 0
  %bs = shl i32 %sign, 31
  %binfz = or i32 %bs, 2139095040
  %br = select i1 %bnan, i32 2143289344, i32 %binfz
  ret i32 %br
b.finite:
  %anorm = icmp ne i32 %ea, 0
  %sa.h = add i32 %sa, 536870912
  %sa.s = shl i32 %sa, 1
  %sa.x = select i1 %anorm, i32 %sa.h, i32 %sa.s
  %ndiff = sub i32 0, %diff
  %sa.j = call i32 @shr_jam32(i32 %sa.x, i32 %ndiff)
  br label %unequal.sum
a.larger:
  %ainf = icmp eq i32 %ea, 255
  br i1 %ainf, label %a.special, label %a.finite
a.special:
  %anan = icmp ne i32 %ma, 0
  %ar = select i1 %anan, i32 2143289344, i32 %a
  ret i32 %ar
a.finite:
  %bnorm = icmp ne i32 %eb, 0
  %sb.h = add i32 %sb, 536870912
  %sb.s = shl i32 %sb, 1
  %sb.x = select i1 %bnorm, i32 %sb.h, i32 %sb.s
  %sb.j = call i32 @shr_jam32(i32 %sb.x, i32 %diff)
  br label %unequal.sum
unequal.sum:
  %ez = phi i32 [ %eb, %b.finite ], [ %ea, %a.finite ]
  %x = phi i32 [ %sa.j, %b.finite ], [ %sa, %a.finite ]
  %y = phi i32 [ %sb, %b.finite ], [ %sb.j, %a.finite ]
  %t0 = add i32 %x, 536870912
  %t1 = add i32 %t0, %y
  %low = icmp ult i32 %t1, 1073741824
  %t2 = shl i32 %t1, 1
  %ez1 = sub i32 %ez, 1
  %usig = select i1 %low, i32 %t2, i32 %t1
  %uexp = select i1 %low, i32 %ez1, i32 %ez
  br label %pack
pack:
  %pe = phi i32 [ %ea, %equal.normal ], [ %uexp, %unequal.sum ]
  %ps = phi i32 [ %s2, %equal.normal ], [ %usig, %unequal.sum ]
  %r = call i32 @round_pack32(i32 %sign, i32 %pe, i32 %ps)
  ret i32 %r
}

define internal i32 @sub_mags32(i32 %a, i32 %b, i32 %sign) #1 {
entry:
  %ea = call i32 @exp32(i32 %a)
  %eb = call i32 @exp32(i32 %b)
  %ma = and i32 %a, 8388607
  %mb = and i32 %b, 8388607
  %diff = sub i32 %ea, %eb
  %same = icmp eq i32 %diff, 0
  br i1 %same, label %equal, label %unequal
equal:
  %einf = icmp eq i32 %ea, 255
  br i1 %einf, label %nan, label %equal.finite
nan:
  ret i32 2143289344
equal.finite:
  %d = sub i32 %ma, %mb
  %dz = icmp eq i32 %d, 0
  br i1 %dz, label %zero, label %equal.nonzero
zero:
  ret i32 0
equal.nonzero:
  %enorm = icmp ne i32 %ea, 0
  %ea1 = sub i32 %ea, 1
  %e = select i1 %enorm, i32 %ea1, i32 %ea
  %neg = icmp slt i32 %d, 0
  %flip = xor i32 %sign, 1
  %sz = select i1 %neg, i32 %flip, i32 %sign
  %nd = sub i32 0, %d
  %ad = select i1 %neg, i32 %nd, i32 %d
  %lz = call i32 @clz32(i32 %ad)
  %shift = sub i32 %lz, 8
  %ez = sub i32 %e, %shift
  %under = icmp slt i32 %ez, 0
  %shift.f = select i1 %under, i32 %e, i32 %shift
  %ez.f = select i1 %under, i32 0, i32 %ez
  %m = shl i32 %ad, %shift.f
  %p = call i32 @pack32(i32 %sz, i32 %ez.f, i32 %m)
  ret i32 %p
unequal:
  %sa = shl i32 %ma, 7
  %sb = shl i32 %mb, 7
  %bbig = icmp slt i32 %diff, 0
  br i1 %bbig, label %b.larger, label %a.larger
b.larger:
  %bsign = xor i32 %sign, 1
  %binf = icmp eq i32 %eb, 255
  br i1 %binf, label %b.special, label %b.finite
b.special:
  %bnan = icmp ne i32 %mb, 0
  %bs = shl i32 %bsign, 31
  %binfz = or i32 %bs, 2139095040
  %br = select i1 %bnan, i32 2143289344, i32 %binfz
  ret i32 %br
b.finite:
  %anorm = icmp ne i32 %ea, 0
  %sa.h = add i32 %sa, 1073741824
  %sa.s = shl i32 %sa, 1
  %sa.x = select i1 %anorm, i32 %sa.h, i32 %sa.s
  %ndiff = sub i32 0, %diff
  %sa.j = call i32 @shr_jam32(i32 %sa.x, i32 %ndiff)
  %sb.h = or i32 %sb, 1073741824
  %bz = sub i32 %sb.h, %sa.j
  br label %norm
a.larger:
  %ainf = icmp eq i32 %ea, 255
  br i1 %ainf, label %a.special, label %a.finite
a.special:
  %anan = icmp ne i32 %ma, 0
  %ar = select i1 %anan, i32 2143289344, i32 %a
  ret i32 %ar
a.finite:
  %bnorm = icmp ne i32 %eb, 0
  %sb.n = add i32 %sb, 1073741824
  %sb.s = shl i32 %sb, 1
  %sb.x = select i1 %bnorm, i32 %sb.n, i32 %sb.s
  %sb.j = call i32 @shr_jam32(i32 %sb.x, i32 %diff)
  %sa.n = or i32 %sa, 1073741824
  %az = sub i32 %sa.n, %sb.j
  br label %norm
norm:
  %ns = phi i32 [ %bsign, %b.finite ], [ %sign, %a.finite ]
  %ne = phi i32 [ %eb, %b.finite ], [ %ea, %a.finite ]
  %nz = phi i32 [ %bz, %b.finite ], [ %az, %a.finite ]
  %ne1 = sub i32 %ne, 1
  %r = call i32 @norm_round_pack32(i32 %ns, i32 %ne1, i32 %nz)
  ret i32 %r
}

define internal i32 @fadd32(i32 %a, i32 %b) #1 {
entry:
  %x = xor i32 %a, %b
  %same = icmp sgt i32 %x, -1
  %sign = lshr i32 %a, 31
  br i1 %same, label %add, label %sub
add:
  %sum = call i32 @add_mags32(i32 %a, i32 %b, i32 %sign)
  ret i32 %sum
sub:
  %difference = call i32 @sub_mags32(i32 %a, i32 %b, i32 %sign)
  ret i32 %difference
}

define weak i32 @dodo_soft_fadd(i32 %a, i32 %b) #1 {
  %r = call i32 @fadd32(i32 %a, i32 %b)
  ret i32 %r
}

define weak i32 @dodo_soft_fsub(i32 %a, i32 %b) #1 {
  %nb = xor i32 %b, -2147483648
  %r = call i32 @fadd32(i32 %a, i32 %nb)
  ret i32 %r
}

; The 48-bit product of the significands, from four 16x16-bit products.
define weak i32 @dodo_soft_fmul(i32 %a, i32 %b) #1 {
entry:
  %x = xor i32 %a, %b
  %sign = lshr i32 %x, 31
  %s31 = and i32 %x, -2147483648
  %aabs = and i32 %a, 2147483647
  %babs = and i32 %b, 2147483647
  %ainf = icmp uge i32 %aabs, 2139095040
  %binf = icmp uge i32 %babs, 2139095040
  %special = or i1 %ainf, %binf
  br i1 %special, label %special.case, label %finite
special.case:
  ; NaN for a NaN operand or infinity times zero, otherwise infinity.
  %anan = icmp ugt i32 %aabs, 2139095040
  %bnan = icmp ugt i32 %babs, 2139095040
  %azero.s = icmp eq i32 %aabs, 0
  %bzero.s = icmp eq i32 %babs, 0
  %n0 = or i1 %anan, %bnan
  %n1 = or i1 %n0, %azero.s
  %n2 = or i1 %n1, %bzero.s
  %inf = or i32 %s31, 2139095040
  %sr = select i1 %n2, i32 2143289344, i32 %inf
  ret i32 %sr
finite:
  %azero = icmp eq i32 %aabs, 0
  %bzero = icmp eq i32 %babs, 0
  %zero = or i1 %azero, %bzero
  br i1 %zero, label %zero.case, label %multiply
zero.case:
  ret i32 %s31
multiply:
  %ua = call { i32, i32 } @unpack32(i32 %a)
  %ub = call { i32, i32 } @unpack32(i32 %b)
  %ea = extractvalue { i32, i32 } %ua, 0
  %ma = extractvalue { i32, i32 } %ua, 1
  %eb = extractvalue { i32, i32 } %ub, 0
  %mb = extractvalue { i32, i32 } %ub, 1
  %eab = add i32 %ea, %eb
  %ez = sub i32 %eab, 127
  ; The product is from 2^46 to 2^48: its top bits, leading 1 at bit 29 or
  ; 30, and the 17 below as the sticky bit.
  %p = call i64 @mul32x32(i32 %ma, i32 %mb)
  %ph = lshr i64 %p, 17
  %hi = trunc i64 %ph to i32
  %pl = trunc i64 %p to i32
  %lost = and i32 %pl, 131071
  %sticky = icmp ne i32 %lost, 0
  %st = zext i1 %sticky to i32
  %z = or i32 %hi, %st
  %low = icmp ult i32 %z, 1073741824
  %z2 = shl i32 %z, 1
  %ez1 = sub i32 %ez, 1
  %sig = select i1 %low, i32 %z2, i32 %z
  %e = select i1 %low, i32 %ez1, i32 %ez
  %r = call i32 @round_pack32(i32 %sign, i32 %e, i32 %sig)
  ret i32 %r
}

; One quotient digit of 26 bits from the reciprocal, corrected against its
; exact remainder, which then gives the sticky bit.
define weak i32 @__aeabi_fdiv(i32 %a, i32 %b) #1 {
entry:
  %x = xor i32 %a, %b
  %sign = lshr i32 %x, 31
  %s31 = and i32 %x, -2147483648
  %aabs = and i32 %a, 2147483647
  %babs = and i32 %b, 2147483647
  %anan = icmp ugt i32 %aabs, 2139095040
  %bnan = icmp ugt i32 %babs, 2139095040
  %ainf = icmp eq i32 %aabs, 2139095040
  %binf = icmp eq i32 %babs, 2139095040
  %azero = icmp eq i32 %aabs, 0
  %bzero = icmp eq i32 %babs, 0
  %n0 = or i1 %anan, %bnan
  %infs = and i1 %ainf, %binf
  %zeros = and i1 %azero, %bzero
  %n1 = or i1 %n0, %infs
  %invalid = or i1 %n1, %zeros
  br i1 %invalid, label %nan, label %check.inf
nan:
  ret i32 2143289344
check.inf:
  %toinf = or i1 %ainf, %bzero
  br i1 %toinf, label %inf, label %check.zero
inf:
  %infz = or i32 %s31, 2139095040
  ret i32 %infz
check.zero:
  %tozero = or i1 %azero, %binf
  br i1 %tozero, label %zero, label %divide
zero:
  ret i32 %s31
divide:
  %ua = call { i32, i32 } @unpack32(i32 %a)
  %ub = call { i32, i32 } @unpack32(i32 %b)
  %ea = extractvalue { i32, i32 } %ua, 0
  %ma = extractvalue { i32, i32 } %ua, 1
  %eb = extractvalue { i32, i32 } %ub, 0
  %mb = extractvalue { i32, i32 } %ub, 1
  %ed = sub i32 %ea, %eb
  %ez = add i32 %ed, 126
  %less = icmp ult i32 %ma, %mb
  %ma2 = shl i32 %ma, 1
  %ez1 = sub i32 %ez, 1
  %num = select i1 %less, i32 %ma2, i32 %ma
  %e = select i1 %less, i32 %ez1, i32 %ez
  ; num / mb is from 1 to 2. x is about 2^55 / mb, and
  ; q = floor(num * 2^25 / mb), with its leading 1 at bit 25.
  %d = shl i32 %mb, 8
  %xr = call i32 @recip32(i32 %d)
  %p = call i64 @mul32x32(i32 %num, i32 %xr)
  %ph = lshr i64 %p, 30
  %qe = trunc i64 %ph to i32
  %n25 = shl i32 %num, 25
  %qm = mul i32 %qe, %mb
  %re = sub i32 %n25, %qm
  %c = call { i32, i32 } @fix_digit32(i32 %qe, i32 %re, i32 %mb)
  %q = extractvalue { i32, i32 } %c, 0
  %rem = extractvalue { i32, i32 } %c, 1
  %qs = shl i32 %q, 5
  %inexact = icmp ne i32 %rem, 0
  %st = zext i1 %inexact to i32
  %sig = or i32 %qs, %st
  %r = call i32 @round_pack32(i32 %sign, i32 %e, i32 %sig)
  ret i32 %r
}

; fmod for binary32.
define weak i32 @fmodf(i32 %x, i32 %y) #1 {
entry:
  %sx = and i32 %x, -2147483648
  %ax = and i32 %x, 2147483647
  %ay = and i32 %y, 2147483647
  %ynan = icmp ugt i32 %ay, 2139095040
  %yzero = icmp eq i32 %ay, 0
  %xbad = icmp uge i32 %ax, 2139095040
  %b0 = or i1 %ynan, %yzero
  %bad = or i1 %b0, %xbad
  br i1 %bad, label %nan, label %check.small
nan:
  ret i32 2143289344
check.small:
  %small = icmp ult i32 %ax, %ay
  br i1 %small, label %same, label %check.equal
same:
  ret i32 %x
check.equal:
  %equal = icmp eq i32 %ax, %ay
  br i1 %equal, label %zero, label %divide
zero:
  ret i32 %sx
divide:
  %ux = call { i32, i32 } @unpack32(i32 %x)
  %uy = call { i32, i32 } @unpack32(i32 %y)
  %ex = extractvalue { i32, i32 } %ux, 0
  %mx = extractvalue { i32, i32 } %ux, 1
  %ey = extractvalue { i32, i32 } %uy, 0
  %my = extractvalue { i32, i32 } %uy, 1
  br label %loop.test
loop.test:
  %e = phi i32 [ %ex, %divide ], [ %e.next, %loop.next ]
  %m = phi i32 [ %mx, %divide ], [ %m.next, %loop.next ]
  %d = sub i32 %m, %my
  %dz = icmp eq i32 %d, 0
  br i1 %dz, label %zero, label %loop.step
loop.step:
  %ge = icmp sge i32 %d, 0
  %m1 = select i1 %ge, i32 %d, i32 %m
  %more = icmp sgt i32 %e, %ey
  br i1 %more, label %loop.next, label %finish
loop.next:
  %m.next = shl i32 %m1, 1
  %e.next = sub i32 %e, 1
  br label %loop.test
finish:
  %lz = call i32 @clz32(i32 %m1)
  %sh = sub i32 %lz, 8
  %rn = shl i32 %m1, %sh
  %er = sub i32 %e, %sh
  %normal = icmp sgt i32 %er, 0
  br i1 %normal, label %pack.normal, label %pack.subnormal
pack.normal:
  %frac = sub i32 %rn, 8388608
  %es = shl i32 %er, 23
  %v0 = or i32 %frac, %es
  %v = or i32 %v0, %sx
  ret i32 %v
pack.subnormal:
  %s = sub i32 1, %er
  %rs = lshr i32 %rn, %s
  %vs = or i32 %rs, %sx
  ret i32 %vs
}

; ---------------------------------------------------------------------------
; binary32 comparisons, as for binary64.

define internal i1 @unordered32(i32 %a, i32 %b) #2 {
  %an = call i1 @isnan32(i32 %a)
  %bn = call i1 @isnan32(i32 %b)
  %un = or i1 %an, %bn
  ret i1 %un
}

; -1, 0 or 1 as a is less than, equal to or greater than b, and `unordered`
; for a NaN. Unequal ordered values with a nonnegative one among them order
; like their bit patterns as signed integers; two negative ones the other way.
define internal i32 @cmp32(i32 %a, i32 %b, i32 %unordered) #1 {
entry:
  %un = call i1 @unordered32(i32 %a, i32 %b)
  br i1 %un, label %unordered.case, label %ordered
unordered.case:
  ret i32 %unordered
ordered:
  %eq = icmp eq i32 %a, %b
  %or = or i32 %a, %b
  %mag = and i32 %or, 2147483647
  %zeros = icmp eq i32 %mag, 0
  %same = or i1 %eq, %zeros
  br i1 %same, label %equal, label %unequal
equal:
  ret i32 0
unequal:
  %both = and i32 %a, %b
  %neg = icmp slt i32 %both, 0
  %lt = icmp slt i32 %a, %b
  %less = xor i1 %lt, %neg
  %r = select i1 %less, i32 -1, i32 1
  ret i32 %r
}

define weak i32 @__aeabi_fcmpeq(i32 %a, i32 %b) #1 {
  %c = call i32 @cmp32(i32 %a, i32 %b, i32 1)
  %eq = icmp eq i32 %c, 0
  %r = zext i1 %eq to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmplt(i32 %a, i32 %b) #1 {
  %c = call i32 @cmp32(i32 %a, i32 %b, i32 1)
  %lt = icmp slt i32 %c, 0
  %r = zext i1 %lt to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmple(i32 %a, i32 %b) #1 {
  %c = call i32 @cmp32(i32 %a, i32 %b, i32 1)
  %le = icmp slt i32 %c, 1
  %r = zext i1 %le to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmpgt(i32 %a, i32 %b) #1 {
  %c = call i32 @cmp32(i32 %a, i32 %b, i32 -1)
  %gt = icmp sgt i32 %c, 0
  %r = zext i1 %gt to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmpge(i32 %a, i32 %b) #1 {
  %c = call i32 @cmp32(i32 %a, i32 %b, i32 -1)
  %ge = icmp sgt i32 %c, -1
  %r = zext i1 %ge to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmpun(i32 %a, i32 %b) #1 {
  %un = call i1 @unordered32(i32 %a, i32 %b)
  %r = zext i1 %un to i32
  ret i32 %r
}

; The GNU comparisons return cmp32's result, and the caller tests its sign:
; 1 for unordered operands in the less-than and equality family, -1 in the
; greater-than family, so that every ordered test is false.
define weak i32 @__lesf2(i32 %a, i32 %b) #1 {
  %r = call i32 @cmp32(i32 %a, i32 %b, i32 1)
  ret i32 %r
}
define weak i32 @__gesf2(i32 %a, i32 %b) #1 {
  %r = call i32 @cmp32(i32 %a, i32 %b, i32 -1)
  ret i32 %r
}
@__ltsf2 = weak alias i32 (i32, i32), ptr @__lesf2
@__eqsf2 = weak alias i32 (i32, i32), ptr @__lesf2
@__nesf2 = weak alias i32 (i32, i32), ptr @__lesf2
@__cmpsf2 = weak alias i32 (i32, i32), ptr @__lesf2
@__gtsf2 = weak alias i32 (i32, i32), ptr @__gesf2

; ---------------------------------------------------------------------------
; binary32 integer conversions, with the same saturation as binary64's.

define weak i32 @__aeabi_f2iz(i32 %a) #1 {
entry:
  %e = call i32 @exp32(i32 %a)
  %small = icmp ult i32 %e, 127
  %nan = call i1 @isnan32(i32 %a)
  %z = or i1 %small, %nan
  br i1 %z, label %zero, label %check
zero:
  ret i32 0
check:
  %neg = icmp slt i32 %a, 0
  %huge = icmp uge i32 %e, 158
  br i1 %huge, label %saturate, label %convert
saturate:
  %sat = select i1 %neg, i32 -2147483648, i32 2147483647
  ret i32 %sat
convert:
  %f = shl i32 %a, 8
  %t = or i32 %f, -2147483648
  %sr = sub i32 158, %e
  %v = lshr i32 %t, %sr
  %nv = sub i32 0, %v
  %sv = select i1 %neg, i32 %nv, i32 %v
  ret i32 %sv
}

define weak i32 @__aeabi_f2uiz(i32 %a) #1 {
entry:
  %e = call i32 @exp32(i32 %a)
  %small = icmp ult i32 %e, 127
  %nan = call i1 @isnan32(i32 %a)
  %neg = icmp slt i32 %a, 0
  %z0 = or i1 %small, %nan
  %z = or i1 %z0, %neg
  br i1 %z, label %zero, label %check
zero:
  ret i32 0
check:
  %huge = icmp uge i32 %e, 159
  br i1 %huge, label %max, label %convert
max:
  ret i32 -1
convert:
  %f = shl i32 %a, 8
  %t = or i32 %f, -2147483648
  %sr = sub i32 158, %e
  %v = lshr i32 %t, %sr
  ret i32 %v
}

; The significand with its leading 1 at bit 63.
define internal i64 @top64_32(i32 %a) #2 {
  %f = shl i32 %a, 8
  %t = or i32 %f, -2147483648
  %w = zext i32 %t to i64
  %r = shl i64 %w, 32
  ret i64 %r
}

define weak i64 @__aeabi_f2lz(i32 %a) #1 {
entry:
  %e = call i32 @exp32(i32 %a)
  %small = icmp ult i32 %e, 127
  %nan = call i1 @isnan32(i32 %a)
  %z = or i1 %small, %nan
  br i1 %z, label %zero, label %check
zero:
  ret i64 0
check:
  %neg = icmp slt i32 %a, 0
  %huge = icmp uge i32 %e, 190
  br i1 %huge, label %saturate, label %convert
saturate:
  %sat = select i1 %neg, i64 -9223372036854775808, i64 9223372036854775807
  ret i64 %sat
convert:
  %t = call i64 @top64_32(i32 %a)
  %sr = sub i32 190, %e
  %v = call i64 @lshr64(i64 %t, i32 %sr)
  %nv = sub i64 0, %v
  %sv = select i1 %neg, i64 %nv, i64 %v
  ret i64 %sv
}

define weak i64 @__aeabi_f2ulz(i32 %a) #1 {
entry:
  %e = call i32 @exp32(i32 %a)
  %small = icmp ult i32 %e, 127
  %nan = call i1 @isnan32(i32 %a)
  %neg = icmp slt i32 %a, 0
  %z0 = or i1 %small, %nan
  %z = or i1 %z0, %neg
  br i1 %z, label %zero, label %check
zero:
  ret i64 0
check:
  %huge = icmp uge i32 %e, 191
  br i1 %huge, label %max, label %convert
max:
  ret i64 -1
convert:
  %t = call i64 @top64_32(i32 %a)
  %sr = sub i32 190, %e
  %v = call i64 @lshr64(i64 %t, i32 %sr)
  ret i64 %v
}

define internal i32 @u32_to_f32(i32 %sign, i32 %m) #1 {
entry:
  %z = icmp eq i32 %m, 0
  br i1 %z, label %zero, label %convert
zero:
  %s = shl i32 %sign, 31
  ret i32 %s
convert:
  %lz = call i32 @clz32(i32 %m)
  %n = shl i32 %m, %lz
  %lost = and i32 %n, 1
  %half = lshr i32 %n, 1
  %sig = or i32 %half, %lost
  %e = sub i32 157, %lz
  %r = call i32 @round_pack32(i32 %sign, i32 %e, i32 %sig)
  ret i32 %r
}

define weak i32 @__aeabi_i2f(i32 %x) #1 {
  %sign = lshr i32 %x, 31
  %nx = sub i32 0, %x
  %neg = icmp slt i32 %x, 0
  %mag = select i1 %neg, i32 %nx, i32 %x
  %r = call i32 @u32_to_f32(i32 %sign, i32 %mag)
  ret i32 %r
}
define weak i32 @__aeabi_ui2f(i32 %x) #1 {
  %r = call i32 @u32_to_f32(i32 0, i32 %x)
  ret i32 %r
}

; The top 31 significant bits, with the rest folded into a sticky bit.
define internal i32 @u64_to_f32(i32 %sign, i64 %m) #1 {
entry:
  %z = icmp eq i64 %m, 0
  br i1 %z, label %zero, label %convert
zero:
  %s = shl i32 %sign, 31
  ret i32 %s
convert:
  %lz = call i32 @clz64(i64 %m)
  %n = call i64 @shl64(i64 %m, i32 %lz)
  %nh = lshr i64 %n, 32
  %hi = trunc i64 %nh to i32
  %lo = trunc i64 %n to i32
  %low = and i32 %hi, 1
  %rest = or i32 %low, %lo
  %lost = icmp ne i32 %rest, 0
  %st = zext i1 %lost to i32
  %half = lshr i32 %hi, 1
  %sig = or i32 %half, %st
  %e = sub i32 189, %lz
  %r = call i32 @round_pack32(i32 %sign, i32 %e, i32 %sig)
  ret i32 %r
}

define weak i32 @__aeabi_ul2f(i64 %x) #1 {
  %r = call i32 @u64_to_f32(i32 0, i64 %x)
  ret i32 %r
}
define weak i32 @__aeabi_l2f(i64 %x) #1 {
  %neg = icmp slt i64 %x, 0
  %nx = sub i64 0, %x
  %mag = select i1 %neg, i64 %nx, i64 %x
  %sign = zext i1 %neg to i32
  %r = call i32 @u64_to_f32(i32 %sign, i64 %mag)
  ret i32 %r
}

; ---------------------------------------------------------------------------
; GNU names for the helpers above. Dodo's own code generator calls these;
; other LLVM configurations call the EABI names.

@__divsf3 = weak alias i32 (i32, i32), ptr @__aeabi_fdiv
@__unorddf2 = weak alias i32 (i64, i64), ptr @__aeabi_dcmpun
@__unordsf2 = weak alias i32 (i32, i32), ptr @__aeabi_fcmpun
@__fixdfdi = weak alias i64 (i64), ptr @__aeabi_d2lz
@__fixunsdfdi = weak alias i64 (i64), ptr @__aeabi_d2ulz
@__fixsfsi = weak alias i32 (i32), ptr @__aeabi_f2iz
@__fixunssfsi = weak alias i32 (i32), ptr @__aeabi_f2uiz
@__fixsfdi = weak alias i64 (i32), ptr @__aeabi_f2lz
@__fixunssfdi = weak alias i64 (i32), ptr @__aeabi_f2ulz
@__floatdidf = weak alias i64 (i64), ptr @__aeabi_l2d
@__floatundidf = weak alias i64 (i64), ptr @__aeabi_ul2d
@__floatsisf = weak alias i32 (i32), ptr @__aeabi_i2f
@__floatunsisf = weak alias i32 (i32), ptr @__aeabi_ui2f
@__floatdisf = weak alias i32 (i64), ptr @__aeabi_l2f
@__floatundisf = weak alias i32 (i64), ptr @__aeabi_ul2f
