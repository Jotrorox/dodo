use std::{fs, process::Command};

#[test]
fn portable_bytes_execute_and_cross_compile() {
    let scratch = std::env::temp_dir().join(format!("dodo-std-bytes-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for fixture in ["std_bytes", "std_bytes_alloc", "std_checked_views"] {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/stdlib/{fixture}.dodo"));
        for optimization in ["0", "3"] {
            let executable = scratch.join(format!(
                "{fixture}-O{optimization}{}",
                std::env::consts::EXE_SUFFIX
            ));
            let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{fixture}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let result = Command::new(executable).output().unwrap();
            assert!(
                result.status.success(),
                "{fixture} -O{optimization}: {result:?}"
            );
        }
        for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
            let object = scratch.join(format!("{fixture}-{target}.o"));
            let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["-O", "3", "--emit", "obj", "--target", target, "-o"])
                .arg(&object)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{fixture} {target}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
    fs::remove_dir_all(scratch).unwrap();
}
