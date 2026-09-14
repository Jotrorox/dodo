#!/usr/bin/env python3
"""Reproduce the math range constants with Python's standard library only.

No source tables, host libm functions, or network access are used. Integer
arithmetic encloses the Taylor series for atan(1/q) and atanh(1/q), scaled by
2**precision. Machin's identity gives pi = 16*atan(1/5) - 4*atan(1/239);
ln(2) = 2*atanh(1/3). Every truncation/rounding must agree at both interval
endpoints, and the entire calculation is repeated at 2048 and 4096 bits.

Run with --check to verify the checked-in blocks, or --write to regenerate
them. With neither option, print the generated blocks without reading them.
See stdlib/std/math/CONSTANTS.md for the derivations and provenance review.
"""

import argparse
from fractions import Fraction
from pathlib import Path
import struct
import sys


ROOT = Path(__file__).resolve().parents[1]
PRECISIONS = (2048, 4096)
LIMB_BITS = 24
LIMB_COUNT = 66
BEGIN = "// BEGIN GENERATED MATH CONSTANTS"
END = "// END GENERATED MATH CONSTANTS"


def reciprocal_series(q: int, scale: int, *, alternating: bool) -> tuple[int, int]:
    """Enclose scale * sum((+/-1)**k / ((2*k+1)*q**(2*k+1)))."""
    if q < 2:
        raise ValueError("series requires q >= 2")
    lower = upper = 0
    power = q
    odd = 1
    sign = 1
    while True:
        term = scale // (odd * power)
        if term == 0:
            # The first omitted term is < 1 scaled unit. An alternating
            # tail has its sign and is no larger; a positive tail is bounded
            # by a geometric series with ratio <= 1/q**2, hence < 2 units.
            if alternating and sign < 0:
                lower -= 1
            else:
                upper += 1 if alternating else 2
            return lower, upper
        # Each exact scaled term lies in [term, term + 1].
        if sign > 0:
            lower += term
            upper += term + 1
        else:
            lower -= term + 1
            upper -= term
        power *= q * q
        odd += 2
        if alternating:
            sign = -sign


def agreed(lower, upper, name):
    if lower != upper:
        raise ValueError(f"insufficient precision to resolve {name}")
    return lower


def rounded(lower: int, upper: int, scale: int, name: str) -> float:
    # Fraction -> float rounds the exact rational to binary64, ties to even.
    return agreed(float(Fraction(lower, scale)), float(Fraction(upper, scale)), name)


def round_up(value: Fraction) -> float:
    """Round a nonzero normal value toward +infinity using binary64 bits."""
    result = float(value)
    if Fraction(result) < value:
        bits = struct.unpack(">Q", struct.pack(">d", result))[0]
        bits += -1 if result < 0 else 1
        result = struct.unpack(">d", struct.pack(">Q", bits))[0]
    return result


