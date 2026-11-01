; binary32 arithmetic and integer conversions in 32-bit integer code, for
; the AVR chips. The compiler appends this file after arm_float.ll (see
; src/hardware/avr.rs), whose binary32 helpers compute through binary64:
; correct, but on an 8-bit core the binary64 code they pull in is several
; kilobytes, too much for 32 KiB of flash when a program only uses f32.
; avr_float.ll gives these their GNU names.
;
; The algorithms follow Berkeley SoftFloat 3 (f32_add, f32_sub, f32_mul,
; f32_div, f32_to_i32 and i32_to_f32 with their 64-bit and unsigned forms),
; with the packing helpers of arm_float.ll: round_pack32 takes the
; significand with its leading 1 at bit 30 and seven rounding bits, and an
; exponent one below the biased one. Results round to nearest, ties to
; even, and subnormals are kept. NaN results are the default quiet NaN.
; Conversions to integers truncate, saturate out of range, and give 0 for
; NaN, as arm_float.ll's do.
;
; Everything is integer code, so on a 64-bit host it needs nothing, which
; is how tests/soft_float.rs checks it against hardware.

attributes #11 = { nounwind "no-builtins" }

; The full 64-bit product of two 32-bit values, from 16-bit halves.
define internal i64 @f32_umul32x32(i32 %x, i32 %y) #11 {
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

; (sign << 31) + (exp << 23) + sig, where a carry out of sig raises exp.
define internal i32 @f32_pack(i32 %sign, i32 %exp, i32 %sig) #11 {
  %s = shl i32 %sign, 31
  %e = shl i32 %exp, 23
  %p0 = add i32 %s, %e
  %p = add i32 %p0, %sig
  ret i32 %p
}

; round_pack32 for a significand that may need normalizing first.
define internal i32 @f32_norm_round_pack(i32 %sign, i32 %exp, i32 %sig) #11 {
entry:
  %lz = call i32 @clz32(i32 %sig)
  %shift = sub i32 %lz, 1
  %e = sub i32 %exp, %shift
  %exact.shift = icmp sge i32 %shift, 7
  %in.range = icmp ult i32 %e, 253
  %exact = and i1 %exact.shift, %in.range
  br i1 %exact, label %pack, label %round
pack:
  %nonzero = icmp ne i32 %sig, 0
  %pe = select i1 %nonzero, i32 %e, i32 0
  %d = sub i32 %shift, 7
  %m = shl i32 %sig, %d
  %p = call i32 @f32_pack(i32 %sign, i32 %pe, i32 %m)
  ret i32 %p
round:
  %s = shl i32 %sig, %shift
  %r = call i32 @round_pack32(i32 %sign, i32 %e, i32 %s)
  ret i32 %r
}

; |a| + |b| with a's sign, for operands with equal signs.
define internal i32 @f32_add_mags(i32 %a, i32 %b) #11 {
entry:
  %ea.s = lshr i32 %a, 23
  %ea = and i32 %ea.s, 255
  %ma = and i32 %a, 8388607
  %eb.s = lshr i32 %b, 23
  %eb = and i32 %eb.s, 255
  %mb = and i32 %b, 8388607
  %sign = lshr i32 %a, 31
  %diff = sub i32 %ea, %eb
  %same = icmp eq i32 %diff, 0
  br i1 %same, label %same.exp, label %different
same.exp:
  %a.subnormal = icmp eq i32 %ea, 0
  br i1 %a.subnormal, label %subnormals, label %same.normal
subnormals:
  ; Fractions add exactly; a carry makes the result normal.
  %sum.sub = add i32 %a, %mb
  ret i32 %sum.sub
same.normal:
  %a.special = icmp eq i32 %ea, 255
  br i1 %a.special, label %same.special, label %same.finite
same.special:
  %ms = or i32 %ma, %mb
  %nan = icmp ne i32 %ms, 0
  %r.special = select i1 %nan, i32 2143289344, i32 %a
  ret i32 %r.special
same.finite:
  %s0 = add i32 %ma, %mb
  %sz = add i32 %s0, 16777216
  %odd = and i32 %sz, 1
  %even = icmp eq i32 %odd, 0
  %roomy = icmp ult i32 %ea, 254
  %exact = and i1 %even, %roomy
  br i1 %exact, label %same.exact, label %same.round
