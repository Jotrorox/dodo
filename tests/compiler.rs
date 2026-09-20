//! End-to-end tests exercise the installed-style CLI and real native programs.
//! Temporary artifacts stay outside the repository and are removed on drop.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const COMPILER: &str = env!("CARGO_BIN_EXE_dodo");
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        loop {
            let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("dodo-test-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test workspace: {error}"),
            }
        }
    }

    fn source(&self, source: &str) -> PathBuf {
        self.file("input.dodo", source)
    }

    fn file(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).expect("create source fixture directory");
        fs::write(&path, source).expect("write source fixture");
        path
    }

    fn compiler(&self) -> Command {
        let mut command = Command::new(COMPILER);
        command.current_dir(&self.0);
        command
    }

    fn build(&self, source: &Path, optimization: u8) -> PathBuf {
        let artifact = self.0.join(format!("program-O{optimization}"));
        let output = self
            .compiler()
            .arg("build")
            .arg(source)
            .arg("-o")
            .arg(&artifact)
            .arg("-O")
            .arg(optimization.to_string())
            .output()
            .expect("run compiler");
        assert_success(&output, "compile native executable");
        assert!(artifact.is_file(), "compiler did not create the executable");
        artifact
    }

    fn execute(&self, artifact: &Path) -> Output {
        Command::new(artifact)
            .current_dir(&self.0)
            .output()
            .expect("execute generated native program")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn native_at_all_levels(source: &str, expected: i32, stdout: &str) {
    let workspace = Workspace::new();
    let source = workspace.source(source);
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization);
        let output = workspace.execute(&executable);
        assert_eq!(
            output.status.code(),
            Some(expected),
            "unexpected native status at O{optimization}: {:?}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), stdout);
    }
}

fn rejects(source: &str, message: &str) {
    let workspace = Workspace::new();
    let source = workspace.source(source);
    let output = workspace
        .compiler()
        .arg("check")
        .arg(&source)
        .output()
        .expect("check rejected program");
    assert!(!output.status.success(), "invalid source was accepted");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(message),
        "expected {message:?}, got:\n{stderr}"
    );
    assert!(
        stderr.contains("input.dodo"),
        "diagnostic should name source: {stderr}"
    );
    assert!(
        !stderr.contains("panicked at"),
        "compiler panicked: {stderr}"
    );
}

#[test]
fn cli_help_version_and_bad_arguments() {
    let workspace = Workspace::new();
    for flag in ["--help", "--version"] {
        let output = workspace.compiler().arg(flag).output().unwrap();
        assert_success(&output, flag);
        assert!(String::from_utf8_lossy(&output.stdout).contains("dodo"));
        assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));
    }
    for arguments in [
        vec!["unknown-command"],
        vec!["--unknown"],
        vec!["compile", "--emit"],
        vec!["check", "--output", "out"],
        vec!["run", "--emit", "obj"],
        vec!["compile", "one.dodo", "two.dodo"],
    ] {
        let output = workspace.compiler().args(arguments).output().unwrap();
        assert!(!output.status.success());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn cli_projects_default_to_main_and_compile_keeps_an_executable() {
    let workspace = Workspace::new();
    workspace.file("main.dodo", "package app\nfn main() -> i32 { 23 }\n");
    // Unimported siblings and folders are not part of the entry file.
    workspace.file("other.dodo", "invalid source");
    workspace.file("unused/broken.dodo", "invalid source");
    let output = workspace.compiler().arg("check").output().unwrap();
    assert_success(&output, "check the default entry");
    assert!(String::from_utf8_lossy(&output.stderr).contains("Checked main.dodo"));
    let output = workspace.compiler().arg("run").output().unwrap();
    assert_eq!(output.status.code(), Some(23), "{output:?}");
    assert!(!workspace.0.join("build").exists());
    let executable = workspace
        .0
        .join("build")
        .join(workspace.0.file_name().unwrap());
    for command in ["compile", "build"] {
        for input in [None, Some("."), Some("main.dodo")] {
            let output = workspace
                .compiler()
                .arg(command)
                .args(input)
                .output()
                .unwrap();
            assert_success(&output, "compile the default entry");
            assert_eq!(workspace.execute(&executable).status.code(), Some(23));
        }
    }
    let output = workspace
        .compiler()
        .args([
            "compile",
            "-O2",
            "--emit",
            "llvm-ir",
            "-o",
            "custom/main.ll",
        ])
        .output()
        .unwrap();
    assert_success(&output, "compile the default entry with options");
    assert!(
        fs::read_to_string(workspace.0.join("custom/main.ll"))
            .unwrap()
            .contains("dodo.app.main")
    );
}

#[test]
fn cli_project_folders_and_libraries_are_ordinary_local_imports() {
    let workspace = Workspace::new();
    let project = workspace.0.join("my project.v1");
    workspace.file("main.dodo", "invalid source in caller directory");
    workspace.file("my project.v1/main.dodo", "package app\nimport \"lib\"\nimport \"features/count\"\nfn main() -> i32 { lib.answer() + count.extra() }\n");
    workspace.file(
        "my project.v1/lib/main.dodo",
        "package lib\npub fn answer() -> i32 { base() + 1 }\n",
    );
    workspace.file(
        "my project.v1/lib/helpers.dodo",
        "package lib\nfn base() -> i32 { 40 }\n",
    );
    workspace.file("my project.v1/lib/unused/broken.dodo", "invalid source");
    workspace.file(
        "my project.v1/features/count/value.dodo",
        "package count\npub fn extra() -> i32 { 1 }\n",
    );
    workspace.file("my project.v1/other.dodo", "invalid source");
    for input in [&project, &project.join("main.dodo")] {
        let output = workspace
            .compiler()
            .arg("check")
            .arg(input)
            .output()
            .unwrap();
        assert_success(&output, "check a project by folder or entry file");
        let output = workspace.compiler().arg("run").arg(input).output().unwrap();
        assert_eq!(output.status.code(), Some(42), "{output:?}");
    }
    for command in ["compile", "build"] {
        for input in [&project, &project.join("main.dodo")] {
            let output = workspace
                .compiler()
                .arg(command)
                .arg(input)
                .output()
                .unwrap();
            assert_success(&output, "compile an explicit project folder or entry file");
            assert_eq!(
                workspace
                    .execute(&workspace.0.join("build/my project.v1"))
                    .status
                    .code(),
                Some(42)
            );
        }
    }
    let output = workspace
        .compiler()
        .args(["compile", "--emit", "llvm-ir"])
        .arg(&project)
        .output()
        .unwrap();
    assert_success(
        &output,
        "emit an artifact using the full project folder name",
    );
    assert!(
        fs::read_to_string(workspace.0.join("build/my project.v1.ll"))
            .unwrap()
            .contains("dodo.app.main")
    );
    let output = workspace
        .compiler()
        .current_dir(&project)
        .args(["run", "."])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(42), "{output:?}");
}

#[test]
fn cli_missing_project_entry_does_not_search_parents_or_other_files() {
    let workspace = Workspace::new();
    workspace.file("main.dodo", "package parent\nfn main() {}\n");
    let project = workspace.0.join("child");
    workspace.file("child/lib.dodo", "package lib\nfn main() {}\n");
    workspace.file("child/src/main.dodo", "package nested\nfn main() {}\n");
    for entry_is_directory in [false, true] {
        if entry_is_directory {
            workspace.file(
                "child/main.dodo/main.dodo",
                "package nested\nfn main() {}\n",
            );
        }
        for command in ["check", "run", "compile", "build"] {
            for input in [None, Some(".")] {
                let output = workspace
                    .compiler()
                    .current_dir(&project)
                    .arg(command)
                    .args(input)
                    .output()
                    .unwrap();
                assert!(!output.status.success(), "{output:?}");
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(stderr.contains("expected project entry file"), "{stderr}");
                assert!(stderr.contains("main.dodo"), "{stderr}");
            }
        }
    }
    assert!(!project.join("build").exists());
    let output = workspace
        .compiler()
        .current_dir(&project)
        .args(["run", "lib.dodo"])
        .output()
        .unwrap();
    assert_success(&output, "an explicit file can have any name");
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
#[test]
fn cli_default_run_passes_program_arguments_after_options() {
    let workspace = Workspace::new();
    workspace.file("main.dodo", include_str!("os/env_checks.dodo"));
    let output = workspace
        .compiler()
        .args(["run", "-O2", "--", "space arg", "é", ""])
        .env("DODO_ENV_TEST", "parent  é")
        .output()
        .unwrap();
    assert_success(
        &output,
        "run default entry with options and program arguments",
    );
}

#[test]
fn cli_check_does_not_need_main_and_reports_source_locations() {
    let workspace = Workspace::new();
    let source =
        workspace.source("package library\npub fn add(a: i32, b: i32) -> i32 { return a + b }\n");
    let output = workspace
        .compiler()
        .arg("check")
        .arg(source)
        .output()
        .unwrap();
    assert_success(&output, "check library without an entry point");
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 1);
    rejects(
        "package bad\nfn broken() -> i32 { return missing }\n",
        "missing",
    );
}

