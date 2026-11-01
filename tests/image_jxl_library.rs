//! The pure-Dodo JPEG XL decoder (std/image/jxl): lossless modular files at
//! every cjxl effort, every predictor and transform, lossy modular (XYB),
//! VarDCT at several distances, efforts and progressive modes (including LF
//! frames), JPEG recompression with chroma subsampling, noise, splines,
//! upsampling, patches, animation with blending, metadata boxes and the ICC
//! profile, checked against djxl references committed in tests/image/jxl,
//! plus robustness against truncated and corrupted input. Runs
//! `dodo test tests/image/jxl` unoptimized and optimized.
use std::path::PathBuf;
use std::process::Command;

#[test]
fn jxl_decoder_matches_djxl_references() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for optimization in ["0", "3"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["test", "tests/image/jxl", "-O", optimization])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "dodo test tests/image/jxl -O{optimization}: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("0 failed"), "{stdout}");
    }
}