same.exact:
  %half = lshr i32 %sz, 1
  %p = call i32 @f32_pack(i32 %sign, i32 %ea, i32 %half)
  ret i32 %p
same.round:
  %sz6 = shl i32 %sz, 6
  br label %round
different:
  %ma6 = shl i32 %ma, 6
  %mb6 = shl i32 %mb, 6
  %b.bigger = icmp slt i32 %diff, 0
  br i1 %b.bigger, label %b.big, label %a.big
b.big:
  %b.special = icmp eq i32 %eb, 255
  br i1 %b.special, label %b.inf, label %b.finite
b.inf:
  %b.nan = icmp ne i32 %mb, 0
  %inf.b = call i32 @f32_pack(i32 %sign, i32 255, i32 0)
  %r.b = select i1 %b.nan, i32 2143289344, i32 %inf.b
  ret i32 %r.b
b.finite:
  %a.normal = icmp ne i32 %ea, 0
  %a.extra = select i1 %a.normal, i32 536870912, i32 %ma6
  %ma6x = add i32 %ma6, %a.extra
  %dist.b = sub i32 0, %diff
  %ma.jam = call i32 @shr_jam32(i32 %ma6x, i32 %dist.b)
  br label %sum
a.big:
  %a.special2 = icmp eq i32 %ea, 255
  br i1 %a.special2, label %a.inf, label %a.finite
a.inf:
  %a.nan = icmp ne i32 %ma, 0
  %r.a = select i1 %a.nan, i32 2143289344, i32 %a
  ret i32 %r.a
a.finite:
  %b.normal = icmp ne i32 %eb, 0
  %b.extra = select i1 %b.normal, i32 536870912, i32 %mb6
  %mb6x = add i32 %mb6, %b.extra
  %mb.jam = call i32 @shr_jam32(i32 %mb6x, i32 %diff)
  br label %sum
sum:
  %ez = phi i32 [ %eb, %b.finite ], [ %ea, %a.finite ]
  %xa = phi i32 [ %ma.jam, %b.finite ], [ %ma6, %a.finite ]
  %xb = phi i32 [ %mb6, %b.finite ], [ %mb.jam, %a.finite ]
  %t0 = add i32 %xa, %xb
  %t = add i32 %t0, 536870912
  %low = icmp ult i32 %t, 1073741824
  %ez.dec = sub i32 %ez, 1
  %t.shl = shl i32 %t, 1
  %ez2 = select i1 %low, i32 %ez.dec, i32 %ez
  %t2 = select i1 %low, i32 %t.shl, i32 %t
  br label %round
round:
  %re = phi i32 [ %ea, %same.round ], [ %ez2, %sum ]
  %rs = phi i32 [ %sz6, %same.round ], [ %t2, %sum ]
  %r = call i32 @round_pack32(i32 %sign, i32 %re, i32 %rs)
  ret i32 %r
}

; |a| - |b| with a's sign, for operands with different signs.
define internal i32 @f32_sub_mags(i32 %a, i32 %b) #11 {
entry:
  %ea.s = lshr i32 %a, 23
  %ea = and i32 %ea.s, 255
  %ma = and i32 %a, 8388607
  %eb.s = lshr i32 %b, 23
  %eb = and i32 %eb.s, 255
  %mb = and i32 %b, 8388607
  %sign = lshr i32 %a, 31
  %diff = sub i32 %ea, %eb
  %same = icmp eq i32 %diff, 0
  br i1 %same, label %same.exp, label %different
same.exp:
  ; Infinity minus infinity, and NaNs, are NaN.
  %a.special = icmp eq i32 %ea, 255
  br i1 %a.special, label %nan, label %same.finite
nan:
  ret i32 2143289344
same.finite:
  %sd = sub i32 %ma, %mb
  %zero = icmp eq i32 %sd, 0
  br i1 %zero, label %positive.zero, label %nonzero
positive.zero:
  ret i32 0