#[test]
fn cli_run_preserves_program_exit_status() {
    let workspace = Workspace::new();
    for code in [0, 23, 255, -1, 256, -256, i32::MIN, i32::MAX] {
        let source = workspace.source(&format!(
            "package app\nfn main() -> i32 {{ return {code} }}\n"
        ));
        // Windows preserves all 32 bits; Unix exposes the low eight bits.
        let expected = if cfg!(windows) {
            code
        } else {
            i32::from(code as u8)
        };
        for optimization in [0, 3] {
            let output = workspace
                .compiler()
                .arg("run")
                .arg(&source)
                .arg("-O")
                .arg(optimization.to_string())
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(expected),
                "return {code} at O{optimization}: {output:?}"
            );
            assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked at"));
        }
    }
}

#[test]
fn cli_accepts_spaces_in_source_and_output_paths() {
    let workspace = Workspace::new();
    let source = workspace.file(
        "source with spaces.dodo",
        "package spaces\nfn main() -> i32 { return 17 }\n",
    );
    let executable = workspace.0.join("program with spaces");
    let output = workspace
        .compiler()
        .arg("build")
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert_success(&output, "compile paths containing spaces");
    assert_eq!(workspace.execute(&executable).status.code(), Some(17));
}

#[test]
fn cli_preserves_sources_and_existing_outputs_when_builds_fail() {
    let workspace = Workspace::new();
    let original = "package app\nfn main() -> i32 { return 0 }\n";
    let source = workspace.source(original);
    let output = workspace
        .compiler()
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "compiler accepted overwriting source input"
    );
    assert_eq!(fs::read_to_string(&source).unwrap(), original);

    let artifact = workspace.0.join("existing-artifact");
    fs::write(&artifact, b"previous successful build").unwrap();
    workspace.source("package app\nfn main() -> i32 { return missing }\n");
    let output = workspace
        .compiler()
        .arg("build")
        .arg(source)
        .arg("-o")
        .arg(&artifact)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(artifact).unwrap(), b"previous successful build");
    assert_eq!(
        fs::read_dir(&workspace.0).unwrap().count(),
        2,
        "failed build leaked temporary output files"
    );
}

#[test]
fn hello_uses_safe_hosted_console() {
    native_at_all_levels(include_str!("../examples/hello.dodo"), 0, "Hello, world!\n");
}

#[test]
fn postfix_unwrap_success_chaining_and_evaluation_order() {
    native_at_all_levels(
        r#"package unwrap
struct Cell<T> {
    value: T
    fn read(self) -> T!u8 { ok(self.value) }
}
fn identity<T>(value: T) -> T { value }
fn once(count: &mut i32) -> i32!u8 {
    *count += 1
    ok(*count)
}
fn done() -> void!u8 { ok() }
fn nested() -> Result<Result<i32, u8>, bool> { ok(ok(7)) }
fn propagate_inner() -> i32!u8 { ok(nested()!?) }
fn propagate_outer() -> i32!bool { ok(nested()?!) }
fn view(value: &mut i32) -> &mut i32!u8 from(value) { ok(value) }
fn main() {
    count := 0i32
    assert_eq(once(&mut count)! + once(&mut count)!, 3)
    assert_eq(count, 2)
    pending := done()
    done()!
    pending!
    assert_eq(identity(Cell<i32>{value: 42}.read()!), 42)
    assert_eq(nested()!!, 7)
    assert_eq(propagate_inner()!, 7)
    assert_eq(propagate_outer()!, 7)
    array: [2]u8!bool = ok([4, 9])
    values := array!
    slice: &[u8]!bool = ok(&values[..])
    assert_eq(slice![1], 9u8)
    result: Cell<i32>!u8 = ok(Cell<i32>{value: 6})
    cell := result!
    borrowed: &Cell<i32>!u8 = ok(&cell)
    assert_eq(borrowed!.value, 6)
    flag: bool!u8 = ok(false)
    assert(!flag!)
    assert(once(&mut count)! != 0)
    reference := view(&mut count)!
    *reference += 1
    assert_eq(count, 4)
}
"#,
        0,
        "",
    );
}

#[test]
fn postfix_unwrap_preserves_payload_ownership_and_cleanup() {
    let source = format!(
        "package unwrap_cleanup\n{DROP_TYPE}\n{}",
        r#"fn make(character: i32) -> Guard!u8 { ok(Guard{character}) }
fn consume(value: Guard) {}
fn main() {
    result := make(65)
    owned := result!
    consume(owned)
    make(66)!
    remaining := make(67)!
}
"#
    );
    native_at_all_levels(&source, 0, "ABC");
}

#[test]
fn postfix_unwrap_rejects_invalid_types_moves_and_borrows() {
    for (source, message) in [
        ("fn f() { 1! }", "postfix `!` requires a Result"),
        ("fn f() { true! }", "postfix `!` requires a Result"),
        (
            "fn f(x: Option<i32>) { x! }",
            "postfix `!` requires a Result",
        ),
        (
            "fn f(x: &Result<i32, u8>) { x! }",
            "postfix `!` requires a Result",
        ),
        ("fn f(x: i32!u8) { x!\nx! }", "moved"),
        (
            "fn f(x: Result<Result<i32, u8>, bool>) { x! }",
            "Result must be handled",
        ),
        (
            "fn f(x: i32!u8) { r := &x\nx!\nmatch r { ok(_) => {}, err(_) => {} } }",
            "borrow",
        ),
        (
            "fn view(x: &i32) -> &i32!u8 from(x) { ok(x) }\nfn f() -> &i32 from(static) { x := 1i32\nview(&x)! }",
            "local",
        ),
        (
            "fn view(x: &mut i32) -> &mut i32!u8 from(x) { ok(x) }\nfn f() { x := 1i32\nr := view(&mut x)!\nx = 2\n_ = *r }",
            "borrow",
        ),
    ] {
        rejects(&format!("package invalid\n{source}\n"), message);
    }
}

#[test]
fn specification_samples_returns_131() {
    native_at_all_levels(include_str!("../examples/samples.dodo"), 131, "");
}

#[test]
fn specification_hex_handles_success_and_both_error_paths() {
    native_at_all_levels(include_str!("../examples/hex.dodo"), 0, "");
}

#[test]
fn recursive_calls_and_checked_arithmetic() {
    native_at_all_levels(include_str!("../examples/fibonacci.dodo"), 21, "");
}

#[test]
fn loops_break_continue_and_mutable_iteration() {
    native_at_all_levels(
        r#"package loops
fn main() -> i32 {
    values := [4]i32{1, 2, 3, 4}
    for item in &mut values {
        *item *= 2
    }
    total := 0i32
    for index, item in values {
        if index == 1 { continue }
        total += *item
    }
    for i := 0; i < 5; i += 1 {
        if i == 3 { break }
        total += i as i32
    }
    count := 0
    for count < 2 { count += 1 }
    for { total += count as i32
        break
    }
    return total
}
"#,
        21,
        "",
    );
}

