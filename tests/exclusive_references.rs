//! `&mut T` parameters of Dodo functions are `noalias`, so LLVM may keep what
//! it loaded through them while storing through other pointers.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn success(output: Output, context: &str) -> Output {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn build(source: &Path, output: &Path, args: &[&str], context: &str) {
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
            .args(args)
            .arg("-o")
            .arg(output)
            .output()
            .unwrap(),
        context,
    );
}

/// The `define` line of the function whose symbol contains `name`.
fn signature<'a>(ir: &'a str, name: &str) -> &'a str {
    ir.lines()
        .find(|line| line.starts_with("define") && line.contains(name))
        .unwrap_or_else(|| panic!("no definition of {name} in\n{ir}"))
}

/// The body of the function whose symbol contains `name`.
fn body<'a>(ir: &'a str, name: &str) -> &'a str {
    let start = ir.find(signature(ir, name)).unwrap();
    let end = start + ir[start..].find("\n}\n").unwrap();
    &ir[start..end]
}

#[test]
fn exclusive_reference_parameters_are_noalias() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch =
        std::env::temp_dir().join(format!("dodo-exclusive-references-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = root.join("tests/fixtures/exclusive_references.dodo");
    for level in ["0", "3"] {
        let executable = scratch.join(format!(
            "exclusive-O{level}{}",
            std::env::consts::EXE_SUFFIX
        ));
        build(
            &source,
            &executable,
            &["-O", level],
            "compile exclusive references",
        );
        success(
            Command::new(&executable).output().unwrap(),
            "execute exclusive references",
        );
    }

    let ir_path = scratch.join("exclusive.ll");
    build(
        &source,
        &ir_path,
        &["--emit", "llvm-ir"],
        "emit unoptimized IR",
    );
    let ir = fs::read_to_string(&ir_path).unwrap();
    assert!(signature(&ir, "Buffer.fill(").contains("(ptr noalias %0, i8 %1)"));
    assert!(signature(&ir, "Tally.bump(").contains("(ptr noalias %0)"));
    // A shared reference may still see atomic writes, so it stays unmarked.
    assert!(signature(&ir, "exclusive.add(").contains("(ptr noalias %0, ptr %1)"));
    // `&mut dyn` is a pair of pointers, which LLVM cannot mark.
    assert!(!signature(&ir, "exclusive.bump_dyn(").contains("noalias"));
    // The result slot and a closure's environment come first.
    assert!(signature(&ir, "exclusive.large(").contains("(ptr %0, ptr noalias %1)"));
    assert!(signature(&ir, "main#closure").contains("(ptr %0, ptr noalias %1)"));

    build(
        &source,
        &ir_path,
        &["--emit", "llvm-ir", "-O", "3"],
        "emit optimized IR",
    );
    let ir = fs::read_to_string(&ir_path).unwrap();
    let fill = body(&ir, "exclusive.fill_buffer(");
    assert!(
        fill.contains("@llvm.memset"),
        "filling a slice field through `&mut self` should become memset:\n{fill}"
    );
    fs::remove_dir_all(scratch).unwrap();
}