nonzero:
  %ea.positive = icmp ne i32 %ea, 0
  %ea.dec = sub i32 %ea, 1
  %ea1 = select i1 %ea.positive, i32 %ea.dec, i32 %ea
  %negative = icmp slt i32 %sd, 0
  %flipped = xor i32 %sign, 1
  %signz = select i1 %negative, i32 %flipped, i32 %sign
  %sd.neg = sub i32 0, %sd
  %mag = select i1 %negative, i32 %sd.neg, i32 %sd
  %lz = call i32 @clz32(i32 %mag)
  %shift0 = sub i32 %lz, 8
  %ez0 = sub i32 %ea1, %shift0
  %under = icmp slt i32 %ez0, 0
  %shift = select i1 %under, i32 %ea1, i32 %shift0
  %ez = select i1 %under, i32 0, i32 %ez0
  %m = shl i32 %mag, %shift
  %p = call i32 @f32_pack(i32 %signz, i32 %ez, i32 %m)
  ret i32 %p
different:
  %ma7 = shl i32 %ma, 7
  %mb7 = shl i32 %mb, 7
  %b.bigger = icmp slt i32 %diff, 0
  br i1 %b.bigger, label %b.big, label %a.big
b.big:
  %sign.b = xor i32 %sign, 1
  %b.special = icmp eq i32 %eb, 255
  br i1 %b.special, label %b.inf, label %b.finite
b.inf:
  %b.nan = icmp ne i32 %mb, 0
  %inf.b = call i32 @f32_pack(i32 %sign.b, i32 255, i32 0)
  %r.b = select i1 %b.nan, i32 2143289344, i32 %inf.b
  ret i32 %r.b
b.finite:
  %ez.b = sub i32 %eb, 1
  %x.b = or i32 %mb7, 1073741824
  %a.normal = icmp ne i32 %ea, 0
  %a.extra = select i1 %a.normal, i32 1073741824, i32 %ma7
  %y.a = add i32 %ma7, %a.extra
  %dist.b = sub i32 0, %diff
  br label %norm
a.big:
  %a.special2 = icmp eq i32 %ea, 255
  br i1 %a.special2, label %a.inf, label %a.finite
a.inf:
  %a.nan = icmp ne i32 %ma, 0
  %r.a = select i1 %a.nan, i32 2143289344, i32 %a
  ret i32 %r.a
a.finite:
  %ez.a = sub i32 %ea, 1
  %x.a = or i32 %ma7, 1073741824
  %b.normal = icmp ne i32 %eb, 0
  %b.extra = select i1 %b.normal, i32 1073741824, i32 %mb7
  %y.b = add i32 %mb7, %b.extra
  br label %norm
norm:
  %ns = phi i32 [ %sign.b, %b.finite ], [ %sign, %a.finite ]
  %ne = phi i32 [ %ez.b, %b.finite ], [ %ez.a, %a.finite ]
  %nx = phi i32 [ %x.b, %b.finite ], [ %x.a, %a.finite ]
  %ny = phi i32 [ %y.a, %b.finite ], [ %y.b, %a.finite ]
  %nd = phi i32 [ %dist.b, %b.finite ], [ %diff, %a.finite ]
  %y.jam = call i32 @shr_jam32(i32 %ny, i32 %nd)
  %sig = sub i32 %nx, %y.jam
  %r = call i32 @f32_norm_round_pack(i32 %ns, i32 %ne, i32 %sig)
  ret i32 %r
}

define weak i32 @dodo_f32_add(i32 %a, i32 %b) #11 {
entry:
  %x = xor i32 %a, %b
  %differ = icmp slt i32 %x, 0
  br i1 %differ, label %subtract, label %add
add:
  %sum = call i32 @f32_add_mags(i32 %a, i32 %b)
  ret i32 %sum
subtract:
  %difference = call i32 @f32_sub_mags(i32 %a, i32 %b)
  ret i32 %difference
}

define weak i32 @dodo_f32_sub(i32 %a, i32 %b) #11 {
  %nb = xor i32 %b, -2147483648
  %r = call i32 @dodo_f32_add(i32 %a, i32 %nb)
  ret i32 %r
}

; A subnormal's fraction normalized: { exponent, fraction with its leading
; 1 at bit 23 }.
define internal { i32, i32 } @f32_normalize(i32 %m) #11 {
  %lz = call i32 @clz32(i32 %m)
  %shift = sub i32 %lz, 8
  %e = sub i32 1, %shift
  %n = shl i32 %m, %shift
  %r.e = insertvalue { i32, i32 } poison, i32 %e, 0
  %r = insertvalue { i32, i32 } %r.e, i32 %n, 1
  ret { i32, i32 } %r
}

