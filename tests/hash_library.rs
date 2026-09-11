//! Published hash vectors, streaming boundaries, and freestanding portability.
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
fn hash_vectors_streaming_and_portable_objects() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-hash-library-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let source = root.join("tests/stdlib/hash_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!(
            "hash-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args(["build", source.to_str().unwrap(), "-O", optimization, "-o"])
                .arg(&executable)
                .current_dir(&scratch)
                .output()
                .unwrap(),
            "compile hash fixture",
        );
        success(
            Command::new(&executable).output().unwrap(),
            "execute hash vectors",
        );
    }
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        let object = scratch.join(format!("hash-{target}.o"));
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
            &format!("emit hash freestanding object for {target}"),
        );
        assert!(fs::metadata(object).unwrap().len() > 0);
    }
    fs::remove_dir_all(scratch).unwrap();
}
