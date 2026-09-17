//! UTF-8 hosted conveniences use controlled files, environment and local children.
use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo hosted Ω {}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output, context: &str) {
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
#[cfg(target_os = "linux")]
fn build(source: &Path, executable: &Path, optimization: &str) {
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
            .args(["-O", optimization, "-o"])
            .arg(executable)
            .output()
            .unwrap(),
        "compile hosted fixture",
    );
}
#[cfg(target_os = "linux")]
#[test]
fn hosted_native_conveniences_at_o0_and_o3() {
    let scratch = Scratch::new();
    success(
        Command::new("cc")
            .args([
                "-std=c11",
                "-D_POSIX_C_SOURCE=200809L",
                "-Wall",
                "-Wextra",
                "-Werror",
            ])
            .arg(root().join("tests/support/os_child.c"))
            .arg("-o")
            .arg(scratch.0.join("os child.exe"))
            .output()
            .unwrap(),
        "compile controlled child",
    );
    fs::create_dir(scratch.0.join("child cwd é")).unwrap();
    fs::copy(
        scratch.0.join("os child.exe"),
        scratch.0.join("child cwd é/os child.exe"),
    )
    .unwrap();
    for fixture in [
        "hosted_fs",
        "hosted_env",
        "hosted_process",
        "hosted_time",
        "hosted_text",
    ] {
        for optimization in ["0", "3"] {
            let executable = scratch.0.join(format!("{fixture}-{optimization}"));
            build(
                &root().join(format!("tests/os/{fixture}.dodo")),
                &executable,
                optimization,
            );
            let mut run = Command::new("timeout");
            run.arg("30")
                .arg(&executable)
                .current_dir(&scratch.0)
                .env("DODO_PARENT_ONLY", "must not leak")
                .env_remove("DODO_HOSTED_ABSENT")
                .env("DODO_HOSTED_EMPTY", "")
                .env("DODO_HOSTED_VALUE", "hé!!");
            if fixture == "hosted_env" {
                run.args(["space arg", "é", ""]);
            }
            success(
                run.output().unwrap(),
                &format!("run {fixture} -O{optimization}"),
            );
            if fixture == "hosted_process" {
                let pid: i32 = fs::read_to_string(scratch.0.join("child-pid"))
                    .unwrap()
                    .parse()
                    .unwrap();
                // The timeout path must have killed/reaped the child, not detached it.
                assert_eq!(
                    unsafe { libc::kill(pid, 0) },
                    -1,
                    "timed-out child {pid} is still alive"
                );
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::ESRCH)
                );
                fs::remove_file(scratch.0.join("child-pid")).unwrap();
            }
        }
    }
}
#[test]
fn hosted_windows_objects_at_o0_and_o3() {
    let scratch = Scratch::new();
    for fixture in [
        "hosted_fs",
        "hosted_env",
        "hosted_process",
        "hosted_time",
        "hosted_text",
    ] {
        for optimization in ["0", "3"] {
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(root().join(format!("tests/os/{fixture}.dodo")))
                    .args([
                        "--target",
                        "x86_64-pc-windows-msvc",
                        "--emit",
                        "obj",
                        "-O",
                        optimization,
                        "-o",
                    ])
                    .arg(scratch.0.join(format!("{fixture}-{optimization}.obj")))
                    .output()
                    .unwrap(),
                "compile Windows hosted fixture",
            );
        }
    }
}
#[test]
fn hosted_borrows_and_private_storage_are_checked() {
    let scratch = Scratch::new();
    for (name, body, diagnostic) in [
        (
            "escape",
            "fn escape() -> &str!error.Error from(static) { output := [0u8; 32]\nworkspace := env.Workspace.new()\nmatch env.get(\"X\", &mut output, &mut workspace, platform.TextPolicy.Strict)? { some(value) => { return ok(value) }, none => { return ok(\"\") } } }",
            "borrow",
        ),
        (
            "mutate",
            "fn main() { output := [0u8; 32]\nworkspace := env.Workspace.new()\nmatch env.get(\"X\", &mut output, &mut workspace, platform.TextPolicy.Strict) { ok(value) => { output[0] = 1\ncore.drop(value) }, err(_) => {} } }",
            "borrow",
        ),
        (
            "private",
            "fn main() { workspace := native.Workspace { data: [0u64; 512], used: 1 } }",
            "private",
        ),
        (
            "unhandled",
            "fn main() { storage := process.CommandStorage.new()\nprocess.Command.new(\"x\", &mut storage) }",
            "Result",
        ),
    ] {
        let source = scratch.0.join(format!("{name}.dodo"));
        fs::write(&source, format!("package rejected\nimport \"std/env\"\nimport \"std/platform\"\nimport \"std/platform/native\"\nimport \"std/platform/error\"\nimport \"std/process\"\n{body}\n")).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        assert!(!result.status.success(), "accepted {name}");
        let message = String::from_utf8_lossy(&result.stderr);
        assert!(message.contains(diagnostic), "{name}: {message}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn invalid_native_text_and_clock_failures() {
    use std::os::unix::ffi::OsStringExt;
    let scratch = Scratch::new();
    for optimization in ["0", "3"] {
        for fixture in ["invalid_linux", "invalid_environment"] {
            let executable = scratch.0.join(format!("{fixture}-{optimization}"));
            build(
                &root().join(format!("tests/hosted/{fixture}.dodo")),
                &executable,
                optimization,
            );
            let invalid = std::ffi::OsString::from_vec(vec![255]);
            success(
                Command::new(&executable)
                    .arg(&invalid)
                    .env("DODO_NATIVE", &invalid)
                    .output()
                    .unwrap(),
                "invalid native text policy",
            );
        }
        let object = scratch.0.join(format!("clock-{optimization}.o"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root().join("tests/hosted/clock_failure.dodo"))
                .args(["--emit", "obj", "-O", optimization, "-o"])
                .arg(&object)
                .output()
                .unwrap(),
            "compile injected clock failure",
        );
        let executable = scratch.0.join(format!("clock-{optimization}"));
        success(
            Command::new("cc")
                .arg(&object)
                .arg(root().join("tests/hosted/clock_failure.c"))
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap(),
            "link injected clock failure",
        );
        success(
            Command::new(&executable).output().unwrap(),
            "clock contract and native errors",
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn arguments_and_environment_capacity_failures() {
    let scratch = Scratch::new();
    let source = scratch.0.join("limits.dodo");
    fs::write(&source, r#"package limits
import "std/env"
import "std/platform"
import "std/platform/error"
fn main() {
    storage := platform.list_workspace()
    match env.Arguments.capture(&mut storage, platform.TextPolicy.Strict) {
        ok(_) => { assert(false) }, err(reason) => { assert(reason.kind == error.Kind.BufferTooSmall) }
    }
    output := [0u8; 8192]
    workspace := env.Workspace.new()
    match env.get("DODO_LARGE", &mut output, &mut workspace, platform.TextPolicy.Strict) {
        ok(_) => { assert(false) }, err(reason) => { assert(reason.kind == error.Kind.BufferTooSmall) }
    }
}
"#).unwrap();
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!("limits-{optimization}"));
        build(&source, &executable, optimization);
        success(
            Command::new(&executable)
                .arg("a".repeat(40000))
                .env("DODO_LARGE", "b".repeat(4096))
                .output()
                .unwrap(),
            "bounded native observations",
        );
    }
}
