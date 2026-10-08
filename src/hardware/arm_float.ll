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
; Results round to nearest, ties to even, and subnormals are kept. NaN
; results are the default quiet NaN; there are no exception flags.
; Out-of-range conversions to integers saturate and NaN converts to zero;
; Dodo checks the range before converting.
;
; Everything is integer code: a float is its bits in an i32 and a double
; its bits in an i64, which the soft-float base ABI passes in the same
; registers. The binary64 algorithms follow Berkeley SoftFloat 3. binary32
; addition, subtraction, multiplication and division compute in binary64 and
; round once more: binary64 has more than 2 * 24 + 2 significand bits, so the
; double rounding gives the correctly rounded binary32 result. These helpers
; need only the integer helpers from arm_eabi.ll, and on a 64-bit host they
; need nothing, which is how tests/firmware.rs checks them against hardware.

attributes #1 = { nounwind "no-builtins" }

; ---------------------------------------------------------------------------
; Bit utilities. Armv6-M has no clz instruction, and LLVM would call
; __clzsi2 for llvm.ctlz.

define internal i32 @clz32(i32 %x) #1 {
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

define internal i32 @clz64(i64 %x) #1 {
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

; Shift right, setting the lowest bit if any 1 bits were shifted out, so
; rounding still sees them.
define internal i64 @shr_jam64(i64 %a, i32 %dist) #1 {
  %d = zext i32 %dist to i64
  %short = icmp ult i32 %dist, 63
  %r = lshr i64 %a, %d
  %back = sub i64 0, %d
  %bm = and i64 %back, 63
  %lost = shl i64 %a, %bm
  %some = icmp ne i64 %lost, 0
  %moved = icmp ne i32 %dist, 0
  %jam = and i1 %some, %moved
  %j = zext i1 %jam to i64
  %rj = or i64 %r, %j
  %any = icmp ne i64 %a, 0
  %all = zext i1 %any to i64
  %out = select i1 %short, i64 %rj, i64 %all
  ret i64 %out
}

define internal i32 @shr_jam32(i32 %a, i32 %dist) #1 {
  %short = icmp ult i32 %dist, 31
  %r = lshr i32 %a, %dist
  %back = sub i32 0, %dist
  %bm = and i32 %back, 31
  %lost = shl i32 %a, %bm
  %some = icmp ne i32 %lost, 0
  %moved = icmp ne i32 %dist, 0
  %jam = and i1 %some, %moved
  %j = zext i1 %jam to i32
  %rj = or i32 %r, %j
  %any = icmp ne i32 %a, 0
  %all = zext i1 %any to i32
  %out = select i1 %short, i32 %rj, i32 %all
  ret i32 %out
}

; The full 128-bit product as { high, low }, from 32x32-bit products.
define internal { i64, i64 } @umul64x64(i64 %a, i64 %b) #1 {
  %a0 = and i64 %a, 4294967295
  %a1 = lshr i64 %a, 32
  %b0 = and i64 %b, 4294967295
  %b1 = lshr i64 %b, 32
  %p00 = mul i64 %a0, %b0
  %p01 = mul i64 %a0, %b1
  %p10 = mul i64 %a1, %b0
  %p11 = mul i64 %a1, %b1
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
; binary64 packing. Bit patterns: sign 0x8000000000000000 =
; -9223372036854775808, infinity 0x7FF0000000000000 = 9218868437227405312,
; default NaN 0x7FF8000000000000 = 9221120237041090560, magnitude mask
; 9223372036854775807, fraction mask 0x000FFFFFFFFFFFFF = 4503599627370495,
; hidden bit 2^52 = 4503599627370496.

define internal i32 @exp64(i64 %a) #1 {
  %s = lshr i64 %a, 52
  %t = trunc i64 %s to i32
  %e = and i32 %t, 2047
  ret i32 %e
}

define internal i1 @isnan64(i64 %a) #1 {
  %abs = and i64 %a, 9223372036854775807
  %nan = icmp ugt i64 %abs, 9218868437227405312
  ret i1 %nan
}

