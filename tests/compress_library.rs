//! Independent zlib vectors and interoperability of all three framing formats.
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn compression_interoperates_with_python_zlib() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-compress-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let executable = scratch.join(format!("compression{}", std::env::consts::EXE_SUFFIX));
    let compile = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args([
            "build",
            root.join("tests/stdlib/compress_checks.dodo")
                .to_str()
                .unwrap(),
            "-O",
            "0",
            "-o",
        ])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "compile compression fixture: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let result = Command::new(&executable).output().unwrap();
    assert!(
        result.status.success(),
        "compression fixture failed: {} {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    let mut python = Command::new("python3")
        .args([
            "-c",
            r#"import sys, zlib
b = sys.stdin.buffer.read()
expected = bytes(range(256)) + bytes([0])
for mode in (-15, 15, 31):
    n = int.from_bytes(b[:4], 'little')
    assert len(b) >= 4 + n
    stream, b = b[4:4+n], b[4+n:]
    assert zlib.decompress(stream, mode) == expected
n = int.from_bytes(b[:4], 'little')
assert len(b) == 4 + n
assert zlib.decompress(b[4:], -15) == bytes(range(256)) * 256
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
        .write_all(&result.stdout)
        .unwrap();
    let status = python.wait_with_output().unwrap();
    assert!(
        status.status.success(),
        "Python rejected encoded stream: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn bounded_malformed_deflate_campaign() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-compress-fuzz-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let executable = scratch.join(format!("campaign{}", std::env::consts::EXE_SUFFIX));
    let compile = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args([
            "build",
            root.join("tests/stdlib/compress_fuzz.dodo")
                .to_str()
                .unwrap(),
            "-O",
            "0",
            "-o",
        ])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "compile malformed-input campaign: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let result = Command::new(&executable).output().unwrap();
    assert!(
        result.status.success(),
        "malformed-input campaign failed: {} {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(scratch).unwrap();
}
