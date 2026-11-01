//! std/image: the PPM, QOI, BMP and PNG codec packages and the unified layer
//! through `dodo test tests/image/codecs` at O0 and O3, plus a freestanding
//! program that must run on the host and build for a browser and a
//! Cortex-M0+ without an OS, allocator or files.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("dodo-image-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn codec_tests_pass_at_every_optimization_level() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for optimization in ["0", "3"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["test", "tests/image/codecs", "-O", optimization])
            .current_dir(&root)
            .output()
            .unwrap();
        success(
            &output,
            &format!("dodo test tests/image/codecs -O{optimization}"),
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(" 0 failed"),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn image_library_runs_and_builds_freestanding() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("freestanding");
    let source = root.join("tests/image/freestanding.dodo");
    let executable = scratch
        .0
        .join(format!("freestanding{}", std::env::consts::EXE_SUFFIX));
    success(
        &Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["build", source.to_str().unwrap(), "-O", "0", "-o"])
            .arg(&executable)
            .output()
            .unwrap(),
        "compile the freestanding image program",
    );
    success(
        &Command::new(&executable).output().unwrap(),
        "run the freestanding image program",
    );
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        let object = scratch.0.join(format!("image-{target}.o"));
        success(
            &Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args([
                    "build",
                    source.to_str().unwrap(),
                    "--emit",
                    "obj",
                    "--target",
                    target,
                    "-O",
                    "3",
                    "-o",
                ])
                .arg(&object)
                .output()
                .unwrap(),
            &format!("emit the image program for {target}"),
        );
        assert!(fs::metadata(object).unwrap().len() > 0);
    }
}
