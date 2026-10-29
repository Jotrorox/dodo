//! Dynamic-Huffman streams are checked by Python's independent zlib decoder.
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[test]
fn dynamic_huffman_encoding_interoperates() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let executable = std::env::temp_dir().join(format!("dodo-dynamic-{}", std::process::id()));
    let compile = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(root.join("tests/stdlib/compress_dynamic_checks.dodo"))
        .args(["-O", "0", "-o"])
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
        "{} {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    let mut python = Command::new("python3")
        .args([
            "-c",
            r#"
import sys, zlib
b = sys.stdin.buffer.read()
expected = [b'', (b'abcde' * 14000), bytes((i * 73 + (i // 251) * 13) & 255 for i in range(70000))]
for value in expected:
    size = int.from_bytes(b[:4], 'little')
    stream, b = b[4:4+size], b[4+size:]
    assert zlib.decompress(stream, -15) == value
    if len(value) and value[0] == 97: assert size < 1000
for mode in (15, 31):
    size = int.from_bytes(b[:4], 'little')
    stream, b = b[4:4+size], b[4+size:]
    assert zlib.decompress(stream, mode) == expected[-1]
state = 0x76543210
random_data = bytearray()
for i in range(1024):
    state ^= (state & 0x7ffff) << 13
    state ^= state >> 17
    state ^= (state & 0x7ffffff) << 5
    random_data.append(state & 255)
for level in range(20):
    for mode in (-15, 15, 31):
        size = int.from_bytes(b[:4], 'little')
        stream, b = b[4:4+size], b[4+size:]
        assert zlib.decompress(stream, mode) == random_data
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
        .write_all(&result.stdout)
        .unwrap();
    let result = python.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_file(executable).unwrap();
}