def generate(precision: int) -> tuple[dict[str, float], list[int]]:
    scale = 1 << precision
    atan5 = reciprocal_series(5, scale, alternating=True)
    atan239 = reciprocal_series(239, scale, alternating=True)
    pi_lower = 16 * atan5[0] - 4 * atan239[1]
    pi_upper = 16 * atan5[1] - 4 * atan239[0]
    table_bits = LIMB_BITS * LIMB_COUNT
    numerator = (2 * scale) << table_bits
    table = agreed(numerator // pi_upper, numerator // pi_lower, "2/pi table")
    mask = (1 << LIMB_BITS) - 1
    digits = [(table >> shift) & mask
              for shift in range(table_bits - LIMB_BITS, -1, -LIMB_BITS)]

    atanh3 = reciprocal_series(3, scale, alternating=False)
    ln_lower, ln_upper = 2 * atanh3[0], 2 * atanh3[1]
    # Truncate ln(2) to 32 fractional bits (21 trailing zero significand bits).
    split_unit = scale >> 32
    high = agreed(ln_lower // split_unit, ln_upper // split_unit, "ln(2) high")
    high_scaled = high * split_unit

    # max_finite = 2**1024 * (1 - 2**-53).
    # ln(1 - 2**-53) = -2*atanh(1/(2**54 - 1)).
    correction = reciprocal_series((1 << 54) - 1, scale, alternating=False)
    constants = {
        "LN_2_HIGH": float(Fraction(high, 1 << 32)),
        "LN_2_LOW": rounded(ln_lower - high_scaled, ln_upper - high_scaled,
                            scale, "ln(2) low"),
        "INV_LN_2": agreed(float(Fraction(scale, ln_upper)),
                           float(Fraction(scale, ln_lower)), "1/ln(2)"),
        "EXP_OVERFLOW_LIMIT": rounded(1024 * ln_lower - 2 * correction[1],
                                      1024 * ln_upper - 2 * correction[0],
                                      scale, "ln(max_finite)"),
        # Half the smallest subnormal is 2**-1075. The strict '<' guard
        # needs the first binary64 input ABOVE its logarithm.
        "EXP_UNDERFLOW_LIMIT": agreed(round_up(Fraction(-1075 * ln_upper, scale)),
                                       round_up(Fraction(-1075 * ln_lower, scale)),
                                       "ln(2**-1075), rounded upward"),
    }
    return constants, digits


def generated_blocks() -> dict[str, str]:
    constants, digits = agreed(*(generate(p) for p in PRECISIONS), "precision agreement")
    header = [BEGIN, "// Generated by scripts/generate_math_constants.py; see std/math/CONSTANTS.md."]
    math_lines = header + [f"const {name}: f64 = {value!r}" for name, value in constants.items()]
    table_lines = ["    " + line for line in header]
    for start in range(0, LIMB_COUNT, 6):
        entries = [f"0x{digit:06X}" + ("u64" if start + i == 0 else "")
                   for i, digit in enumerate(digits[start:start + 6])]
        prefix = "    digits := [" if start == 0 else "    "
        suffix = "]" if start + 6 == LIMB_COUNT else ","
        table_lines.append(prefix + ", ".join(entries) + suffix)
    return {
        "stdlib/std/math.dodo": "\n".join(math_lines + [END]),
        "stdlib/std/math/trig.dodo": "\n".join(table_lines + ["    " + END]),
    }


def replace_block(source: str, block: str) -> str:
    lines = source.splitlines(keepends=True)
    starts = [i for i, line in enumerate(lines) if line.strip() == BEGIN]
    ends = [i for i, line in enumerate(lines) if line.strip() == END]
    if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
        raise ValueError("expected exactly one ordered pair of generated-block markers")
    return "".join(lines[:starts[0]]) + block + "\n" + "".join(lines[ends[0] + 1:])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="fail if generated blocks differ")
    mode.add_argument("--write", action="store_true", help="replace the generated blocks")
    args = parser.parse_args()
    blocks = generated_blocks()
    if not (args.check or args.write):
        for relative, block in blocks.items():
            print(f"// {relative}\n{block}\n")
        return 0

    # Validate all markers before writing either file.
    changes = []
    for relative, block in blocks.items():
        path = ROOT / relative
        source = path.read_text(encoding="utf-8")
        replacement = replace_block(source, block)
        if source != replacement:
            changes.append((path, replacement))
    if args.check and changes:
        for path, _ in changes:
            print(f"Outdated generated constants: {path.relative_to(ROOT)}", file=sys.stderr)
        print("Run python3 scripts/generate_math_constants.py --write", file=sys.stderr)
        return 1
    for path, replacement in changes:
        path.write_text(replacement, encoding="utf-8")
    print(f"{'Verified' if args.check else 'Generated'} {LIMB_COUNT} limbs and 5 binary64 "
          f"constants at {PRECISIONS[0]} and {PRECISIONS[1]} bits.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
