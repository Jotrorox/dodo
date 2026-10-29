use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn wrapped_stored_encoders_stream_under_backpressure() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = std::env::temp_dir().join(format!("dodo-wrapped-store-{}", std::process::id()));
    let build = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(root.join("tests/stdlib/wrapped_store_checks.dodo"))
        .arg("-O")
        .arg("0")
        .arg("-o")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let output = Command::new(&path).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut python = Command::new("python3")
        .args([
            "-c",
            r#"import sys,zlib
b=sys.stdin.buffer.read()
for mode in (15,31):
 n=int.from_bytes(b[:4], 'little'); body,b=b[4:4+n],b[4+n:]
 assert zlib.decompress(body,mode)==bytes(i & 255 for i in range(10000))
assert not b
"#,
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    python
        .stdin
        .take()
        .unwrap()
        .write_all(&output.stdout)
        .unwrap();
    let result = python.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::remove_file(path).unwrap();
}
