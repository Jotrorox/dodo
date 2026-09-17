//! Native process ownership, bounded concurrent pipe draining and arguments.
use std::fs;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::process::Command;
#[cfg(target_os = "linux")]
use std::process::Output;
#[cfg(target_os = "linux")]
fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(target_os = "linux")]
#[test]
fn process_native_fixtures_at_o0_and_o3() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo process é {}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    success(
        Command::new("cc")
            .args([
                "-std=c11",
                "-D_POSIX_C_SOURCE=200809L",
                "-Wall",
                "-Wextra",
                "-Werror",
            ])
            .arg(root.join("tests/support/os_child.c"))
            .arg("-o")
            .arg(scratch.join("os child"))
            .output()
            .unwrap(),
        "compile controlled child",
    );
    fs::create_dir_all(scratch.join("child cwd é")).unwrap();
    fs::write(scratch.join("child cwd é/cwd-marker"), b"ok").unwrap();
    fs::copy(
        scratch.join("os child"),
        scratch.join("child cwd é/os child"),
    )
    .unwrap();
    for optimization in ["0", "3"] {
        let executable = scratch.join(format!("process-O{optimization}"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join("tests/os/process_checks.dodo"))
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile process fixture",
        );
        success(
            Command::new("timeout")
                .arg("25")
                .arg(&executable)
                .current_dir(&scratch)
                .env("DODO_PARENT_ONLY", "must not leak")
                .output()
                .unwrap(),
            "execute process fixture",
        );
    }
    fs::remove_dir_all(&scratch).unwrap();
}

#[test]
fn process_results_and_allocated_output_lifetimes_are_checked() {
    let scratch = std::env::temp_dir().join(format!("dodo-process-reject-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    for (name, body, expected) in [
        (
            "result",
            "fn misuse(child: &mut process.Child) { child.wait() }\nfn main() {}",
            "Result",
        ),
        (
            "output_escape",
            "fn escape(child: &mut process.Child) -> output_alloc.Output<arena.Arena, arena.Arena>!output_alloc.Error from(static) {\nfirst := [0u8; 64]\nsecond := [0u8; 64]\na := arena.Arena.new(&mut first)\nb := arena.Arena.new(&mut second)\nreturn output_alloc.collect_arena(child, &mut a, &mut b, 64, 64, 5)\n}\nfn main() {}",
            "borrow",
        ),
    ] {
        let source = scratch.join(format!("{name}.dodo"));
        fs::write(&source, format!("package rejected\nimport \"std/process\"\nimport \"std/process/alloc\" as output_alloc\nimport \"alloc/arena\"\n{body}\n")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source)
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
