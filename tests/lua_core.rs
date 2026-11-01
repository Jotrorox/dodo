//! std/script/lua runtime: the embedding API, and programs precompiled with
//! the reference `luac` 5.4.9 run through the standalone interpreter
//! (examples/lua), compared with the output of the reference interpreter.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-lua-core-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures() -> PathBuf {
    root().join("tests/fixtures/lua_core")
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn build(source: &Path, exe: &Path) {
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
            .arg("-o")
            .arg(exe)
            .output()
            .unwrap(),
    );
}

// Replace addresses and the interpreter path so outputs are comparable.
fn normalize(text: &str, program: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let line = match line.strip_prefix(program) {
            Some(rest) if rest.starts_with(": ") => format!("lua{rest}"),
            _ => line.to_string(),
        };
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'0' && i + 1 < bytes.len() && bytes[i + 1] == b'x' {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_hexdigit() {
                    j += 1;
                }
                if j > i + 2 {
                    out.push_str("ADDR");
                    i = j;
                    continue;
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out.push('\n');
    }
    out
}

#[test]
fn embedding_api() {
    let ws = Workspace::new();
    let exe = ws.0.join(format!("api{}", std::env::consts::EXE_SUFFIX));
    build(&fixtures().join("api.dodo"), &exe);
    let output = success(Command::new(&exe).output().unwrap());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "api ok\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn interpreter_runs_precompiled_programs_and_options() {
    let ws = Workspace::new();
    let exe = ws.0.join(format!("lua{}", std::env::consts::EXE_SUFFIX));
    build(&root().join("examples/lua"), &exe);
    let program = exe.to_str().unwrap().to_string();
    for name in ["arith", "tables", "closures", "errors", "coro", "gc", "tbc"] {
        let output = Command::new(&exe)
            .arg(format!("{name}.luac"))
            .current_dir(fixtures())
            .output()
            .unwrap();
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        let expected = fs::read_to_string(fixtures().join(format!("{name}.expected"))).unwrap();
        assert_eq!(normalize(&text, &program), expected, "fixture {name}");
    }
    let version = success(Command::new(&exe).arg("-v").output().unwrap());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout),
        "Lua 5.4.9  Copyright (C) 1994-2026 Lua.org, PUC-Rio\n"
    );
    let bad = Command::new(&exe).arg("-x").output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let stderr = normalize(&String::from_utf8_lossy(&bad.stderr), &program);
    assert!(
        stderr.starts_with("lua: unrecognized option '-x'\nusage: "),
        "{stderr}"
    );
    let missing = Command::new(&exe).arg("no-such-file.lua").output().unwrap();
    assert_eq!(missing.status.code(), Some(1));
    let stderr = normalize(&String::from_utf8_lossy(&missing.stderr), &program);
    assert!(
        stderr.starts_with("lua: cannot open no-such-file.lua"),
        "{stderr}"
    );
    // Script arguments and the `arg` table.
    let args = success(
        Command::new(&exe)
            .args(["args.luac", "one", "two"])
            .current_dir(fixtures())
            .output()
            .unwrap(),
    );
    assert_eq!(
        String::from_utf8_lossy(&args.stdout),
        "one\ttwo\nargs.luac\t2\n"
    );
}
