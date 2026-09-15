//! Portable UTF-8, exact decimal parsing, and owned string lifetime regressions.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-std-text-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn execute(source: &std::path::Path, scratch: &Scratch, stem: &str) {
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "{stem}-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(source)
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile text fixture",
        );
        success(
            Command::new(&executable).output().unwrap(),
            "execute text fixture",
        );
    }
}

#[test]
fn text_executes_and_cross_compiles() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new();
    for fixture in ["std_text", "std_text_alloc"] {
        let source = root.join(format!("tests/stdlib/{fixture}.dodo"));
        execute(&source, &scratch, fixture);
        for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
            let object = scratch.0.join(format!("{fixture}-{target}.o"));
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(&source)
                    .args(["--emit", "obj", "--target", target, "-O", "3", "-o"])
                    .arg(&object)
                    .output()
                    .unwrap(),
                "cross-compile text fixture",
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
}

#[test]
fn decimal_parser_matches_binary64_reference() {
    let scratch = Scratch::new();
    let source = scratch.0.join("differential.dodo");
    let mut cases = Vec::new();
    let mut state = 0x26b5_159a_f014_bebdu64;
    for _ in 0..128 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let value = f64::from_bits(state);
        if value.is_finite() {
            cases.push(format!("{value:.17e}"));
        }
        let exponent = ((state >> 48) % 660) as i32 - 345;
        cases.push(format!("{}.{}e{exponent}", state >> 20, state & 0xfffff));
    }
    // Values adjacent to half a subnormal, and long exact binary fractions.
    cases.extend([
        "2.4703282292062327e-324".to_owned(),
        "2.4703282292062328e-324".to_owned(),
        "2.2250738585072012e-308".to_owned(),
        "1.7976931348623158e308".to_owned(),
        "0.00000000000000000000000000000000000000000000000001".to_owned(),
        format!("1{}e-767", "0".repeat(767)),
        format!("0.{}1e443", "0".repeat(766)),
    ]);
    let mut program = String::from(
        "package differential\nimport \"std/text\"\nimport \"core/ptr\"\nfn bits(value: f64) -> u64 { unsafe { return ptr.read(ptr.from_ref(&value) as *const u64) } }\nfn main() -> i32 {\n",
    );
    for (index, input) in cases.iter().enumerate() {
        let expected: f64 = input.parse().unwrap();
        let code = index % 250 + 1;
        if expected.is_infinite() {
            program.push_str(&format!(
                "match text.parse_f64(b\"{input}\") {{ ok(_) => {{ return {code} }}, err(_) => {{}} }}\n"
            ));
        } else {
            program.push_str(&format!(
                "match text.parse_f64(b\"{input}\") {{ ok(value) => {{ if bits(value) != {}u64 {{ return {code} }} }}, err(_) => {{ return {code} }} }}\n",
                expected.to_bits()
            ));
        }
    }
    program.push_str("return 0\n}\n");
    fs::write(&source, program).unwrap();
    execute(&source, &scratch, "differential");
}

#[test]
fn text_views_preserve_borrows_and_results() {
    let scratch = Scratch::new();
    let source = scratch.0.join("rejected.dodo");
    let cases = [
        (
            "package bad\nimport \"std/text\"\nfn escape() -> text.Text from(static) { storage := [65u8]\nmatch text.Text.new(&storage) { ok(value) => { return value.trim_ascii() }, err(_) => { return text.Text.from_str(\"\") } } }\n",
            "borrow",
        ),
        (
            "package bad\nimport \"std/text\"\nfn main() -> i32 { storage := [0u8; 8]\nbuilder := text.Builder.new(&mut storage)\nview := builder.as_text()\ntrimmed := view.trim_ascii()\nbuilder.clear()\nreturn trimmed.len_bytes() as i32 }\n",
            "borrow",
        ),
        (
            "package bad\nimport \"std/text\"\nfn escape() -> text.Text!text.Error from(static) { data := [65u8]\nreturn text.Text.new(&data) }\n",
            "borrow",
        ),
        (
            "package bad\nimport \"std/text\"\nfn main() -> i32 { storage := [0u8; 8]\nbuilder := text.Builder.new(&mut storage)\nview := builder.as_text()\nbuilder.clear()\nreturn view.len_bytes() as i32 }\n",
            "borrow",
        ),
        (
            "package bad\nimport \"std/text\"\nfn main() -> i32 { value := text.Text.new(b\"valid\")\nreturn 0 }\n",
            "Result",
        ),
        (
            "package bad\nimport \"std/text\"\nimport \"core/mem\"\nfn escape() -> &[u8] from(static) { data := [65u8]\nunsafe { value := mem.str_from_utf8(&data)\nreturn mem.str_bytes(value) } }\n",
            "borrow",
        ),
        (
            "package bad\nimport \"core/mem\"\nfn main() -> i32 { value := mem.str_from_utf8(b\"valid\")\nreturn value.len as i32 }\n",
            "unsafe",
        ),
        (
            "package bad\nimport \"std/text_alloc\"\nimport \"std/arena_bytes\"\nimport \"alloc/arena\"\nfn main() -> i32 { storage := [0u8; 128]\na := arena.Arena.new(&mut storage)\nmatch arena_bytes.new(&mut a, 4, 64) { ok(buffer) => { match text_alloc.String.new(buffer) { ok(value) => { view := value.as_text()\nmatch value.reserve(32) { ok() => {}, err(_) => {} }\nreturn view.len_bytes() as i32 }, err(_) => { return 1 } } }, err(_) => { return 2 } } }\n",
            "borrow",
        ),
        (
            "package bad\nimport \"std/text_alloc\"\nimport \"std/arena_bytes\"\nimport \"alloc/arena\"\nfn main() -> i32 { storage := [0u8; 128]\na := arena.Arena.new(&mut storage)\nmatch arena_bytes.new(&mut a, 4, 64) { ok(buffer) => { match text_alloc.String.new(buffer) { ok(value) => { view := value.as_text()\ncore.drop(value)\nreturn view.len_bytes() as i32 }, err(_) => { return 1 } } }, err(_) => { return 2 } } }\n",
            "borrow",
        ),
    ];
    for (program, message) in cases {
        fs::write(&source, program).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source)
            .output()
            .unwrap();
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "accepted invalid source: {program}"
        );
        assert!(error.contains(message), "expected {message:?}: {error}");
        assert!(!error.contains("panicked at"), "{error}");
    }
}
