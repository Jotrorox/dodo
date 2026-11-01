//! Differential test of the Lua 5.4 pattern engine (stdlib/std/script/lua/pattern.dodo).
//!
//! tests/lua_pattern/cases.txt holds hand-written edge cases plus random
//! patterns; expected.txt holds the results of PUC Lua 5.4.9 for string.find,
//! string.match, string.gmatch and string.gsub on each case. Regenerate both with
//! `python3 tests/lua_pattern/generate.py --lua /path/to/lua5.4` (add
//! `--random N --seed S --out DIR` for larger local runs; then set
//! DODO_LUA_PATTERN_CASES=DIR to check that directory with this test).
//!
//! The engine's functions are package-internal, so the driver
//! (tests/stdlib/lua_pattern_driver.dodo) is appended to pattern.dodo to form one
//! source file of package `lua`.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn combine(engine: &str, driver: &str) -> String {
    let mut imports: Vec<&str> = Vec::new();
    let mut body = String::new();
    for source in [engine, driver] {
        for line in source.lines() {
            if line.starts_with("package ") {
                continue;
            }
            if line.starts_with("import ") {
                if !imports.contains(&line) {
                    imports.push(line);
                }
                continue;
            }
            body.push_str(line);
            body.push('\n');
        }
    }
    format!("package lua\n{}\n{}", imports.join("\n"), body)
}

fn describe(case: &str) -> String {
    let field = |text: &str| -> String {
        let hex = text.strip_prefix('x').unwrap_or(text);
        let bytes: Vec<u8> = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        format!("{:?}", String::from_utf8_lossy(&bytes))
    };
    let parts: Vec<&str> = case.split(' ').collect();
    if parts.len() != 3 {
        return case.to_string();
    }
    format!("subject {} pattern {} init {}", field(parts[0]), field(parts[1]), parts[2])
}

#[test]
fn lua_patterns_match_reference_interpreter() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cases_dir = std::env::var_os("DODO_LUA_PATTERN_CASES")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("tests/lua_pattern"));
    let scratch = std::env::temp_dir().join(format!("dodo-lua-pattern-{}", std::process::id()));
    fs::create_dir_all(&scratch).unwrap();
    let engine = fs::read_to_string(root.join("stdlib/std/script/lua/pattern.dodo")).unwrap();
    let driver = fs::read_to_string(root.join("tests/stdlib/lua_pattern_driver.dodo")).unwrap();
    let source = scratch.join("main.dodo");
    fs::write(&source, combine(&engine, &driver)).unwrap();
    let executable = scratch.join(format!("lua_pattern{}", std::env::consts::EXE_SUFFIX));
    let compile = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(&source)
        .args(["-O", "0", "-o"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "compile Lua pattern driver: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let cases = fs::read_to_string(cases_dir.join("cases.txt")).unwrap();
    let expected = fs::read_to_string(cases_dir.join("expected.txt")).unwrap();
    let actual = run(&executable, &cases);
    let case_lines: Vec<&str> = cases.lines().collect();
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    assert_eq!(case_lines.len(), expected_lines.len(), "fixture line counts differ");
    let mut failures = Vec::new();
    for (i, case) in case_lines.iter().enumerate() {
        let got = actual_lines.get(i).copied().unwrap_or("<missing>");
        if got != expected_lines[i] {
            failures.push(format!(
                "case {} ({}):\n  expected {}\n  actual   {}",
                i + 1,
                describe(case),
                expected_lines[i],
                got
            ));
        }
    }
    assert!(
        failures.is_empty() && actual_lines.len() == case_lines.len(),
        "{} of {} Lua pattern cases differ from the reference:\n{}",
        failures.len(),
        case_lines.len(),
        failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
    fs::remove_dir_all(scratch).unwrap();
}

fn run(executable: &Path, input: &str) -> String {
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let data = input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&data).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "Lua pattern driver failed: {} {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
