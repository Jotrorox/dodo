//! Portable formatting and an independent Rust binary64 formatting oracle.
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn execute(source: &Path, scratch: &Path, name: &str) {
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!(
            "{name}-{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args(["build"])
                .arg(source)
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            &format!("compile {name} -O{optimization}"),
        );
        success(
            Command::new(executable).output().unwrap(),
            &format!("execute {name} -O{optimization}"),
        );
    }
}

#[test]
fn formatting_executes_and_cross_compiles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-std-fmt-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = root.join("tests/stdlib/std_fmt.dodo");
    execute(&source, &scratch, "formatting");
    execute(
        &root.join("tests/stdlib/std_fmt_alloc.dodo"),
        &scratch,
        "allocated-formatting",
    );
    execute(&root.join("examples/formatting.dodo"), &scratch, "example");
    for fixture in ["std_fmt", "std_fmt_alloc"] {
        let source = root.join(format!("tests/stdlib/{fixture}.dodo"));
        for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
            for optimization in ["0", "3"] {
                success(
                    Command::new(env!("CARGO_BIN_EXE_dodo"))
                        .arg("build")
                        .arg(&source)
                        .args([
                            "--emit",
                            "obj",
                            "--target",
                            target,
                            "-O",
                            optimization,
                            "-o",
                        ])
                        .arg(scratch.join(format!("{fixture}-{target}-O{optimization}.o")))
                        .output()
                        .unwrap(),
                    &format!("cross compile {fixture} for {target} at -O{optimization}"),
                );
            }
        }
    }
    fs::remove_dir_all(scratch).unwrap();
}

fn scientific(value: f64, precision: usize) -> String {
    let formatted = format!("{value:.precision$e}");
    let (mantissa, exponent) = formatted.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    format!("{mantissa}e{exponent:+03}")
}

#[test]
fn binary64_conversion_matches_independent_rust_oracle() {
    let scratch = std::env::temp_dir().join(format!("dodo-float-oracle-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let mut source = String::from(
        r#"package float_oracle
import "core/ptr"
import "core/bytes" as core_bytes
import "std/io"
import "std/fmt"
fn check(bits:u64, precision:usize, scientific:bool, expected:&[u8])->bool!io.Error {
    value := unsafe { ptr.read_unaligned(ptr.from_ref(&bits) as *const f64) }
    storage := [0u8;640]
    sink := io.MemoryWriter.new(&mut storage)
    {
        output := fmt.Formatter.new::<io.MemoryWriter>(&mut sink)
        options := fmt.defaults()
        if scientific {
            output.floating(value,precision,fmt.FloatStyle.Scientific,&options)?
        } else {
            output.floating(value,precision,fmt.FloatStyle.Fixed,&options)?
        }
    }
    return ok(core_bytes.equal(sink.written(),expected))
}
fn verify()->i32!io.Error {
"#,
    );
    let boundaries: [f64; 20] = [
        0.0,
        -0.0,
        0.5,
        2.5,
        3.5,
        0.125,
        0.375,
        1.005,
        9.999,
        99.999,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::from_bits(0x000f_ffff_ffff_ffff),
        f64::MAX,
        -f64::MAX,
        1e-100,
        1e100,
        f64::from_bits(1.0f64.to_bits() - 1),
        1.0,
        f64::from_bits(1.0f64.to_bits() + 1),
    ];
    let mut cases = Vec::new();
    for value in boundaries {
        for precision in [0, 2, 17, 324] {
            for is_scientific in [false, true] {
                cases.push((value, precision, is_scientific));
            }
        }
    }
    let mut bits = 0x6a09_e667_f3bc_c909u64;
    for index in 0..192 {
        bits ^= bits << 13;
        bits ^= bits >> 7;
        bits ^= bits << 17;
        let value = f64::from_bits(bits);
        if value.is_finite() {
            cases.push((
                value,
                [0, 1, 2, 6, 17, 30, 100, 324][index % 8],
                index % 2 == 0,
            ));
        }
    }
    for (index, &(value, precision, is_scientific)) in cases.iter().enumerate() {
        let expected = if is_scientific {
            scientific(value, precision)
        } else {
            format!("{value:.precision$}")
        };
        source.push_str(&format!(
            "if!check({}u64, {precision}, {is_scientific}, b\"{expected}\")? {{ return ok({}) }}\n",
            value.to_bits(),
            index % 240 + 1
        ));
    }
    source.push_str("return ok(0)\n}\nfn main()->i32 { match verify() { ok(code)=>{return code}, err(_)=>{return 250} } }\n");
    let input = scratch.join("float_oracle.dodo");
    fs::write(&input, source).unwrap();
    execute(&input, &scratch, "float-oracle");
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn formatting_retains_sink_borrows_and_result_obligations() {
    let scratch = std::env::temp_dir().join(format!("dodo-fmt-safety-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let cases = [
        (
            "escaping_formatter",
            r#"package escaping_formatter
import "std/fmt"
import "std/io"
fn escape()->fmt.Formatter<io.MemoryWriter> from(static) {
    storage := [0u8;16]
    sink := io.MemoryWriter.new(&mut storage)
    return fmt.Formatter.new::<io.MemoryWriter>(&mut sink)
}
"#,
            "borrow",
        ),
        (
            "unhandled_format_error",
            r#"package unhandled_format_error
import "std/fmt"
import "std/io"
fn main()->i32 {
    storage := [0u8;16]
    sink := io.MemoryWriter.new(&mut storage)
    output := fmt.Formatter.new::<io.MemoryWriter>(&mut sink)
    output.string("unchecked")
    return 0
}
"#,
            "result",
        ),
    ];
    for (name, contents, diagnostic) in cases {
        let source = scratch.join(format!("{name}.dodo"));
        fs::write(&source, contents).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{name} unexpectedly accepted");
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        assert!(stderr.contains(diagnostic), "{name}: {stderr}");
    }
    fs::remove_dir_all(scratch).unwrap();
}