#[test]
fn floats_casts_and_short_circuit_preserve_side_effects() {
    native_at_all_levels(
        r#"package numeric
fn touch(value: &mut i32) -> bool {
    *value += 1
    return true
}
fn main() -> i32 {
    f64 a = 5.5
    f32 b = 2.0
    number := (a * (b as f64) - 1.0) as i32
    count := 0i32
    if false && touch(&mut count) { return 1 }
    if true || touch(&mut count) {
        return number + count
    }
    return 2
}
"#,
        10,
        "",
    );
}

#[test]
fn references_reborrow_and_disjoint_fields() {
    native_at_all_levels(
        r#"package references
struct Pair { i32 left
    i32 right
}
fn increment(value: &mut i32) -> void { *value += 1 }
fn main() -> i32 {
    pair := Pair{left: 10, right: 20}
    left := &mut pair.left
    right := &mut pair.right
    increment(left)
    increment(left)
    increment(right)
    return *left + *right
}
"#,
        33,
        "",
    );
}

#[test]
fn borrowed_return_contracts_and_static_storage() {
    native_at_all_levels(
        r#"package borrowing
fn choose(a: &i32, b: &i32, first: bool) -> &i32 from(a, b) {
    if first { return a }
    return b
}
fn bytes() -> &[u8] from(static) { return b"ab" }
fn main() -> i32 {
    a := 10i32
    b := 20i32
    selected := choose(&a, &b, false)
    return *selected + bytes().len as i32
}
"#,
        22,
        "",
    );
}

#[test]
fn explicit_pointer_access_and_target_memory_queries() {
    native_at_all_levels(
        r#"package memory
import "core/ptr"
import "core/mem"
fn main() -> i32 {
    value := 12i32
    unsafe {
        pointer := &mut value as *mut i32
        ptr.write(pointer, 33)
        read := ptr.read(pointer)
        return read + mem.size_of<[4]u16>() as i32
    }
}
"#,
        41,
        "",
    );
}

#[test]
fn generic_functions_and_option_payloads() {
    native_at_all_levels(
        r#"package generic
fn identity<T>(value: T) -> T { return value }
fn unwrap(value: Option<i32>) -> i32 {
    match value {
        some(number) => { return number }
        none => { return 0 }
    }
}
fn main() -> i32 {
    Option<i32> value = some(identity<i32>(42))
    return unwrap(value)
}
"#,
        42,
        "",
    );
}

#[test]
fn generic_specialization_breadth_and_depth_have_separate_limits() {
    let mut source = String::from("package wide\nfn accept<T>() {}\nfn main() {\n");
    for count in 0..300 {
        source.push_str(&format!("accept::<[{count}]u8>()\n"));
    }
    source.push_str("}\n");
    native_at_all_levels(&source, 0, "");

    let mut excessive = String::from("package wide\nfn accept<T>() {}\nfn main() {\n");
    for count in 0..4097 {
        excessive.push_str(&format!("accept::<[{count}]u8>()\n"));
    }
    excessive.push_str("}\n");
    rejects(
        &excessive,
        "generic instantiation limit exceeded (4096 specializations)",
    );
    for source in [
        "package recursive\nfn grow<T>() { grow::<Option<T>>() }\nfn main() { grow::<u8>() }",
        "package recursive\nstruct Grow<T> { next: Option<Grow<Option<T>>> }\nfn use(value: &Grow<u8>) {}",
        "package recursive\nimport \"core/mem\"\nfn grow<T>() { unsafe { _ = mem.callback::<Option<T>>(grow) } }\nfn main() { grow::<u8>() }",
    ] {
        rejects(source, "generic instantiation depth exceeded");
    }
}

#[test]
fn generic_struct_methods_keep_borrowed_returns() {
    native_at_all_levels(
        r#"package generic
struct Box<T> {
    T value
    fn get(self: &Self) -> &T { return &self.value }
}
fn main() -> i32 {
    value := Box<i32>{value: 42}
    view := value.get()
    return *view
}
"#,
        42,
        "",
    );
}

#[test]
fn enum_payload_matching_and_borrowed_patterns() {
    native_at_all_levels(
        r#"package variants
enum Value { Empty, Number(i32 number) }
fn main() -> i32 {
    value := Value.Number(27)
    match &value {
        Value.Empty => { return 0 }
        Value.Number(number) => { return *number }
    }
}
"#,
        27,
        "",
    );
}

#[test]
fn immutable_runtime_bindings_preserve_mutable_references_and_function_tails() {
    native_at_all_levels(
        r#"package bindings
struct Counter { value: i32 }
fn read_limit() -> i32 { 20 }
fn add(a: i32, b: i32) -> i32 { a + b }
fn early(value: i32) -> i32 {
    if value < 0 { return 0 }
    { value + 1 }
}
fn main() -> i32 {
    const CAPACITY: usize = 2
    let limit: i32 = read_limit()
    count := Counter{value: 0}
    let reference = &mut count
    reference.value = limit
    let contents = &mut reference.value
    *contents += 1
    data := [0i32; CAPACITY]
    let view = &mut data[..]
    view[0] = early(0)
    let second = &mut view[1]
    *second = 20
    add(count.value, data[0] + data[1])
}
"#,
        42,
        "",
    );
}

#[test]
fn recursive_patterns_ranges_guards_and_conditional_bindings_execute() {
    native_at_all_levels(
        r#"package patterns
struct Inner { value: Option<Option<i32>> }
struct Outer { inner: Inner, unused: i32 }
fn unpack(optional: Option<i32>) -> i32 {
    let some(value) = optional else { return 0 }
    value
}
fn classify(value: Option<Option<bool>>) -> i32 {
    match value {
        some(some(true)) => 1,
        some(some(false)) => 2,
        some(none) => 3,
        none => 4,
    }
}
fn digit(ch: u8) -> i32 {
    match ch {
        b'0'..=b'9' => 10,
        b'a'..b'g' | b'A'..=b'F' => 20,
        _ => 0,
    }
}
fn main() -> i32 {
    let outer = Outer{inner: Inner{value: some(some(7i32))}, unused: 99}
    total := 0i32
    if let Outer{inner: Inner{value: some(some(value))}, ..} = outer {
        total += value
    } else { return 1 }
    let Inner{value: optional} = Inner{value: some(some(2i32))}
    let some(some(value)) = optional else { return 2 }
    total += value
    match digit(b'a') {
        value if value > 10 => { total += value }
        _ => { return 3 }
    }
    total + unpack(some(3i32)) + unpack(none) + classify(some(some(true))) + classify(some(some(false))) + classify(some(none)) + classify(none)
}
"#,
        42,
        "",
    );
}

#[test]
fn alternative_patterns_retry_guards_in_order() {
    native_at_all_levels(
        r#"package guards
unsafe extern "C" fn putchar(value: i32) -> i32
enum Pair { Values(i32, i32) }
fn accept(value: i32) -> bool {
    unsafe { putchar(64 + value) }
    value == 1
}
fn main() -> i32 {
    match Pair.Values(1, 2) {
        Pair.Values(1, value) | Pair.Values(value, 2) if accept(value) => 42,
        _ => 0,
    }
}
"#,
        42,
        "BA",
    );
}

#[test]
fn statement_match_expression_arms_handle_results_and_function_tails_forward_them() {
    native_at_all_levels(
        r#"package results
fn read(fail: bool) -> i32!i32 {
    if fail { err(7) } else { ok(42) }
}
fn forward() -> i32!i32 { read(false) }
fn consume(value: i32) {}
fn main() -> i32 {
    match read(true) {
        ok(value) => consume(value),
        err(error) => consume(error),
    }
    match forward() {
        ok(value) => value,
        err(_) => 0,
    }
}
"#,
        42,
        "",
    );
}

#[test]
fn recursive_borrowed_patterns_keep_mutable_payload_access() {
    native_at_all_levels(
        r#"package borrows
struct Item { value: i32 }
fn main() -> i32 {
    item := some(some(Item{value: 40}))
    if let some(some(Item{value})) = &mut item { *value += 1 }
    let some(some(Item{value})) = &mut item else { return 0 }
    *value += 1
    *value
}
"#,
        42,
        "",
    );
}

