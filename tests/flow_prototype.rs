//! Architectural experiment only. An empty initialization issue list is not a
//! verdict on type safety, borrow safety, Result handling, or native cleanup.
#[path = "support/flow/mod.rs"]
mod flow;

use dodoc::{parser, sema};
use flow::{Operation, Terminator};

fn compare(source: &str, production_error: Option<&str>, uninitialized: &[&str]) -> flow::Body {
    let program = parser::parse(&format!("package experiment\n{source}")).unwrap();
    let mut checked = program.clone();
    match (sema::check(&mut checked), production_error) {
        (Ok(()), None) => (),
        (Err(error), Some(expected)) => assert!(error.message.contains(expected), "{error:?}"),
        (actual, expected) => {
            panic!("production mismatch: {actual:?}, expected {expected:?}\n{source}")
        }
    }
    // Use the original parse, not the partly annotated tree left by a failed check.
    let body = flow::lower(&program, "f").unwrap_or_else(|error| panic!("{error:?}\n{source}"));
    let result = body.initialization();
    let names: Vec<_> = result
        .issues
        .iter()
        .map(|issue| issue.name.as_str())
        .collect();
    assert_eq!(names, uninitialized, "{source}\n{body:#?}");
    body
}

#[test]
fn existing_definite_initialization_regressions_agree() {
    // Exact bodies from sema::tests::{definite_initialization_branches,
    // early_return_initialization}; no dependency on a successful production check.
    compare(
        "fn f(b: bool) -> u8 {\n u8 x\n if b { x = 1 } else { x = 2 }\n return x\n}",
        None,
        &[],
    );
    compare(
        "fn f(b: bool) -> u8 {\n u8 x\n if b { x = 1 }\n return x\n}",
        Some("uninitialized"),
        &["x"],
    );
    compare(
        "fn f(b: bool) -> u8 {\n u8 x\n if b { return 0 } else { x = 1 }\n return x\n}",
        None,
        &[],
    );
}

#[test]
fn moves_reinitialization_and_branch_exits_agree() {
    let prefix = "struct S { u8 n }\nfn take(s: S) {}\nfn fresh() -> S { return S{n: 1} }\n";
    for (body, error, issues) in [
        ("fn f(s: S) { take(s)\ntake(s) }", Some("moved"), vec!["s"]),
        ("fn f(s: S, t: S) { take(s)\ns = t\ntake(s) }", None, vec![]),
        (
            "fn f(s: S, b: bool) { if b { take(s) }\ntake(s) }",
            Some("moved"),
            vec!["s"],
        ),
        (
            "fn f(s: S, b: bool) { if b { take(s)\nreturn }\ntake(s) }",
            None,
            vec![],
        ),
        (
            "fn f(s: S, b: bool) { for b { take(s) } }",
            Some("moved in a loop"),
            vec!["s"],
        ),
        (
            "fn f(s: S, b: bool) { for b { take(s)\ns = fresh() } }",
            None,
            vec![],
        ),
    ] {
        compare(&format!("{prefix}{body}"), error, &issues);
    }
}

#[test]
fn conditional_continue_preserves_moves_on_the_back_edge() {
    // The terminating then-arm reaches the loop header. With b=true, the
    // second iteration would consume the already moved s.
    compare(
        "struct S { u8 n }\nfn take(s: S) {}\nfn f(s: S, b: bool) { for b { if b { take(s)\ncontinue } } }",
        Some("moved in a loop"),
        &["s"],
    );
}

#[test]
fn conditional_break_preserves_moves_on_the_loop_exit() {
    // With b=true this terminates after consuming s twice. The outer use must
    // observe the moved state on the break edge, not the state before the loop.
    compare(
        "struct S { u8 n }\nfn take(s: S) {}\nfn f(s: S, b: bool) { for { if b { take(s)\nbreak } }\ntake(s) }",
        Some("moved"),
        &["s"],
    );
}

#[test]
fn call_arguments_move_in_evaluation_order() {
    compare(
        "struct S { u8 n }\nfn take(a: S, b: S) {}\nfn f(s: S) { take(s, s) }",
        Some("moved"),
        &["s"],
    );
}