; Adds rather than ors the fields, so a significand carry into bit 52
; increments the exponent.
define internal i64 @pack64(i64 %sign, i32 %exp, i64 %sig) #1 {
  %s = shl i64 %sign, 63
  %e = zext i32 %exp to i64
  %es = shl i64 %e, 52
  %a = add i64 %s, %es
  %r = add i64 %a, %sig
  ret i64 %r
}

; Rounds a significand with its leading 1 at bit 62 and ten bits below the
; result's last bit. `exp` is one less than the biased exponent; out of range
; it underflows to a subnormal or zero, or overflows to infinity.
define internal i64 @round_pack64(i64 %sign, i32 %exp, i64 %sig) #1 {
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
  %s = shl i64 %sign, 63
  %inf = or i64 %s, 9218868437227405312
  ret i64 %inf
round:
  %e = phi i32 [ %exp, %entry ], [ 0, %subnormal ], [ %exp, %check ]
  %m = phi i64 [ %sig, %entry ], [ %ssig, %subnormal ], [ %sig, %check ]
  %bits = and i64 %m, 1023
  %up = add i64 %m, 512
  %q = lshr i64 %up, 10
  %tie = icmp eq i64 %bits, 512
  %even = and i64 %q, -2
  %r = select i1 %tie, i64 %even, i64 %q
  %zero = icmp eq i64 %r, 0
  %ez = select i1 %zero, i32 0, i32 %e
  %packed = call i64 @pack64(i64 %sign, i32 %ez, i64 %r)
  ret i64 %packed
}

; round_pack64 for a significand that is not normalized.
define internal i64 @norm_round_pack64(i64 %sign, i32 %exp, i64 %sig) #1 {
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
  %d10w = zext i32 %d10 to i64
  %s = shl i64 %sig, %d10w
  %z = icmp eq i64 %sig, 0
  %ee = select i1 %z, i32 0, i32 %e
  %p = call i64 @pack64(i64 %sign, i32 %ee, i64 %s)
  ret i64 %p
round:
  %dw = zext i32 %dist to i64
  %s2 = shl i64 %sig, %dw
  %p2 = call i64 @round_pack64(i64 %sign, i32 %e, i64 %s2)
  ret i64 %p2
}

; The exponent and significand of a finite nonzero value, with the leading 1
; of the significand at bit 52. Subnormals get exponents below 1.
define internal { i32, i64 } @unpack64(i64 %a) #1 {
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
  %dw = zext i32 %d to i64
  %s = shl i64 %m, %dw
  %s.e = insertvalue { i32, i64 } poison, i32 %ne, 0
  %r = insertvalue { i32, i64 } %s.e, i64 %s, 1
  ret { i32, i64 } %r
}

; ---------------------------------------------------------------------------
; binary64 arithmetic

; |a| + |b| with the sign `sign`.
define internal i64 @add_mags64(i64 %a, i64 %b, i64 %sign) #1 {
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
  %bs = shl i64 %sign, 63
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
  %r = call i64 @round_pack64(i64 %sign, i32 %pe, i64 %ps)
  ret i64 %r
}

; |a| - |b| with the sign of a, flipped when |b| is larger.
define internal i64 @sub_mags64(i64 %a, i64 %b, i64 %sign) #1 {
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
  %flip = xor i64 %sign, 1
  %sz = select i1 %neg, i64 %flip, i64 %sign
  %nd = sub i64 0, %d
  %ad = select i1 %neg, i64 %nd, i64 %d
  %lz = call i32 @clz64(i64 %ad)
  %shift = sub i32 %lz, 11
  %ez = sub i32 %e, %shift
  %under = icmp slt i32 %ez, 0
  %shift.f = select i1 %under, i32 %e, i32 %shift
  %ez.f = select i1 %under, i32 0, i32 %ez
  %sw = zext i32 %shift.f to i64
  %m = shl i64 %ad, %sw
  %p = call i64 @pack64(i64 %sz, i32 %ez.f, i64 %m)
  ret i64 %p
