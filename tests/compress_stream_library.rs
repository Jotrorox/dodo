//! Streaming wrappers, backpressure, and I/O progress contracts.
use std::path::PathBuf;
use std::process::Command;

#[test]
fn streaming_wrappers_and_io() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = std::env::temp_dir().join(format!("dodo-compress-stream-{}", std::process::id()));
    let compile = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(root.join("tests/stdlib/compress_stream_checks.dodo"))
        .args(["-O", "0", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let result = Command::new(&output).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::remove_file(output).unwrap();
}
