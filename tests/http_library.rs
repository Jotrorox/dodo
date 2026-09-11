//! Strict HTTP/1.1 fragmentation, bounded fuzzing, independent decoding, lifetimes.
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
fn http_protocol_fragmentation_fuzzing_and_freestanding() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-http-protocol-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for fixture in ["http_checks", "http_client_checks"] {
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
                    .output()
                    .unwrap(),
                "compile HTTP protocol fixture",
            );
            success(
                Command::new(executable).output().unwrap(),
                "run HTTP protocol fixture",
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
                &format!("emit freestanding HTTP object for {target}"),
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn http_parser_views_keep_workspace_borrowed() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for fixture in ["borrow_escape", "borrow_mutation"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(root.join(format!("tests/http/{fixture}.dodo")))
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "{fixture} must fail ownership checking"
        );
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostics.contains("borrow"), "{fixture}: {diagnostics}");
    }
}

#[test]
fn http_serialization_interoperates_with_python_standard_library() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-http-interop-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!(
            "interop-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join("tests/http/interoperability.dodo"))
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile independent HTTP interoperability fixture",
        );
        success(
            Command::new("python3")
                .arg(root.join("tests/http/interop.py"))
                .arg(executable)
                .output()
                .unwrap(),
            "CPython HTTPResponse independently decodes Dodo wire output",
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
