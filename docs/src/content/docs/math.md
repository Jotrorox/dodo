---
title: Portable mathematics
section: "Using Dodo"
order: 142
description: "Checked integers and portable binary64 elementary functions, accuracy, special values, and reference tests."
---

`std/math` supplies allocation-free integer algorithms, checked signed arithmetic,
and binary64 (`f64`) mathematics. `std/math/trig` separately supplies trigonometry
and its range-reduction table. Neither package imports an allocator, clock, OS,
libc, libm, or runtime startup. No hardware backend is required or currently
provided. Cortex-M0 builds use the target's ordinary compiler support routines
for soft floating-point arithmetic; firmware supplies those when linking.

```dodo test
package vector_example
import "std/math"
import "std/math/trig"

@test
fn reconstructs_the_horizontal_component() {
    length := math.hypot(3.0, 4.0)
    angle := trig.atan2(4.0, 3.0)
    match trig.cos(angle) {
        ok(value) => { assert(math.abs(length * value - 3.0) < 1e-12) },
        err(_) => { assert(false, "a finite vector should have a valid angle") },
    }
}
```

See `examples/math_portable.dodo` for an executable example. All checked results
must be handled or propagated with `?`; math does not bypass Result checking.

## Integers

`checked_abs`, `checked_add`, `checked_sub`, `checked_mul`, `checked_div`,
`checked_rem`, and `checked_pow` operate on `i64`. Overflow produces
`MathError.Overflow`; division or remainder by zero produces
`MathError.DivisionByZero`. Unlike the primitive signed-remainder operator,
`checked_rem(I64_MIN, -1)` returns zero. `unsigned_abs` returns a `u64` magnitude,
including the signed minimum. `checked_pow_u64` accepts an unsigned base. Both
power functions take a `u32` exponent, use exponentiation by squaring, and define
zero to the zeroth power as one.

`gcd` uses Euclid's algorithm; `gcd(0, 0)` is zero. `checked_lcm` divides before
multiplication and returns zero if either input is zero. `isqrt` is the exact
floor of the square root throughout `u64`, including its maximum; it uses a
restoring binary algorithm without floating-point conversion.
`checked_next_power_of_two` maps zero and one to one and reports overflow beyond
`2^63`. Pointer-sized checked and wrapping resource operations remain available
in `core/num`.

These routines use constant storage. Powers take logarithmic time in the
exponent; GCD takes logarithmic iterations in the input magnitudes; integer
square root has at most 32 iterations. Checked scalar arithmetic is constant
time, without claiming timing independence from its inputs.

## Binary64 values and rounding

`to_bits` and `from_bits` preserve all 64 representation bits on every target,
independent of byte order. `classify` distinguishes `Zero`, `Subnormal`, `Normal`,
`Infinite`, and `Nan`; predicates are also available. `sign_bit` observes negative
zero. `abs` clears the sign bit; `copy_sign` replaces only the sign bit. Constants
include `PI`, `TAU`, `E`, `LN_2`, `LN_10`, `EPSILON`, `MIN_NORMAL`, and `MAX_FINITE`.
`infinity()` and `nan()` construct positive infinity and a quiet NaN.

`trunc`, `floor`, and `ceil` round toward zero, negative infinity, and positive
infinity. `round` rounds to nearest with ties away from zero; `round_even` uses
ties to even. Integral rounding is exact, passes through NaNs and infinities,
and preserves the sign when its result is zero. No large float-to-integer cast
is used. `scalbn(x, n)` scales by an `i32` power of two; it can produce infinity
or gradual underflow and preserves signed zero. Scaling is deliberately an IEEE
value operation, rather than a checked resource operation.

The implementation assumes IEEE 754 binary64 with round-to-nearest, ties-to-even
arithmetic and gradual underflow. Dodo emits arithmetic without fast-math flags.
A caller that changes hardware rounding or enables flush-to-zero steps outside
the numerical accuracy contract. No floating-point exception flags, signaling
NaN behavior, or preservation of NaN payloads through arithmetic are promised.
Binary32 functions are not yet supplied; callers may explicitly promote `f32`
inputs and use the binary64 results.

## Elementary functions and exceptional inputs

| Functions | Contract |
| --- | --- |
| `sqrt` | Negative values except negative zero are `Domain`; signed zero and positive infinity pass through. |
| `cbrt` | Real cube root for either sign; signed zero and infinities pass through. |
| `hypot` | Scaled Euclidean norm; infinity takes precedence over NaN; overflow yields infinity. |
| `log`, `log2`, `log10` | Negative arguments are `Domain`; either zero returns negative infinity; positive infinity passes through. |
| `log1p` | Stable log of `1+x`; below -1 is `Domain`; exactly -1 returns negative infinity. |
| `exp`, `exp2`, `expm1` | Finite overflow is `Overflow`; underflow is gradual and may round to zero; positive infinity gives infinity. |
| `powi` | `i32` exponent; zero to a negative exponent is `DivisionByZero`; finite overflow is `Overflow`. |
| `pow` | Real exponent; a negative base requires an exactly integral exponent; other such inputs are `Domain`. |
| `trig.sin`, `trig.cos`, `trig.tan` | Radians, every finite binary64 argument supported; infinity is `Domain`. Tangent overflow is `Overflow`. |
| `trig.asin`, `trig.acos` | Outside [-1, 1] is `Domain`; angles in [-pi/2, pi/2] and [0, pi]. |
| `trig.atan`, `trig.atan2` | Angles in [-pi/2, pi/2] and [-pi, pi]; signed zeros and infinite quadrants handled explicitly. |

