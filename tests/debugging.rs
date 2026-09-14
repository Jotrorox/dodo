//! Real artifacts, runtime failures, and batch debugger sessions.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-debug-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
    fn build(&self, source: &Path, output: &Path, args: &[&str]) {
        success(
            &Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(source)
                .args(args)
                .arg("-o")
                .arg(output)
                .output()
                .unwrap(),
        );
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn tool(name: &str) -> bool {
    let present = Command::new(name)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    assert!(
        present || std::env::var_os("DODO_REQUIRE_DEBUGGER_TESTS").is_none(),
        "required debugger tool {name} missing"
    );
    if !present {
        eprintln!(
            "skipping optional {name} smoke test; set DODO_REQUIRE_DEBUGGER_TESTS=1 to require it"
        );
    }
    present
}

#[test]
fn debug_is_opt_in_and_survives_all_artifact_formats() {
    let w = Workspace::new();
    let source = w.file(
        "source with spaces.dodo",
        "package debug\npub fn add(a:i32,b:i32)->i32 { result:=a+b\nreturn result }\n",
    );
    let ir = w.0.join("plain.ll");
    w.build(&source, &ir, &["--emit", "llvm-ir"]);
    assert!(!fs::read_to_string(&ir).unwrap().contains("!DICompileUnit"));
    for level in ["0", "3"] {
        for (format, extension) in [
            ("llvm-ir", "ll"),
            ("bitcode", "bc"),
            ("obj", "o"),
            ("asm", "s"),
        ] {
            let output = w.0.join(format!("debug-{level}.{extension}"));
            w.build(&source, &output, &["-g", "-O", level, "--emit", format]);
            let bytes = fs::read(&output).unwrap();
            assert!(!bytes.is_empty());
            if format == "llvm-ir" {
                let ir = String::from_utf8(bytes).unwrap();
                for expected in [
                    "!DICompileUnit",
                    "!DISubprogram",
                    "!DILocation",
                    "!DILocalVariable",
                    "!DIBasicType",
                    "name: \"result\"",
                    "arg: 1",
                    "source with spaces.dodo",
                ] {
                    assert!(ir.contains(expected), "missing {expected}: {ir}");
                }
                assert_eq!(ir.contains("isOptimized: true"), level == "3");
            }
        }
    }
    if tool("llvm-dwarfdump-22") {
        for level in ["0", "3"] {
            success(
                &Command::new("llvm-dwarfdump-22")
                    .arg("--verify")
                    .arg(w.0.join(format!("debug-{level}.o")))
                    .output()
                    .unwrap(),
            );
        }
    }
}

#[test]
fn recursive_and_aggregate_debug_types_use_the_target_layout() {
    let w = Workspace::new();
    let source = w.file("types.dodo", "package types\npub struct Node { flag:bool, next:*const Node, value:i64 }\npub enum Choice { Empty, Value(i32) }\npub fn inspect(node:&Node, slice:&[i32], array:[3]u16, text:&str, option:Option<i32>, result:Result<i32,u8>, choice:Choice, storage:MaybeUninit<i64>, number:f64) {\ncore.drop(node)\ncore.drop(slice)\ncore.drop(array)\ncore.drop(text)\ncore.drop(option)\nmatch result { ok(value) => { core.drop(value) }, err(error) => { core.drop(error) } }\ncore.drop(choice)\ncore.drop(storage)\ncore.drop(number)\n}\n");
    let dwarf = tool("llvm-dwarfdump-22");
    for target in [
        "x86_64-unknown-linux-gnu",
        "thumbv7em-none-eabi",
        "wasm32-unknown-unknown",
        "x86_64-w64-windows-gnu",
    ] {
        let object = w.0.join(format!("{target}.o"));
        w.build(
            &source,
            &object,
            &["-g", "--emit", "obj", "--target", target],
        );
        if dwarf {
            success(
                &Command::new("llvm-dwarfdump-22")
                    .arg("--verify")
                    .arg(&object)
                    .output()
                    .unwrap(),
            );
            let output = Command::new("llvm-dwarfdump-22")
                .arg("--debug-info")
                .arg(&object)
                .output()
                .unwrap();
            success(&output);
            let info = String::from_utf8_lossy(&output.stdout);
            for expected in [
                "Node",
                "next",
                "DW_TAG_pointer_type",
                "DW_TAG_array_type",
                "Choice",
                "DW_TAG_enumeration_type",
                "is_some",
                "is_error",
                "MaybeUninit<i64>",
            ] {
                assert!(
                    info.contains(expected),
                    "missing {expected} for {target}: {info}"
                );
            }
        }
    }
}

#[test]
fn failures_name_checks_and_exact_locations_with_and_without_debug() {
    let w = Workspace::new();
    for (expression, reason) in [
        ("255u8 + 1u8", "arithmetic overflow"),
        ("0u8 - 1u8", "arithmetic overflow"),
        ("200u8 * 2u8", "arithmetic overflow"),
        ("12i32 / 0i32", "division by zero"),
        ("-128i8 / -1i8", "division overflow"),
        ("1u8 << 8u8", "shift amount"),
        ("128u8 << 1u8", "shift overflow"),
        ("256u32 as u8", "numeric conversion"),
        ("300.0f64 as u8", "numeric conversion"),
        ("values[index]", "index bounds"),
        ("values[0usize..index + 1usize]", "slice bounds"),
    ] {
        let source = w.file("failure.dodo", &format!("package failure\nfn main() {{\n    values := [1i32, 2]\n    index := 2usize\n    _ = {expression}\n}}\n"));
        for level in ["0", "3"] {
            for debug in [false, true] {
                let program = w.0.join("failure");
                let mut args = vec!["-O", level];
                if debug {
                    args.push("-g");
                }
                w.build(&source, &program, &args);
                let output = Command::new(&program).output().unwrap();
                assert!(!output.status.success(), "{expression} did not fail");
                let expected = format!("dodo: {reason} check failed at {}:5:9\n", source.display());
                assert_eq!(
                    String::from_utf8_lossy(&output.stderr),
                    expected,
                    "{expression}, -O{level}, debug={debug}"
                );
            }
        }
    }
}

#[test]
fn panic_hooks_receive_source_details_and_cannot_resume_after_failure() {
    let w = Workspace::new();
    let source = w.file(
        "hook.dodo",
        "package hook\nfn main() -> i32 {\n    return 12 / 0\n}\n",
    );
    for returns in [false, true] {
        let c = w.file("hook.c", &format!("#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\nvoid board_panic(const char *check, const char *file, uint32_t line, uint32_t column) {{ fprintf(stderr, \"HOOK %s %s:%u:%u\\n\", check, file, line, column); {} }}\n", if returns { "" } else { "_Exit(73);" }));
        for level in ["0", "3"] {
            let program = w.0.join("hook");
            w.build(
                &source,
                &program,
                &[
                    "-g",
                    "-O",
                    level,
                    "--panic-hook",
                    "board_panic",
                    "--link-arg",
                    c.to_str().unwrap(),
                ],
            );
            let output = Command::new(&program).output().unwrap();
            assert_eq!(output.status.code(), if returns { None } else { Some(73) });
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("HOOK division by zero {}:3:12\n", source.display())
            );
        }
    }
}