define weak i32 @dodo_f32_mul(i32 %a, i32 %b) #11 {
entry:
  %ea.s = lshr i32 %a, 23
  %ea = and i32 %ea.s, 255
  %ma = and i32 %a, 8388607
  %eb.s = lshr i32 %b, 23
  %eb = and i32 %eb.s, 255
  %mb = and i32 %b, 8388607
  %x = xor i32 %a, %b
  %signz = lshr i32 %x, 31
  %signed.zero = and i32 %x, -2147483648
  %a.special = icmp eq i32 %ea, 255
  br i1 %a.special, label %a.inf, label %check.b
a.inf:
  %a.nan = icmp ne i32 %ma, 0
  %b.special.a = icmp eq i32 %eb, 255
  %b.fraction = icmp ne i32 %mb, 0
  %b.nan.a = and i1 %b.special.a, %b.fraction
  %either.nan = or i1 %a.nan, %b.nan.a
  br i1 %either.nan, label %nan, label %a.inf.other
a.inf.other:
  %b.mag = or i32 %eb, %mb
  br label %infinite
check.b:
  %b.special = icmp eq i32 %eb, 255
  br i1 %b.special, label %b.inf, label %finite
b.inf:
  %b.nan = icmp ne i32 %mb, 0
  br i1 %b.nan, label %nan, label %b.inf.other
b.inf.other:
  %a.mag = or i32 %ea, %ma
  br label %infinite
infinite:
  ; Infinity times zero is NaN.
  %other = phi i32 [ %b.mag, %a.inf.other ], [ %a.mag, %b.inf.other ]
  %by.zero = icmp eq i32 %other, 0
  %inf = or i32 %signed.zero, 2139095040
  %r.inf = select i1 %by.zero, i32 2143289344, i32 %inf
  ret i32 %r.inf
nan:
  ret i32 2143289344
finite:
  %a.subnormal = icmp eq i32 %ea, 0
  br i1 %a.subnormal, label %a.small, label %a.ready
a.small:
  %a.zero = icmp eq i32 %ma, 0
  br i1 %a.zero, label %zero, label %a.norm
a.norm:
  %an = call { i32, i32 } @f32_normalize(i32 %ma)
  %an.e = extractvalue { i32, i32 } %an, 0
  %an.m = extractvalue { i32, i32 } %an, 1
  br label %a.ready
a.ready:
  %e1 = phi i32 [ %ea, %finite ], [ %an.e, %a.norm ]
  %m1 = phi i32 [ %ma, %finite ], [ %an.m, %a.norm ]
  %b.subnormal = icmp eq i32 %eb, 0
  br i1 %b.subnormal, label %b.small, label %b.ready
b.small:
  %b.zero = icmp eq i32 %mb, 0
  br i1 %b.zero, label %zero, label %b.norm
b.norm:
  %bn = call { i32, i32 } @f32_normalize(i32 %mb)
  %bn.e = extractvalue { i32, i32 } %bn, 0
  %bn.m = extractvalue { i32, i32 } %bn, 1
  br label %b.ready
b.ready:
  %e2 = phi i32 [ %eb, %a.ready ], [ %bn.e, %b.norm ]
  %m2 = phi i32 [ %mb, %a.ready ], [ %bn.m, %b.norm ]
  %ez0 = add i32 %e1, %e2
  %ez = sub i32 %ez0, 127
  %sa0 = or i32 %m1, 8388608
  %sa = shl i32 %sa0, 7
  %sb0 = or i32 %m2, 8388608
  %sb = shl i32 %sb0, 8
  %product = call i64 @f32_umul32x32(i32 %sa, i32 %sb)
  %high64 = lshr i64 %product, 32
  %high = trunc i64 %high64 to i32
  %low = trunc i64 %product to i32
  %inexact = icmp ne i32 %low, 0
  %sticky = zext i1 %inexact to i32
  %sz = or i32 %high, %sticky
  %small = icmp ult i32 %sz, 1073741824
  %ez.dec = sub i32 %ez, 1
  %sz.shl = shl i32 %sz, 1
  %ez2 = select i1 %small, i32 %ez.dec, i32 %ez
  %sz2 = select i1 %small, i32 %sz.shl, i32 %sz
  %r = call i32 @round_pack32(i32 %signz, i32 %ez2, i32 %sz2)
  ret i32 %r
