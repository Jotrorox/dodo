//! Exercise discovery, assertions, diagnostics, and crash isolation through the
//! installed-style CLI; no JIT invocation can terminate the Rust test harness.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-native-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dodo"));
        command.current_dir(&self.0);
        command
    }
    fn test(&self, args: &[&str]) -> Output {
        self.command().arg("test").args(args).output().unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn success(output: &Output) {
    assert!(output.status.success(), "{}", text(output));
}
fn failure(output: &Output) {
    assert_eq!(output.status.code(), Some(1), "{}", text(output));
}

#[test]
fn no_arguments_discover_inline_companion_nested_and_documentation_tests() {
    let w = Workspace::new();
    w.file("main.dodo", "package app\nfn sum(a: i32, b: i32) -> i32 { a + b }\n@test fn inline() { assert_eq(sum(1, 2), 3) }\nfn main() -> i32 { 99 }\n");
    w.file(
        "math_test.dodo",
        "package app\nfn test_companion() { assert_eq(sum(20, 22), 42) }\n",
    );
    w.file(
        "nested/custom name.dodo",
        "package nested\n@test fn nested() { assert(true) }\n",
    );
    w.file(
        "guide.md",
        "# Guide\n```dodo test\npackage example\nfn main() { assert_eq(2 + 2, 4) }\n```\n",
    );
    for ignored in [
        "build/broken.dodo",
        "target/broken.dodo",
        "node_modules/broken.dodo",
        "dist/broken.dodo",
        "vendor/broken.dodo",
        ".hidden/broken.dodo",
    ] {
        w.file(ignored, "@test fn broken(");
    }
    w.file(
        "fixture.dodo",
        "This is an unrelated negative compiler fixture.",
    );
    // Keep unrelated package files out of the companion package in this fixture.
    fs::rename(w.0.join("fixture.dodo"), w.0.join("nested/fixture.dodo")).unwrap();
    let listed = w.test(&["--list", "--linker", "missing-linker"]);
    success(&listed);
    assert!(
        text(&listed).contains("4 tests listed"),
        "{}",
        text(&listed)
    );
    let output = w.test(&[]);
    success(&output);
    assert!(
        text(&output).contains("4 passed; 0 failed"),
        "{}",
        text(&output)
    );
    assert!(!w.0.join("build/tests").exists());
}

#[test]
fn directory_discovery_skips_artifacts_at_every_depth() {
    let w = Workspace::new();
    w.file("main.dodo", "package app\n@test fn root() {}\n");
    w.file("nested/main.dodo", "package nested\n@test fn nested() {}\n");
    for directory in [
        "build",
        "target",
        "dist",
        "node_modules",
        "vendor",
        ".hidden",
    ] {
        w.file(&format!("{directory}/broken.dodo"), "@test fn broken(");
        w.file(
            &format!("nested/{directory}/broken.dodo"),
            "@test fn broken(",
        );
    }
    for args in [vec!["--list"], vec!["--list", "."]] {
        let output = w.test(&args);
        success(&output);
        assert!(
            text(&output).contains("2 tests listed; 0 filtered out; 0 discovery errors"),
            "{}",
            text(&output)
        );
    }
}

#[test]
fn assertions_evaluate_once_and_preserve_types_at_all_optimization_levels() {
    let w = Workspace::new();
    w.file(
        "assertions.dodo",
        r#"package app
fn next(value: &mut i32) -> i32 { *value += 1; return *value }
fn identity<T>(value: T) -> T { value }
@test fn numbers() {
    count := 0i32
    assert_eq(next(&mut count), 1)
    assert_eq(count, 1)
    assert_ne(next(&mut count), 1)
    assert_eq(count, 2)
    assert_eq(255u8, 255)
    assert_eq(-9223372036854775808i64, -9223372036854775808i64)
    assert_eq(18446744073709551615u64, 18446744073709551615u64)
    assert_eq(identity::<f32>(1.5), 1.5)
    assert_ne(false, true)
    core.assert(true, "fully qualified")
}
@test fn strings() {
    assert_eq("héllo", "héllo")
    assert_ne("a", "ab")
    assert_ne("ab", "ac")
    assert_eq("", "")
    assert_eq("a\0b", "a\0b")
    assert_ne("a\0b", "a\0c")
}
"#,
    );
    for level in ["0", "3"] {
        let output = w.test(&["-O", level]);
        success(&output);
        assert!(text(&output).contains("2 passed"));
    }
}

#[test]
fn a_failed_assertion_and_runtime_trap_do_not_stop_other_tests() {
    let w = Workspace::new();
    w.file("checks.dodo", "package app\nfn overflow(x: u8) -> u8 { x + 1 }\n@test fn comparison() { assert_eq(2, 3, \"numbers differ\") }\n@test fn checked_trap() { value := overflow(255) }\n@test fn after_failures() { assert(true) }\n");
    for level in ["0", "3"] {
        let output = w.test(&["-O", level]);
        failure(&output);
        let output = text(&output);
        for expected in [
            "assert_eq failed",
            "checks.dodo:3:25",
            "left: 2",
            "right: 3",
            "numbers differ",
            "runtime check failed",
            "checks.dodo:2:28",
            "1 passed; 2 failed",
            "after_failures ... ok",
        ] {
            assert!(output.contains(expected), "missing {expected}:\n{output}");
        }
    }
    assert!(!w.0.join("core").exists());
    let output = w.test(&["--fail-fast"]);
    failure(&output);
    assert!(text(&output).contains("2 not run"), "{}", text(&output));
}

#[test]
fn filters_ignored_tests_empty_suites_and_options_are_explicit() {
    let w = Workspace::new();
    failure(&w.test(&[]));
    success(&w.test(&["--allow-empty"]));
    w.file("a.dodo", "package app\n@test fn add() { assert(true) }\nfn test_add_more() { assert(true) }\n@test @ignore(\"needs hardware\") fn hardware() { assert(false) }\n");
    let output = w.test(&[]);
    success(&output);
    assert!(text(&output).contains("2 passed; 0 failed; 1 ignored"));
    let output = w.test(&["--filter", "add", "--exact"]);
    success(&output);
    assert!(text(&output).contains("1 passed; 0 failed; 0 ignored; 2 filtered out"));
    let output = w.test(&["--filter", "a.dodo::add", "--exact", "--list"]);
    success(&output);
    assert!(text(&output).contains("1 tests listed"));
    success(&w.test(&["--filter", "add", "--skip", "more"]));
    failure(&w.test(&["--filter", "does-not-exist"]));
    failure(&w.test(&["--ignored"]));
    failure(&w.test(&["--include-ignored"]));
    success(&w.test(&["--help"]));
    failure(&w.test(&["missing.dodo"]));
    for args in [
        vec!["--timeout", "NaN"],
        vec!["--timeout", "-1"],
        vec!["--timeout", "inf"],
        vec!["--filter"],
        vec!["--exact"],
        vec!["--doc", "--no-doc"],
        vec!["--ignored", "--include-ignored"],
        vec!["--target", "wasm32-unknown-unknown"],
        vec!["--emit", "obj"],
        vec!["--link-arg", "-ofoo"],
    ] {
        let output = w.test(&args);
        assert_eq!(output.status.code(), Some(2), "{}", text(&output));
    }
}

#[test]
fn hangs_are_timed_out_and_output_is_captured_without_deadlocking() {
    let w = Workspace::new();
    w.file(
        "output.dodo",
        r#"package app
unsafe extern "C" fn putchar(c: i32) -> i32
@test fn noisy() { for i := 0; i < 100000; i += 1 { unsafe { putchar(120) } } }
@test fn hangs() { for {} }
@test fn survivor() { assert(true) }
"#,
    );
    // Keep the deliberately noisy passing test independent of the short deadline.
    success(&w.test(&["--filter", "noisy", "--timeout", "5"]));
    let output = w.test(&["--skip", "noisy", "--timeout", "0.2"]);
    failure(&output);
    assert!(
        text(&output).contains("timed out after 0.200s"),
        "{}",
        text(&output)
    );
    assert!(
        text(&output).contains("1 passed; 1 failed"),
        "{}",
        text(&output)
    );
    assert!(!text(&output).contains("xxx"));
    let output = w.test(&["--filter", "noisy", "--show-output", "--timeout", "0"]);
    success(&output);
    assert!(text(&output).contains("xxx"));
    assert!(text(&output).contains("output truncated after 64 KiB"));
}

#[test]
fn markdown_is_opt_in_and_locations_refer_to_the_original_document() {
    let w = Workspace::new();
    w.file("guide.md", "# Héllo\r\n```dodo\r\nintentionally incomplete\r\n```\r\n~~~dodo test\r\npackage example\r\nfn main() { assert_eq(1, 2) }\r\n~~~\r\n\n```dodo test\npackage example\n@test fn works() { assert(true) }\n```\n");
    let output = w.test(&["--doc"]);
    failure(&output);
    assert!(text(&output).contains("guide.md:7:13"), "{}", text(&output));
    assert!(
        text(&output).contains("1 passed; 1 failed"),
        "{}",
        text(&output)
    );
    assert!(!text(&output).contains("dodo-doctest"));
    w.file("broken.md", "```dodo test\npackage example\nfn main() {}\n");
    let output = w.test(&["--doc", "--list"]);
    failure(&output);
    assert!(text(&output).contains("unclosed executable documentation fence"));
}

#[test]
fn invalid_tests_and_assertions_report_diagnostics_and_other_files_still_run() {
    let w = Workspace::new();
    for source in [
        "@test fn invalid(x: i32) {}",
        "@test fn invalid() -> i32 { 0 }",
        "@test unsafe fn invalid() {}",
        "@test fn invalid<T>() {}",
        "@test struct Invalid {}",
        "@test @test fn invalid() {}",
        "@ignore(\"why\") fn invalid() {}",
        "fn test_invalid(x: i32) {}",
        "@test fn invalid() { assert(1) }",
        "@test fn invalid() { assert_eq(1, true) }",
        "@test fn invalid() { assert_eq(1) }",
        "@test fn invalid() { assert(true, 1) }",
        "@test fn invalid() { assert::<i32>(true) }",
    ] {
        w.file("invalid.dodo", &format!("package app\n{source}\n"));
        let output = w.test(&[]);
        failure(&output);
        assert!(text(&output).contains("invalid.dodo"), "{}", text(&output));
        assert!(!text(&output).contains("panicked at"), "{}", text(&output));
    }
    w.file(
        "valid.dodo",
        "package app\n@test fn works() { assert(true) }\n",
    );
    let output = w.test(&[]);
    failure(&output);
    assert!(
        text(&output).contains("1 passed; 1 failed"),
        "{}",
        text(&output)
    );
    w.file("invalid.dodo", "package app\n@test fn syntax(\n");
    let output = w.test(&[]);
    failure(&output);
    assert!(
        text(&output).contains("1 passed; 0 failed"),
        "{}",
        text(&output)
    );
    assert!(text(&output).contains("1 discovery errors"));
}

#[test]
fn formatting_and_ordinary_compilation_support_assertions_and_test_attributes() {
    let w = Workspace::new();
    w.file("main.dodo", "package app\n@test\n@ignore(\"later\")\nfn sample(){assert_eq(2,2)}\nfn main(){assert_ne(1,2);assert_eq(\"abc\",\"abc\")}\n");
    success(&w.command().arg("fmt").output().unwrap());
    success(&w.command().args(["fmt", "--check"]).output().unwrap());
    success(&w.command().arg("check").output().unwrap());
    success(&w.command().arg("run").output().unwrap());
    let output = w
        .command()
        .args(["compile", "--emit", "llvm-ir", "-o", "program.ll"])
        .output()
        .unwrap();
    success(&output);
    assert!(
        !fs::read_to_string(w.0.join("program.ll"))
            .unwrap()
            .contains("dodo_test_")
    );
    success(
        &w.command()
            .args([
                "compile",
                "--emit",
                "obj",
                "--target",
                "wasm32-unknown-unknown",
            ])
            .output()
            .unwrap(),
    );
}

#[test]
fn imported_tests_are_unique_and_helper_failures_keep_their_original_location() {
    let w = Workspace::new();
    w.file("lib/part.dodo", "package lib\npub fn compare<T>(a: T, b: T) { assert_eq(a, b) }\n@test fn own_test() { assert(true) }\n");
    w.file(
        "app.dodo",
        "package app\nimport \"lib\"\n@test fn caller() { lib.compare::<u8>(1, 2) }\n",
    );
    w.file("guide.md", "```dodo test\npackage example\nimport \"lib\"\nfn main() { lib.compare::<u8>(3, 3) }\n```\n");
    for level in ["0", "3"] {
        let output = w.test(&["-O", level]);
        failure(&output);
        let text = text(&output).replace('\\', "/");
        for expected in [
            "Discovered 3 tests",
            "lib/part.dodo:2:",
            "left: 1",
            "right: 2",
            "2 passed; 1 failed",
            "--filter app.dodo::caller",
        ] {
            assert!(text.contains(expected), "missing {expected}:\n{text}");
        }
    }
}

#[test]
fn process_memory_is_fresh_and_failure_values_are_readable() {
    let w = Workspace::new();
    w.file(
        "state.dodo",
        r#"package app
static mut counter: i32 = 0
fn increment() { unsafe { assert_eq(counter, 0); counter += 1 } }
@test fn first() { increment() }
@test fn second() { increment() }
@test fn boolean() { assert_eq(true, false) }
@test fn floating() { assert_eq(1.5f32, 2.5f32) }
@test fn strings() { assert_eq("a\0b", "a\0c") }
@test fn signed() { assert_eq(-1i8, -2i8) }
@test fn unsigned() { assert_eq(18446744073709551615u64, 0) }
"#,
    );
    let output = w.test(&[]);
    failure(&output);
    let text = text(&output);
    for expected in [
        "2 passed; 5 failed",
        "left: true",
        "right: false",
        "left: 1.5",
        "right: 2.5",
        "left: \"a\\x00b\"",
        "left: -1",
        "right: -2",
        "left: 18446744073709551615",
    ] {
        assert!(text.contains(expected), "missing {expected}:\n{text}");
    }
}

#[cfg(unix)]
#[test]
fn directory_discovery_does_not_follow_symlinks() {
    let w = Workspace::new();
    w.file("a.dodo", "package app\n@test fn once() {}\n");
    std::os::unix::fs::symlink(&w.0, w.0.join("cycle")).unwrap();
    std::os::unix::fs::symlink(w.0.join("a.dodo"), w.0.join("copy.dodo")).unwrap();
    let output = w.test(&[]);
    success(&output);
    assert!(text(&output).contains("1 passed"));
}

#[test]
fn assertions_preserve_argument_order_messages_and_scalar_equality_rules() {
    let w = Workspace::new();
    w.file(
        "values.dodo",
        r#"package app
import "std/math"
import "core/ptr"
fn step(count: &mut i32) -> i32 { *count += 1; return *count }
fn message(count: &mut i32) -> &str from(static) { *count += 1; return "evaluated once" }
enum State { Ready, Done }
@test fn evaluation_order() {
    count := 0i32
    assert_eq(step(&mut count), step(&mut count) - 1, message(&mut count))
    assert_eq(count, 3)
    assert(true, message(&mut count))
    assert_eq(count, 4)
}
@test fn equality_edges() {
    nan := math.from_bits(0x7ff8000000000000u64)
    assert_ne(nan, nan)
    assert_eq(0.0, -0.0)
    assert_ne(State.Ready, State.Done)
    assert_eq(State.Ready, State.Ready)
    left := 1i32
    right := 1i32
    assert_eq(ptr.from_ref(&left), ptr.from_ref(&left))
    assert_ne(ptr.from_ref(&left), ptr.from_ref(&right))
}
"#,
    );
    for level in ["0", "3"] {
        success(&w.test(&["-O", level]));
    }
    w.file("shadow.dodo", "package app\nfn assert(value: i32) -> i32 { value + 1 }\nfn assert_eq<T>(value: T) -> T { value }\n@test fn shadowing() { core.assert_eq(assert(41), 42); core.assert_eq(assert_eq::<u8>(7), 7) }\n");
    success(&w.test(&["shadow.dodo"]));
}

#[test]
fn every_checked_trap_reports_the_operation_in_the_helper_at_o0_and_o3() {
    let w = Workspace::new();
    w.file("traps.dodo", r#"package app
fn divide(a: i32, b: i32) -> i32 { a / b }
fn narrow(a: u32) -> u8 { a as u8 }
fn shift(a: u8, b: u8) -> u8 { a << b }
fn element(index: usize) -> u8 { values := [7u8]; return values[index] }
fn slice(start: usize, end: usize) { values := [7u8]; view := &values[start..end]; assert_eq(view.len, 0usize) }
fn floating(value: f64) -> i32 { value as i32 }
@test fn division() { value := divide(1, 0) }
@test fn division_overflow() { value := divide(-2147483648i32, -1) }
@test fn conversion() { value := narrow(256) }
@test fn shift_count() { value := shift(1, 8) }
@test fn shift_overflow() { value := shift(128, 1) }
@test fn bounds() { value := element(1) }
@test fn slicing() { slice(1, 0) }
@test fn float_conversion() { value := floating(2147483648.0) }
@test fn survives() { assert(true) }
"#);
    for level in ["0", "3"] {
        let output = w.test(&["-O", level]);
        failure(&output);
        let text = text(&output);
        for reason in [
            "division or remainder by zero",
            "signed division overflow",
            "integer conversion out of range",
            "shift count out of range",
            "left shift overflow",
            "index out of bounds",
            "slice bounds out of range",
            "float-to-integer conversion out of range",
        ] {
            assert!(
                text.contains(&format!("runtime check failed: {reason}")),
                "{text}"
            );
        }
        for line in 2..=7 {
            assert!(text.contains(&format!("traps.dodo:{line}:")), "{text}");
        }
        assert!(text.contains("1 passed; 8 failed"), "{text}");
    }
}

#[test]
fn assertion_type_errors_do_not_discard_results_or_weaken_borrows() {
    let w = Workspace::new();
    for body in [
        "values := [1i32]; assert_eq(values, values)",
        "value := 1i32; assert_eq(&value, &value)",
        "result: i32!u8 = ok(1); assert_eq(result, result)",
        "value := 1i32; view := &value; value = 2; assert_eq(*view, 1)",
        "assert_ne(true, false, 42)",
    ] {
        w.file(
            "invalid.dodo",
            &format!("package app\n@test fn rejected() {{ {body} }}\n"),
        );
        let output = w.test(&[]);
        failure(&output);
        assert!(!text(&output).contains("panicked at"), "{}", text(&output));
    }
    for declaration in [
        "@test unsafe extern \"C\" fn foreign()",
        "struct Value { @test fn method() {} }",
        "struct Value { @ignore(\"later\") field: i32 }",
        "@test @ignore(\"a\") @ignore(\"b\") fn twice() {}",
        "@test @ignore(4) fn reason() {}",
        "@test fn generic<T>() {}",
    ] {
        w.file("invalid.dodo", &format!("package app\n{declaration}\n"));
        failure(&w.test(&["--list"]));
    }
}

#[test]
fn discovery_respects_lexical_boundaries_companions_and_explicit_inputs() {
    let w = Workspace::new();
    w.file(
        "notes.dodo",
        "not a program\n// @test fn imaginary() {}\n\"fn test_string() {}\"\n",
    );
    w.file("good.dodo", "package app\n@test fn works() {}\n");
    success(&w.test(&[]));
    // A named companion with broken syntax must be diagnosed even before its tests.
    w.file("broken_test.dodo", "package broken\nfn (\n");
    let output = w.test(&["--list"]);
    failure(&output);
    assert!(text(&output).contains("broken_test.dodo"));
    fs::remove_file(w.0.join("broken_test.dodo")).unwrap();
    success(&w.test(&["good.dodo"]));
    failure(&w.test(&["notes.dodo"]));
    w.file(
        "project/library.dodo",
        "package library\nfn value() -> i32 { 42 }\n",
    );
    w.file(
        "project/library.test.dodo",
        "package library\n@test fn companion() { assert_eq(value(), 42) }\n",
    );
    success(&w.test(&["project"]));
    w.file("--literal.dodo", "package literal\n@test fn works() {}\n");
    success(&w.test(&["--", "--literal.dodo"]));
    w.file("source.txt", "package wrong_extension\n");
    failure(&w.test(&["source.txt"]));
}

#[test]
fn markdown_quoting_selection_exits_and_errors_are_checked() {
    let w = Workspace::new();
    w.file("guide.mdx", "````markdown\n```dodo test\nthis is quoted documentation\n```\n````\n\n   ~~~~dodo test\npackage example\nfn main() -> i32 { 7 }\n   ~~~~\n\n```dodo test\npackage example\n@test fn first() {}\n@test fn second() {}\n```");
    w.file("source.dodo", "package source\n@test fn source_test() {}\n");
    let output = w.test(&["--doc"]);
    failure(&output);
    assert!(
        text(&output).contains("2 passed; 1 failed"),
        "{}",
        text(&output)
    );
    assert!(text(&output).contains("exit status 7"));
    let output = w.test(&["--no-doc"]);
    success(&output);
    assert!(text(&output).contains("1 passed; 0 failed"));
    success(&w.test(&["--doc", "--filter", "first", "--filter", "second"]));
    for (source, expected) in [
        ("```dodo test unknown\n", "use exactly"),
        (
            "```dodo test\npackage example\nfn helper() {}\n```\n",
            "needs fn main()",
        ),
        (
            "```dodo test\npackage example\nfn main() {\n```\n",
            "guide.mdx",
        ),
    ] {
        w.file("guide.mdx", source);
        let output = w.test(&["--doc", "--list", "--allow-empty"]);
        failure(&output);
        assert!(text(&output).contains(expected), "{}", text(&output));
    }
}

#[test]
fn build_errors_continue_other_sources_and_fail_fast_reports_unrun_tests() {
    let w = Workspace::new();
    w.file(
        "a.dodo",
        "package app\n@test fn broken() { missing() }\n@test fn same_source() {}\n",
    );
    w.file("b.dodo", "package app\n@test fn healthy() {}\n");
    let output = w.test(&[]);
    failure(&output);
    assert!(
        text(&output).contains("1 passed; 2 failed"),
        "{}",
        text(&output)
    );
    let output = w.test(&["--fail-fast"]);
    failure(&output);
    assert!(text(&output).contains("1 not run"), "{}", text(&output));
    success(&w.test(&["--filter", "healthy"]));
    let output = w.test(&["b.dodo", "--linker", "missing-linker"]);
    failure(&output);
    assert!(text(&output).contains("could not execute linker"));
    w.file(
        "ignored.dodo",
        "package app\n@test @ignore(\"optional\") fn skip() { missing() }\n",
    );
    success(&w.test(&["ignored.dodo", "--linker", "missing-linker"]));
    failure(&w.test(&["ignored.dodo", "--include-ignored"]));
}

#[cfg(unix)]
#[test]
fn test_sources_link_once_and_relocated_compilers_embed_the_runtime() {
    use std::os::unix::fs::PermissionsExt;
    let w = Workspace::new();
    w.file(
        "driver with spaces",
        "#!/bin/sh\nprintf 'linked\\n' >> link-count\nexec cc \"$@\"\n",
    );
    fs::set_permissions(
        w.0.join("driver with spaces"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    w.file("helpers.c", "int answer(void) { return 42; }\n");
    w.file("tests.dodo", "package app\nunsafe extern \"C\" fn answer() -> i32\n@test fn first() { unsafe { assert_eq(answer(), 42) } }\n@test fn second() { unsafe { assert_eq(answer(), 42) } }\n");
    let output = w
        .command()
        .env("DODO_CC", w.0.join("driver with spaces"))
        .args(["test", "--link-arg", "helpers.c"])
        .output()
        .unwrap();
    success(&output);
    assert_eq!(
        fs::read_to_string(w.0.join("link-count")).unwrap(),
        "linked\n"
    );
    let compiler = w.0.join("relocated-dodo");
    fs::copy(env!("CARGO_BIN_EXE_dodo"), &compiler).unwrap();
    let output = Command::new(compiler)
        .current_dir(&w.0)
        .args(["test", "--link-arg", "helpers.c"])
        .output()
        .unwrap();
    success(&output);
}

#[cfg(unix)]
#[test]
fn rerun_commands_preserve_literal_paths_and_do_not_execute_shell_substitutions() {
    let w = Workspace::new();
    w.file(
        "name ' $(touch injected).dodo",
        "package literal\n@test fn failing() { assert(false) }\n",
    );
    let output = w.test(&[]);
    failure(&output);
    let report = text(&output);
    let command = report
        .split("Rerun the first failure:\n  ")
        .nth(1)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    // Execute the actual printed command with a local dodo on PATH.
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_dodo"), w.0.join("dodo")).unwrap();
    let mut paths = vec![w.0.clone()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let rerun = Command::new("sh")
        .args(["-c", command])
        .env("PATH", std::env::join_paths(paths).unwrap())
        .current_dir(&w.0)
        .output()
        .unwrap();
    failure(&rerun);
    assert!(text(&rerun).contains("1 failed"), "{}", text(&rerun));
    assert!(!w.0.join("injected").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn timeouts_terminate_descendants_in_the_test_process_group() {
    let w = Workspace::new();
    w.file(
        "child.c",
        r#"#include <stdio.h>
#include <sys/types.h>
#include <unistd.h>
int start_descendant(void) {
    pid_t child = fork();
    if (child == 0) { for (;;) pause(); }
    if (child < 0) return -1;
    printf("DESCENDANT=%ld\n", (long)child);
    fflush(stdout);
    return 0;
}
"#,
    );
    w.file("process.dodo", "package app\nunsafe extern \"C\" fn start_descendant() -> i32\n@test fn hanging_parent() { unsafe { assert_eq(start_descendant(), 0) }; for {} }\n@test fn next_test() { assert(true) }\n");
    let output = w.test(&["--link-arg", "child.c", "--timeout", "1"]);
    failure(&output);
    let report = text(&output);
    assert!(report.contains("1 passed; 1 failed"), "{report}");
    let pid: i32 = report
        .split("DESCENDANT=")
        .nth(1)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    struct Descendant(i32);
    impl Drop for Descendant {
        fn drop(&mut self) {
            unsafe {
                libc::kill(self.0, libc::SIGKILL);
            }
        }
    }
    let cleanup = Descendant(pid);
    let state = PathBuf::from(format!("/proc/{pid}/stat"));
    let mut alive = true;
    for _ in 0..100 {
        // An orphan can briefly remain a zombie until the system reaps it.
        alive = fs::read_to_string(&state).is_ok_and(|s| !s.contains(") Z "));
        if !alive {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !alive {
        std::mem::forget(cleanup);
    }
    assert!(
        !alive,
        "timed-out test left descendant {pid} running:\n{report}"
    );
}

#[test]
fn native_environment_is_initialized_and_cwd_and_stdin_are_predictable() {
    let w = Workspace::new();
    w.file("marker", "fixture");
    w.file(
        "environment.dodo",
        r#"package app
import "std/env"
import "std/platform/native"
unsafe extern "C" fn getchar() -> i32
@test fn arguments_are_initialized() {
    bytes := [0u8; 4096]
    wide := [0u16; 4096]
    match env.arguments(&mut bytes, &mut wide) {
        ok(arguments) => { assert(arguments.len() >= 1) },
        err(_) => { assert(false, "argument runtime was not initialized") },
    }
    cwd_bytes := [0u8; 4096]
    cwd_wide := [0u16; 4096]
    match env.current_dir(&mut cwd_bytes, &mut cwd_wide) {
        ok(directory) => { assert(directory.units().len > 0) },
        err(_) => { assert(false, "working directory must be available") },
    }
    unsafe { assert_eq(getchar(), -1, "stdin is closed") }
}
"#,
    );
    success(&w.test(&[]));
    w.file("cwd.c", "#include <stdio.h>\nint has_fixture(void) { FILE *f = fopen(\"marker\", \"r\"); if (!f) return 0; fclose(f); return 1; }\n");
    w.file("nested/cwd.dodo", "package cwd\nunsafe extern \"C\" fn has_fixture() -> i32\n@test fn from_caller() { unsafe { assert_eq(has_fixture(), 1) } }\n");
    success(&w.test(&["nested", "--link-arg", "cwd.c"]));
}
