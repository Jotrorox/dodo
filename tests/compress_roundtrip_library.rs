//! Encoder round trips at every format and several levels, checked against
//! Python's independent zlib implementation.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const CASES: &str = r#"
import random, struct, sys
rng = random.Random(7031)
text = open(sys.argv[1], 'rb').read()[:200000]
cases = [b'', b'a', b'ab' * 3, bytes(range(256)), b'a' * 70000,
         bytes(rng.randrange(256) for _ in range(70000)), text,
         bytes(rng.randrange(4) for _ in range(40000)),
         (b'hello streaming! ' * 4000)[:66000]]
for data in cases:
    sys.stdout.buffer.write(struct.pack('<I', len(data)) + data)
"#;

const VERIFY: &str = r#"
import subprocess, struct, sys, zlib
cases = open(sys.argv[1], 'rb').read()
out = sys.stdin.buffer.read()
def take(b):
    n = struct.unpack('<I', b[:4])[0]
    return b[4:4 + n], b[4 + n:]
levels = [0, 1, 3, 4, 6, 9]
while cases:
    data, cases = take(cases)
    for wbits in (-15, 15, 31):
        for level in levels:
            for kind in ('one-shot', 'streaming'):
                stream, out = take(out)
                d = zlib.decompressobj(wbits)
                assert d.decompress(stream) == data and d.eof and not d.unused_data, (len(data), wbits, level, kind)
                if level == 0:
                    assert len(stream) <= len(data) + 5 * (len(data) // 16384 + 2) + 18
                if level == 9 and kind == 'one-shot' and len(data) > 1000:
                    reference = len(zlib.compress(data, 9))
                    assert len(stream) - (wbits == 31) * 12 <= reference * 1.02 + 16, (len(stream), reference)
assert not out
"#;

#[test]
fn encoders_round_trip_and_interoperate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch =
        std::env::temp_dir().join(format!("dodo-compress-roundtrip-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let executable = scratch.join(format!("roundtrip{}", std::env::consts::EXE_SUFFIX));
    let build = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(root.join("tests/stdlib/compress_roundtrip.dodo"))
        .args(["-O", "0", "-o"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let corpus = root.join("docs/src/content/docs/language-spec-0.1.md");
    let cases = Command::new("python3")
        .arg("-c")
        .arg(CASES)
        .arg(&corpus)
        .output()
        .unwrap();
    assert!(
        cases.status.success(),
        "{}",
        String::from_utf8_lossy(&cases.stderr)
    );
    let cases_path = scratch.join("cases");
    std::fs::write(&cases_path, &cases.stdout).unwrap();
    let mut fixture = Command::new(&executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    fixture
        .stdin
        .take()
        .unwrap()
        .write_all(&cases.stdout)
        .unwrap();
    let encoded = fixture.wait_with_output().unwrap();
    assert!(
        encoded.status.success(),
        "fixture failed: {} {}",
        encoded.status,
        String::from_utf8_lossy(&encoded.stderr)
    );
    let mut python = Command::new("python3")
        .arg("-c")
        .arg(VERIFY)
        .arg(&cases_path)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    python
        .stdin
        .take()
        .unwrap()
        .write_all(&encoded.stdout)
        .unwrap();
    let result = python.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::remove_dir_all(scratch).unwrap();
}