zero:
  ret i32 %signed.zero
}

define weak i32 @dodo_f32_div(i32 %a, i32 %b) #11 {
entry:
  %ea.s = lshr i32 %a, 23
  %ea = and i32 %ea.s, 255
  %ma = and i32 %a, 8388607
  %eb.s = lshr i32 %b, 23
  %eb = and i32 %eb.s, 255
  %mb = and i32 %b, 8388607
  %x = xor i32 %a, %b
  %signz = lshr i32 %x, 31
  %signed.zero = and i32 %x, -2147483648
  %a.special = icmp eq i32 %ea, 255
  br i1 %a.special, label %a.inf, label %check.b
a.inf:
  ; NaN / x, infinity / infinity and infinity / NaN are NaN.
  %a.nan = icmp ne i32 %ma, 0
  %b.special.a = icmp eq i32 %eb, 255
  %invalid = or i1 %a.nan, %b.special.a
  br i1 %invalid, label %nan, label %infinity
check.b:
  %b.special = icmp eq i32 %eb, 255
  br i1 %b.special, label %b.inf, label %b.finite
b.inf:
  %b.nan = icmp ne i32 %mb, 0
  br i1 %b.nan, label %nan, label %zero
b.finite:
  %b.subnormal = icmp eq i32 %eb, 0
  br i1 %b.subnormal, label %b.small, label %b.ready
b.small:
  %b.zero = icmp eq i32 %mb, 0
  br i1 %b.zero, label %by.zero, label %b.norm
by.zero:
  ; Zero / zero is NaN; anything else / zero is infinite.
  %a.bits = or i32 %ea, %ma
  %a.zero.too = icmp eq i32 %a.bits, 0
  br i1 %a.zero.too, label %nan, label %infinity
b.norm:
  %bn = call { i32, i32 } @f32_normalize(i32 %mb)
  %bn.e = extractvalue { i32, i32 } %bn, 0
  %bn.m = extractvalue { i32, i32 } %bn, 1
  br label %b.ready
b.ready:
  %e2 = phi i32 [ %eb, %b.finite ], [ %bn.e, %b.norm ]
  %m2 = phi i32 [ %mb, %b.finite ], [ %bn.m, %b.norm ]
  %a.subnormal = icmp eq i32 %ea, 0
  br i1 %a.subnormal, label %a.small, label %a.ready
a.small:
  %a.zero = icmp eq i32 %ma, 0
  br i1 %a.zero, label %zero, label %a.norm
a.norm:
  %an = call { i32, i32 } @f32_normalize(i32 %ma)
  %an.e = extractvalue { i32, i32 } %an, 0
  %an.m = extractvalue { i32, i32 } %an, 1
  br label %a.ready
a.ready:
  %e1 = phi i32 [ %ea, %b.ready ], [ %an.e, %a.norm ]
  %m1 = phi i32 [ %ma, %b.ready ], [ %an.m, %a.norm ]
  %ez0 = sub i32 %e1, %e2
  %ez1 = add i32 %ez0, 126
  %sa = or i32 %m1, 8388608
  %sb = or i32 %m2, 8388608
  %smaller = icmp ult i32 %sa, %sb
  %ez.dec = sub i32 %ez1, 1
  %ez = select i1 %smaller, i32 %ez.dec, i32 %ez1
  ; The quotient's leading 1 lands at bit 30: sa * 2^31 / sb when sa < sb,
  ; else sa * 2^30 / sb, one bit per step. The remainder stays below
  ; 2 * sb < 2^25.
  %steps = select i1 %smaller, i32 32, i32 31
  br label %loop
loop:
  %n = phi i32 [ %steps, %a.ready ], [ %n.next, %loop ]
  %rem = phi i32 [ %sa, %a.ready ], [ %rem.next, %loop ]
  %q = phi i32 [ 0, %a.ready ], [ %q.next, %loop ]
  %take = icmp uge i32 %rem, %sb
  %rem.sub = sub i32 %rem, %sb
  %rem.kept = select i1 %take, i32 %rem.sub, i32 %rem
  %bit = zext i1 %take to i32
  %q.shift = shl i32 %q, 1
  %q.next = or i32 %q.shift, %bit
  %rem.next = shl i32 %rem.kept, 1
  %n.next = sub i32 %n, 1
  %more = icmp ne i32 %n.next, 0
  br i1 %more, label %loop, label %done
