//! Exercise bundled portable core code as native programs and freestanding objects.
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
fn portable_core_executes_and_stdlib_cross_compiles() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-core-library-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = root.join("tests/stdlib/core_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!(
            "core-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args(["build", source.to_str().unwrap(), "-O", optimization, "-o"])
                .arg(&executable)
                .current_dir(&scratch)
                .output()
                .unwrap(),
            "compile core fixture",
        );
        success(
            Command::new(&executable).output().unwrap(),
            "execute core fixture",
        );
    }
    for fixture in [
        "core_checks",
        "core_intrinsics",
        "alloc_layout",
        "alloc_arena",
        "alloc_pool",
        "alloc_boxed",
    ] {
        let source = root.join(format!("tests/stdlib/{fixture}.dodo"));
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
                &format!("emit {fixture} freestanding object for {target}"),
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn slice_helpers_cannot_outlive_their_storage() {
    let scratch = std::env::temp_dir().join(format!("dodo-core-borrow-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = scratch.join("escape.dodo");
    fs::write(&source, "package escape\nimport \"core/slice\"\nfn escape() -> Option<&i32> from(static) {\nvalues := [42i32]\nreturn slice.first(&values)\n}\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("check")
        .arg(source)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "borrowed slice escaped its storage"
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("borrow") || error.contains("return"),
        "{error}"
    );
    fs::remove_dir_all(scratch).unwrap();
}