unequal:
  %sa = shl i64 %ma, 10
  %sb = shl i64 %mb, 10
  %bbig = icmp slt i32 %diff, 0
  br i1 %bbig, label %b.larger, label %a.larger
b.larger:
  %bsign = xor i64 %sign, 1
  %binf = icmp eq i32 %eb, 2047
  br i1 %binf, label %b.special, label %b.finite
b.special:
  %bnan = icmp ne i64 %mb, 0
  %bs = shl i64 %bsign, 63
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
  %ns = phi i64 [ %bsign, %b.finite ], [ %sign, %a.finite ]
  %ne = phi i32 [ %eb, %b.finite ], [ %ea, %a.finite ]
  %nz = phi i64 [ %bz, %b.finite ], [ %az, %a.finite ]
  %ne1 = sub i32 %ne, 1
  %r = call i64 @norm_round_pack64(i64 %ns, i32 %ne1, i64 %nz)
  ret i64 %r
}

define weak i64 @__aeabi_dadd(i64 %a, i64 %b) #1 {
entry:
  %x = xor i64 %a, %b
  %same = icmp sgt i64 %x, -1
  %sign = lshr i64 %a, 63
  br i1 %same, label %add, label %sub
add:
  %sum = call i64 @add_mags64(i64 %a, i64 %b, i64 %sign)
  ret i64 %sum
sub:
  %difference = call i64 @sub_mags64(i64 %a, i64 %b, i64 %sign)
  ret i64 %difference
}

define weak i64 @__aeabi_dsub(i64 %a, i64 %b) #1 {
  %nb = xor i64 %b, -9223372036854775808
  %r = call i64 @__aeabi_dadd(i64 %a, i64 %nb)
  ret i64 %r
}

define weak i64 @__aeabi_dmul(i64 %a, i64 %b) #1 {
entry:
  %x = xor i64 %a, %b
  %sign = lshr i64 %x, 63
  %s63 = shl i64 %sign, 63
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
  %p = call { i64, i64 } @umul64x64(i64 %sa, i64 %sb)
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
  %r = call i64 @round_pack64(i64 %sign, i32 %e, i64 %sig)
  ret i64 %r
}

; Restoring division, one quotient bit per step, then the remainder as the
; sticky bit.
define weak i64 @__aeabi_ddiv(i64 %a, i64 %b) #1 {
entry:
  %x = xor i64 %a, %b
  %sign = lshr i64 %x, 63
  %s63 = shl i64 %sign, 63
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
  br label %loop
loop:
  %i = phi i32 [ 63, %divide ], [ %i.next, %loop ]
  %r = phi i64 [ %num, %divide ], [ %r.next, %loop ]
  %q = phi i64 [ 0, %divide ], [ %q.next, %loop ]
  %take = icmp uge i64 %r, %mb
  %rs = sub i64 %r, %mb
  %r1 = select i1 %take, i64 %rs, i64 %r
  %bit = zext i1 %take to i64
  %q.shift = shl i64 %q, 1
  %q.next = or i64 %q.shift, %bit
  %r.next = shl i64 %r1, 1
  %i.next = sub i32 %i, 1
  %done = icmp eq i32 %i.next, 0
  br i1 %done, label %exit, label %loop
exit:
  %inexact = icmp ne i64 %r.next, 0
  %st = zext i1 %inexact to i64
  %qz = or i64 %q.next, %st
  %result = call i64 @round_pack64(i64 %sign, i32 %e, i64 %qz)
  ret i64 %result
}

; C fmod: the exact remainder of a / b truncated toward zero, with the sign
; of a. Shift-subtract over the exponent difference, as in musl.
define weak i64 @fmod(i64 %x, i64 %y) #1 {
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
  %shw = zext i32 %sh to i64
  %rn = shl i64 %m1, %shw
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
  %sw = zext i32 %s to i64
  %rs = lshr i64 %rn, %sw
  %vs = or i64 %rs, %sx
  ret i64 %vs
}