done:
  %inexact = icmp ne i32 %rem.next, 0
  %sticky = zext i1 %inexact to i32
  %sz = or i32 %q.next, %sticky
  %r = call i32 @round_pack32(i32 %signz, i32 %ez, i32 %sz)
  ret i32 %r
nan:
  ret i32 2143289344
infinity:
  %inf = or i32 %signed.zero, 2139095040
  ret i32 %inf
zero:
  ret i32 %signed.zero
}

; ---------------------------------------------------------------------------
; Conversions. To integers: the magnitude, truncated, of a value that
; fits; `limit` is the first biased exponent that does not.

define weak i32 @dodo_f32_to_i32(i32 %a) #11 {
entry:
  %e.s = lshr i32 %a, 23
  %e = and i32 %e.s, 255
  %m = and i32 %a, 8388607
  %negative = icmp slt i32 %a, 0
  %nan = call i1 @isnan32(i32 %a)
  br i1 %nan, label %zero, label %number
number:
  %small = icmp ult i32 %e, 127
  br i1 %small, label %zero, label %range
range:
  %over = icmp uge i32 %e, 158
  br i1 %over, label %saturate, label %convert
saturate:
  %limit = select i1 %negative, i32 -2147483648, i32 2147483647
  ret i32 %limit
convert:
  %sig = or i32 %m, 8388608
  %shift = sub i32 %e, 150
  %left = icmp sge i32 %shift, 0
  %right = sub i32 0, %shift
  %up = shl i32 %sig, %shift
  %down = lshr i32 %sig, %right
  %v = select i1 %left, i32 %up, i32 %down
  %nv = sub i32 0, %v
  %r = select i1 %negative, i32 %nv, i32 %v
  ret i32 %r
zero:
  ret i32 0
}

define weak i32 @dodo_f32_to_u32(i32 %a) #11 {
entry:
  %e.s = lshr i32 %a, 23
  %e = and i32 %e.s, 255
  %m = and i32 %a, 8388607
  %negative = icmp slt i32 %a, 0
  %nan = call i1 @isnan32(i32 %a)
  %none = or i1 %nan, %negative
  br i1 %none, label %zero, label %number
number:
  %small = icmp ult i32 %e, 127
  br i1 %small, label %zero, label %range
range:
  %over = icmp uge i32 %e, 159
  br i1 %over, label %saturate, label %convert
saturate:
  ret i32 -1
convert:
  %sig = or i32 %m, 8388608
  %shift = sub i32 %e, 150
  %left = icmp sge i32 %shift, 0
  %right = sub i32 0, %shift
  %up = shl i32 %sig, %shift
  %down = lshr i32 %sig, %right
  %v = select i1 %left, i32 %up, i32 %down
  ret i32 %v
zero:
  ret i32 0
}

define weak i64 @dodo_f32_to_i64(i32 %a) #11 {
entry:
  %e.s = lshr i32 %a, 23
  %e = and i32 %e.s, 255
  %m = and i32 %a, 8388607
  %negative = icmp slt i32 %a, 0
  %nan = call i1 @isnan32(i32 %a)
  br i1 %nan, label %zero, label %number
number:
  %small = icmp ult i32 %e, 127
  br i1 %small, label %zero, label %range
range:
  %over = icmp uge i32 %e, 190
  br i1 %over, label %saturate, label %convert
saturate:
  %limit = select i1 %negative, i64 -9223372036854775808, i64 9223372036854775807
  ret i64 %limit
convert:
  %sig32 = or i32 %m, 8388608
  %sig = zext i32 %sig32 to i64
  %shift = sub i32 %e, 150
  %left = icmp sge i32 %shift, 0
  %right = sub i32 0, %shift
  %shift.w = zext i32 %shift to i64
  %right.w = zext i32 %right to i64
  %up.amount = select i1 %left, i64 %shift.w, i64 0
  %down.amount = select i1 %left, i64 0, i64 %right.w
  %up = shl i64 %sig, %up.amount
  %v = lshr i64 %up, %down.amount
  %nv = sub i64 0, %v
  %r = select i1 %negative, i64 %nv, i64 %v
  ret i64 %r
