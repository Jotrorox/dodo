//! Differential coverage for the production subset. An empty issue list is not a
//! verdict on type safety, borrow safety, Result handling, or native cleanup.
use dodoc::{parser, sema};
use flow::{Operation, Terminator};
use sema::flow;

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
fn loop_exit_initialization_agrees() {
    for body in [
        "fn f() -> u8 { u8 x\nfor { x = 1\nbreak }\nreturn x }",
        "fn f(b: bool) -> u8 { u8 x\nfor { if b { x = 1\nbreak } else { x = 2\nbreak } }\nreturn x }",
        "fn f(b: bool) -> u8 { u8 x\nfor { if b { continue }\nx = 1\nbreak }\nreturn x }",
    ] {
        compare(body, None, &[]);
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
            "fn f() { x := 1u8\n_ = x.missing }",
            "field requires a struct",
        ),
        ("fn f() { x := 1u8\nif x {} }", "type mismatch"),
        ("fn f() { u8 x = true }", "type mismatch"),
        ("fn f() { x := 256u8 }", "range"),
        ("fn f() { _ = false && 1 }", "requires integer type"),
        ("fn f() { _ = true || missing }", "resolved whole local"),
        ("fn f(r: &u8) { *r = 1 }", "shared reference"),
        (
            "struct S { u8 n }\nfn f(r: &S) { r.n = 1 }",
            "shared reference",
        ),
        (
            "struct S { u8 n }\nfn f(r: &mut S) { _ = *r }",
            "moves out of projected",
        ),
        (
            "struct S { u8 n }\nfn f() { _ = S{n: 1, n: 2} }",
            "duplicate struct field",
        ),
        (
            "struct S { u8 n }\nfn f() { _ = S{} }",
            "missing struct field",
        ),
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
    // Both bodies have initialization coverage, but that does not establish
    // borrow safety or make the invalid fixture acceptable in production.
    assert!(
        flow::lower(&program, "main")
            .unwrap()
            .initialization()
            .issues
            .is_empty()
    );
    assert!(
        flow::lower(&program, "consume")
            .unwrap()
            .initialization()
            .issues
            .is_empty()
    );
}

#[test]
fn moved_value_fixture_is_now_covered() {
    let program = parser::parse(include_str!("../examples/diagnostics/moved_value.dodo")).unwrap();
    let issues = flow::lower(&program, "main")
        .unwrap()
        .initialization()
        .issues;
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].name, "value");
    assert!(issues[0].moved_at.is_some());
}

#[test]
fn short_circuit_assignments_and_nested_operands_join_both_paths() {
    for op in ["&&", "||"] {
        compare(
            &format!("fn f(b: bool) -> u8 {{ u8 x\n_ = b {op} {{ x = 1\ntrue }}\nreturn x }}"),
            Some("uninitialized"),
            &["x"],
        );
        compare(
            &format!("fn f(b: bool) -> u8 {{ u8 x\n_ = {{ x = 1\nb }} {op} true\nreturn x }}"),
            None,
            &[],
        );
        compare(
            &format!(
                "fn f(b: bool) -> bool {{ u8 x\nreturn (b {op} {{ x = 1\ntrue }}) && x == 1 }}"
            ),
            Some("uninitialized"),
            &["x"],
        );
        // Literal conditions remain conservative, including skipped syntax.
        compare(
            &format!("fn f() {{ u8 x\n_ = false {op} (x == 1) }}"),
            Some("uninitialized"),
            &["x"],
        );
    }
}

#[test]
fn short_circuit_moves_and_reinitialization_are_path_sensitive() {
    let prefix = "struct S { u8 n }\nfn take(s: S) -> bool { return true }\n";
    for op in ["&&", "||"] {
        compare(
            &format!("{prefix}fn f(s: S, b: bool) {{ _ = b {op} take(s)\n_ = take(s) }}"),
            Some("moved"),
            &["s"],
        );
        compare(
            &format!(
                "{prefix}fn f(s: S, b: bool) {{ _ = take(s)\n_ = b {op} {{ s = S{{n: 1}}\ntrue }}\n_ = take(s) }}"
            ),
            Some("moved"),
            &["s"],
        );
        compare(
            &format!(
                "{prefix}fn f(s: S, b: bool) {{ _ = b {op} take(s)\ns = S{{n: 2}}\n_ = take(s) }}"
            ),
            None,
            &[],
        );
        compare(
            &format!("{prefix}fn f(s: S, b: bool) {{ for b {op} take(s) {{}} }}"),
            Some("moved in a loop"),
            &["s"],
        );
    }
}

