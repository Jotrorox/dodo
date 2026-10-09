//! The firmware runtime's floating-point helpers (src/hardware/arm_float.ll)
//! are integer-only IR, so they also compile for the host. This builds them
//! for the host, links them into a small C driver, and checks each helper
//! against the host's IEEE 754 hardware on special values, boundary cases
//! and random operands. Rust's `as` conversions saturate and map NaN to zero,
//! which is the helpers' contract too.
#![cfg(unix)]
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const RUNTIME: &str = include_str!("../src/hardware/arm_float.ll");

// Reads `name a b` lines in hex and prints each result in hex.
const DRIVER: &str = r#"
#include <inttypes.h>
#include <stdio.h>
#include <string.h>
typedef uint32_t u32;
typedef uint64_t u64;
#define BINARY(X) \
    X(__aeabi_dadd, u64, u64) X(__aeabi_dsub, u64, u64) X(__aeabi_dmul, u64, u64) \
    X(__aeabi_ddiv, u64, u64) X(soft_fmod, u64, u64) \
    X(__aeabi_fadd, u32, u32) X(__aeabi_fsub, u32, u32) X(__aeabi_fmul, u32, u32) \
    X(__aeabi_fdiv, u32, u32) X(soft_fmodf, u32, u32) \
    X(__aeabi_dcmpeq, u32, u64) X(__aeabi_dcmplt, u32, u64) X(__aeabi_dcmple, u32, u64) \
    X(__aeabi_dcmpgt, u32, u64) X(__aeabi_dcmpge, u32, u64) X(__aeabi_dcmpun, u32, u64) \
    X(__aeabi_fcmpeq, u32, u32) X(__aeabi_fcmplt, u32, u32) X(__aeabi_fcmple, u32, u32) \
    X(__aeabi_fcmpgt, u32, u32) X(__aeabi_fcmpge, u32, u32) X(__aeabi_fcmpun, u32, u32) \
    X(__ledf2, u32, u64) X(__ltdf2, u32, u64) X(__eqdf2, u32, u64) X(__nedf2, u32, u64) \
    X(__gedf2, u32, u64) X(__gtdf2, u32, u64) \
    X(__lesf2, u32, u32) X(__ltsf2, u32, u32) X(__eqsf2, u32, u32) X(__nesf2, u32, u32) \
    X(__gesf2, u32, u32) X(__gtsf2, u32, u32)
#define UNARY(X) \
    X(__aeabi_d2iz, u32, u64) X(__aeabi_d2uiz, u32, u64) X(__aeabi_d2lz, u64, u64) \
    X(__aeabi_d2ulz, u64, u64) X(__aeabi_f2iz, u32, u32) X(__aeabi_f2uiz, u32, u32) \
    X(__aeabi_f2lz, u64, u32) X(__aeabi_f2ulz, u64, u32) \
    X(__aeabi_i2d, u64, u32) X(__aeabi_ui2d, u64, u32) X(__aeabi_l2d, u64, u64) \
    X(__aeabi_ul2d, u64, u64) X(__aeabi_i2f, u32, u32) X(__aeabi_ui2f, u32, u32) \
    X(__aeabi_l2f, u32, u64) X(__aeabi_ul2f, u32, u64) \
    X(__aeabi_f2d, u64, u32) X(__aeabi_d2f, u32, u64)
#define DECLARE2(name, R, A) R name(A, A);
#define DECLARE1(name, R, A) R name(A);
BINARY(DECLARE2)
UNARY(DECLARE1)
#define CALL2(name, R, A) if (!strcmp(op, #name)) r = (R)name((A)a, (A)b); else
#define CALL1(name, R, A) if (!strcmp(op, #name)) r = (R)name((A)a); else
int main(void) {
    char op[32];
    u64 a, b, r;
    while (scanf("%31s %" SCNx64 " %" SCNx64, op, &a, &b) == 3) {
        BINARY(CALL2) UNARY(CALL1) return 2;
        printf("%" PRIx64 "\n", r);
    }
    return 0;
}
"#;

#[derive(Clone, Copy)]
enum Result {
    F64(f64),
    F32(f32),
    Bits(u64),
}

struct Case {
    op: &'static str,
    a: u64,
    b: u64,
    expected: Result,
}

