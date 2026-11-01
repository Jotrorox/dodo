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

// A large record built into a Result while values with drops are still in
// scope: the drops run between loading the Result and storing it to the
// return slot, so the load cannot simply become a memmove.
const RESULT_WITH_DROPS: &str = r#"package main
struct Table {
    first: u64
    second: u32
    words: [100]u64
    small: [4]u32
    flag: u8
}
struct Inner {
    table: Table
    other: Table
    count: u32
    name: [256]u8
}
struct Other {
    x: u32
    y: [7]u16
    z: u64
}
enum Raw {
    First(Inner),
    Second(Other),
    Third(Table)
}
struct Guard {
    value: u32
    fn drop(&mut self) {
        self.value = 0
    }
}
struct Device {
    raw: Raw
    log: [64]u64
    name: [256]u8
    length: usize
    fn open(flag: bool) -> Device!u32 {
        if flag {
            return err(1)
        }
        table := Table { first: 1, second: 2, words: [3u64; 100], small: [4u32; 4], flag: 5 }
        other := Table { first: 1, second: 2, words: [3u64; 100], small: [4u32; 4], flag: 6 }
        return Device.from_raw(Raw.First(Inner { table: table, other: other, count: 3, name: [0u8; 256] }))
    }
    fn from_raw(raw: Raw) -> Device!u32 {
        device := Device { raw: raw, log: [0u64; 64], name: [0u8; 256], length: 0 }
        guard := Guard { value: 3 }
        name := [0u8; 256]
        for i in 0usize..16 {
            name[i] = 65
        }
        device.name = name
        device.length = 16 + guard.value as usize
        device.length += device.peek()
        return ok(device)
    }
    fn peek(&self) -> usize {
        match &self.raw {
            Raw.First(inner) => {
                return inner.table.words[5] as usize + inner.count as usize
            }
            Raw.Second(other) => {
                return other.y[3] as usize + other.z as usize
            }
            Raw.Third(table) => {
                return table.flag as usize
            }
        }
    }
    fn drop(&mut self) {
        self.length = 0
    }
}
fn main() -> i32 {
    device := Device.open(false)!
    if device.length != 16 + 3 + 3 + 3 {
        return 1
    }
    return 0
}
"#;

#[test]
fn large_results_built_before_drops_compile_quickly() {
    // Before such loads were copied aside this took over a minute at -O1.
    let dir = scratch("drops");
    let input = dir.join("main.dodo");
    fs::write(&input, RESULT_WITH_DROPS).unwrap();
    for level in ["0", "1", "3"] {
        let exe = dir.join(format!("main{level}{}", std::env::consts::EXE_SUFFIX));
        dodo(&["-O", level], &input, &exe);
        let status = Command::new(&exe).status().unwrap();
        assert_eq!(status.code(), Some(0), "O{level}");
    }
    fs::remove_dir_all(dir).unwrap();
}
