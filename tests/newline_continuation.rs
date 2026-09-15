//! Leading-dot chains preserve expression precedence and statement boundaries.

use dodoc::ast::{BinaryOp, Block, Expr, ExprKind, StmtKind};
use dodoc::parser::parse;
#[cfg(feature = "llvm")]
use std::fs;
#[cfg(feature = "llvm")]
use std::path::PathBuf;
#[cfg(feature = "llvm")]
use std::process::Command;
#[cfg(feature = "llvm")]
use std::sync::atomic::{AtomicU64, Ordering};

fn body(source: &str) -> Block {
    let source = format!("package test\nfn main() {{\n{source}\n}}\n");
    parse(&source)
        .unwrap_or_else(|error| panic!("{}", error.render("test.dodo", &source)))
        .functions
        .remove(0)
        .body
        .unwrap()
}

fn initializer(statements: &Block, index: usize) -> &Expr {
    let StmtKind::Let {
        value: Some(value), ..
    } = &statements[index].kind
    else {
        panic!("expected an initialized binding");
    };
    value
}

#[test]
fn leading_dot_chains_methods_and_fields_across_comments_and_blank_lines() {
    let statements = body(
        "result := source // receiver\n\
         // decoding follows\n\n\
         .decode() // first step\n\
             .validate()\n\
             .payload\n\
             .value\n\
         next := 7",
    );
    assert_eq!(statements.len(), 2);
    let ExprKind::Field(payload, field) = &initializer(&statements, 0).kind else {
        panic!("expected the final field");
    };
    assert_eq!(field, "value");
    let ExprKind::Field(validated, field) = &payload.kind else {
        panic!("expected the payload field");
    };
    assert_eq!(field, "payload");
    let ExprKind::MethodCall { receiver, name, .. } = &validated.kind else {
        panic!("expected validate method");
    };
    assert_eq!(name, "validate");
    assert!(
        matches!(&receiver.kind, ExprKind::MethodCall { receiver, name, .. }
        if name == "decode" && matches!(&receiver.kind, ExprKind::Name(name) if name == "source"))
    );
}

#[test]
fn leading_dot_keeps_postfix_precedence() {
    let statements = body("result := 1 + source\n    .value * 2\nnext := 3");
    assert_eq!(statements.len(), 2);
    let ExprKind::Binary(BinaryOp::Add, _, rhs) = &initializer(&statements, 0).kind else {
        panic!("expected addition");
    };
    assert!(matches!(&rhs.kind, ExprKind::Binary(BinaryOp::Mul, lhs, _)
        if matches!(&lhs.kind, ExprKind::Field(receiver, field)
            if field == "value" && matches!(&receiver.kind, ExprKind::Name(name) if name == "source"))));
}

#[test]
fn leading_dot_works_in_assignment_targets_conditions_and_return_values() {
    let statements = body(
        "source\n .value = 4\n\
         if source\n .valid() { return source\n .value }\n\
         for source\n .valid() { break }",
    );
    assert_eq!(statements.len(), 3);
    assert!(
        matches!(&statements[0].kind, StmtKind::Assign { target, .. }
        if matches!(&target.kind, ExprKind::Field(_, field) if field == "value"))
    );
    let StmtKind::If {
        condition,
        then_block,
        ..
    } = &statements[1].kind
    else {
        panic!("expected if statement");
    };
    assert!(matches!(&condition.kind, ExprKind::MethodCall { name, .. } if name == "valid"));
    assert!(matches!(&then_block[0].kind, StmtKind::Return(Some(value))
        if matches!(&value.kind, ExprKind::Field(_, field) if field == "value")));
    assert!(
        matches!(&statements[2].kind, StmtKind::For { condition: Some(condition), .. }
        if matches!(&condition.kind, ExprKind::MethodCall { name, .. } if name == "valid"))
    );
}

#[test]
fn ordinary_newlines_still_end_completed_expressions() {
    for next in ["next := 2", "(1)", "[1, 2]", "-1", "*other"] {
        let statements = body(&format!("value := source\n{next}"));
        assert_eq!(statements.len(), 2, "unexpected continuation: {next}");
        assert!(
            matches!(&initializer(&statements, 0).kind, ExprKind::Name(name) if name == "source")
        );
    }
}

#[test]
fn semicolons_always_stop_leading_dot_continuation() {
    for source in [
        "value := source;\n .decode()",
        "value := source\n;\n .decode()",
        "value := (source;\n .decode())",
        "value := source.\n decode()",
        ".decode()",
    ] {
        let source = format!("package test\nfn main() {{\n{source}\n}}");
        assert!(parse(&source).is_err(), "accepted invalid chain: {source}");
    }
}

#[test]
fn existing_operator_and_delimiter_continuations_still_work() {
    let statements = body("a := 1 +\n 2\nb := (1\n + 2)\nc := f(\n source\n .value,\n 4\n)");
    assert_eq!(statements.len(), 3);
    assert!(matches!(
        initializer(&statements, 0).kind,
        ExprKind::Binary(BinaryOp::Add, ..)
    ));
    assert!(matches!(
        initializer(&statements, 1).kind,
        ExprKind::Binary(BinaryOp::Add, ..)
    ));
    assert!(
        matches!(&initializer(&statements, 2).kind, ExprKind::Call { args, .. }
        if args.len() == 2 && matches!(&args[0].kind, ExprKind::Field(_, field) if field == "value"))
    );
}

#[cfg(feature = "llvm")]
struct Workspace(PathBuf);

#[cfg(feature = "llvm")]
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let next = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("dodo-newline-{}-{next}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test workspace: {error}"),
            }
        }
    }
}

#[cfg(feature = "llvm")]
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(feature = "llvm")]
#[test]
fn method_and_field_chains_execute_at_both_optimization_levels() {
    let workspace = Workspace::new();
    let source = workspace.0.join("chain.dodo");
    fs::write(
        &source,
        r#"package chain
struct Source {
    value: i32
    fn decode(self) -> Self { return Self{value: self.value + 2} }
    fn validate(self) -> Self { return Self{value: self.value * 3} }
}
fn main() -> i32 {
    source := Source{value: 4}
    result := source // a chain can include comments
        .decode()

        // and blank lines
        .validate()
    result
        .value += 1
    return result
        .value - 19
}
"#,
    )
    .unwrap();
    for optimization in ["0", "3"] {
        let artifact = workspace.0.join(format!("chain-O{optimization}"));
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