#[test]
fn loop_exit_initialization_is_more_precise() {
    for body in [
        "fn f() -> u8 { u8 x\nfor { x = 1\nbreak }\nreturn x }",
        "fn f(b: bool) -> u8 { u8 x\nfor { if b { x = 1\nbreak } else { x = 2\nbreak } }\nreturn x }",
        "fn f(b: bool) -> u8 { u8 x\nfor { if b { continue }\nx = 1\nbreak }\nreturn x }",
    ] {
        compare(body, Some("uninitialized"), &[]);
    }
    // A zero-iteration path and a break that skips assignment must still fail.
    compare(
        "fn f(b: bool) -> u8 { u8 x\nfor b { x = 1\nbreak }\nreturn x }",
        Some("uninitialized"),
        &["x"],
    );
    compare(
        "fn f(b: bool) -> u8 { u8 x\nfor { if b { break }\nx = 1\nbreak }\nreturn x }",
        Some("uninitialized"),
        &["x"],
    );
}

#[test]
fn moves_on_exiting_loop_paths_do_not_reach_a_back_edge() {
    for exit in ["break", "return"] {
        compare(
            &format!(
                "struct S {{ u8 n }}\nfn take(s: S) {{}}\nfn f(s: S) {{ for {{ take(s)\n{exit} }} }}"
            ),
            None,
            &[],
        );
    }
    // The inner break must not be mistaken for an exit from the outer loop.
    compare(
        "struct S { u8 n }\nfn take(s: S) {}\nfn f(s: S) { for { for { take(s)\nbreak } } }",
        Some("moved in a loop"),
        &["s"],
    );
}

#[test]
fn stable_places_distinguish_shadowing_and_repeated_scopes() {
    compare(
        "fn f() -> u8 { u8 x\n{ x := 1u8\n_ = x }\nreturn x }",
        Some("uninitialized"),
        &["x"],
    );
    compare(
        "fn f(b: bool) { for b { u8 x\nif b { x = 1 }\n_ = x } }",
        Some("uninitialized"),
        &["x"],
    );
    compare(
        "fn f(b: bool) { x := 1u8\nfor b { { x := 2u8\n_ = x }\n_ = x } }",
        None,
        &[],
    );
}

#[test]
fn borrow_initialization_is_checked_without_claiming_loan_safety() {
    compare(
        "fn observe(x: &u8) {}\nfn f() { u8 x\nobserve(&x) }",
        Some("uninitialized"),
        &["x"],
    );
    let body = compare(
        "fn observe(x: &u8) {}\nfn f() { x := 1u8\nobserve(&x) }",
        None,
        &[],
    );
    assert!(
        body.blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .any(|i| matches!(i.operation, Operation::Borrow { mutable: false, .. }))
    );
    compare(
        "fn observe(x: &mut u8) {}\nfn f() { x := 1u8\nobserve(&mut x) }",
        None,
        &[],
    );
    // This known invalid program has no initialization error. Keep the gap
    // executable so nobody mistakes the experiment for a replacement checker.
    compare(
        "fn observe(x: &u8) {}\nfn f() { x := 1u8\nr := &x\nx = 2\nobserve(r) }",
        Some("live shared borrow"),
        &[],
    );
}

#[test]
fn cleanup_is_conditional_and_return_temporaries_survive_it() {
    let body = compare("struct S { u8 n }\nfn f(s: S) -> S { return s }", None, &[]);
    let block = &body.blocks[0];
    let Terminator::Return(Some(flow::Operand::Move(returned))) = block.terminator else {
        panic!("{block:?}")
    };
    assert!(
        !block
            .instructions
            .iter()
            .any(|i| matches!(i.operation, Operation::Cleanup(p) if p == returned))
    );
    assert!(
        block
            .instructions
            .iter()
            .any(|i| matches!(i.operation, Operation::Cleanup(flow::Place(0))))
    );
    compare(
        "struct S { u8 n }\nfn take(s: S) {}\nfn f(s: S, b: bool) { if b { take(s) } }",
        None,
        &[],
    );
    compare("fn f() { u8 unused }", None, &[]);
}

