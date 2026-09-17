//! Constant conversions round directly to their destination representation.
use dodoc::{consteval, parser, sema};

fn constant_float(expression: &str, float_bits: u32, pointer_bits: u32) -> f64 {
    let source = format!("package numeric\nconst VALUE: f{float_bits} = {expression}\n");
    let mut program = parser::parse(&source).unwrap();
    sema::check_for_target(&mut program, pointer_bits)
        .unwrap_or_else(|error| panic!("{}", error.render("numeric.dodo", &source)));
    let consteval::Scalar::Float(value) =
        consteval::eval(&program.constants[0].value, pointer_bits).unwrap()
    else {
        panic!("expected a floating constant: {expression}");
    };
    value
}

#[test]
fn integer_to_f32_constants_round_once_at_midpoints() {
    // Adjacent binary32 values here are 2^30 apart. Binary64 cannot preserve
    // the +/-1 offsets, so using it as an intermediate would create false ties.
    for (integer, expected_bits) in [
        ("9007199791611903u64", 0x5a000000),
        ("9007199791611904u64", 0x5a000000),
        ("9007199791611905u64", 0x5a000001),
        ("9007200865353727u64", 0x5a000001),
        ("9007200865353728u64", 0x5a000002),
        ("9007200865353729u64", 0x5a000002),
        ("9007199791611905i64", 0x5a000001),
        ("-9007199791611903i64", 0xda000000),
        ("-9007199791611904i64", 0xda000000),
        ("-9007199791611905i64", 0xda000001),
        ("-9007200865353727i64", 0xda000001),
        ("-9007200865353728i64", 0xda000002),
        ("-9007200865353729i64", 0xda000002),
        // Exercise unsigned values beyond i64::MAX as well.
        ("9223372586610589695u64", 0x5f000000),
        ("9223372586610589696u64", 0x5f000000),
        ("9223372586610589697u64", 0x5f000001),
        // Conversion of an evaluated integer expression follows the same rule.
        ("(9007199791611904u64 + 1u64)", 0x5a000001),
    ] {
        for pointer_bits in [32, 64] {
            let actual = constant_float(&format!("{integer} as f32"), 32, pointer_bits);
            assert_eq!(
                actual.to_bits(),
                f64::from(f32::from_bits(expected_bits)).to_bits(),
                "{integer}, {pointer_bits}-bit target"
            );
        }
    }
}

#[test]
fn integer_to_f32_constants_preserve_width_and_signedness() {
    for (integer, expected_bits) in [
        ("0u8", 0x00000000),
        ("255u8", 0x437f0000),
        ("-128i8", 0xc3000000),
        ("65535u16", 0x477fff00),
        ("-32768i16", 0xc7000000),
        ("4294967295u32", 0x4f800000),
        ("-2147483648i32", 0xcf000000),
        ("18446744073709551615u64", 0x5f800000),
        ("9223372036854775807i64", 0x5f000000),
        ("-9223372036854775808i64", 0xdf000000),
        ("4294967295usize", 0x4f800000),
        ("-2147483648isize", 0xcf000000),
    ] {
        for pointer_bits in [32, 64] {
            assert_eq!(
                constant_float(&format!("{integer} as f32"), 32, pointer_bits),
                f64::from(f32::from_bits(expected_bits)),
                "{integer}, {pointer_bits}-bit target"
            );
        }
    }
    assert_eq!(
        constant_float("9007199791611905usize as f32", 32, 64),
        9007200328482816.0
    );
    assert_eq!(
        constant_float("-9007199791611905isize as f32", 32, 64),
        -9007200328482816.0
    );
}

#[test]
fn explicit_f64_intermediate_keeps_its_own_rounding() {
    assert_eq!(
        constant_float("9007199791611905u64 as f64", 64, 64),
        9007199791611904.0
    );
    assert_eq!(
        constant_float("9007199791611905u64 as f64 as f32", 32, 64),
        9007199254740992.0
    );
}

#[test]
fn float_identity_and_widening_constants_preserve_special_values() {
    for (source_bits, target_bits) in [(32, 32), (64, 64), (32, 64)] {
        for (expression, expected) in [
            (format!("0f{source_bits} / 0f{source_bits}"), f64::NAN),
            (format!("1f{source_bits} / 0f{source_bits}"), f64::INFINITY),
            (
                format!("-1f{source_bits} / 0f{source_bits}"),
                f64::NEG_INFINITY,
            ),
            (format!("-0f{source_bits}"), -0.0),
            (format!("0f{source_bits}"), 0.0),
            (format!("1.5f{source_bits}"), 1.5),
        ] {
            for pointer_bits in [32, 64] {
                let cast = format!("({expression}) as f{target_bits}");
                let actual = constant_float(&cast, target_bits, pointer_bits);
                if expected.is_nan() {
                    assert!(actual.is_nan(), "{cast}, {pointer_bits}-bit target");
                } else {
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "{cast}, {pointer_bits}-bit target"
                    );
                }
            }
        }
    }
}

#[test]
fn constant_float_narrowing_still_rejects_non_finite_and_out_of_range_values() {
    for expression in [
        "(0f64 / 0f64) as f32",
        "(1f64 / 0f64) as f32",
        "(-1f64 / 0f64) as f32",
        "3.5e38f64 as f32",
        "-3.5e38f64 as f32",
        "(0f32 / 0f32) as f32 as f64 as f32",
        "(1f32 / 0f32) as f32 as f64 as f32",
    ] {
        for pointer_bits in [32, 64] {
            let source = format!("package numeric\nconst VALUE: f32 = {expression}\n");
            let mut program = parser::parse(&source).unwrap();
            let error = sema::check_for_target(&mut program, pointer_bits).unwrap_err();
            assert!(
                error
                    .message
                    .contains("constant floating conversion out of range"),
                "{}",
                error.render("numeric.dodo", &source)
            );
        }
    }
}
