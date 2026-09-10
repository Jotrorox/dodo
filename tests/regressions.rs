//! Regressions found by independently reviewing native ownership and ABI lowering.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        loop {
            let path = std::env::temp_dir().join(format!(
                "dodo-regression-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => panic!("create test directory: {error}"),
            }
        }
    }
    fn file(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).expect("write regression source");
        path
    }
    fn build(&self, source: &Path, optimization: u8, link: Option<&Path>) -> PathBuf {
        let executable = self.0.join(format!("program-O{optimization}"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_dodo"));
        command
            .arg("build")
            .arg(source)
            .arg("-o")
            .arg(&executable)
            .arg("-O")
            .arg(optimization.to_string());
        if let Some(link) = link {
            command.arg("--link-arg").arg(link);
        }
        let output = command.output().expect("invoke compiler");
        assert!(
            output.status.success(),
            "compile failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        executable
    }
    fn run(&self, executable: &Path) -> Output {
        Command::new(executable)
            .output()
            .expect("run regression program")
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const DROPPING_TOKEN: &str = r#"
unsafe extern "C" fn putchar(value: i32) -> i32
struct Token {
    i32 id
    fn drop(self: &mut Self) -> void { unsafe { putchar(self.id) } }
}
fn fail() -> i32!i32 { return err(1) }
fn fail_token() -> Token!i32 { return err(1) }
fn take(token: Token, number: i32) -> void {}
"#;

fn propagation_cleanup(body: &str, extra: &str, expected: &[u8]) {
    let workspace = Workspace::new();
    let source = workspace.file("main.dodo", &format!("package regression\n{DROPPING_TOKEN}\n{extra}\nfn attempt() -> void!i32 {{\n{body}\nreturn ok()\n}}\nfn main() -> void {{ match attempt() {{ ok() => {{}} err(_) => {{}} }} }}\n"));
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization, None);
        let output = workspace.run(&executable);
        assert!(
            output.status.success(),
            "regression program failed: {}",
            output.status
        );
        assert_eq!(
            output.stdout, expected,
            "owned values evaluated before `?` must be destroyed at O{optimization}"
        );
    }
}

#[test]
fn propagation_drops_previously_evaluated_call_arguments() {
    propagation_cleanup("take(Token{id: 65}, fail()?)", "", b"A");
}
#[test]
fn propagation_drops_moved_call_arguments_exactly_once() {
    propagation_cleanup("token := Token{id: 65}\ntake(token, fail()?)", "", b"A");
}
#[test]
fn propagation_drops_initialized_struct_fields() {
    propagation_cleanup(
        "pair := Pair{token: Token{id: 65}, number: fail()?}",
        "struct Pair { Token token\n i32 number }",
        b"A",
    );
}
#[test]
fn propagation_drops_initialized_array_elements() {
    propagation_cleanup("values := [2]Token{Token{id: 65}, fail_token()?}", "", b"A");
}
#[test]
fn propagation_drops_initialized_enum_payloads() {
    propagation_cleanup(
        "pair := Pair.Both(Token{id: 65}, fail()?)",
        "enum Pair { Both(Token token, i32 number) }",
        b"A",
    );
}
#[test]
fn negative_narrow_indices_trap_instead_of_wrapping_into_the_array() {
    let workspace = Workspace::new();
    let mut values = vec!["0"; 256];
    values[255] = "42";
    let source = workspace.file("main.dodo", &format!("package regression\nfn main() -> i32 {{\ndata := [256]u8{{{}}}\nindex := -1i8\nreturn data[index] as i32\n}}\n", values.join(",")));
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization, None);
        let output = workspace.run(&executable);
        assert!(
            !output.status.success(),
            "negative indexing did not trap at O{optimization}"
        );
        assert_ne!(
            output.status.code(),
            Some(42),
            "negative index wrapped to element 255 at O{optimization}"
        );
    }
}
#[test]
fn imported_constants_and_plain_enum_values_resolve() {
    let workspace = Workspace::new();
    workspace.file(
        "items.dodo",
        "package items\npub const i32 VALUE = 42\npub enum State { Ready, Done }\n",
    );
    let source = workspace.file("main.dodo", "package app\nimport \"items\"\nfn main() -> i32 {\nvalue := items.State.Ready\nmatch value { items.State.Ready => { return items.VALUE } items.State.Done => { return 0 } }\n}\n");
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization, None);
        assert_eq!(workspace.run(&executable).status.code(), Some(42));
    }
}
#[test]
fn foreign_narrow_integer_arguments_follow_the_c_abi() {
    let workspace = Workspace::new();
    // Rust's extern C lowering supplies a second compiler's ABI implementation.
    // At optimization, SysV callees may use the required sign-extended register
    // directly; a C compiler that redundantly extends it can hide this bug.
    let foreign = workspace.file("foreign.rs", "#[no_mangle] pub extern \"C\" fn signed_byte(value: i8) -> i32 { value as i32 }\n#[no_mangle] pub extern \"C\" fn signed_short(value: i16) -> i32 { value as i32 }\n#[no_mangle] pub extern \"C\" fn unsigned_byte(value: u8) -> i32 { value as i32 }\n");
    let object = workspace.0.join("foreign.o");
    let output = Command::new("rustc")
        .arg("--edition=2021")
        .arg("--crate-type=lib")
        .arg("--emit=obj")
        .arg("-Copt-level=3")
        .arg(&foreign)
        .arg("-o")
        .arg(&object)
        .output()
        .expect("compile independent C ABI fixture");
    assert!(
        output.status.success(),
        "foreign fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let source = workspace.file("main.dodo", "package regression\nunsafe extern \"C\" fn signed_byte(value: i8) -> i32\nunsafe extern \"C\" fn signed_short(value: i16) -> i32\nunsafe extern \"C\" fn unsigned_byte(value: u8) -> i32\nfn main() -> i32 {\nunsafe {\nif signed_byte(-1i8) != -1 { return 41 }\nif signed_short(-32768i16) != -32768 { return 42 }\nif unsigned_byte(255u8) != 255 { return 43 }\n}\nreturn 0\n}\n");
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization, Some(&object));
        assert_eq!(
            workspace.run(&executable).status.code(),
            Some(0),
            "narrow C ABI argument extension failed at O{optimization}"
        );
    }
}