#[test]
fn new_binding_and_pattern_forms_reject_invalid_programs() {
    for (body, message) in [
        ("fn f() { let x = 1\nx = 2 }", "immutable"),
        (
            "struct S { x: i32 }\nfn f() { let s = S{x: 1}\ns.x = 2 }",
            "immutable",
        ),
        ("fn f() { let a = [1i32]\na[0] = 2 }", "immutable"),
        ("fn f() { let x = 1\nr := &mut x }", "immutable"),
        ("fn f() { let x = 1\nconst N: isize = x }", "compile-time"),
        ("fn f() -> i32 { 42; }", "returning"),
        ("fn f() -> i32 { false }", "expected"),
        (
            "fn fallible() -> i32!i32 { ok(1) }\nfn f() { match true { true => fallible(), false => fallible() } }",
            "Result",
        ),
        ("fn f(value: i32!i32) { if let ok(x) = value {} }", "Result"),
        (
            "fn f(value: i32!i32) { let ok(x) = value else { return } }",
            "Result",
        ),
        (
            "fn f(value: Option<i32!i32>) { if let some(ok(x)) = value {} }",
            "Result",
        ),
        (
            "fn f(value: Option<i32!i32>) { match value { some(_) => {}, none => {} } }",
            "Result",
        ),
        (
            "fn f(value: Option<i32>) { let some(x) = value else {} }",
            "diverge",
        ),
        (
            "fn f(value: Option<i32>) { let some(x) = value }",
            "refutable",
        ),
        (
            "fn f(value: Option<i32>) { match value { some(x) | none => {}, _ => {} } }",
            "bind",
        ),
        (
            "fn f(value: Option<Option<bool>>) { match value { some(some(true)) => {}, some(none) => {}, none => {} } }",
            "exhaustive",
        ),
        (
            "fn f(value: bool) { match value { true if false => {}, false => {} } }",
            "exhaustive",
        ),
        (
            "fn f(value: u8) { match value { 300..=400 => {}, _ => {} } }",
            "range",
        ),
        (
            "fn f(value: u8) { match value { 9..=1 => {}, _ => {} } }",
            "range",
        ),
        (
            "struct S { x: i32 }\nfn take(s: S) -> bool { true }\nfn f(value: Option<S>) { match value { some(s) if take(s) => {}, _ => {} } }",
            "guard",
        ),
    ] {
        rejects(&format!("package invalid\n{body}\n"), message);
    }
}
const DROP_TYPE: &str = r#"
unsafe extern "C" fn putchar(character: i32) -> i32
struct Guard {
    i32 character
    fn drop(self: &mut Self) -> void {
        // SAFETY: Every fixture uses an ASCII character accepted by putchar.
        unsafe { putchar(self.character) }
    }
}
"#;

#[test]
fn cleanup_reverses_order_and_does_not_double_drop_moves() {
    let source = format!(
        "package cleanup\n{DROP_TYPE}\n{}",
        r#"fn main() -> i32 {
    first := Guard{character: 65}
    second := Guard{character: 66}
    moved := first
    return 0
}
"#
    );
    native_at_all_levels(&source, 0, "AB");
}

#[test]
fn cleanup_runs_on_overwrite_explicit_drop_break_and_continue() {
    let source = format!(
        "package cleanup\n{DROP_TYPE}\n{}",
        r#"fn main() -> i32 {
    value := Guard{character: 65}
    value = Guard{character: 66}
    core.drop(value)
    for i := 0; i < 2; i += 1 {
        current := Guard{character: 67 + i as i32}
        if i == 0 { continue }
        break
    }
    return 0
}
"#
    );
    native_at_all_levels(&source, 0, "ABCD");
}

#[test]
fn cleanup_runs_during_result_propagation() {
    let source = format!(
        "package cleanup\n{DROP_TYPE}\n{}",
        r#"fn fail() -> i32!i32 { return err(7) }
fn attempt() -> i32!i32 {
    value := Guard{character: 65}
    number := fail()?
    return ok(number)
}
fn main() -> i32 {
    match attempt() {
        ok(_) => { return 1 }
        err(code) => { return code }
    }
}
"#
    );
    native_at_all_levels(&source, 7, "A");
}

#[test]
fn custom_destructor_runs_before_nested_fields() {
    let source = format!(
        "package cleanup\n{DROP_TYPE}\n{}",
        r#"struct Outer {
    Guard inner
    fn drop(self: &mut Self) -> void {
        unsafe { putchar(65) }
    }
}
fn main() -> void {
    outer := Outer{inner: Guard{character: 66}}
}
"#
    );
    native_at_all_levels(&source, 0, "AB");
}

#[test]
fn rejects_conflicting_borrows_and_moves() {
    for body in [
        "value := 1i32\nview := &value\nvalue = 2\nreturn *view",
        "value := 1i32\nfirst := &mut value\nsecond := &mut value\nreturn *first + *second",
        "values := [1]i32{1}\nview := &values\nmoved := values\nreturn view[0]",
    ] {
        rejects(
            &format!("package bad\nfn main() -> i32 {{\n{body}\n}}\n"),
            "borrow",
        );
    }
    rejects(
        "package bad\nfn main() -> i32 {\nvalues := [1]i32{1}\nmoved := values\nreturn values[0]\n}\n",
        "moved",
    );
}

#[test]
fn rejects_escaping_or_ambiguous_borrows() {
    rejects(
        "package bad\nfn invalid() -> &i32 from(static) {\nvalue := 1i32\nreturn &value\n}\n",
        "return",
    );
    rejects(
        "package bad\nfn ambiguous(a: &i32, b: &i32) -> &i32 { return a }\n",
        "from",
    );
}

#[test]
fn rejects_unhandled_results_and_incompatible_propagation() {
    for statement in ["fallible()", "_ = fallible()", "unused := fallible()"] {
        rejects(
            &format!(
                "package bad\nfn fallible() -> i32!i32 {{ return ok(1) }}\nfn main() -> void {{ {statement}\n}}\n"
            ),
            "Result",
        );
    }
    rejects(
        "package bad\nfn fallible() -> i32!u8 { return err(1) }\nfn wrong() -> i32!i32 {\nvalue := fallible()?\nreturn ok(value)\n}\n",
        "error type",
    );
}

#[test]
fn unsafe_function_body_still_needs_an_explicit_block() {
    rejects(
        "package bad\nunsafe extern \"C\" fn putchar(character: i32) -> i32\nunsafe fn wrong() -> i32 { return putchar(65) }\n",
        "unsafe",
    );
    rejects(
        "package bad\nfn read(pointer: *const i32) -> i32 { return *pointer }\n",
        "unsafe",
    );
}

#[test]
fn rejects_invalid_types_returns_visibility_and_matches() {
    for (source, message) in [
        ("package bad\nfn missing() { return 1 }\n", "expected"),
        (
            "package bad\nfn missing() -> i32 {\nvalue := 1\n}\n",
            "return",
        ),
        (
            "package bad\nfn wrong() -> i32 { return true }\n",
            "expected",
        ),
        (
            "package bad\nstruct Private { i32 value }\npub fn expose(value: Private) -> void {}\n",
            "private",
        ),
        (
            "package bad\nenum E { A, B }\nfn missing(value: E) -> void { match value { E.A => {} } }\n",
            "exhaustive",
        ),
    ] {
        rejects(source, message);
    }
}