#[test]
fn unreachable_blocks_do_not_contribute_initialization_facts() {
    let body = compare("fn f() { for { continue } }", None, &[]);
    assert!(body.initialization().incoming.iter().any(Option::is_none));
}

#[test]
fn issues_keep_use_and_declaration_spans() {
    let source = "package experiment\nfn f() -> u8 { u8 x\nreturn x }";
    let program = parser::parse(source).unwrap();
    let body = flow::lower(&program, "f").unwrap();
    let result = body.initialization();
    assert_eq!(result.issues.len(), 1);
    let issue = &result.issues[0];
    assert_eq!(&source[issue.span.start..issue.span.end], "x");
    assert!(source[issue.declaration.start..issue.declaration.end].contains("u8 x"));
}

#[test]
fn unsupported_and_ill_typed_inputs_never_produce_analysis_success() {
    for (source, reason) in [
        (
            "fn f() { values := [2]u8{1,2} }",
            "expression outside subset",
        ),
        ("fn f(values: &[u8]) {}", "type is outside"),
        ("fn f() { for x in 0..2 {} }", "statement outside subset"),
        (
            "fn f() { for i := 0; true; i += 1 {} }",
            "statement outside subset",
        ),
        (
            "fn f(x: bool) { match x { true => {} false => {} } }",
            "statement outside subset",
        ),
        ("fn f() { unsafe {} }", "statement outside subset"),
        ("fn f<T>(x: T) {}", "generics"),
        (
            "struct S { u8 n\nfn drop(&mut self) {} }\nfn f() {}",
            "destructors",
        ),
        ("struct S { &u8 n }\nfn f() {}", "scalar fields"),
        ("fn f(x: &u8) -> &u8 { return x }", "borrowed returns"),
        (
            "fn f() { x := 1u8\n_ = x as u32 }",
            "expression outside subset",
        ),
        (
            "fn f() { x := 1u8\n_ = &x\n_ = x + 1 }",
            "expression outside subset",
        ),
        (
            "fn f() { x := 1u8\n_ = x.missing }",
            "expression outside subset",
        ),
        ("fn f() { x := 1u8\nif x {} }", "type mismatch"),
        ("fn f() { u8 x = true }", "type mismatch"),
        ("fn f() { x := 256u8 }", "range"),
        ("fn take(x: u8) {}\nfn f() { take() }", "arity"),
        (
            "fn take(x: &mut u8) {}\nfn f(x: &mut u8) { take(x) }",
            "reborrow",
        ),
        ("fn f() { return\n_ = missing }", "syntax after"),
        (
            "fn f() { if false { _ = missing } }",
            "resolved whole local",
        ),
        ("fn f() { core.assert(true) }", "expression outside subset"),
    ] {
        let program = parser::parse(&format!("package experiment\n{source}"))
            .unwrap_or_else(|error| panic!("{error:?}\n{source}"));
        let error = flow::lower(&program, "f").expect_err(source);
        assert!(error.reason.contains(reason), "{error:?}\n{source}");
        assert!(error.span.start <= error.span.end);
    }
}

#[test]
fn existing_ownership_examples_are_explicitly_outside_the_subset() {
    for source in [
        include_str!("../examples/borrowing.dodo"),
        include_str!("../examples/diagnostics/moved_value.dodo"),
        include_str!("../examples/diagnostics/return_source.dodo"),
        include_str!("stdlib/shared_storage.dodo"),
    ] {
        let program = parser::parse(source).unwrap();
        assert!(
            flow::lower(&program, "main").is_err(),
            "unexpectedly modeled full fixture: {source}"
        );
    }
}

#[test]
fn existing_borrow_diagnostic_fixture_has_only_initialization_coverage() {
    let source = include_str!("../examples/diagnostics/borrow_conflict.dodo");
    let program = parser::parse(source).unwrap();
    assert!(
        sema::check(&mut program.clone())
            .unwrap_err()
            .message
            .contains("live shared borrow")
    );
    // main is in the subset; consume's dereference is not. A per-body result
    // neither validates its callee nor checks main's live loan conflict.
    assert!(
        flow::lower(&program, "main")
            .unwrap()
            .initialization()
            .issues
            .is_empty()
    );
    assert!(flow::lower(&program, "consume").is_err());
}
