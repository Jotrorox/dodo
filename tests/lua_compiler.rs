//! The Lua compiler in `std/script/lua` must produce the same bytecode,
//! debug information and syntax errors as the reference `luac` 5.4.
//!
//! The test tool in `tests/lua/tool` is assembled with the package sources
//! (as `tests/lua/build_tool.sh` does) and run over `tests/lua/snippets`
//! (listings in the `luac -l -l` format) and `tests/lua/errors` (syntax
//! error messages). The expected outputs were produced by PUC Lua 5.4.9's
//! luac with `tests/lua/update_expected.sh`.
#![cfg(feature = "llvm")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMPILER: &str = env!("CARGO_BIN_EXE_dodo");

struct Workspace(PathBuf);

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn build_tool() -> (Workspace, PathBuf) {
    let dir = std::env::temp_dir().join(format!("dodo-lua-compiler-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let workspace = Workspace(dir.clone());
    let package = dir.join("lua");
    fs::create_dir_all(&package).unwrap();
    for entry in fs::read_dir(root().join("stdlib/std/script/lua")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "dodo") {
            fs::copy(&path, package.join(path.file_name().unwrap())).unwrap();
        }
    }
    fs::copy(root().join("tests/lua/tool/lua_tool.dodo"), package.join("lua_tool.dodo")).unwrap();
    fs::copy(root().join("tests/lua/tool/main.dodo"), dir.join("main.dodo")).unwrap();
    let tool = dir.join("luatool");
    let output = Command::new(COMPILER)
        .arg("build")
        .arg(dir.join("main.dodo"))
        .arg("-o")
        .arg(&tool)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "building the Lua test tool failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (workspace, tool)
}

fn lua_sources(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".lua"))
        .collect();
    names.sort();
    names
}

// Replace every 0x... address by ADDR, like the expected files.
fn normalize_addresses(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'0' && i + 1 < bytes.len() && bytes[i + 1] == b'x' {
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            out.push_str("ADDR");
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

#[test]
fn lua_compiler_matches_reference_luac() {
    let (_workspace, tool) = build_tool();
    let mut failures = Vec::new();

    let snippets = root().join("tests/lua/snippets");
    for name in lua_sources(&snippets) {
        let output = Command::new(&tool).current_dir(&snippets).arg("-l").arg(&name).output().unwrap();
        let expected = fs::read_to_string(snippets.join(name.replace(".lua", ".expected"))).unwrap();
        let actual = normalize_addresses(&String::from_utf8_lossy(&output.stdout));
        if !output.status.success() || actual != expected {
            failures.push(format!(
                "{name}: listing differs\n--- expected\n{expected}\n--- actual\n{actual}\n{}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    let errors = root().join("tests/lua/errors");
    for name in lua_sources(&errors) {
        let output = Command::new(&tool).current_dir(&errors).arg(&name).output().unwrap();
        let expected = fs::read_to_string(errors.join(name.replace(".lua", ".expected"))).unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let actual = stderr.strip_prefix("luatool: ").unwrap_or(&stderr);
        let as_expected = if expected.is_empty() {
            output.status.success() && actual.is_empty()
        } else {
            !output.status.success() && actual == expected
        };
        if !as_expected {
            failures.push(format!("{name}: expected {expected:?}, got {actual:?}"));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