#[test]
fn arithmetic_and_index_traps_survive_optimization() {
    let fixtures = [
        (
            "unsigned overflow",
            "u8 value = 255\nvalue += 1\nreturn value as i32",
        ),
        (
            "signed overflow",
            "i8 value = 127\nvalue += 1\nreturn value as i32",
        ),
        ("underflow", "u8 value = 0\nvalue -= 1\nreturn value as i32"),
        (
            "multiply overflow",
            "u8 value = 200\nvalue *= 2\nreturn value as i32",
        ),
        (
            "overflow in wrapping argument",
            "value := 255u8\nreturn core.wrapping_mul(value + 1u8, 0u8) as i32",
        ),
        (
            "overflow after wrapping result",
            "value := core.wrapping_sub(0u8, 1u8)\nreturn (value + 1u8) as i32",
        ),
        (
            "division by zero",
            "i32 value = 12\ni32 zero = 0\nreturn value / zero",
        ),
        (
            "signed division overflow",
            "i8 value = -128\ni8 minus = -1\nreturn (value / minus) as i32",
        ),
        (
            "shift",
            "u8 value = 1\nu8 amount = 8\nreturn (value << amount) as i32",
        ),
        (
            "bounds",
            "values := [2]i32{1, 2}\nindex := 2usize\nreturn values[index]",
        ),
        (
            "narrowing cast",
            "u32 value = 256\nreturn (value as u8) as i32",
        ),
        (
            "negative cast",
            "i32 value = -1\nreturn (value as u32) as i32",
        ),
        (
            "float cast",
            "f64 value = 300.0\nreturn (value as u8) as i32",
        ),
    ];
    for (name, body) in fixtures {
        let workspace = Workspace::new();
        let source = workspace.source(&format!("package trap\nfn main() -> i32 {{\n{body}\n}}\n"));
        for optimization in [0, 3] {
            let artifact = workspace.build(&source, optimization);
            let output = workspace.execute(&artifact);
            assert!(
                !output.status.success(),
                "{name} did not trap at O{optimization}"
            );
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                assert!(
                    output.status.signal().is_some(),
                    "{name} exited instead of trapping at O{optimization}: {}",
                    output.status
                );
            }
        }
    }
}

#[test]
fn emits_llvm_ir_bitcode_assembly_and_object_files() {
    let workspace = Workspace::new();
    let source =
        workspace.source("package output\npub fn add(a: i32, b: i32) -> i32 { return a + b }\n");
    for (format, extension) in [
        ("llvm-ir", "ll"),
        ("bitcode", "bc"),
        ("asm", "s"),
        ("obj", "o"),
    ] {
        let output_path = workspace.0.join(format!("output.{extension}"));
        let result = workspace
            .compiler()
            .arg("build")
            .arg(&source)
            .arg("--emit")
            .arg(format)
            .arg("-o")
            .arg(&output_path)
            .output()
            .unwrap();
        assert_success(&result, format);
        let bytes = fs::read(output_path).unwrap();
        assert!(!bytes.is_empty(), "empty {format} artifact");
        if format == "llvm-ir" {
            let ir = String::from_utf8(bytes).unwrap();
            assert!(ir.contains("dodo.output.add"));
            assert!(ir.contains("llvm.trap"));
        } else if format == "bitcode" {
            assert!(bytes.starts_with(b"BC\xc0\xde"));
        }
    }
}

#[test]
fn gpio_emits_volatile_accesses_without_host_execution() {
    let workspace = Workspace::new();
    let source = workspace.source(include_str!("../examples/gpio.dodo"));
    let path = workspace.0.join("gpio.ll");
    let output = workspace
        .compiler()
        .arg("build")
        .arg(source)
        .args(["--emit", "llvm-ir", "-O", "3", "-o"])
        .arg(&path)
        .output()
        .unwrap();
    assert_success(&output, "compile GPIO library");
    let ir = fs::read_to_string(path).unwrap();
    assert!(
        ir.contains("store volatile i32"),
        "MMIO lost volatile semantics:\n{ir}"
    );
}

#[test]
fn cross_target_ir_generation_uses_target_pointer_width() {
    let workspace = Workspace::new();
    let source =
        workspace.source("package target\npub fn width(value: usize) -> usize { return value }\n");
    let path = workspace.0.join("target.ll");
    let output = workspace
        .compiler()
        .arg("build")
        .arg(source)
        .args([
            "--emit",
            "llvm-ir",
            "--target",
            "i686-unknown-linux-gnu",
            "-o",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert_success(&output, "cross-target LLVM IR");
    let ir = fs::read_to_string(path).unwrap();
    assert!(ir.contains("target triple = \"i686-unknown-linux-gnu\""));
    assert!(ir.contains("define i32 @dodo.target.width(i32"));
}

const COUNTER_PACKAGE: &str = r#"package counter
pub struct Counter {
    pub i32 value
    i32 hidden
    pub fn increment(self: &mut Self) -> void {
        self.value += self.hidden
    }
}
pub fn make(value: i32) -> Counter {
    return Counter{value: value, hidden: 1}
}
fn secret() -> i32 { return 7 }
"#;

#[test]
fn directory_packages_import_public_types_fields_and_methods() {
    let workspace = Workspace::new();
    workspace.file("app/counter/counter.dodo", COUNTER_PACKAGE);
    workspace.file(
        "app/counter/helpers.dodo",
        "package counter\npub fn extra() -> i32 { return 1 }\n",
    );
    workspace.file(
        "app/main.dodo",
        r#"package app
import "counter"
fn main() -> i32 {
    value := counter.make(40)
    value.increment()
    return value.value + counter.extra()
}
"#,
    );
    for optimization in [0, 3] {
        let executable = workspace.build(&workspace.0.join("app"), optimization);
        assert_eq!(workspace.execute(&executable).status.code(), Some(42));
    }
}

#[test]
fn imports_reject_private_functions_fields_and_field_construction() {
    let workspace = Workspace::new();
    workspace.file("counter.dodo", COUNTER_PACKAGE);
    for body in [
        "return counter.secret()",
        "value := counter.make(40)\nreturn value.hidden",
        "value := counter.Counter{value: 40, hidden: 2}\nreturn value.value",
    ] {
        let source = workspace.source(&format!(
            "package app\nimport \"counter\"\nfn main() -> i32 {{\n{body}\n}}\n"
        ));
        let output = workspace
            .compiler()
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "private package member was accepted: {body}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("private"),
            "expected privacy diagnostic:\n{stderr}"
        );
        assert!(stderr.contains("input.dodo"));
    }
}

#[test]
fn imported_foreign_declarations_keep_their_c_symbol_name() {
    let workspace = Workspace::new();
    workspace.file(
        "console.dodo",
        r#"package console
unsafe extern "C" fn putchar(character: i32) -> i32
pub fn emit() -> void {
    // SAFETY: 88 is a valid unsigned byte for putchar.
    unsafe { putchar(88) }
}
"#,
    );
    let source =
        workspace.source("package app\nimport \"console\"\nfn main() -> void { console.emit() }\n");
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization);
        let output = workspace.execute(&executable);
        assert_success(&output, "run imported C ABI declaration");
        assert_eq!(output.stdout, b"X");
    }
}

#[test]
fn repeated_foreign_declarations_across_packages_share_a_c_symbol() {
    let workspace = Workspace::new();
    for (package, character) in [("left", 76), ("right", 82)] {
        workspace.file(
            &format!("{package}.dodo"),
            &format!(
                "package {package}\nunsafe extern \"C\" fn putchar(character: i32) -> i32\npub fn emit() -> void {{\nunsafe {{ putchar({character}) }}\n}}\n"
            ),
        );
    }
    let source = workspace.source(
        "package app\nimport \"left\"\nimport \"right\"\nfn main() -> void {\nleft.emit()\nright.emit()\n}\n",
    );
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization);
        let output = workspace.execute(&executable);
        assert_success(&output, "run shared foreign symbol across packages");
        assert_eq!(output.stdout, b"LR");
    }
}

#[test]
fn concise_declarations_receivers_and_field_initializers() {
    native_at_all_levels(
        r#"package concise
const STEP: u32 = 2
struct Counter {
    pub value: u32
    fn read(&self) -> u32 { return self.value }
    fn advance(&mut self) { self.value += STEP }
    fn finish(self) -> u32 { return self.value }
}
fn touch() {}
fn main() -> i32 {
    value := 40u32
    counter := Counter{value}
    touch()
    counter.advance()
    result: u32
    result = counter.read()
    const EXPECTED: u32 = 42
    if result != EXPECTED { return 1 }
    return counter.finish() as i32
}
"#,
        42,
        "",
    );
}

