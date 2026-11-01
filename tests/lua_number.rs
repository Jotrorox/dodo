//! Lua number conversions (std/script/lua/number.dodo) against recorded
//! vectors: C printf formatting from macOS libc and Lua 5.4.9 `tonumber`.
//!
//! The conversion functions are package-internal, so the test copies
//! number.dodo next to a driver file of the same package into a scratch
//! project, builds it, and feeds it tests/fixtures/lua_number_vectors.txt.
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn lua_number_conversions_match_recorded_vectors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = std::env::temp_dir().join(format!("dodo-lua-number-{}", std::process::id()));
    let package = scratch.join("lua");
    fs::create_dir_all(&package).unwrap();
    fs::copy(
        root.join("stdlib/std/script/lua/number.dodo"),
        package.join("number.dodo"),
    )
    .unwrap();
    fs::copy(
        root.join("tests/stdlib/lua_number/harness.dodo"),
        package.join("harness.dodo"),
    )
    .unwrap();
    fs::copy(
        root.join("tests/stdlib/lua_number/main.dodo"),
        scratch.join("main.dodo"),
    )
    .unwrap();

    let vectors = fs::read_to_string(root.join("tests/fixtures/lua_number_vectors.txt")).unwrap();
    let mut commands = String::new();
    let mut expected = Vec::new();
    for line in vectors.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (command, result) = line.split_once('\t').expect("COMMAND<TAB>EXPECTED");
        commands.push_str(command);
        commands.push('\n');
        expected.push((command, result));
    }
    assert!(expected.len() > 5000, "vector file looks truncated");

    for optimization in ["0", "2"] {
        let executable = scratch.join(format!(
            "lua-number-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let build = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["build", "main.dodo", "-O", optimization, "-o"])
            .arg(&executable)
            .current_dir(&scratch)
            .output()
            .unwrap();
        success(&build, "compile lua number driver");

        let mut child = Command::new(&executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = commands.clone();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        success(&output, "run lua number driver");

        let text = String::from_utf8(output.stdout).expect("driver output is ASCII");
        let actual: Vec<&str> = text.lines().collect();
        assert_eq!(actual.len(), expected.len(), "one result line per command");
        let mut failures = Vec::new();
        for ((command, want), got) in expected.iter().zip(actual.iter()) {
            if want != got {
                failures.push(format!(
                    "{command}\n  expected [{want}]\n  actual   [{got}]"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "-O{optimization}: {} of {} vectors differ:\n{}",
            failures.len(),
            expected.len(),
            failures
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
