//! Repeated local arrays initialize their destination without huge SSA values.
use std::fs;
use std::path::PathBuf;
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

#[test]
fn large_repetition_executes_once_and_preserves_cleanup() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-array-storage-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = root.join("tests/stdlib/array_storage.dodo");
    for level in ["0", "3"] {
        let executable = scratch.join(format!("large-O{level}{}", std::env::consts::EXE_SUFFIX));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args(["build", source.to_str().unwrap(), "-O", level, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile large repeated local",
        );
        success(
            Command::new(&executable).output().unwrap(),
            "execute large repeated local",
        );
    }
    let ir = scratch.join("large.ll");
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args([
                "build",
                source.to_str().unwrap(),
                "--emit",
                "llvm-ir",
                "-O",
                "0",
                "-o",
            ])
            .arg(&ir)
            .output()
            .unwrap(),
        "emit repeated local IR",
    );
    let ir = fs::read_to_string(ir).unwrap();
    assert!(
        !ir.contains("load [262144 x i8]"),
        "large initializer loaded as an SSA aggregate"
    );
    assert!(
        !ir.contains("store [262144 x i8]"),
        "large initializer stored as an SSA aggregate"
    );
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        let object = scratch.join(format!("large-{target}.o"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args([
                    "build",
                    source.to_str().unwrap(),
                    "--emit",
                    "obj",
                    "--target",
                    target,
                    "-O",
                    "3",
                    "-o",
                ])
                .arg(&object)
                .output()
                .unwrap(),
            "emit large repeated local portable object",
        );
        assert!(fs::metadata(object).unwrap().len() > 0);
    }
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn repetition_keeps_copy_and_reference_requirements() {
    let scratch = std::env::temp_dir().join(format!("dodo-array-reject-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for (name, program, reason) in [
        (
            "copy",
            "package app\nstruct Owned {}\nfn main(){values:=[Owned {};0]}\n",
            "copyable",
        ),
        (
            "borrow",
            "package app\nfn main()->i32{value:=1i32\nreferences:=[&value;3]\nvalue=2\nreturn *references[0]}\n",
            "borrow",
        ),
        (
            "result",
            "package app\nfn value()->u8!i32{return ok(1)}\nfn main(){values:=[value();4]}\n",
            "copyable",
        ),
    ] {
        let source = scratch.join(format!("{name}.dodo"));
        fs::write(&source, program).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted invalid {name}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(reason), "{name}: {error}");
    }
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn large_internal_calls_preserve_values_and_ownership() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-large-calls-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = root.join("tests/stdlib/large_value_calls.dodo");
    for level in ["0", "3"] {
        let executable = scratch.join(format!("calls-{level}{}", std::env::consts::EXE_SUFFIX));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["-O", level, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile large internal calls",
        );
        success(
            Command::new(&executable).output().unwrap(),
            "execute large internal calls",
        );
        for target in [
            "wasm32-unknown-unknown",
            "thumbv6m-none-eabi",
            "x86_64-pc-windows-msvc",
        ] {
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(&source)
                    .args(["-O", level, "--target", target, "--emit", "obj", "-o"])
                    .arg(scratch.join(format!("{target}-{level}.o")))
                    .output()
                    .unwrap(),
                "cross-compile large internal calls",
            );
        }
    }
    let ir = scratch.join("calls.ll");
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .args(["--emit", "llvm-ir", "-o"])
            .arg(&ir)
            .output()
            .unwrap(),
        "inspect large internal call ABI",
    );
    let ir = fs::read_to_string(ir).unwrap();
    assert!(
        ir.contains("define internal void @dodo.large_value_calls.through(ptr"),
        "large Result must return through caller storage"
    );
    fs::remove_dir_all(scratch).unwrap();
}