/// xorshift64*: deterministic, so a failure reproduces.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// Bits of a binary float with `exp` exponent and `frac` fraction bits,
    /// weighted toward zeros, subnormals, infinities, NaNs, the overflow
    /// boundary and fractions with few set bits, where rounding ties live.
    fn float(&mut self, exp: u32, frac: u32) -> u64 {
        let max = (1u64 << exp) - 1;
        let mask = (1u64 << frac) - 1;
        let sign = self.below(2) << (exp + frac);
        let bias = max >> 1;
        let (e, f) = match self.below(9) {
            0 => return self.next() & (u64::MAX >> (63 - exp - frac)),
            1 => (self.below(3), self.next() & mask),
            2 => (max - self.below(4), self.next() & mask),
            3 => (
                [0, max][self.below(2) as usize],
                [0, 1, mask][self.below(3) as usize],
            ),
            4 => (bias + self.below(130) - 65, self.next() & mask),
            5 => {
                let few = self.next() & self.next() & self.next();
                (self.below(max + 1), few & mask)
            }
            6 => (
                self.below(max + 1),
                [0, mask, 1, 1 << (frac - 1)][self.below(4) as usize],
            ),
            7 => (
                bias + self.below(64),
                self.next() & mask & !(mask >> self.below(frac as u64 + 1)),
            ),
            _ => (self.below(max + 1), self.next() & mask),
        };
        sign | e << frac | f
    }
    fn f64(&mut self) -> u64 {
        self.float(11, 52)
    }
    fn f32(&mut self) -> u64 {
        self.float(8, 23)
    }

    /// A second operand near the first half the time, for cancellation,
    /// exact results and equal comparisons.
    fn partner(&mut self, a: u64, width: u32, fresh: u64) -> u64 {
        let sign = 1u64 << (width - 1);
        match self.below(6) {
            0 => a,
            1 => a ^ sign,
            2 => a ^ (self.next() & 0xF),
            3 => a.wrapping_add(self.below(4) << (width - 12)) & (sign | (sign - 1)),
            _ => fresh,
        }
    }

    fn int(&mut self) -> u64 {
        match self.below(4) {
            0 => [
                0,
                1,
                u64::MAX,
                1 << 63,
                (1 << 63) - 1,
                1 << 53,
                (1 << 53) + 1,
                1 << 24,
            ][self.below(8) as usize],
            _ => self.next() >> self.below(64),
        }
    }
}

fn cases() -> Vec<Case> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut cases = vec![];
    let push = |cases: &mut Vec<Case>, op, a, b, expected| cases.push(Case { op, a, b, expected });
    for _ in 0..30_000 {
        let a = rng.f64();
        let fresh = rng.f64();
        let b = rng.partner(a, 64, fresh);
        let (x, y) = (f64::from_bits(a), f64::from_bits(b));
        push(&mut cases, "__aeabi_dadd", a, b, Result::F64(x + y));
        push(&mut cases, "__aeabi_dsub", a, b, Result::F64(x - y));
        push(&mut cases, "__aeabi_dmul", a, b, Result::F64(x * y));
        push(&mut cases, "__aeabi_ddiv", a, b, Result::F64(x / y));
        push(&mut cases, "soft_fmod", a, b, Result::F64(x % y));
        for (op, value) in [
            ("__aeabi_dcmpeq", x == y),
            ("__aeabi_dcmplt", x < y),
            ("__aeabi_dcmple", x <= y),
            ("__aeabi_dcmpgt", x > y),
            ("__aeabi_dcmpge", x >= y),
            ("__aeabi_dcmpun", x.is_nan() || y.is_nan()),
        ] {
            push(&mut cases, op, a, b, Result::Bits(value as u64));
        }
        for (op, unordered) in [
            ("__ledf2", 1),
            ("__ltdf2", 1),
            ("__eqdf2", 1),
            ("__nedf2", 1),
            ("__gedf2", -1),
            ("__gtdf2", -1),
        ] {
            push(
                &mut cases,
                op,
                a,
                b,
                Result::Bits(three_way(x.partial_cmp(&y), unordered)),
            );
        }
        push(
            &mut cases,
            "__aeabi_d2iz",
            a,
            0,
            Result::Bits(x as i32 as u32 as u64),
        );
        push(
            &mut cases,
            "__aeabi_d2uiz",
            a,
            0,
            Result::Bits(x as u32 as u64),
        );
        push(
            &mut cases,
            "__aeabi_d2lz",
            a,
            0,
            Result::Bits(x as i64 as u64),
        );
        push(&mut cases, "__aeabi_d2ulz", a, 0, Result::Bits(x as u64));
        push(&mut cases, "__aeabi_d2f", a, 0, Result::F32(x as f32));

        let a = rng.f32();
        let fresh = rng.f32();
        let b = rng.partner(a, 32, fresh);
        let (x, y) = (f32::from_bits(a as u32), f32::from_bits(b as u32));
        push(&mut cases, "__aeabi_fadd", a, b, Result::F32(x + y));
        push(&mut cases, "__aeabi_fsub", a, b, Result::F32(x - y));
        push(&mut cases, "__aeabi_fmul", a, b, Result::F32(x * y));
        push(&mut cases, "__aeabi_fdiv", a, b, Result::F32(x / y));
        push(&mut cases, "soft_fmodf", a, b, Result::F32(x % y));
        for (op, value) in [
            ("__aeabi_fcmpeq", x == y),
            ("__aeabi_fcmplt", x < y),
            ("__aeabi_fcmple", x <= y),
            ("__aeabi_fcmpgt", x > y),
            ("__aeabi_fcmpge", x >= y),
            ("__aeabi_fcmpun", x.is_nan() || y.is_nan()),
        ] {
            push(&mut cases, op, a, b, Result::Bits(value as u64));
        }
        for (op, unordered) in [
            ("__lesf2", 1),
            ("__ltsf2", 1),
            ("__eqsf2", 1),
            ("__nesf2", 1),
            ("__gesf2", -1),
            ("__gtsf2", -1),
        ] {
            push(
                &mut cases,
                op,
                a,
                b,
                Result::Bits(three_way(x.partial_cmp(&y), unordered)),
            );
        }
        push(
            &mut cases,
            "__aeabi_f2iz",
            a,
            0,
            Result::Bits(x as i32 as u32 as u64),
        );
        push(
            &mut cases,
            "__aeabi_f2uiz",
            a,
            0,
            Result::Bits(x as u32 as u64),
        );
        push(
            &mut cases,
            "__aeabi_f2lz",
            a,
            0,
            Result::Bits(x as i64 as u64),
        );
        push(&mut cases, "__aeabi_f2ulz", a, 0, Result::Bits(x as u64));
        push(&mut cases, "__aeabi_f2d", a, 0, Result::F64(x as f64));

        let n = rng.int();
        let n = if rng.below(2) == 0 {
            n
        } else {
            n.wrapping_neg()
        };
        let n32 = n as u32 as u64;
        push(
            &mut cases,
            "__aeabi_i2d",
            n32,
            0,
            Result::F64(n as i32 as f64),
        );
        push(
            &mut cases,
            "__aeabi_ui2d",
            n32,
            0,
            Result::F64(n as u32 as f64),
        );
        push(
            &mut cases,
            "__aeabi_l2d",
            n,
            0,
            Result::F64(n as i64 as f64),
        );
        push(&mut cases, "__aeabi_ul2d", n, 0, Result::F64(n as f64));
        push(
            &mut cases,
            "__aeabi_i2f",
            n32,
            0,
            Result::F32(n as i32 as f32),
        );
        push(
            &mut cases,
            "__aeabi_ui2f",
            n32,
            0,
            Result::F32(n as u32 as f32),
        );
        push(
            &mut cases,
            "__aeabi_l2f",
            n,
            0,
            Result::F32(n as i64 as f32),
        );
        push(&mut cases, "__aeabi_ul2f", n, 0, Result::F32(n as f32));
    }
    cases
}

