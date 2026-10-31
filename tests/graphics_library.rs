//! CPU graphics: exact pixels for every primitive and pixel format,
//! antialiased paths, gradients and TrueType text, the image codecs and fake
//! display at O0 and O3, freestanding code generation, golden images rendered
//! through `dodo test`, and the display panel drivers against recorded and
//! simulated controllers.
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
        let path =
            std::env::temp_dir().join(format!("dodo-graphics-{name}-{}", std::process::id()));
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
fn drawing_formats_codecs_and_displays_execute_and_stay_freestanding() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("checks");
    let source = root.join("tests/stdlib/graphics_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "graphics-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            &Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args(["build", source.to_str().unwrap(), "-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            &format!("compile graphics fixture -O{optimization}"),
        );
        success(
            &Command::new(&executable).output().unwrap(),
            &format!("execute graphics fixture -O{optimization}"),
        );
    }
    // The whole family needs no OS, allocator or floating point, so it must
    // build for a browser and for a Cortex-M0+ without an FPU.
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        let object = scratch.0.join(format!("graphics-{target}.o"));
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
            &format!("emit graphics fixture for {target}"),
        );
        assert!(fs::metadata(object).unwrap().len() > 0);
    }
    // Font tables, bitmap and TrueType, are read-only data, which firmware
    // keeps in flash.
    let ir = scratch.0.join("graphics.ll");
    success(
        &Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args([
                "build",
                source.to_str().unwrap(),
                "--emit",
                "llvm-ir",
                "--target",
                "thumbv6m-none-eabi",
                "-O",
                "0",
                "-o",
            ])
            .arg(&ir)
            .output()
            .unwrap(),
        "emit graphics fixture IR",
    );
    let ir = fs::read_to_string(ir).unwrap();
    for table in ["SMALL_GLYPHS", "MEDIUM_GLYPHS", "TEST_FONT"] {
        assert!(
            ir.lines()
                .any(|line| line.contains(table) && line.contains("internal constant")),
            "{table} is not a constant global"
        );
    }
    assert!(!ir.contains(" float ") && !ir.contains(" double "));
}

#[test]
fn golden_images_match_at_every_optimization_level() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for optimization in ["0", "3"] {
        success(
            &Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args(["test", "tests/graphics", "-O", optimization])
                .env_remove("DODO_UPDATE_GOLDEN")
                .env_remove("DODO_GOLDEN_DIR")
                .current_dir(&root)
                .output()
                .unwrap(),
            &format!("golden images -O{optimization}"),
        );
    }
}

#[test]
fn display_drivers_execute_against_simulated_controllers_and_stay_freestanding() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("displays");
    let source = root.join("tests/stdlib/display_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "displays-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            &Command::new(env!("CARGO_BIN_EXE_dodo"))
                .args(["build", source.to_str().unwrap(), "-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            &format!("compile display fixture -O{optimization}"),
        );
        success(
            &Command::new(&executable).output().unwrap(),
            &format!("execute display fixture -O{optimization}"),
        );
    }
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        let object = scratch.0.join(format!("displays-{target}.o"));
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
            &format!("emit display fixture for {target}"),
        );
        assert!(fs::metadata(object).unwrap().len() > 0);
    }
}

#[test]
fn thermometer_example_tests_pass_against_the_fake_display() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args(["test", "examples/thermometer.dodo"])
        .current_dir(&root)
        .output()
        .unwrap();
    success(&output, "test examples/thermometer.dodo");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("3 passed"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn poster_example_draws_with_a_truetype_font_and_writes_a_png() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("poster");
    let font = root.join("tests/graphics/fonts/dodo-test.ttf");
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("run")
        .arg(root.join("examples/poster.dodo"))
        .arg("--")
        .arg(&font)
        .current_dir(&scratch.0)
        .output()
        .unwrap();
    success(&output, "run examples/poster.dodo");
    let png = fs::read(scratch.0.join("poster.png")).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    // IHDR: 360 x 240, 8-bit RGBA.
    assert_eq!(&png[16..26], &[0, 0, 1, 104, 0, 0, 0, 240, 8, 6]);
}