#[test]
fn inferred_arrays_and_repetition_evaluate_once_even_when_empty() {
    native_at_all_levels(
        r#"package arrays
unsafe extern "C" fn putchar(c: i32) -> i32
fn once() -> u8 { unsafe { putchar(65) }
return 7 }
struct Samples { values: [4]u16 }
fn main() -> i32 {
    samples := Samples{values: [1, 2, 3, 4]}
    repeated := [once(); 4]
    empty := [once(); 0]
    typed_empty: [0]u8 = []
    nested: [2][2]u8 = [[1, 2], [3, 4]]
    const DATA: [3]u8 = [2; 3]
    return samples.values[3] as i32 + repeated[2] as i32 + nested[1][1] as i32 + DATA[1] as i32 + empty.len as i32 + typed_empty.len as i32
}
"#,
        17,
        "AA",
    );
}

#[test]
fn constants_compose_and_define_array_types() {
    native_at_all_levels(
        r#"package constants
const TOTAL: usize = BASE + 2
const BASE: usize = 2
const DATA: [TOTAL]u8 = [7; TOTAL]
const OTHER: [TOTAL]u8 = DATA
struct Packet { bytes: [TOTAL - 1]u8 }
fn sum(values: &[TOTAL]u8) -> u8 { return values[0] + values[3] }
fn main() -> i32 {
    const LOCAL: usize = TOTAL + 1
    local: [LOCAL]u8 = [2; LOCAL]
    packet := Packet{bytes: [1, 2, 3]}
    return sum(&OTHER) as i32 + local[4] as i32 + packet.bytes[2] as i32
}
"#,
        19,
        "",
    );
}

#[test]
fn generic_arguments_infer_from_values_parameters_and_return_context() {
    native_at_all_levels(
        r#"package inference
fn identity<T>(value: T) -> T { return value }
fn first<T>(a: T, b: T) -> T { return a }
fn unwrap<T>(value: Option<T>, fallback: T) -> T {
    return match value { some(n) => n, none => fallback }
}
fn accepts(value: i32) -> i32 { return value }
struct Box<T> { value: T
fn get(&self) -> &T { return &self.value } }
fn unbox<T>(value: Box<T>) -> T {
    // Aggregates still move; use a borrow to read scalar instantiations.
    return value.value
}
fn main() -> i32 {
    a: i32 = identity(40)
    optional := some(identity(a))
    box := Box{value: unwrap(optional, 0)}
    number := first(2, 3i32)
    comparison := identity(number == 2)
    if !comparison { return 1 }
    contextual: Box<i32> = Box{value: 0}
    return accepts(identity(*box.get())) + unbox(Box{value: number}) + *contextual.get()
}
"#,
        42,
        "",
    );
}

#[test]
fn value_expressions_select_lazily_and_preserve_cleanup() {
    native_at_all_levels(
        r#"package values
unsafe extern "C" fn putchar(c: i32) -> i32
struct Token { id: i32
fn drop(&mut self) { unsafe { putchar(self.id) } } }
fn fail() -> i32!i32 { return err(1) }
fn consume(a: Token, b: Token) {}
fn attempt() -> void!i32 {
    consume(Token{id: 65}, if true {
        local := Token{id: 66}
        fail()?
        Token{id: 67}
    } else { Token{id: 68} })
    return ok()
}
fn make() -> Token {
    return if true {
        local := Token{id: 69}
        result := Token{id: 70}
        result
    } else { Token{id: 71} }
}
fn main() {
    match attempt() { ok() => {} err(_) => {} }
    token := make()
    number := match 1 { 1 => { local := Token{id: 72}
        42i32 }, _ => { local := Token{id: 73}
        0i32 } }
    byte := unsafe { putchar(number + 23) }
}
"#,
        0,
        "BAEHAF",
    );
}

#[test]
fn value_blocks_preserve_moves_and_borrowed_results() {
    native_at_all_levels(
        r#"package values
struct Box { value: i32 }
fn choose(a: &i32, b: &i32, first: bool) -> &i32 from(a, b) {
    return if first { a } else { b }
}
fn main() -> i32 {
    source := Box{value: 42}
    output := match true { true => source, false => Box{value: 0} }
    a := 40i32
    b := 2i32
    r := if true { choose(&a, &b, true) } else { &b }
    return *r + output.value - 40
}
"#,
        42,
        "",
    );
}

#[test]
fn range_bounds_are_captured_once_and_iteration_bindings_are_fresh() {
    native_at_all_levels(
        r#"package ranges
unsafe extern "C" fn putchar(c: i32) -> i32
fn start() -> u8 { unsafe { putchar(65) }
return 1 }
fn end() -> u8 { unsafe { putchar(66) }
return 5 }
fn identity<T>(v: T) -> T { return v }
fn main() -> i32 {
    total := 0i32
    for i in start()..end() {
        if i == 2 { continue }
        total += identity(i) as i32
        i = 100
    }
    limit := 3i32
    for _ in 0..limit { total += 1
        limit = 0 }
    for _ in 4..4 { return 1 }
    for _ in 5..2 { return 2 }
    for i in 254u8..255u8 { total += (i - 253) as i32 }
    for i in -2i8..1i8 { total += (i + 2) as i32 }
    for i in 0..10 { if i == 1 { break }
        total += 1 }
    return total
}
"#,
        16,
        "AB",
    );
}

#[test]
fn subslices_preserve_offsets_mutability_and_borrowed_returns() {
    native_at_all_levels(
        r#"package slices
fn tail(data: &[u8]) -> &[u8] { return &data[1..] }
fn main() -> i32 {
    data := [1u8, 2, 3, 4]
    middle := &mut data[1..3]
    middle[0] = 40
    middle[1] = 2
    nested := &middle[..1]
    first := nested[0]
    rest := tail(&data)
    empty := &data[4..4]
    all := &data[..]
    return first as i32 + rest[1] as i32 + empty.len as i32 + all.len as i32 - 4
}
"#,
        42,
        "",
    );
}

#[test]
fn new_forms_reject_invalid_types_moves_lifetimes_and_unhandled_errors() {
    for (body, message) in [
        ("fn main() { return 1 }", "expected"),
        ("fn f(&self) {}", "inside a struct"),
        ("struct S { x: i32 }\nfn main() { s := S{x} }", "unknown"),
        (
            "struct Holds { number: i32\nreference: &i32 }\nfn main() { x := 1i32\nreference := &x\nh := Holds{number: { x = 2\n0i32 }, reference} }",
            "conflict",
        ),
        ("fn main() { a := [] }", "empty array"),
        ("fn main() { a: [2]u8 = [1, 2, 3] }", "expected"),
        (
            "struct S { x: i32 }\nfn main() { a := [S{x: 1}; 2] }",
            "copyable",
        ),
        ("fn main() { x := 0\na := [&mut x; 2] }", "copyable"),
        ("const A: usize = B\nconst B: usize = A", "cyclic"),
        ("const A: u8 = 255\nconst B: u8 = A + 1", "outside"),
        ("fn main() { a: [-1]u8 }", "nonnegative"),
        ("fn main() { n := 2\na: [n]u8 }", "unknown"),
        (
            "fn make<T>() -> T { for {} }\nfn main() { x := make() }",
            "cannot infer",
        ),
        (
            "fn same<T>(a: T, b: T) {}\nfn main() { same(1u8, 2u16) }",
            "conflicting types",
        ),
        ("fn main() { x := none }", "context"),
        ("fn main() { x := if true { 1 } }", "every continuing path"),
        (
            "fn main() { x := if true { 1 } else { false } }",
            "expected",
        ),
        ("fn main() { x := match true { true => 1 } }", "exhaustive"),
        (
            "fn main() { r := if true { x := 1\n&x } else { y := 2\n&y } }",
            "local storage",
        ),
        (
            "struct S { x: i32 }\nfn main() { s := S{x: 1}\nt := match true { true => s, false => S{x: 2} }\nu := s }",
            "moved",
        ),
        (
            "fn f(a: &mut i32, b: i32) {}\nfn main() { x := 1i32\nf(&mut x, if true { x = 2\n3 } else { 4 }) }",
            "overlapping",
        ),
        (
            "fn result() -> i32!i32 { return ok(1) }\nfn main() { x := if true { r := result()\n1 } else { 0 } }",
            "Result",
        ),
        (
            "unsafe extern \"C\" fn getchar() -> i32\nfn main() { x := if true { getchar() } else { 0 } }",
            "unsafe",
        ),
        ("fn main() { for i in 0..2u8 { i = 300 } }", "out of range"),
        ("fn main() { for i in 0.0..2.0 {} }", "integers"),
        (
            "fn main() { data := [1u8, 2]\na := &data[..]\nb := &mut a[..] }",
            "shared",
        ),
        (
            "fn main() { data := [1u8, 2]\na := &mut data[..]\nb := &mut data[..]\na[0] = 2 }",
            "conflict",
        ),
        (
            "fn bad() -> &[u8] from(static) { data := [1u8, 2]\nreturn &data[..] }",
            "local",
        ),
        (
            "enum State { Ready, Done }\nfn take(s: State) {}\nfn main() { s := State.Ready\ntake(s)\ntake(s) }",
            "moved",
        ),
    ] {
        rejects(&format!("package rejected\n{body}\n"), message);
    }
}