#[test]
fn conditional_owned_arguments_only_drop_when_evaluated() {
    let workspace = Workspace::new();
    let source = workspace.file("main.dodo", &format!("package regression\n{DROPPING_TOKEN}\nfn consume(token: Token) -> bool {{ return true }}\nfn conjunction(flag: bool) -> bool {{ return flag && consume(Token{{id: 65}}) }}\nfn disjunction(flag: bool) -> bool {{ return flag || consume(Token{{id: 66}}) }}\nfn main() -> void {{\nfor i := 0; i < 5; i += 1 {{\nconjunction(false)\nconjunction(true)\ndisjunction(true)\ndisjunction(false)\n}}\n}}\n"));
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization, None);
        let output = workspace.run(&executable);
        assert!(output.status.success());
        assert_eq!(
            output.stdout, b"ABABABABAB",
            "conditional ownership cleanup at O{optimization}"
        );
    }
}

#[test]
fn returning_a_void_call_preserves_effects_and_cleanup() {
    let workspace = Workspace::new();
    let source = workspace.file("main.dodo", &format!("package regression\n{DROPPING_TOKEN}\nfn effect() -> void {{ unsafe {{ putchar(65) }} }}\nfn main() -> void {{ token := Token{{id: 66}}\nreturn effect() }}\n"));
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization, None);
        let output = workspace.run(&executable);
        assert!(output.status.success());
        assert_eq!(
            output.stdout, b"AB",
            "evaluate the returned call before scope cleanup at O{optimization}"
        );
    }
}

#[test]
fn global_literal_aggregates_strings_and_computed_scalars() {
    let workspace = Workspace::new();
    let source = workspace.file(
        "main.dodo",
        r#"package globals
const &[u8] TEXT = b"abc"
const i32 NUMBER = (6 * 7) - 3
const [2]u8 ARRAY = [2]u8{0, 0}
fn main() -> i32 {
    return NUMBER + TEXT.len as i32 + ARRAY[0] as i32
}
"#,
    );
    for optimization in [0, 3] {
        let executable = workspace.build(&source, optimization, None);
        assert_eq!(workspace.run(&executable).status.code(), Some(42));
    }
}