; ---------------------------------------------------------------------------
; binary64 comparisons. Each returns 1 or 0; any NaN operand gives 0, except
; for __aeabi_dcmpun.

define internal i1 @lt64(i64 %a, i64 %b) #1 {
  %an = call i1 @isnan64(i64 %a)
  %bn = call i1 @isnan64(i64 %b)
  %un = or i1 %an, %bn
  %sa = icmp slt i64 %a, 0
  %sb = icmp slt i64 %b, 0
  %signs = xor i1 %sa, %sb
  %or = or i64 %a, %b
  %mag = and i64 %or, 9223372036854775807
  %nonzero = icmp ne i64 %mag, 0
  %differ = and i1 %sa, %nonzero
  %ne = icmp ne i64 %a, %b
  %below = icmp ult i64 %a, %b
  %order = xor i1 %sa, %below
  %alike = and i1 %ne, %order
  %lt = select i1 %signs, i1 %differ, i1 %alike
  %ordered = xor i1 %un, true
  %r = and i1 %ordered, %lt
  ret i1 %r
}

define internal i1 @le64(i64 %a, i64 %b) #1 {
  %an = call i1 @isnan64(i64 %a)
  %bn = call i1 @isnan64(i64 %b)
  %un = or i1 %an, %bn
  %sa = icmp slt i64 %a, 0
  %sb = icmp slt i64 %b, 0
  %signs = xor i1 %sa, %sb
  %or = or i64 %a, %b
  %mag = and i64 %or, 9223372036854775807
  %zeros = icmp eq i64 %mag, 0
  %differ = or i1 %sa, %zeros
  %eq = icmp eq i64 %a, %b
  %below = icmp ult i64 %a, %b
  %order = xor i1 %sa, %below
  %alike = or i1 %eq, %order
  %le = select i1 %signs, i1 %differ, i1 %alike
  %ordered = xor i1 %un, true
  %r = and i1 %ordered, %le
  ret i1 %r
}

define weak i32 @__aeabi_dcmpeq(i64 %a, i64 %b) #1 {
  %le = call i1 @le64(i64 %a, i64 %b)
  %ge = call i1 @le64(i64 %b, i64 %a)
  %eq = and i1 %le, %ge
  %r = zext i1 %eq to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmplt(i64 %a, i64 %b) #1 {
  %c = call i1 @lt64(i64 %a, i64 %b)
  %r = zext i1 %c to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmple(i64 %a, i64 %b) #1 {
  %c = call i1 @le64(i64 %a, i64 %b)
  %r = zext i1 %c to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmpgt(i64 %a, i64 %b) #1 {
  %c = call i1 @lt64(i64 %b, i64 %a)
  %r = zext i1 %c to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmpge(i64 %a, i64 %b) #1 {
  %c = call i1 @le64(i64 %b, i64 %a)
  %r = zext i1 %c to i32
  ret i32 %r
}
define weak i32 @__aeabi_dcmpun(i64 %a, i64 %b) #1 {
  %an = call i1 @isnan64(i64 %a)
  %bn = call i1 @isnan64(i64 %b)
  %un = or i1 %an, %bn
  %r = zext i1 %un to i32
  ret i32 %r
}

