//! Explicit bracket-list types preserve legacy array type and length checks.

use dodoc::ast::{ExprKind, StmtKind, Type};
use dodoc::{parser, sema};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const ARRAYS: &str = r#"package arrays
const N: usize = 2
const TABLE: [N][2]u16 = ([
    ([1, 2]: [2]u16),
    ([3, 4]: [2]u16),
]: [N][2]u16)
fn main() -> i32 {
    const ROWS: usize = 2
    matrix := ([[5, 6], [7, 8]]: [ROWS][2]u16)
    empty := ([]: [0]u8)
    return TABLE[1][0] as i32 + matrix[0][1] as i32 + empty.len as i32 - 9
}
"#;

#[test]
fn annotated_bracket_lists_retain_explicit_types() {
    let program = parser::parse(
        "package arrays\nfn main() {\nsmall := ([1, 2]: [2]u16)\nempty := ([]: [0]u8)\n}",
    )
    .unwrap();
    let body = program.functions[0].body.as_ref().unwrap();
    for (statement, expected, length) in [
        (
            &body[0],
            Type::Array(
                2,
                Box::new(Type::Int {
                    signed: false,
                    bits: 16,
                }),
            ),
            2,
        ),
        (&body[1], Type::Array(0, Box::new(Type::u8())), 0),
    ] {
        let StmtKind::Let {
            value: Some(value), ..
        } = &statement.kind
        else {
            panic!("expected initialized binding");
        };
        let ExprKind::Array(ty, items) = &value.kind else {
            panic!("expected typed array literal");
        };
        assert_eq!(ty, &expected);
        assert_eq!(items.len(), length);
    }
}

#[test]
fn annotations_support_nested_arrays_and_local_and_global_constant_lengths() {
    let mut program = parser::parse(ARRAYS).unwrap();
    sema::check(&mut program)
        .unwrap_or_else(|error| panic!("{}", error.render("arrays.dodo", ARRAYS)));
}

#[test]
fn annotations_preserve_array_length_and_element_constraints() {
    for (source, message) in [
        ("value := ([1]: [2]u8)", "array literal needs 2 elements"),
        ("value := ([]: [1]u8)", "array literal needs 1 elements"),
        ("value := ([256]: [1]u8)", "out of range for `u8`"),
        (
            "const N: usize = 2\nvalue := ([1, 2]: [N + 1]u8)",
            "array literal needs 3 elements",
        ),
        (
            "value: [1]u16 = ([1]: [1]u8)",
            "expected `[1]u16`, found `[1]u8`",
        ),
    ] {
        let source = format!("package arrays\nfn main() {{\n{source}\n}}");
        let mut program = parser::parse(&source).unwrap();
        let error = sema::check(&mut program).unwrap_err();
        assert!(
            error.message.contains(message),
            "expected {message:?}: {}",
            error.render("arrays.dodo", &source)
        );
    }
}

#[test]
fn annotations_reject_other_expressions_and_cannot_replace_existing_constraints() {
    for (expression, message) in [
        ("([1]: u8)", "requires a fixed-size array type"),
        (
            "(value: [1]u8)",
            "expression type annotations require a bracket array literal",
        ),
        (
            "([1; 2]: [3]u8)",
            "array repetition annotations are not supported",
        ),
        (
            "(([1]: [1]u8): [2]u16)",
            "array literal already has an explicit type",
        ),
        (
            "([1]u8{1}: [2]u16)",
            "array literal already has an explicit type",
        ),
    ] {
        let source = format!("package arrays\nfn main() {{ result := {expression} }}");
        let error = parser::parse(&source).unwrap_err();
        assert!(
            error.message.contains(message),
            "expected {message:?}: {}",
            error.render("arrays.dodo", &source)
        );
    }
}

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let next = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "dodo-array-annotation-{}-{next}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test workspace: {error}"),
            }
        }
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn annotated_arrays_execute_at_both_optimization_levels() {
    let workspace = Workspace::new();
    let source = workspace.0.join("arrays.dodo");
    fs::write(&source, ARRAYS).unwrap();
    for optimization in ["0", "3"] {
        let artifact = workspace.0.join(format!("arrays-O{optimization}"));
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .arg("-O")
            .arg(optimization)
            .arg("-o")
            .arg(&artifact)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "build at O{optimization}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(&artifact).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "execute at O{optimization}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