#[test]
fn freestanding_checks_need_only_the_selected_hook() {
    let w = Workspace::new();
    let source = w.file(
        "board.dodo",
        "package board\npub fn add(a:u32,b:u32)->u32 { return a+b }\n",
    );
    for mode in ["auto", "trap", "hook"] {
        let ir = w.0.join(format!("{mode}.ll"));
        let mut args = vec!["-g", "--emit", "llvm-ir", "--target", "thumbv7em-none-eabi"];
        args.extend(if mode == "hook" {
            ["--panic-hook", "board_panic"]
        } else {
            ["--panic", mode]
        });
        w.build(&source, &ir, &args);
        let ir = fs::read_to_string(ir).unwrap();
        assert!(ir.contains("llvm.trap"));
        assert!(!ir.contains("@write") && !ir.contains("@abort") && !ir.contains("@_write"));
        assert_eq!(ir.contains("call void @board_panic"), mode == "hook");
        if mode == "hook" {
            assert!(ir.contains("i32 2, i32 39"), "{ir}");
        }
    }
    // Explicit trap mode also makes hosted-target objects usable without libc.
    let ir = w.0.join("host-trap.ll");
    w.build(&source, &ir, &["--emit", "llvm-ir", "--panic", "trap"]);
    assert!(!fs::read_to_string(ir).unwrap().contains("@write"));
}