; The GNU comparisons return -1, 0 or 1 as a is less than, equal to or
; greater than b, and the caller tests the sign. Unordered operands give
; `unordered`: 1 for the less-than and equality family, -1 for the
; greater-than family, so that every ordered test is false.
define internal i32 @cmp64(i64 %a, i64 %b, i32 %unordered) #1 {
  %an = call i1 @isnan64(i64 %a)
  %bn = call i1 @isnan64(i64 %b)
  %un = or i1 %an, %bn
  %lt = call i1 @lt64(i64 %a, i64 %b)
  %le = call i1 @le64(i64 %a, i64 %b)
  %ge = call i1 @le64(i64 %b, i64 %a)
  %eq = and i1 %le, %ge
  %r0 = select i1 %eq, i32 0, i32 1
  %r1 = select i1 %lt, i32 -1, i32 %r0
  %r = select i1 %un, i32 %unordered, i32 %r1
  ret i32 %r
}
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
  %f = and i64 %a, 4503599627370495
  %m = or i64 %f, 4503599627370496
  %left = icmp uge i32 %e, 1075
  %sl = sub i32 %e, 1075
  %sr = sub i32 1075, %e
  %slw = zext i32 %sl to i64
  %srw = zext i32 %sr to i64
  %l = shl i64 %m, %slw
  %r = lshr i64 %m, %srw
  %v = select i1 %left, i64 %l, i64 %r
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
  %f = and i64 %a, 4503599627370495
  %m = or i64 %f, 4503599627370496
  %left = icmp uge i32 %e, 1075
  %sl = sub i32 %e, 1075
  %sr = sub i32 1075, %e
  %slw = zext i32 %sl to i64
  %srw = zext i32 %sr to i64
  %l = shl i64 %m, %slw
  %r = lshr i64 %m, %srw
  %v = select i1 %left, i64 %l, i64 %r
  %nv = sub i64 0, %v
  %sv = select i1 %neg, i64 %nv, i64 %v
  ret i64 %sv
}

define weak i32 @__aeabi_d2iz(i64 %a) #1 {
  %v = call i64 @__aeabi_d2lz(i64 %a)
  %over = icmp sgt i64 %v, 2147483647
  %c0 = select i1 %over, i64 2147483647, i64 %v
  %under = icmp slt i64 %c0, -2147483648
  %c1 = select i1 %under, i64 -2147483648, i64 %c0
  %r = trunc i64 %c1 to i32
  ret i32 %r
}

define weak i32 @__aeabi_d2uiz(i64 %a) #1 {
  %v = call i64 @__aeabi_d2ulz(i64 %a)
  %over = icmp ugt i64 %v, 4294967295
  %c = select i1 %over, i64 4294967295, i64 %v
  %r = trunc i64 %c to i32
  ret i32 %r
}

define internal i64 @u64_to_f64(i64 %sign, i64 %m) #1 {
entry:
  %z = icmp eq i64 %m, 0
  br i1 %z, label %zero, label %convert
zero:
  %s = shl i64 %sign, 63
  ret i64 %s
convert:
  %lz = call i32 @clz64(i64 %m)
  %lzw = zext i32 %lz to i64
  %top = shl i64 %m, %lzw
  %lost = and i64 %top, 1
  %half = lshr i64 %top, 1
  %sig = or i64 %half, %lost
  %e = sub i32 1085, %lz
  %r = call i64 @round_pack64(i64 %sign, i32 %e, i64 %sig)
  ret i64 %r
}

define weak i64 @__aeabi_ul2d(i64 %x) #1 {
  %r = call i64 @u64_to_f64(i64 0, i64 %x)
  ret i64 %r
}
define weak i64 @__aeabi_l2d(i64 %x) #1 {
  %neg = icmp slt i64 %x, 0
  %nx = sub i64 0, %x
  %mag = select i1 %neg, i64 %nx, i64 %x
  %sign = zext i1 %neg to i64
  %r = call i64 @u64_to_f64(i64 %sign, i64 %mag)
  ret i64 %r
}
define weak i64 @__aeabi_ui2d(i32 %x) #1 {
  %w = zext i32 %x to i64
  %r = call i64 @u64_to_f64(i64 0, i64 %w)
  ret i64 %r
}
define weak i64 @__aeabi_i2d(i32 %x) #1 {
  %w = sext i32 %x to i64
  %r = call i64 @__aeabi_l2d(i64 %w)
  ret i64 %r
}

; ---------------------------------------------------------------------------
; binary32 packing and conversion to and from binary64. Bit patterns:
; infinity 0x7F800000 = 2139095040, default NaN 0x7FC00000 = 2143289344,
; magnitude mask 2147483647, fraction mask 0x007FFFFF = 8388607.