#[test]
fn assignments_read_only_the_storage_they_need() {
    compare("fn f() -> u8 { u8 x\nx = 1\nx += 2\nreturn x }", None, &[]);
    compare("fn f() { u8 x\nx += 1 }", Some("uninitialized"), &["x"]);
    compare(
        "struct S { u8 n }\nfn f() { S s\ns.n = 1 }",
        Some("uninitialized"),
        &["s"],
    );
    compare(
        "struct S { u8 n }\nfn f() { s := S{n: 1}\ns.n += 2\n_ = s.n }",
        None,
        &[],
    );
    compare("fn f(r: &mut u8) { *r += 1\n*r = 2 }", None, &[]);
    compare(
        "struct S { u8 n }\nfn f(r: &mut S) { r.n = 1\n(*r).n += 2 }",
        None,
        &[],
    );
    compare(
        "fn f() { &mut u8 r\n*r = 1 }",
        Some("uninitialized"),
        &["r"],
    );
    // A projected write cannot establish whole-binding initialization.
    compare(
        "struct S { u8 n }\nfn f() { S s\ns.n = 1\n_ = s }",
        Some("uninitialized"),
        &["s", "s"],
    );
}

#[test]
fn compound_destination_is_captured_and_loaded_before_the_rhs() {
    let body = compare(
        "struct S { u8 n }\nfn rhs() -> u8 { return 1 }\nfn f(s: S) { s.n += rhs() }",
        None,
        &[],
    );
    let operations: Vec<_> = body.blocks[0]
        .instructions
        .iter()
        .map(|i| &i.operation)
        .collect();
    let capture = operations
        .iter()
        .position(|op| matches!(op, Operation::Capture { .. }))
        .unwrap();
    let load = operations
        .iter()
        .position(|op| matches!(op, Operation::Load { .. }))
        .unwrap();
    let call = operations
        .iter()
        .position(|op| matches!(op, Operation::Call { .. }))
        .unwrap();
    let store = operations
        .iter()
        .position(|op| matches!(op, Operation::Store { .. }))
        .unwrap();
    assert!(capture < load && load < call && call < store);
    assert_eq!(
        operations
            .iter()
            .filter(|op| matches!(op, Operation::Capture { .. }))
            .count(),
        1
    );
    compare(
        "fn f() { u8 x\nx += { x = 1\n2 } }",
        Some("uninitialized"),
        &["x"],
    );
}

#[test]
fn cleanup_facts_distinguish_live_moved_and_maybe_live_slots() {
    use flow::CleanupKind::{Always, Conditional, Never};
    for (source, name, expected) in [
        ("fn f() { u8 x }", "x", Never),
        ("fn f() { x := 1u8 }", "x", Always),
        (
            "struct S { u8 n }\nfn take(s: S) {}\nfn f(s: S) { take(s) }",
            "s",
            Never,
        ),
        (
            "struct S { u8 n }\nfn take(s: S) {}\nfn f(s: S, b: bool) { if b { take(s) } }",
            "s",
            Conditional,
        ),
    ] {
        let body = compare(source, None, &[]);
        let result = body.initialization();
        let cleanup: Vec<_> = result
            .cleanup
            .iter()
            .filter(|site| body.locals[site.place.0].name == name)
            .collect();
        assert_eq!(cleanup.len(), 1, "{source}");
        assert_eq!(cleanup[0].kind, expected, "{source}");
    }
}

#[test]
fn overwrite_moves_rhs_before_cleaning_up_old_value() {
    use flow::CleanupKind::{Always, Never};
    let body = compare(
        "struct S { u8 n }\nfn f(s: S) { s = s\ns = S{n: 2} }",
        None,
        &[],
    );
    let result = body.initialization();
    let kinds: Vec<_> = result
        .cleanup
        .iter()
        .filter(|site| body.locals[site.place.0].name == "s")
        .map(|site| site.kind)
        .collect();
    assert_eq!(kinds, [Never, Always, Always]);
}

#[test]
fn cleanup_on_break_continue_and_return_is_inner_to_outer() {
    for exit in ["break", "continue", "return"] {
        let body = compare(
            &format!(
                "struct S {{ u8 n }}\nfn f(s: S) {{ for {{ a := S{{n: 1}}\n{{ b := S{{n: 2}}\n{exit} }} }} }}"
            ),
            None,
            &[],
        );
        let result = body.initialization();
        let cleanup: Vec<_> = result
            .cleanup
            .iter()
            .filter(|site| matches!(body.locals[site.place.0].name.as_str(), "a" | "b"))
            .map(|site| (body.locals[site.place.0].name.as_str(), site.kind))
            .collect();
        assert_eq!(
            cleanup,
            [
                ("b", flow::CleanupKind::Always),
                ("a", flow::CleanupKind::Always)
            ]
        );
    }
}

