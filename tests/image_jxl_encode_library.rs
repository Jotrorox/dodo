//! The pure-Dodo JPEG XL encoder (std/image/jxl/encode): lossless golden
//! codestreams, container and header structure, buffer errors, size bounds,
//! every pixel layout and embedded ICC profiles, and lossy round trips
//! (layouts, sizes, efforts, color encodings, bounds), through `dodo test
//! tests/image/jxl_encode`.
//! Decoding is checked against libjxl by tests/image/jxl_encode/verify.py
//! (lossless, bit-exact) and verify_lossy.py (needs djxl, so not run here).
use std::path::PathBuf;
use std::process::Command;

#[test]
fn jpeg_xl_encoder_tests_pass() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args(["test", "tests/image/jxl_encode", "-O", "0"])
        .env_remove("DODO_UPDATE_GOLDEN")
        .current_dir(&root)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "dodo test tests/image/jxl_encode: {}\n{}\n{}",
        output.status,
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("19 passed"), "{stdout}");
}
