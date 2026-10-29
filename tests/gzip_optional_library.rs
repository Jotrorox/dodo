//! Exercise all optional gzip header fields and header CRC validation.
use std::path::PathBuf;
use std::process::Command;

#[test]
fn gzip_optional_header_fields() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let executable = std::env::temp_dir().join(format!("dodo-gzip-header-{}", std::process::id()));
    let compile = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(root.join("tests/stdlib/gzip_optional_checks.dodo"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let result = Command::new(&executable).output().unwrap();
    assert!(
        result.status.success(),
        "fixture failed: {} {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::remove_file(executable).unwrap();
}