define internal i1 @isnan32(i32 %a) #1 {
  %abs = and i32 %a, 2147483647
  %nan = icmp ugt i32 %abs, 2139095040
  ret i1 %nan
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
  %zero = icmp eq i32 %r, 0
  %ez = select i1 %zero, i32 0, i32 %e
  %ss = shl i32 %sign, 31
  %es = shl i32 %ez, 23
  %p0 = add i32 %ss, %es
  %p = add i32 %p0, %r
  ret i32 %p
}

define weak i64 @__aeabi_f2d(i32 %a) #1 {
entry:
  %top = lshr i32 %a, 31
  %sign = zext i32 %top to i64
  %s63 = shl i64 %sign, 63
  %es = lshr i32 %a, 23
  %e = and i32 %es, 255
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

define weak i32 @__aeabi_d2f(i64 %a) #1 {
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
; binary32 arithmetic, through binary64 (see the top of the file).

define weak i32 @__aeabi_fadd(i32 %a, i32 %b) #1 {
  %da = call i64 @__aeabi_f2d(i32 %a)
  %db = call i64 @__aeabi_f2d(i32 %b)
  %d = call i64 @__aeabi_dadd(i64 %da, i64 %db)
  %r = call i32 @__aeabi_d2f(i64 %d)
  ret i32 %r
}
define weak i32 @__aeabi_fsub(i32 %a, i32 %b) #1 {
  %da = call i64 @__aeabi_f2d(i32 %a)
  %db = call i64 @__aeabi_f2d(i32 %b)
  %d = call i64 @__aeabi_dsub(i64 %da, i64 %db)
  %r = call i32 @__aeabi_d2f(i64 %d)
  ret i32 %r
}
define weak i32 @__aeabi_fmul(i32 %a, i32 %b) #1 {
  %da = call i64 @__aeabi_f2d(i32 %a)
  %db = call i64 @__aeabi_f2d(i32 %b)
  %d = call i64 @__aeabi_dmul(i64 %da, i64 %db)
  %r = call i32 @__aeabi_d2f(i64 %d)
  ret i32 %r
}
define weak i32 @__aeabi_fdiv(i32 %a, i32 %b) #1 {
  %da = call i64 @__aeabi_f2d(i32 %a)
  %db = call i64 @__aeabi_f2d(i32 %b)
  %d = call i64 @__aeabi_ddiv(i64 %da, i64 %db)
  %r = call i32 @__aeabi_d2f(i64 %d)
  ret i32 %r
}
; The remainder is exact in binary64 and representable in binary32.
define weak i32 @fmodf(i32 %a, i32 %b) #1 {
  %da = call i64 @__aeabi_f2d(i32 %a)
  %db = call i64 @__aeabi_f2d(i32 %b)
  %d = call i64 @fmod(i64 %da, i64 %db)
  %r = call i32 @__aeabi_d2f(i64 %d)
  ret i32 %r
}

; ---------------------------------------------------------------------------
; binary32 comparisons

define internal i1 @lt32(i32 %a, i32 %b) #1 {
  %an = call i1 @isnan32(i32 %a)
  %bn = call i1 @isnan32(i32 %b)
  %un = or i1 %an, %bn
  %sa = icmp slt i32 %a, 0
  %sb = icmp slt i32 %b, 0
  %signs = xor i1 %sa, %sb
  %or = or i32 %a, %b
  %mag = and i32 %or, 2147483647
  %nonzero = icmp ne i32 %mag, 0
  %differ = and i1 %sa, %nonzero
  %ne = icmp ne i32 %a, %b
  %below = icmp ult i32 %a, %b
  %order = xor i1 %sa, %below
  %alike = and i1 %ne, %order
  %lt = select i1 %signs, i1 %differ, i1 %alike
  %ordered = xor i1 %un, true
  %r = and i1 %ordered, %lt
  ret i1 %r
}

