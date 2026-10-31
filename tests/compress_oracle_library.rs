//! Differential decoding against Python's independent zlib implementation.
use std::path::PathBuf;
use std::process::Command;

fn run_oracle(fixture: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = std::env::temp_dir().join(format!(
        "dodo-compress-oracle-{}-{}",
        std::process::id(),
        fixture
    ));
    let build = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(root.join("tests/stdlib").join(fixture))
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
    let result = Command::new("python3").arg("-c").arg(r#"
import random, subprocess, sys, zlib
rng = random.Random(60321)
rnd = bytes(rng.randrange(256) for _ in range(30000))
cases = [b'', b'a', b'a' * 100000, bytes(range(256)) * 400,
         rnd + b'x' * 1000 + rnd, bytes(rng.randrange(256) for _ in range(50000))]
for i in range(24):
    size = rng.randrange(0, 30000)
    alphabet = rng.randrange(1, 257)
    cases.append(bytes(rng.randrange(alphabet) for _ in range(size)))
for data in cases:
    for level, strategy in [(0, zlib.Z_DEFAULT_STRATEGY), (6, zlib.Z_FIXED), (6, zlib.Z_DEFAULT_STRATEGY)]:
        encoder = zlib.compressobj(level, zlib.DEFLATED, -15, 8, strategy)
        packed = encoder.compress(data) + encoder.flush()
        p = subprocess.run([sys.argv[1]], input=packed, capture_output=True)
        assert p.returncode == 0, (len(data), level, strategy, p.returncode, p.stderr)
        assert p.stdout == data, (len(data), level, strategy)
"#).arg(&path).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn raw_decoder_matches_python_streams() {
    run_oracle("compress_oracle.dodo");
}

#[test]
fn incremental_decoder_matches_python_streams() {
    run_oracle("inflate_oracle.dodo");
}