NaN arguments generally produce a NaN inside `ok` for checked functions, or a
NaN value for unchecked ones. Power defines `NaN^0 = 1` and `1^NaN = 1`, and
`hypot(infinity, NaN) = infinity`. `exp(-infinity) = 0`,
`expm1(-infinity) = -1`, and `exp2(-infinity) = 0`.
`powi(-0, positive odd exponent)` returns negative zero. Zero to the zeroth
power is one. For a finite integral exponent beyond binary64's exact consecutive
integer range, parity follows the represented exponent, which is even.
`atan2(+0, -0) = +pi` and `atan2(-0, -0) = -pi`; positive x preserves y's zero
sign. Sine, tangent, inverse sine, arctangent, `log1p`, and `expm1` preserve signed zero.

## Algorithms and accuracy

Roots use Newton iteration after exact exponent normalization. Logarithms use
range reduction into `[sqrt(1/2), sqrt(2)]` and a 12-term atanh series.
Exponentials use split-ln(2) range reduction and a 15-term Taylor expansion.
`log1p` corrects the rounding error in forming `1+x`; its small-input path and
`expm1` avoid cancellation. Integer floating powers use repeated squaring; real
powers compose logarithm and exponential.

Trigonometry uses fixed-point Payne–Hanek reduction, multiplying the exact
significand by 1,584 bits of `2/pi`. It retains 128 fractional bits and the
quadrant before applying Taylor kernels on `[-pi/4, pi/4]`. This supports very
large arguments, including the largest finite binary64 value, without reducing
against a rounded `2*pi`. The table and its temporary arithmetic storage are
only included when importing `std/math/trig`; they use 66 and 69 `u64` slots,
respectively. The kernels have fixed iteration counts. Inverse tangent uses
reciprocal and pi/4 reductions followed by a 24-term alternating series.

The split ln(2) constants and 2/pi digits come from Sun's
[fdlibm exponential](https://www.netlib.org/fdlibm/e_exp.c) and
[argument reduction source](https://www.netlib.org/fdlibm/e_rem_pio2.c).
Their permission notice is retained in `stdlib/std/math/NOTICE` and `trig.dodo`.
These independently implemented kernels do **not** inherit fdlibm's one-ulp
claim. The table reduction follows the established Payne–Hanek method described
by [Hanek and Payne's argument-reduction paper](https://dl.acm.org/doi/10.1145/1057600.1057602).

The regression suite compares 8,767 deterministic samples against [MPFR](https://www.mpfr.org/mpfr-current/mpfr.html) at
256-bit precision, including exponent-stratified inputs, normal/subnormal
boundaries, large angles, neighbors of multiples of pi/2, and inverse-function
endpoints. Its current acceptance bounds are:

| Function | Tested error bound |
| --- | --- |
| `sqrt` | `2e-15 * abs(reference)` |
| `cbrt`, `log`, `expm1` | `3e-15 * abs(reference)` |
| `log2`, `log10`, `log1p` | `4e-15 * abs(reference)` |
| `exp` | `3e-15 * abs(reference) + 2 * smallest_subnormal` |
| `exp2` | `4e-15 * abs(reference) + 2 * smallest_subnormal` |
| `pow` | `3e-13 * abs(reference) + 2 * smallest_subnormal` |
| `hypot` | `3e-15 * abs(reference) + 2 * smallest_subnormal` |
| sine, cosine, arctangent/atan2, inverse sine/cosine | `3e-15` absolute |
| tangent | `2e-14 * abs(reference) + 2 * smallest_subnormal` |

These are reproducible regression expectations, not an exhaustive proof or a
correct-rounding guarantee for every input. Relative error is unsuitable near
trigonometric zeros, and tangent is ill-conditioned near its poles. Composed
`pow`, `hypot`, and base-converted functions can accumulate rounding error; no
uniform ulp guarantee is made for them. Applications needing certified rounding,
interval enclosures, decimal arithmetic, or arbitrary precision need a separate
numerical package.

`python3 tests/math_reference.py` regenerates the checked-in vectors using the
host MPFR shared library. Normal tests need neither MPFR nor Python. The
`math_library` test runs both fixtures at `-O0` and `-O3`, emits WebAssembly and
Cortex-M0 objects, and checks that optimized IR has no external elementary-libm
calls. Windows validation runs the same fixtures through the standard-library
Windows script. Object emission verifies portability of code generation, not
execution on a particular board or WebAssembly runtime.