#[test]
fn invalid_subslice_bounds_trap_at_all_optimization_levels() {
    for bounds in ["-1i8..2", "0..5", "3..2", "0..256u16"] {
        let workspace = Workspace::new();
        let source = workspace.source(&format!("package bounds\nfn main() -> i32 {{ data := [1u8, 2, 3, 4]\nview := &data[{bounds}]\nreturn view.len as i32 }}\n"));
        for optimization in [0, 3] {
            let executable = workspace.build(&source, optimization);
            assert!(
                !workspace.execute(&executable).status.success(),
                "invalid slice {bounds} did not trap at O{optimization}"
            );
        }
    }
}

#[test]
fn ergonomic_forms_work_across_packages_and_respect_constant_shadowing() {
    let workspace = Workspace::new();
    workspace.file(
        "values.dodo",
        r#"package values
pub const N: usize = 4
const SECRET: usize = 1
pub const DATA: [N]u8 = [7; N]
pub struct Box<T> { pub value: T
pub fn get(&self) -> &T { return &self.value } }
pub fn identity<T>(value: T) -> T { return value }
pub fn local() -> u8 {
    const N: usize = 2
    box := Box<[N]u8>{value: [1, 2]}
    data: [N]u8 = [3; N]
    return box.value[1] + data[1]
}
"#,
    );
    let source = workspace.source(
        r#"package app
import "values"
const M: usize = values.N + 1
fn main() -> i32 {
    box := values.Box{value: values.identity(30i32)}
    repeated: [M]u8 = [values.DATA[0]; M]
    return *box.get() + repeated[4] as i32 + values.local() as i32
}
"#,
    );
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization);
        assert_eq!(workspace.execute(&executable).status.code(), Some(42));
    }
    for body in [
        "const LEAK: usize = values.SECRET",
        "fn leak() -> usize { return values.SECRET }",
    ] {
        let source = workspace.source(&format!("package app\nimport \"values\"\n{body}\n"));
        let output = workspace
            .compiler()
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("private"));
    }
}

#[test]
fn value_expression_exits_cleanup_on_return_break_and_continue() {
    native_at_all_levels(
        r#"package exits
unsafe extern "C" fn putchar(c: i32) -> i32
struct Token { id: i32
fn drop(&mut self) { unsafe { putchar(self.id) } } }
fn consume(a: Token, b: i32) {}
fn early(flag: bool) -> i32 {
    local := Token{id: 66}
    consume(Token{id: 65}, if flag { return 42 } else { 0 })
    return 0
}
fn main() -> i32 {
    for i in 0..3 {
        guard := Token{id: 67}
        x := if i == 0 { continue } else if i == 1 { break } else { 1 }
    }
    flag := false && if true { token := Token{id: 68}
        true } else { false }
    return early(true)
}
"#,
        42,
        "CCAB",
    );
}

#[test]
fn constant_cycles_width_and_expansion_limits_are_diagnosed() {
    rejects(
        "package constants\nstatic mut N: usize = 2\nconst M: usize = N + 1\n",
        "mutable static",
    );
    let mut source = String::from("package constants\nconst C0: usize = 1\n");
    for n in 1..30 {
        source.push_str(&format!("const C{n}: usize = C{} + C{}\n", n - 1, n - 1));
    }
    rejects(&source, "expansion");
    let workspace = Workspace::new();
    let source = workspace.source(
        "package constants\nconst N: usize = 4294967295\nconst M: usize = N + 1\nfn main() {}\n",
    );
    let output = workspace
        .compiler()
        .arg("build")
        .arg(source)
        .args(["--emit", "obj", "--target", "wasm32-unknown-unknown"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside usize"));
}

#[test]
fn local_constant_expansion_is_bounded_before_cloning() {
    for nested in [false, true] {
        let mut source = String::from("package constants\nfn main() {\nconst C0: usize = 1\n");
        for n in 1..18 {
            if nested {
                source.push_str("{\n");
            }
            source.push_str(&format!("const C{n}: usize = C{} + C{}\n", n - 1, n - 1));
        }
        if nested {
            source.push_str(&"}\n".repeat(17));
        }
        source.push_str("}\n");
        rejects(
            &source,
            "constant expansion exceeds the supported size limit",
        );
    }
}

#[test]
fn slice_source_and_bounds_evaluate_once_in_order() {
    native_at_all_levels(
        r#"package slice_order
unsafe extern "C" fn putchar(c: i32) -> i32
fn source(data: &[u8]) -> &[u8] { unsafe { putchar(65) }
return data }
fn writable(data: &mut [u8]) -> &mut [u8] { return data }
fn start() -> usize { unsafe { putchar(66) }
return 1 }
fn end() -> usize { unsafe { putchar(67) }
return 3 }
fn main() -> i32 {
    data := [0u8, 40, 2, 0]
    view := &source(&data)[start()..end()]
    result := view[0] + view[1]
    whole := &mut data[0..data.len]
    whole[0] = 1
    last := &mut writable(&mut data)[3..4]
    last[0] = 2
    return result as i32
}
"#,
        42,
        "ABC",
    );
    rejects(
        "package slice_loan\nfn take(data: [2]u8) {}\nfn main() { data := [1u8, 2]\nview := &data[{ take(data)\n0 }..] }\n",
        "overlapping",
    );
    rejects(
        "package slice_loan\nfn main() { data := [1u8, 2]\nview := &data[{ data[0] = 9\n0 }..] }\n",
        "overlapping",
    );
}

#[test]
fn pattern_example_runs() {
    native_at_all_levels(include_str!("../examples/patterns.dodo"), 0, "");
}

#[test]
fn nested_patterns_drop_ignored_values_and_guard_failures_once() {
    let source = format!(
        "package cleanup\n{DROP_TYPE}\n{}",
        r#"struct Pair { first: Guard, second: Guard }
fn run(value: Option<Pair>) -> i32 {
    match value {
        some(Pair{first, ..}) if first.character == 0 => 1,
        some(Pair{first, ..}) => first.character,
        none => 0,
    }
}
fn conditional(value: Option<Option<Guard>>) {
    if let some(some(guard)) = value { core.drop(guard) }
}
fn main() -> i32 {
    let code = run(some(Pair{first: Guard{character: 65}, second: Guard{character: 66}}))
    conditional(some(some(Guard{character: 67})))
    conditional(some(none))
    let some(guard) = some(Guard{character: 68}) else { return 1 }
    core.drop(guard)
    code - 65
}
"#
    );
    native_at_all_levels(&source, 0, "BACD");
}

#[test]
fn if_let_failure_drops_unmatched_owned_payload() {
    let source = format!(
        "package cleanup\n{DROP_TYPE}\n{}",
        r#"struct Pair { first: Guard, second: Guard }
fn main() -> i32 {
    let pair = some(Pair{first: Guard{character: 65}, second: Guard{character: 66}})
    if let none = pair { return 2 }
    0
}
"#
    );
    native_at_all_levels(&source, 0, "BA");
}

#[test]
fn imported_recursive_patterns_resolve_types_bindings_and_guard_constants() {
    let workspace = Workspace::new();
    workspace.file(
        "items/lib.dodo",
        r#"package items
pub const MIN: i32 = 40
pub struct Item { pub value: Option<i32> }
pub enum Envelope { Value(Item), Empty }
pub fn make() -> Envelope { Envelope.Value(Item{value: some(42i32)}) }
"#,
    );
    let source = workspace.source(
        r#"package app
import "items"
fn main() -> i32 {
    let item = items.make()
    match item {
        items.Envelope.Value(items.Item{value: some(value)}) if value > items.MIN => value,
        _ => 0,
    }
}
"#,
    );
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization);
        assert_eq!(workspace.execute(&executable).status.code(), Some(42));
    }
}

