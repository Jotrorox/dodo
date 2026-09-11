//! Portable math runtime, high-precision reference vectors and freestanding codegen.
use std::fs;
use std::path::PathBuf;
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

#[test]
fn portable_math_boundaries_and_mpfr_vectors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-math-library-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for fixture in ["math_checks", "math_reference"] {
        let source = root.join(format!("tests/stdlib/{fixture}.dodo"));
        for optimization in ["0", "3"] {
            let executable = scratch.join(format!(
                "{fixture}-O{optimization}{}",
                std::env::consts::EXE_SUFFIX
            ));
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .args(["build", source.to_str().unwrap(), "-O", optimization, "-o"])
                    .arg(&executable)
                    .current_dir(&scratch)
                    .output()
                    .unwrap(),
                &format!("compile {fixture} -O{optimization}"),
            );
            success(
                Command::new(&executable).output().unwrap(),
                &format!("execute {fixture} -O{optimization}"),
            );
        }
        for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
            let object = scratch.join(format!("{fixture}-{target}.o"));
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
                &format!("emit {fixture} for {target}"),
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
    // Portable implementation does not lower its elementary functions to libm.
    let ir = scratch.join("math.ll");
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args([
                "build",
                "tests/stdlib/math_checks.dodo",
                "--emit",
                "llvm-ir",
                "-O",
                "3",
                "-o",
            ])
            .arg(&ir)
            .current_dir(&root)
            .output()
            .unwrap(),
        "emit portable math IR",
    );
    let ir_text = fs::read_to_string(ir).unwrap();
    for external in [
        "@sqrt(", "@cbrt(", "@log(", "@exp(", "@sin(", "@cos(", "@tan(",
    ] {
        assert!(
            !ir_text.contains(external),
            "unconditional libm dependency: {external}"
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
