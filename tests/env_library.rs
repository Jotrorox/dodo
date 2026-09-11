//! Copied native observations and explicit child-environment construction.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn native_environment_snapshots_at_o0_and_o3() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo env é {}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!(
            "env-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join("tests/os/env_checks.dodo"))
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile environment fixture",
        );
        success(
            Command::new(&executable)
                .current_dir(&scratch)
                .args(["space arg", "é", ""])
                .env("DODO_ENV_TEST", "parent  é")
                .output()
                .unwrap(),
            "execute environment fixture",
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
#[test]
fn environment_snapshot_borrows_and_results_are_checked() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-env-reject-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for (name, body, expected) in [
        (
            "escape",
            "fn escape() -> native.NativeString!error.Error from(static) { bytes := [0u8; 1024]\nwide := [0u16; 1024]\nreturn env.current_dir(&mut bytes, &mut wide) }\nfn main() {}",
            "borrow",
        ),
        (
            "result",
            "fn main() { bytes := [0u8; 1024]\nwide := [0u16; 1024]\nenv.snapshot(&mut bytes, &mut wide) }",
            "Result",
        ),
        (
            "builder_alias",
            "fn main() { bytes := [0u8; 1024]\nwide := [0u16; 1024]\nbuilder := env.child_environment(&mut bytes, &mut wide)\nsnapshot := match builder.snapshot() {ok(value)=>{value},err(_)=>{return}}\nbuilder.clear()\ncore.drop(snapshot) }",
            "borrow",
        ),
    ] {
        let source = scratch.join(format!("{name}.dodo"));
        fs::write(&source, format!("package reject\nimport \"std/env\"\nimport \"std/platform/native\"\nimport \"std/platform/error\"\n{body}\n")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {name}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .to_lowercase()
                .contains(&expected.to_lowercase()),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn unix_environment_and_arguments_preserve_non_unicode_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-env-native-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!("native-O{optimization}"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join("tests/os/env_native_checks.dodo"))
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile native byte fixture",
        );
        let native = std::ffi::OsString::from_vec(vec![255]);
        success(
            Command::new(&executable)
                .arg(&native)
                .env("DODO_NATIVE", &native)
                .output()
                .unwrap(),
            "execute native byte fixture",
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