#[test]
fn pattern_codegen_guard_return_drops_scrutinee_once() {
    let source = format!(
        "package guard_return\n{DROP_TYPE}\n{}",
        r#"fn inspect(stop: bool) -> i32 {
    match some(Guard{character: 65}) {
        some(value) if {
            if stop { return 40 }
            value.character == 65
        } => 2,
        some(_) => 0,
        none => 0,
    }
}
fn main() -> i32 { inspect(true) + inspect(false) }
"#
    );
    native_at_all_levels(&source, 42, "AA");
}

#[test]
fn pattern_codegen_guard_propagation_drops_scrutinee_once() {
    let source = format!(
        "package guard_propagation\n{DROP_TYPE}\n{}",
        r#"fn condition(value: &Guard, fail: bool) -> bool!i32 {
    if fail { err(40) } else { ok(value.character == 66) }
}
fn inspect(fail: bool) -> i32!i32 {
    match some(Guard{character: 66}) {
        some(value) if condition(&value, fail)? => ok(2),
        some(_) => ok(0),
        none => ok(0),
    }
}
fn main() -> i32 {
    let first = match inspect(true) { ok(value) => value, err(error) => error }
    let second = match inspect(false) { ok(value) => value, err(error) => error }
    first + second
}
"#
    );
    native_at_all_levels(&source, 42, "BB");
}

#[test]
fn pattern_codegen_let_else_alternatives_share_binding_storage() {
    let source = format!(
        "package let_alternatives\n{DROP_TYPE}\n{}",
        r#"struct Pair { left: Option<Guard>, right: Option<Guard> }
fn inspect(right: bool) -> i32 {
    pair := if right {
        Pair{left: none, right: some(Guard{character: 66})}
    } else {
        Pair{left: some(Guard{character: 65}), right: none}
    }
    let Pair{left: some(value), right: none}
        | Pair{left: none, right: some(value)} = pair else { return 0 }
    value.character - 44
}
fn main() -> i32 { inspect(false) + inspect(true) - 1 }
"#
    );
    native_at_all_levels(&source, 42, "AB");
}

#[test]
fn generic_patterns_and_unqualified_variants_preserve_existing_syntax() {
    native_at_all_levels(
        r#"package generic_patterns
struct Box<T> { value: T }
enum State { Ready, Done }
enum Message<T> { Value(T), Empty }
fn unbox<T>(input: Box<T>) -> T {
    let Box{value} = input
    value
}
fn classify(state: State) -> i32 {
    match state { Ready => 1, Done => 2 }
}
fn main() -> i32 {
    let value = Box<Option<i32>>{value: some(40i32)}
    let Box<Option<i32>>{value: some(number)} = value else { return 0 }
    let message = Message.Value::<Box<i32>>(Box<i32>{value: number})
    match message {
        Message<Box<i32>>.Value(Box{value}) => {
            let boxed = Box{value}
            unbox(boxed) + classify(State.Done)
        },
        Message<Box<i32>>.Empty => 0,
    }
}
"#,
        42,
        "",
    );
}

#[test]
fn patterns_respect_constant_shadowing_and_conditional_binding_scopes() {
    native_at_all_levels(
        r#"package pattern_scopes
const value: i32 = 40
fn identity<T>(input: T) -> T { input }
fn main() -> i32 {
    const value: i32 = 20
    result := 0i32
    if let some(value) = some(2i32) { result = identity(value) }
    result += value
    {
        let some(value) = some(20i32) else { return value }
        result + identity(value)
    }
}
"#,
        42,
        "",
    );
}

#[test]
fn pattern_codegen_signed_ranges_cover_the_integer_domain() {
    native_at_all_levels(
        r#"package signed_patterns
fn score(value: i8) -> i32 {
    match value { -128..=-1 => 20, 0..=127 => 22 }
}
fn main() -> i32 { score(-128i8) + score(127i8) }
"#,
        42,
        "",
    );
}

#[test]
fn pattern_codegen_recursive_reference_fields_preserve_mutability() {
    native_at_all_levels(
        r#"package reference_fields
struct Holder { item: &mut Option<i32> }
fn main() -> i32 {
    item := some(41i32)
    let holder = Holder{item: &mut item}
    if let Holder{item: some(value)} = holder { *value += 1 }
    match item { some(value) => value, none => 0 }
}
"#,
        42,
        "",
    );
}

#[test]
fn pattern_codegen_alternatives_preserve_lexical_binding_drop_order() {
    let source = format!(
        "package alternative_cleanup\n{DROP_TYPE}\n{}",
        r#"enum Pair { Values(i32, Guard, Guard) }
fn matched() -> i32 {
    match Pair.Values(2, Guard{character: 65}, Guard{character: 66}) {
        Pair.Values(1, first, second) | Pair.Values(2, second, first) => {
            first.character + second.character - 110
        },
        _ => 0,
    }
}
fn conditional() -> i32 {
    if let Pair.Values(1, first, second) | Pair.Values(2, second, first) = Pair.Values(2, Guard{character: 67}, Guard{character: 68}) {
        return first.character + second.character - 114
    }
    0
}
fn main() -> i32 { matched() + conditional() }
"#
    );
    native_at_all_levels(&source, 42, "ABCD");
}

#[test]
fn borrowed_patterns_preserve_disjoint_fields_and_explicit_result_handling() {
    native_at_all_levels(
        r#"package pattern_loans
struct Pair { left: i32, right: i32 }
fn borrowed_if() -> i32 {
    result: i32!i32 = err(42)
    total := 0i32
    if let ok(value) | err(value) = &result { total = *value }
    total
}
fn borrowed_let() -> i32 {
    result: i32!i32 = err(42)
    let ok(value) | err(value) = &result
    *value
}
fn main() -> i32 {
    pair := some(Pair{left: 0, right: 0})
    if let some(Pair{left, right}) = &mut pair {
        *left = 20
        *right = 22
    }
    let some(Pair{left, right}) = &mut pair else { return 1 }
    *left += 1
    *right -= 1
    if borrowed_if() != 42 || borrowed_let() != 42 { return 2 }
    *left + *right
}
"#,
        42,
        "",
    );
}

#[test]
fn conditional_patterns_allow_result_free_failure_paths() {
    native_at_all_levels(
        r#"package conditional_results
fn process(optional: Option<i32!i32>) -> i32 {
    total := 0i32
    if let some(result) = optional {
        total = match result { ok(value) => value, err(error) => error }
    }
    total
}
fn require(optional: Option<i32!i32>) -> i32 {
    let some(result) = optional else { return 0 }
    match result { ok(value) => value, err(error) => error }
}
fn main() -> i32 {
    if process(none) != 0 || require(none) != 0 { return 1 }
    if process(some(err(42))) != 42 { return 2 }
    require(some(ok(42)))
}
"#,
        42,
        "",
    );
}
