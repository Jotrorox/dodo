//! std/gpu/vulkan and std/dylib: the binding checks at both optimization
//! levels, code generation for every hosted target, and the compute example
//! on whatever Vulkan driver the machine has (lavapipe in a Linux container
//! is enough). Without a driver the example must say so and exit cleanly.
//! The window example presents under Xvfb in window_library.rs.
use std::fs;
use std::path::{Path, PathBuf};
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

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("dodo-vulkan-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn build(source: &Path, executable: &Path, optimization: &str) {
    success(
        &Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["build", source.to_str().unwrap(), "-O", optimization, "-o"])
            .arg(executable)
            .output()
            .unwrap(),
        &format!("compile {} -O{optimization}", source.display()),
    );
}

#[test]
fn binding_and_dylib_checks_execute() {
    let scratch = Scratch::new("checks");
    let source = root().join("tests/stdlib/vulkan_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "checks-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        build(&source, &executable, optimization);
        success(
            &Command::new(&executable).output().unwrap(),
            &format!("execute vulkan checks -O{optimization}"),
        );
    }
}

#[test]
fn compute_example_runs_or_reports_a_missing_driver() {
    let scratch = Scratch::new("compute");
    let executable = scratch
        .0
        .join(format!("compute{}", std::env::consts::EXE_SUFFIX));
    build(
        &root().join("examples/vulkan_compute.dodo"),
        &executable,
        "0",
    );
    let output = Command::new(&executable).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    match output.status.code() {
        Some(0) => assert!(
            stdout.contains("1000 of 1000 values correct") && stdout.contains("compute ok"),
            "{stdout}"
        ),
        // No Vulkan loader on this machine.
        Some(2) => assert_eq!(stdout, "NotFound\n"),
        // A loader without any driver.
        Some(1) => assert!(
            stdout == "ERROR_INCOMPATIBLE_DRIVER\n" || stdout == "ERROR_INITIALIZATION_FAILED\n",
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        ),
        _ => panic!(
            "compute example: {}\n{stdout}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

#[test]
fn vulkan_builds_for_every_hosted_target() {
    let scratch = Scratch::new("targets");
    for (name, source) in [
        ("checks", root().join("tests/stdlib/vulkan_checks.dodo")),
        ("compute", root().join("examples/vulkan_compute.dodo")),
        ("window", root().join("examples/vulkan_window.dodo")),
    ] {
        for target in [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "x86_64-pc-windows-gnu",
        ] {
            let object = scratch.0.join(format!("{name}-{target}.o"));
            success(
                &Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .args([
                        "build",
                        source.to_str().unwrap(),
                        "--emit",
                        "obj",
                        "--target",
                        target,
                        "-o",
                    ])
                    .arg(&object)
                    .output()
                    .unwrap(),
                &format!("emit {name} for {target}"),
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
}

#[test]
fn dylib_adapters_follow_the_target() {
    let scratch = Scratch::new("adapters");
    for (import, target, accepted) in [
        ("std/dylib/native", "x86_64-unknown-linux-gnu", true),
        ("std/dylib/posix", "aarch64-apple-darwin", true),
        ("std/dylib/windows", "x86_64-pc-windows-msvc", true),
        ("std/dylib/windows", "x86_64-unknown-linux-gnu", false),
        ("std/dylib/posix", "x86_64-pc-windows-gnu", false),
    ] {
        let source = scratch.0.join("adapter.dodo");
        fs::write(
            &source,
            format!(
                "package main\nimport \"{import}\" as native\nfn main() {{\n    unsafe {{\n        native.open(0usize as *const u8)\n    }}\n}}\n"
            ),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["check", source.to_str().unwrap(), "--target", target])
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            accepted,
            "{import} for {target}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if !accepted {
            assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used for target"));
        }
    }
}
