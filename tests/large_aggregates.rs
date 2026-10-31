//! Large records moved by value must compile as memory copies. Whole-value
//! loads and stores make LLVM split them into one operation per element,
//! which once cost minutes, gigabytes of compiler memory, and megabytes of
//! stack. Builds here are killed after a time limit rather than left to grow.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(60);

fn source(bytes: usize) -> String {
    format!(
        r#"package main
pub struct Big {{
    data: [{bytes}]u8
    words: [{words}]u32
    count: usize
    pub fn new(count: usize) -> Big {{
        return Big {{ data: [7u8; {bytes}], words: [0u32; {words}], count: count }}
    }}
    pub fn make(count: usize) -> Big!i32 {{
        if count == 99 {{
            return err(1)
        }}
        return ok(Big {{ data: [0u8; {bytes}], words: [3u32; {words}], count: count }})
    }}
}}
fn take(big: Big) -> usize {{
    return big.count + (big.data[1] as usize) + (big.words[2] as usize)
}}
pub fn pass(big: Big) -> Big {{
    return big
}}
fn main() -> i32 {{
    a := Big.new(1)
    b := Big.make(2)!
    moved := pass(a)
    total := take(moved) + take(b)
    if total != 1 + 7 + 0 + 2 + 0 + 3 {{
        return 1
    }}
    return 0
}}
"#,
        words = bytes / 4
    )
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dodo-large-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

// Runs the compiler, killing it at the time limit.
fn dodo(args: &[&str], input: &Path, output: &Path) {
    let start = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(input)
        .args(args)
        .arg("-o")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut stderr = String::new();
            std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut stderr).unwrap();
            assert!(status.success(), "{args:?}: {stderr}");
            return;
        }
        if start.elapsed() > LIMIT {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{args:?}: compiling a large record exceeded {LIMIT:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn large_records_lower_to_memory_copies() {
    let dir = scratch("ir");
    let input = dir.join("main.dodo");
    fs::write(&input, source(1 << 20)).unwrap();
    let ir = dir.join("main.ll");
    dodo(&["--emit", "llvm-ir"], &input, &ir);
    let text = fs::read_to_string(&ir).unwrap();
    for line in text.lines() {
        let line = line.trim();
        let whole = [
            "load %Big",
            "store %Big",
            "ret %Big",
            "load { i1",
            "store { i1",
        ];
        assert!(
            !whole.iter().any(|op| line.contains(op))
                && !line.contains("x i8] zeroinitializer, ptr"),
            "whole-value operation on a large record: {line}"
        );
    }
    for level in ["0", "3"] {
        dodo(
            &["--emit", "obj", "-O", level],
            &input,
            &dir.join(format!("main{level}.o")),
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn large_records_run_on_small_stacks() {
    // Small enough for a 1 MiB main-thread stack with several copies live.
    let dir = scratch("run");
    let input = dir.join("main.dodo");
    fs::write(&input, source(64 << 10)).unwrap();
    for level in ["0", "3"] {
        let exe = dir.join(format!("main{level}{}", std::env::consts::EXE_SUFFIX));
        dodo(&["-O", level], &input, &exe);
        let status = Command::new(&exe).status().unwrap();
        assert_eq!(status.code(), Some(0), "O{level}");
    }
    fs::remove_dir_all(dir).unwrap();
}