define internal i1 @le32(i32 %a, i32 %b) #1 {
  %an = call i1 @isnan32(i32 %a)
  %bn = call i1 @isnan32(i32 %b)
  %un = or i1 %an, %bn
  %sa = icmp slt i32 %a, 0
  %sb = icmp slt i32 %b, 0
  %signs = xor i1 %sa, %sb
  %or = or i32 %a, %b
  %mag = and i32 %or, 2147483647
  %zeros = icmp eq i32 %mag, 0
  %differ = or i1 %sa, %zeros
  %eq = icmp eq i32 %a, %b
  %below = icmp ult i32 %a, %b
  %order = xor i1 %sa, %below
  %alike = or i1 %eq, %order
  %le = select i1 %signs, i1 %differ, i1 %alike
  %ordered = xor i1 %un, true
  %r = and i1 %ordered, %le
  ret i1 %r
}

define weak i32 @__aeabi_fcmpeq(i32 %a, i32 %b) #1 {
  %le = call i1 @le32(i32 %a, i32 %b)
  %ge = call i1 @le32(i32 %b, i32 %a)
  %eq = and i1 %le, %ge
  %r = zext i1 %eq to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmplt(i32 %a, i32 %b) #1 {
  %c = call i1 @lt32(i32 %a, i32 %b)
  %r = zext i1 %c to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmple(i32 %a, i32 %b) #1 {
  %c = call i1 @le32(i32 %a, i32 %b)
  %r = zext i1 %c to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmpgt(i32 %a, i32 %b) #1 {
  %c = call i1 @lt32(i32 %b, i32 %a)
  %r = zext i1 %c to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmpge(i32 %a, i32 %b) #1 {
  %c = call i1 @le32(i32 %b, i32 %a)
  %r = zext i1 %c to i32
  ret i32 %r
}
define weak i32 @__aeabi_fcmpun(i32 %a, i32 %b) #1 {
  %an = call i1 @isnan32(i32 %a)
  %bn = call i1 @isnan32(i32 %b)
  %un = or i1 %an, %bn
  %r = zext i1 %un to i32
  ret i32 %r
}

; GNU comparisons, as cmp64.
define internal i32 @cmp32(i32 %a, i32 %b, i32 %unordered) #1 {
  %an = call i1 @isnan32(i32 %a)
  %bn = call i1 @isnan32(i32 %b)
  %un = or i1 %an, %bn
  %lt = call i1 @lt32(i32 %a, i32 %b)
  %le = call i1 @le32(i32 %a, i32 %b)
  %ge = call i1 @le32(i32 %b, i32 %a)
  %eq = and i1 %le, %ge
  %r0 = select i1 %eq, i32 0, i32 1
  %r1 = select i1 %lt, i32 -1, i32 %r0
  %r = select i1 %un, i32 %unordered, i32 %r1
  ret i32 %r
}
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
; binary32 integer conversions. To integers, and from 32-bit integers, go
; through binary64 exactly. From 64-bit integers, the bits below the top 32
; significant ones fold into a sticky bit first, so the value is exact in
; binary64 and rounds only once, to binary32.

define weak i32 @__aeabi_f2iz(i32 %a) #1 {
  %d = call i64 @__aeabi_f2d(i32 %a)
  %r = call i32 @__aeabi_d2iz(i64 %d)
  ret i32 %r
}
define weak i32 @__aeabi_f2uiz(i32 %a) #1 {
  %d = call i64 @__aeabi_f2d(i32 %a)
  %r = call i32 @__aeabi_d2uiz(i64 %d)
  ret i32 %r
}
define weak i64 @__aeabi_f2lz(i32 %a) #1 {
  %d = call i64 @__aeabi_f2d(i32 %a)
  %r = call i64 @__aeabi_d2lz(i64 %d)
  ret i64 %r
}
define weak i64 @__aeabi_f2ulz(i32 %a) #1 {
  %d = call i64 @__aeabi_f2d(i32 %a)
  %r = call i64 @__aeabi_d2ulz(i64 %d)
  ret i64 %r
}
define weak i32 @__aeabi_i2f(i32 %x) #1 {
  %d = call i64 @__aeabi_i2d(i32 %x)
  %r = call i32 @__aeabi_d2f(i64 %d)
  ret i32 %r
}
define weak i32 @__aeabi_ui2f(i32 %x) #1 {
  %d = call i64 @__aeabi_ui2d(i32 %x)
  %r = call i32 @__aeabi_d2f(i64 %d)
  ret i32 %r
}