/// The GNU comparison result as the driver prints it.
fn three_way(order: Option<std::cmp::Ordering>, unordered: i32) -> u64 {
    let value = order.map_or(unordered, |order| order as i32);
    value as u32 as u64
}

fn matches(expected: Result, actual: u64) -> bool {
    match expected {
        // Any NaN will do: the helpers return the default NaN.
        Result::F64(v) if v.is_nan() => f64::from_bits(actual).is_nan(),
        Result::F32(v) if v.is_nan() => actual >> 32 == 0 && f32::from_bits(actual as u32).is_nan(),
        Result::F64(v) => v.to_bits() == actual,
        Result::F32(v) => v.to_bits() as u64 == actual,
        Result::Bits(v) => v == actual,
    }
}

#[test]
fn soft_float_helpers_match_host_hardware() {
    let dir = std::env::temp_dir().join(format!("dodo-soft-float-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let object = dir.join("arm_float.o");
    // The host's C runtime has its own fmod, fmodf and GNU helpers (with
    // hard-float arguments), and macOS lets those replace weak definitions
    // at load time. Rename fmod and make everything strong.
    let runtime = RUNTIME
        .replace("@fmod(", "@soft_fmod(")
        .replace("@fmodf(", "@soft_fmodf(")
        .replace("define weak ", "define ")
        .replace("weak alias ", "alias ");
    let options = dodoc::codegen::Options {
        optimization: 2,
        ..Default::default()
    };
    dodoc::codegen::compile_ir_object(&options, "arm_float", &runtime, &object).unwrap();
    let driver = dir.join("driver.c");
    fs::write(&driver, DRIVER).unwrap();
    let program: PathBuf = dir.join("driver");
    let cc = std::env::var_os("DODO_CC").unwrap_or_else(|| "cc".into());
    let output = Command::new(&cc)
        .arg(&driver)
        .arg(&object)
        .arg("-o")
        .arg(&program)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let cases = cases();
    let mut input = String::new();
    for case in &cases {
        writeln!(input, "{} {:x} {:x}", case.op, case.a, case.b).unwrap();
    }
    let mut child = Command::new(&program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(), "driver failed: {}", output.status);
    let text = String::from_utf8(output.stdout).unwrap();
    let results: Vec<u64> = text
        .lines()
        .map(|line| u64::from_str_radix(line, 16).unwrap())
        .collect();
    assert_eq!(results.len(), cases.len());
    let failures: Vec<String> = cases
        .iter()
        .zip(&results)
        .filter(|(case, actual)| !matches(case.expected, **actual))
        .map(|(case, actual)| {
            let expected = match case.expected {
                Result::F64(v) => format!("{:#x} ({v:e})", v.to_bits()),
                Result::F32(v) => format!("{:#x} ({v:e})", v.to_bits()),
                Result::Bits(v) => format!("{v:#x}"),
            };
            format!(
                "{}({:#x}, {:#x}) = {actual:#x}, expected {expected}",
                case.op, case.a, case.b
            )
        })
        .collect();
    let _ = fs::remove_dir_all(&dir);
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures[..failures.len().min(40)].join("\n")
    );
}
