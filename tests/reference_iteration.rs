//! Reference-pattern syntax, ownership checks, and native loop behavior.

use dodoc::ast::{ExprKind, StmtKind, Type};
use dodoc::{parser, sema};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn program(source: &str) -> dodoc::ast::Program {
    parser::parse(&format!("package iteration\n{source}\n")).expect("parse fixture")
}

fn accepts(source: &str) {
    sema::check(&mut program(source)).expect("check valid reference iteration");
}

fn rejects(source: &str, expected: &str) {
    let error = sema::check(&mut program(source)).expect_err("invalid loop was accepted");
    assert!(
        error.message.contains(expected),
        "expected {expected:?}, got {error:?}"
    );
}

#[test]
fn parser_distinguishes_reference_and_copy_patterns() {
    let parsed = program(
        "fn f() {\nfor value in values {}\nfor &value in values {}\nfor i, &value in values {}\nfor i, value in &mut values {}\n}",
    );
    let body = parsed.functions[0].body.as_ref().unwrap();
    assert!(matches!(
        body[0].kind,
        StmtKind::ForEach { copy: false, .. }
    ));
    assert!(matches!(body[1].kind, StmtKind::ForEach { copy: true, .. }));
    assert!(matches!(
        &body[2].kind,
        StmtKind::ForEach { index: Some(index), name, copy: true, .. }
            if index == "i" && name == "value"
    ));
    assert!(matches!(
        &body[3].kind,
        StmtKind::ForEach { copy: false, iterable, .. }
            if matches!(iterable.kind, ExprKind::Unary(..))
    ));
}

#[test]
fn invalid_reference_pattern_positions_have_specific_errors() {
    for (pattern, expected) in [
        ("&i, value", "loop indices are values"),
        ("&mut value", "`&mut` loop patterns are unsupported"),
        ("i, &mut value", "`&mut` loop patterns are unsupported"),
    ] {
        let error = parser::parse(&format!(
            "package iteration\nfn f() {{ for {pattern} in values {{}} }}\n"
        ))
        .expect_err("invalid pattern was accepted");
        assert!(error.message.contains(expected), "{error:?}");
    }
}

#[test]
fn copy_bindings_have_element_types_and_support_generic_inference() {
    let mut parsed = program(
        "fn identity<T>(value: T) -> T { return value }\nfn f(values: &[i32]) -> i32 {\nfor &value in values { return identity(value) }\nreturn 0\n}",
    );
    sema::check(&mut parsed).unwrap();
    let function = parsed.functions.iter().find(|f| f.name == "f").unwrap();
    let StmtKind::ForEach { body, .. } = &function.body.as_ref().unwrap()[0].kind else {
        panic!("expected foreach");
    };
    let StmtKind::Return(Some(expression)) = &body[0].kind else {
        panic!("expected return");
    };
    let ExprKind::Call { args, .. } = &expression.kind else {
        panic!("expected generic call");
    };
    assert_eq!(
        args[0].ty,
        Type::Int {
            signed: true,
            bits: 32
        }
    );
    accepts(
        "fn first<T>(values: &[T], fallback: T) -> T {\nfor &value in values { return value }\nreturn fallback\n}\nfn f() -> i32 {\nvalues := [3i32, 4]\nreturn first(&values, 0)\n}",
    );
}

#[test]
fn aggregates_and_mutable_references_cannot_be_copied() {
    for source in [
        "struct Item { value: i32 }\nfn f() {\nvalues := [Item{value: 1}]\nfor &value in values {}\n}",
        "fn f() {\nvalues := [[1i32, 2]]\nfor &value in values {}\n}",
        "fn f() {\nx := 1\nvalues := [&mut x]\nfor &value in values {}\n}",
        "fn f() {\ndata := [1i32, 2]\nvalues := [&mut data[..]]\nfor &value in values {}\n}",
        "fn f() {\nvalues: [1]Option<i32> = [some(1)]\nfor &value in values {}\n}",
    ] {
        rejects(source, "requires a copyable element");
    }
}

#[test]
fn ranges_and_mutable_iteration_reject_shared_copy_patterns() {
    rejects(
        "fn f() { for &value in 0..3 {} }",
        "range loops yield integer values",
    );
    for source in [
        "fn f() {\nvalues := [1, 2]\nfor &value in &mut values {}\n}",
        "fn f(values: &mut [i32]) { for &value in values {} }",
        "fn f(values: &mut [2]i32) { for &value in values {} }",
    ] {
        rejects(source, "requires shared iteration");
    }
    accepts("fn f(values: &mut [i32]) { for &value in values[..] { _ = value } }");
    accepts("fn f(values: &mut [2]i32) { for &value in &*values { _ = value } }");
}

#[test]
fn copied_values_keep_the_collection_borrowed_across_iterations() {
    rejects(
        "fn f() {\nvalues := [1, 2]\nfor &value in values {\nvalue = 0\nvalues[0] = value\n}\n}",
        "live shared borrow",
    );
    rejects(
        "fn take(values: [2]isize) {}\nfn f() {\nvalues := [1, 2]\nfor &value in values { take(values) }\n}",
        "live shared borrow",
    );
    rejects(
        "fn f(values: &mut [i32]) {\nfor &value in values[..] { values[0] = value }\n}",
        "live shared borrow",
    );
    rejects(
        "fn f() {\nvalues := [1, 2]\nfor &_ in values { values[0] = 0 }\n}",
        "live shared borrow",
    );
}

