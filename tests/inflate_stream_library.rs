use std::path::PathBuf;
use std::process::Command;

#[test]
fn incremental_inflate_with_tiny_buffers() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/stdlib/inflate_stream_checks.dodo");
    let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("run")
        .arg(fixture)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}: {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
}