#[test]
fn value_block_moves_its_result_out_before_cleaning_up_locals() {
    compare(
        "struct S { u8 n }\nfn f() -> S { return { s := S{n: 1}\ns } }",
        None,
        &[],
    );
    let body = compare(
        "struct S { u8 n }\nfn f(s: S) { _ = { t := s\nt } }",
        None,
        &[],
    );
    let result = body.initialization();
    assert!(
        result
            .cleanup
            .iter()
            .any(|site| body.locals[site.place.0].name == "$value"
                && site.kind == flow::CleanupKind::Always)
    );
}

#[test]
fn move_diagnostics_preserve_origin_and_reinitialization_clears_it() {
    let source = "package experiment\nstruct S { u8 n }\nfn take(s: S) {}\nfn f(s: S, b: bool) { if b { take(s) }\ntake(s) }";
    let program = parser::parse(source).unwrap();
    let result = flow::lower(&program, "f").unwrap().initialization();
    assert_eq!(result.issues.len(), 1);
    let issue = &result.issues[0];
    let moved = issue.moved_at.unwrap();
    assert_eq!(&source[moved.start..moved.end], "s");
    assert!(moved.start < issue.span.start);
    compare(
        "struct S { u8 n }\nfn take(s: S) {}\nfn f(s: S) { take(s)\ns = S{n: 1}\ntake(s) }",
        None,
        &[],
    );
}

#[test]
fn target_width_and_unsupported_bodies_keep_explicit_fallback() {
    let program = parser::parse("package experiment\nfn f() { x := 4294967296usize }").unwrap();
    assert!(flow::lower_for_target(&program, "f", 64).is_ok());
    assert!(
        flow::lower_for_target(&program, "f", 32)
            .unwrap_err()
            .reason
            .contains("range")
    );
    assert!(sema::check_for_target(&mut program.clone(), 32).is_err());
    let mut program = parser::parse(
        "package experiment\nfn f() { x := 1u8\nx += 2 }\nfn g() { values := [2]u8{1,2} }",
    )
    .unwrap();
    assert!(flow::lower(&program, "f").is_ok());
    assert!(flow::lower(&program, "g").is_err());
    sema::check(&mut program).unwrap();
}

#[test]
fn recovery_keeps_existing_diagnostics_without_duplicates() {
    let mut program =
        parser::parse("package experiment\nfn f() { u8 x\nx += 1 }\nfn g() { u8 y\n_ = y }")
            .unwrap();
    let diagnostics = sema::check_recovering(&mut program, 64);
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics[0].message.contains("`x` is uninitialized"));
    assert!(diagnostics[1].message.contains("`y` is uninitialized"));
}

#[test]
fn captured_destination_reservations_still_require_the_borrow_checker() {
    // Initialization alone cannot see address invalidation when an owner is
    // moved and then restored during RHS evaluation. The production reservation
    // check must continue to reject it even though the graph's slots are live.
    compare(
        "struct S { u8 n }\nfn take(s: S) {}\nfn f(s: S) { s.n = { take(s)\ns = S{n: 2}\n1 } }",
        Some("borrow"),
        &[],
    );
}

#[test]
fn loop_condition_temporaries_are_cleaned_before_either_successor() {
    let body = compare(
        "fn condition() -> bool { return true }\nfn f() { for condition() { break } }",
        None,
        &[],
    );
    let result = body.initialization();
    let (id, block) = body
        .blocks
        .iter()
        .enumerate()
        .find(|(_, block)| {
            block
                .instructions
                .iter()
                .any(|i| matches!(i.operation, Operation::Call { .. }))
        })
        .unwrap();
    let Terminator::Branch { yes, no, .. } = block.terminator else {
        panic!("{block:?}")
    };
    let call = body
        .locals
        .iter()
        .position(|local| local.name == "$call")
        .unwrap();
    assert!(result.cleanup.iter().any(|site| site.block == id
        && site.place.0 == call
        && site.kind == flow::CleanupKind::Always));
    for successor in [yes, no] {
        assert!(!result.incoming[successor].as_ref().unwrap()[call].maybe_initialized);
    }
}