#[test]
fn copied_references_preserve_payload_borrows_after_the_loop() {
    for iterable in [
        "references",
        "&references",
        "references[..]",
        "view(&references)",
    ] {
        rejects(
            &format!(
                "fn view(values: &[&i32]) -> &[&i32] from(values) {{ return values }}\nfn f() -> i32 {{\ninitial := 0i32\nresult := &initial\nx := 1i32\nreferences := [&x]\nfor &reference in {iterable} {{ result = reference }}\nx = 2\nreturn *result\n}}"
            ),
            "live shared borrow",
        );
        rejects(
            &format!(
                "fn view(values: &[&i32]) -> &[&i32] from(values) {{ return values }}\nfn f() -> i32 {{\ninitial := 0i32\nresult := &initial\n{{\nx := 1i32\nreferences := [&x]\nfor &reference in {iterable} {{ result = reference }}\n}}\nreturn *result\n}}"
            ),
            "outlives its source",
        );
    }
    // The copied pointer does not borrow the array which held it.
    accepts(
        "fn f(source: &i32) -> &i32 from(source) {\nreferences := [source]\nfor &reference in &references { return reference }\nreturn source\n}",
    );
    accepts(
        "fn f(sources: &[&i32], fallback: &i32) -> &i32 from(sources, fallback) {\nfor &source in sources { return source }\nreturn fallback\n}",
    );
    accepts(
        "fn f() -> i32 {\nresult := 0i32\n{\nvalues := [1i32, 2]\nfor &value in values { result = value }\n}\nreturn result\n}",
    );
}

#[test]
fn reassigning_copied_references_keeps_collection_and_new_source_loans() {
    rejects(
        "fn f() {\nx := 1i32\ny := 2i32\nreferences := [&x]\nfor &reference in references {\nreference = &y\nx = 3\n_ = *reference\n}\n}",
        "live shared borrow",
    );
    rejects(
        "fn f() -> i32 {\nx := 1i32\ny := 2i32\nresult := &x\nreferences := [&x]\nfor &reference in references {\nreference = &y\nresult = reference\n}\ny = 3\nreturn *result\n}",
        "live shared borrow",
    );
    accepts(
        "fn f() -> i32 {\nx := 1i32\ny := 2i32\nresult := &x\nreferences := [&x]\nfor &reference in references {\nreference = &y\nresult = reference\n}\nreturn *result\n}",
    );
}

#[test]
fn temporary_collection_expressions_keep_transitive_payload_loans() {
    for iterable in ["view(&references)", "{ view(&references) }"] {
        rejects(
            &format!(
                "fn view(values: &[&i32]) -> &[&i32] from(values) {{ return values }}\nfn f() {{\nx := 1i32\ny := 2i32\nreferences := [&x]\nfor &reference in {iterable} {{\nreference = &y\nx = 3\n}}\n}}"
            ),
            "live shared borrow",
        );
    }
    accepts(
        "fn view(values: &[&i32]) -> &[&i32] from(values) { return values }\nfn f(sources: &[&i32], fallback: &i32) -> &i32 from(sources, fallback) {\nfor &source in view(sources) { return source }\nreturn fallback\n}",
    );
}

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "dodo-reference-iteration-{}-{id}",
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
fn native_copy_iteration_preserves_values_control_flow_and_reference_iteration() {
    let workspace = Workspace::new();
    let source = workspace.0.join("input.dodo");
    fs::write(
        &source,
        r#"package iteration
fn identity<T>(value: T) -> T { return value }
fn sum(values: &[i32]) -> i32 {
    total := 0i32
    for &value in values { total += identity(value) }
    return total
}
fn elements(calls: &mut i32, values: &[i32]) -> &[i32] from(values) {
    *calls += 1
    return values
}
fn main() -> i32 {
    values := [1i32, 2, 3]
    total := 0i32
    for value in values { total += *value }
    if total != 6 { return 1 }
    for value in &mut values { *value += 1 }
    total = 0
    for i, &value in values {
        total += (i as i32 + 1) * value
        value = 99
    }
    if total != 20 || values[0] != 2 { return 2 }
    if sum(&values) != 9 { return 3 }
    total = 0
    for &value in values[1..] { total += value }
    if total != 7 { return 4 }
    for &value in &values {
        if value == 2 { continue }
        total += value
        break
    }
    if total != 10 { return 5 }
    empty: [0]i32 = []
    for &value in empty { return 6 }
    flags := [true, false, true]
    for &flag in flags { if flag { total += 1 } }
    if total != 12 { return 7 }
    floats := [1.5, 2.5]
    decimal := 0.0
    for &value in floats { decimal += value }
    if decimal != 4.0 { return 8 }
    a := 4i32
    b := 5i32
    references := [&a, &b]
    total = 0
    for &reference in references { total += *reference }
    if total != 9 { return 9 }
    words := ["hi", "dodo"]
    length := 0usize
    for &word in words { length += word.len }
    if length != 6 { return 10 }
    views := [values[..1], values[1..]]
    total = 0
    for &view in views { total += sum(view) }
    if total != 9 { return 11 }
    calls := 0i32
    total = 0
    for &value in elements(&mut calls, &values) { total += value }
    if calls != 1 || total != 9 { return 12 }
    total = 0
    for &_ in values { total += 1 }
    if total != 3 { return 13 }
    return 0
}
"#,
    )
    .unwrap();
    for optimization in ["0", "3"] {
        let executable = workspace.0.join(format!("program-O{optimization}"));
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .args(["build", "-O", optimization])
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "compile O{optimization}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(executable).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "native behavior failed at O{optimization}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