define internal i32 @u64_to_f32(i64 %sign, i64 %m) #1 {
  %lz = call i32 @clz64(i64 %m)
  %wide = icmp ult i32 %lz, 32
  %drop = sub i32 32, %lz
  %dropw = zext i32 %drop to i64
  %j = call i64 @shr_jam64(i64 %m, i32 %drop)
  %back = shl i64 %j, %dropw
  %k = select i1 %wide, i64 %back, i64 %m
  %d = call i64 @u64_to_f64(i64 %sign, i64 %k)
  %r = call i32 @__aeabi_d2f(i64 %d)
  ret i32 %r
}

define weak i32 @__aeabi_ul2f(i64 %x) #1 {
  %r = call i32 @u64_to_f32(i64 0, i64 %x)
  ret i32 %r
}
define weak i32 @__aeabi_l2f(i64 %x) #1 {
  %neg = icmp slt i64 %x, 0
  %nx = sub i64 0, %x
  %mag = select i1 %neg, i64 %nx, i64 %x
  %sign = zext i1 %neg to i64
  %r = call i32 @u64_to_f32(i64 %sign, i64 %mag)
  ret i32 %r
}

; ---------------------------------------------------------------------------
; GNU names for the helpers above. Dodo's own code generator calls these;
; other LLVM configurations call the EABI names.

@__adddf3 = weak alias i64 (i64, i64), ptr @__aeabi_dadd
@__subdf3 = weak alias i64 (i64, i64), ptr @__aeabi_dsub
@__muldf3 = weak alias i64 (i64, i64), ptr @__aeabi_dmul
@__divdf3 = weak alias i64 (i64, i64), ptr @__aeabi_ddiv
@__addsf3 = weak alias i32 (i32, i32), ptr @__aeabi_fadd
@__subsf3 = weak alias i32 (i32, i32), ptr @__aeabi_fsub
@__mulsf3 = weak alias i32 (i32, i32), ptr @__aeabi_fmul
@__divsf3 = weak alias i32 (i32, i32), ptr @__aeabi_fdiv
@__unorddf2 = weak alias i32 (i64, i64), ptr @__aeabi_dcmpun
@__unordsf2 = weak alias i32 (i32, i32), ptr @__aeabi_fcmpun
@__extendsfdf2 = weak alias i64 (i32), ptr @__aeabi_f2d
@__truncdfsf2 = weak alias i32 (i64), ptr @__aeabi_d2f
@__fixdfsi = weak alias i32 (i64), ptr @__aeabi_d2iz
@__fixunsdfsi = weak alias i32 (i64), ptr @__aeabi_d2uiz
@__fixdfdi = weak alias i64 (i64), ptr @__aeabi_d2lz
@__fixunsdfdi = weak alias i64 (i64), ptr @__aeabi_d2ulz
@__fixsfsi = weak alias i32 (i32), ptr @__aeabi_f2iz
@__fixunssfsi = weak alias i32 (i32), ptr @__aeabi_f2uiz
@__fixsfdi = weak alias i64 (i32), ptr @__aeabi_f2lz
@__fixunssfdi = weak alias i64 (i32), ptr @__aeabi_f2ulz
@__floatsidf = weak alias i64 (i32), ptr @__aeabi_i2d
@__floatunsidf = weak alias i64 (i32), ptr @__aeabi_ui2d
@__floatdidf = weak alias i64 (i64), ptr @__aeabi_l2d
@__floatundidf = weak alias i64 (i64), ptr @__aeabi_ul2d
@__floatsisf = weak alias i32 (i32), ptr @__aeabi_i2f
@__floatunsisf = weak alias i32 (i32), ptr @__aeabi_ui2f
@__floatdisf = weak alias i32 (i64), ptr @__aeabi_l2f
@__floatundisf = weak alias i32 (i64), ptr @__aeabi_ul2f