#[test]
fn invalid_panic_options_and_conflicting_symbols_are_diagnostics() {
    let w = Workspace::new();
    let source = w.file(
        "input.dodo",
        "package app\nunsafe extern \"C\" fn bad_hook()\nfn main()->i32 { return 1 / 0 }\n",
    );
    for args in [
        vec!["--panic", "invalid"],
        vec!["--panic-hook", ""],
        vec!["--panic-hook", "bad symbol"],
        vec!["--panic-hook", "bad_hook"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .args(args)
            .arg("--emit")
            .arg("llvm-ir")
            .arg("-o")
            .arg(w.0.join("bad.ll"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked at"));
    }
}

#[test]
fn gdb_breakpoints_stepping_locals_types_and_shadowing() {
    if !tool("gdb") {
        return;
    }
    let w = Workspace::new();
    let source = w.file(
        "main.dodo",
        "package app\nimport \"helper\"\nfn main() -> i32 {\n    return helper.calculate(7)\n}\n",
    );
    w.file("helper/helper.dodo", "package helper\nstruct Pair { small:u8, value:i64 }\npub fn calculate(seed:i32) -> i32 {\n    local := seed + 5\n    values := [10i32, 20, 30]\n    pair := Pair { small:2, value:1234 }\n    borrowed := &local\n    text := \"hello\"\n    _ = *borrowed\n    {\n        local := 99i32\n        local += seed\n    }\n    local += 1\n    return local\n}\n");
    let program = w.0.join("debug program");
    w.build(&source, &program, &["-g", "-O0"]);
    let mut command = Command::new("gdb");
    command.args(["--batch", "-nx", "-q"]).arg(&program);
    for action in [
        "set debuginfod enabled off",
        "break helper.dodo:12",
        "run",
        "print local",
        "break helper.dodo:14",
        "continue",
        "info args",
        "print local",
        "print values[1]",
        "print pair.value",
        "print text.len",
        "print *borrowed",
        "ptype pair",
        "bt",
        "next",
        "until 15",
        "print local",
    ] {
        command.args(["-ex", action]);
    }
    let output = command.output().unwrap();
    success(&output);
    let log = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "helper.dodo:12",
        "helper.dodo:14",
        "seed = 7",
        "$1 = 99",
        "$2 = 12",
        "$3 = 20",
        "$4 = 1234",
        "$5 = 5",
        "$6 = 12",
        "u8 small;",
        "i64 value;",
        "main.dodo:4",
        "$7 = 13",
    ] {
        assert!(log.contains(expected), "missing {expected}: {log}");
    }
}

#[test]
fn gdb_failure_backtraces_and_generic_import_locations_survive_optimization() {
    if !tool("gdb") {
        return;
    }
    let w = Workspace::new();
    let source = w.file("main.dodo", "package app\nimport \"helper\"\nfn main() -> i32 {\n    return helper.divide::<i32>(12, 0)\n}\n");
    let helper = w.file(
        "helper/helper.dodo",
        "package helper\npub fn divide<T>(a:T,b:T)->T {\n    return a / b\n}\n",
    );
    for level in ["0", "3"] {
        let program = w.0.join("failure");
        w.build(&source, &program, &["-g", "-O", level]);
        let output = Command::new(&program).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!(
                "dodo: division by zero check failed at {}:3:12\n",
                helper.display()
            )
        );
        let output = Command::new("gdb")
            .args(["--batch", "-nx", "-q"])
            .arg(&program)
            .args([
                "-ex",
                "set debuginfod enabled off",
                "-ex",
                "run",
                "-ex",
                "bt",
            ])
            .output()
            .unwrap();
        success(&output);
        let log = String::from_utf8_lossy(&output.stdout);
        for expected in ["SIGABRT", "helper.dodo:3", "main.dodo:4"] {
            assert!(
                log.contains(expected),
                "missing {expected} at O{level}: {log}"
            );
        }
    }
}