zero:
  ret i64 0
}

define weak i64 @dodo_f32_to_u64(i32 %a) #11 {
entry:
  %e.s = lshr i32 %a, 23
  %e = and i32 %e.s, 255
  %m = and i32 %a, 8388607
  %negative = icmp slt i32 %a, 0
  %nan = call i1 @isnan32(i32 %a)
  %none = or i1 %nan, %negative
  br i1 %none, label %zero, label %number
number:
  %small = icmp ult i32 %e, 127
  br i1 %small, label %zero, label %range
range:
  %over = icmp uge i32 %e, 191
  br i1 %over, label %saturate, label %convert
saturate:
  ret i64 -1
convert:
  %sig32 = or i32 %m, 8388608
  %sig = zext i32 %sig32 to i64
  %shift = sub i32 %e, 150
  %left = icmp sge i32 %shift, 0
  %right = sub i32 0, %shift
  %shift.w = zext i32 %shift to i64
  %right.w = zext i32 %right to i64
  %up.amount = select i1 %left, i64 %shift.w, i64 0
  %down.amount = select i1 %left, i64 0, i64 %right.w
  %up = shl i64 %sig, %up.amount
  %v = lshr i64 %up, %down.amount
  ret i64 %v
zero:
  ret i64 0
}

; From integers: the magnitude normalized to a leading 1 at bit 30, with
; the bits shifted out folded into the sticky bit 0.
define internal i32 @f32_from_u32(i32 %sign, i32 %mag) #11 {
entry:
  %zero = icmp eq i32 %mag, 0
  br i1 %zero, label %is.zero, label %nonzero
is.zero:
  ret i32 0
nonzero:
  %lz = call i32 @clz32(i32 %mag)
  %m = shl i32 %mag, %lz
  %high = lshr i32 %m, 1
  %low = and i32 %m, 1
  %sig = or i32 %high, %low
  %e = sub i32 157, %lz
  %r = call i32 @round_pack32(i32 %sign, i32 %e, i32 %sig)
  ret i32 %r
}

define internal i32 @f32_from_u64(i32 %sign, i64 %mag) #11 {
entry:
  %high = lshr i64 %mag, 32
  %narrow = icmp eq i64 %high, 0
  br i1 %narrow, label %small, label %wide
small:
  %m32 = trunc i64 %mag to i32
  %r.small = call i32 @f32_from_u32(i32 %sign, i32 %m32)
  ret i32 %r.small
wide:
  %lz = call i32 @clz64(i64 %mag)
  %lz.w = zext i32 %lz to i64
  %m = shl i64 %mag, %lz.w
  %top = lshr i64 %m, 33
  %top32 = trunc i64 %top to i32
  %rest = and i64 %m, 8589934591
  %lost = icmp ne i64 %rest, 0
  %sticky = zext i1 %lost to i32
  %sig = or i32 %top32, %sticky
  %e = sub i32 189, %lz
  %r = call i32 @round_pack32(i32 %sign, i32 %e, i32 %sig)
  ret i32 %r
}

define weak i32 @dodo_f32_from_u32(i32 %x) #11 {
  %r = call i32 @f32_from_u32(i32 0, i32 %x)
  ret i32 %r
}

define weak i32 @dodo_f32_from_i32(i32 %x) #11 {
  %negative = icmp slt i32 %x, 0
  %nx = sub i32 0, %x
  %mag = select i1 %negative, i32 %nx, i32 %x
  %sign = zext i1 %negative to i32
  %r = call i32 @f32_from_u32(i32 %sign, i32 %mag)
  ret i32 %r
}

define weak i32 @dodo_f32_from_u64(i64 %x) #11 {
  %r = call i32 @f32_from_u64(i32 0, i64 %x)
  ret i32 %r
}

define weak i32 @dodo_f32_from_i64(i64 %x) #11 {
  %negative = icmp slt i64 %x, 0
  %nx = sub i64 0, %x
  %mag = select i1 %negative, i64 %nx, i64 %x
  %sign = zext i1 %negative to i32
  %r = call i32 @f32_from_u64(i32 %sign, i64 %mag)
  ret i32 %r
}
