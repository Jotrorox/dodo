//! std/image/ops: geometry, resizing, quantization, comparison, histograms
//! and compositing, through `dodo test tests/image/ops` at O0 and O3.
use std::path::PathBuf;
use std::process::Command;

#[test]
fn image_ops_tests_pass_at_every_optimization_level() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for optimization in ["0", "3"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["test", "tests/image/ops", "-O", optimization])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "dodo test tests/image/ops -O{optimization}: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(" 0 failed"),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
