//! Incremental tagged-value coverage: construction, selection, and exits.
use super::*;
use flow::{CleanupKind, Operand, PatternMode};

#[test]
fn constructors_transfer_plain_payloads_in_source_order() {
    compare(
        "enum E { A, B(u8) }\nfn f(b: bool) -> E { if b { return E.A }\nreturn E.B(1) }",
        None,
        &[],
    );
    compare(
        "fn f(b: bool) -> Option<u8> { if b { return some(1) }\nreturn none }",
        None,
        &[],
    );
    compare(
        "enum E { A }\nfn f(b: bool) -> u8!E { if b { return ok(1) }\nreturn err(E.A) }",
        None,
        &[],
    );
    compare(
        "struct S { u8 n }\nenum E { Pair(S, S) }\nfn f(s: S) { _ = E.Pair(s, s) }",
        Some("moved"),
        &["s"],
    );
    compare(
        "struct S { u8 n }\nfn take(o: Option<S>) {}\nfn f(s: S) { take(some(s))\n_ = s }",
        Some("moved"),
        &["s"],
    );
    compare(
        "struct S { Option<Option<u8>> n }\nfn f() { _ = S{n: some(some(1u8))} }",
        None,
        &[],
    );
    compare(
        "struct S { u8 n }\nstruct Box { S value }\nfn f(s: S) { _ = Box{value: s}\n_ = s }",
        Some("moved"),
        &["s"],
    );
    compare("enum E { A }\nfn f() -> void!E { return ok() }", None, &[]);
}

#[test]
fn match_joins_only_surviving_arms_and_tracks_payload_moves() {
    compare(
        "fn f(b: bool) -> u8 { u8 x\nmatch b { true => { x = 1 }, false => { x = 2 } }\nreturn x }",
        None,
        &[],
    );
    compare(
        "fn f(b: bool) -> u8 { u8 x\nmatch b { true => { x = 1 }, false => {} }\nreturn x }",
        Some("uninitialized"),
        &["x"],
    );
    compare(
        "fn f(o: Option<u8>) -> u8 { u8 x\nmatch o { some(n) => { x = n }, none => { return 0 } }\nreturn x }",
        None,
        &[],
    );
    compare(
        "struct S { u8 n }\nfn take(s: S) {}\nfn f(o: Option<S>) { match o { some(s) => { take(s)\ntake(s) }, none => {} } }",
        Some("moved"),
        &["s"],
    );
    compare(
        "struct S { u8 n }\nfn f(o: Option<S>) { match o { some(_) => {}, none => {} }\n_ = o }",
        Some("moved"),
        &["o"],
    );
    compare(
        "enum E { A, B(u8) }\nfn f(e: E) -> u8 { match e { E.A => 0, B(n) => n } }",
        None,
        &[],
    );
    compare(
        "fn f(r: u8!u8) -> u8 { match r { ok(n) => n, err(e) => e } }",
        None,
        &[],
    );
}

#[test]
fn nested_alternatives_guards_and_struct_patterns_are_analyzed() {
    compare(
        "fn f(o: Option<Option<u8>>) -> u8 { match o { some(some(n)) => n, some(none) | none => 0 } }",
        None,
        &[],
    );
    compare(
        "enum E { A(u8), B(u8) }\nfn f(e: E) -> u8 { match e { E.A(n) | E.B(n) if n < 10 => n, _ => 0 } }",
        None,
        &[],
    );
    compare(
        "fn f(n: u8) -> u8 { match n { 0..=9 => 1, 10 | 20 => 2, n if n < 100 => 3, _ => 4 } }",
        None,
        &[],
    );
    compare(
        "struct Reading { Option<Option<u8>> value\nbool valid }\nfn f(r: Reading) -> u8 { let Reading{value: some(some(n)), valid: true} = r else { return 0 }\nreturn n }",
        None,
        &[],
    );
    compare(
        "struct S { u8 n\nu8 m }\nfn f(s: S) -> u8 { let S{n, ..} = s\nreturn n }",
        None,
        &[],
    );
    // Pattern selection does not initialize unrelated storage.
    compare(
        "fn f(o: Option<u8>) -> u8 { u8 x\nmatch o { some(n) if x == 1 => n, _ => 0 } }",
        Some("uninitialized"),
        &["x"],
    );
    // Guard observers see previews; selected payload ownership is transferred
    // only after the guard. The later arm still has a live scrutinee.
    let body = compare(
        "struct S { u8 n }\nfn observe(s: &S) -> bool { return s.n == 1 }\nfn f(o: Option<S>) { match o { some(s) if observe(&s) => { _ = s }, some(t) => { _ = t }, none => {} } }",
        None,
        &[],
    );
    assert!(
        body.blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .any(|i| matches!(
                i.operation,
                Operation::PatternBind {
                    mode: PatternMode::Preview,
                    ..
                }
            ))
    );
}

#[test]
fn conditional_patterns_consume_owned_subjects_on_both_paths() {
    compare(
        "fn f(o: Option<u8>) -> u8 { u8 x\nif let some(n) = o { x = n } else { x = 0 }\nreturn x }",
        None,
        &[],
    );
    compare(
        "fn f(o: Option<u8>) -> u8 { u8 x\nif let some(n) = o { x = n }\nreturn x }",
        Some("uninitialized"),
        &["x"],
    );
    compare(
        "fn f(o: Option<u8>) { if let some(n) = o { _ = n }\n_ = o }",
        Some("moved"),
        &["o"],
    );
    compare(
        "fn f(o: Option<u8>) { if let some(n) = o { return }\n_ = o }",
        Some("moved"),
        &["o"],
    );
    compare(
        "fn f(o: Option<u8>) -> u8 { let some(n) = o else { return 0 }\nreturn n }",
        None,
        &[],
    );
    compare(
        "fn f(o: Option<u8>) { let some(n) = o else { return }\n_ = o }",
        Some("moved"),
        &["o"],
    );
    compare(
        "fn f(o: Option<u8>) { for { let some(n) = o else { break }\n_ = n } }",
        Some("moved in a loop"),
        &["o"],
    );
    compare(
        "fn f(o: Option<u8>) { for { let some(n) = o else { break }\n_ = n\no = none } }",
        None,
        &[],
    );
    compare(
        "fn f(o: Option<u8>) { for { let some(n) = o else { continue }\n_ = n\nbreak } }",
        Some("moved in a loop"),
        &["o"],
    );
}

#[test]
fn borrowed_patterns_keep_owner_availability_and_payload_permissions() {
    compare(
        "fn f(o: Option<u8>) { match &o { some(n) => { _ = *n }, none => {} }\n_ = o }",
        None,
        &[],
    );
    compare(
        "fn f(o: Option<u8>) { match &mut o { some(n) => { *n += 1 }, none => {} }\n_ = o }",
        None,
        &[],
    );
    compare(
        "fn f(o: Option<u8>) { if let some(n) = &o { _ = *n }\n_ = o }",
        None,
        &[],
    );
    compare(
        "struct S { u8 n }\nfn f(s: S) { let S{n} = &s\n_ = s\n_ = *n }",
        Some("borrow"),
        &[],
    );
    compare(
        "fn f() { Option<u8> o\nmatch &o { some(_) => {}, none => {} } }",
        Some("uninitialized"),
        &["o"],
    );
}

#[test]
fn propagation_moves_success_payloads_and_excludes_error_returns_from_joins() {
    compare(
        "fn get() -> u8!u8 { return ok(1) }\nfn f() -> u8!u8 { n := get()?\nreturn ok(n) }",
        None,
        &[],
    );
    compare(
        "fn get() -> void!u8 { return ok() }\nfn f() -> void!u8 { get()?\nreturn ok() }",
        None,
        &[],
    );
    compare(
        "struct S { u8 n }\nfn get() -> S!u8 { return ok(S{n: 1}) }\nfn take(s: S) {}\nfn f() -> void!u8 { s := get()?\ntake(s)\ntake(s)\nreturn ok() }",
        Some("moved"),
        &["s"],
    );
    compare(
        "fn f(r: u8!u8) -> u8!u8 { n := r?\nreturn r }",
        Some("moved"),
        &["r"],
    );
    compare(
        "fn get() -> bool!u8 { return ok(true) }\nfn f() -> u8!u8 { u8 x\nif get()? { x = 1 } else { x = 2 }\nreturn ok(x) }",
        None,
        &[],
    );
    compare(
        "fn get() -> bool!u8 { return ok(true) }\nfn f() -> u8!u8 { u8 x\nif get()? { x = 1 }\nreturn ok(x) }",
        Some("uninitialized"),
        &["x"],
    );
    compare(
        "fn get() -> u8!u8 { return ok(1) }\nfn f(b: bool) -> u8!u8 { for b { _ = get()? }\nreturn ok(0) }",
        None,
        &[],
    );
    compare("fn f(r: u8!u8) -> u8 { return r! }", None, &[]);
}

#[test]
fn propagation_cleans_live_scopes_and_partially_evaluated_arguments() {
    let body = compare(
        "struct S { u8 n }\nfn get() -> u8!u8 { return ok(1) }\nfn consume(s: S, n: u8, t: S) {}\nfn f(s: S, t: S) -> void!u8 { outer := S{n: 1}\n{ inner := S{n: 2}\nconsume(s, get()?, t) }\nreturn ok() }",
        None,
        &[],
    );
    let result = body.initialization();
    let (error_id, error) = body
        .blocks
        .iter()
        .enumerate()
        .find(|(_, b)| {
            b.instructions.iter().any(
                |i| matches!(&i.operation, Operation::Tagged { variant, .. } if variant == "err"),
            )
        })
        .unwrap();
    let returned = match error.terminator {
        Terminator::Return(Some(Operand::Move(p))) => p,
        _ => panic!("{error:?}"),
    };
    assert!(
        !error
            .instructions
            .iter()
            .any(|i| matches!(i.operation, Operation::Cleanup(p) if p == returned))
    );
    let cleanup: Vec<_> = result
        .cleanup
        .iter()
        .filter(|s| s.block == error_id)
        .collect();
    let slot = |name: &str| body.locals.iter().position(|l| l.name == name).unwrap();
    for name in ["outer", "inner", "t"] {
        assert!(
            cleanup
                .iter()
                .any(|s| s.place.0 == slot(name) && s.kind == CleanupKind::Always),
            "{name}: {cleanup:?}"
        );
    }
    assert!(
        cleanup
            .iter()
            .any(|s| s.place.0 == slot("s") && s.kind == CleanupKind::Never)
    );
    let inner = cleanup
        .iter()
        .position(|s| s.place.0 == slot("inner"))
        .unwrap();
    let outer = cleanup
        .iter()
        .position(|s| s.place.0 == slot("outer"))
        .unwrap();
    assert!(inner < outer);
    // The first argument's owned temporary is still live when get()? exits.
    assert!(
        cleanup
            .iter()
            .any(|s| body.locals[s.place.0].name == "$value"
                && body.locals[s.place.0].ty == dodoc::ast::Type::Named("S".into())
                && s.kind == CleanupKind::Always)
    );
    assert!(!error.instructions.iter().any(
        |i| matches!(&i.operation, Operation::Call { function, .. } if function == "consume")
    ));
}

#[test]
fn result_obligations_stay_with_the_ast_checker() {
    compare(
        "fn get() -> u8!u8 { return ok(1) }\nfn f() { r := get() }",
        Some("never handled"),
        &[],
    );
    compare("fn f(r: u8!u8) { _ = r }", Some("Result"), &[]);
    compare(
        "fn f(r: u8!u8) { if let ok(n) = r { _ = n } }",
        Some("Result"),
        &[],
    );
}

#[test]
fn pattern_value_blocks_transfer_payloads_across_yield_cleanup() {
    compare(
        "fn f(o: Option<u8>) -> u8 { n := match o { some(n) => n, none => 0u8 }\nreturn n }",
        None,
        &[],
    );
    compare(
        "struct S { u8 n }\nfn f(o: Option<S>) { s := match o { some(s) => s, none => S{n: 0} }\n_ = s\n_ = s }",
        Some("moved"),
        &["s"],
    );
    compare(
        "fn f(o: Option<u8>) -> u8 { n := if let some(n) = o { n } else { 0u8 }\nreturn n }",
        None,
        &[],
    );
    compare(
        "fn f(o: Option<u8>) -> u8 { n := match o { some(n) => n, none => { return 0 } }\nreturn n }",
        None,
        &[],
    );
    compare(
        "fn get() -> u8!u8 { return ok(1) }\nfn f(o: Option<u8>) -> u8!u8 { n := match o { some(n) => n, none => get()? }\nreturn ok(n) }",
        None,
        &[],
    );
}

#[test]
fn nested_results_and_owned_errors_transfer_only_the_selected_payload() {
    compare(
        "struct S { u8 n }\nfn f(r: Result<Result<S,u8>,u8>) -> S!u8 { s := r??\nreturn ok(s) }",
        None,
        &[],
    );
    let body = compare(
        "struct S { u8 n }\nfn f(r: u8!S) -> void!S { _ = r?\nreturn ok() }",
        None,
        &[],
    );
    let result = body.initialization();
    let payload = body.locals.iter().position(|l| l.name == "$error").unwrap();
    assert!(
        result
            .cleanup
            .iter()
            .any(|s| s.place.0 == payload && s.kind == CleanupKind::Never)
    );
    compare(
        "fn f(r: Option<u8!u8>) -> u8 { match r { some(ok(n)) => n, some(err(e)) => e, none => 0 } }",
        None,
        &[],
    );
    compare(
        "enum E { R(u8!u8), Empty }\nfn f(e: E) -> u8 { match e { E.R(ok(n)) => n, E.R(err(e)) => e, E.Empty => 0 } }",
        None,
        &[],
    );
}

#[test]
fn propagation_cleanup_uses_converged_conditional_liveness() {
    let body = compare(
        "struct S { u8 n }\nfn take(s: S) {}\nfn get() -> u8!u8 { return ok(1) }\nfn f(s: S, b: bool) -> void!u8 { if b { take(s) }\n_ = get()?\nreturn ok() }",
        None,
        &[],
    );
    let result = body.initialization();
    let source = body.locals.iter().position(|l| l.name == "s").unwrap();
    let error = body
        .blocks
        .iter()
        .position(|b| {
            b.instructions.iter().any(
                |i| matches!(&i.operation, Operation::Tagged { variant, .. } if variant == "err"),
            )
        })
        .unwrap();
    assert!(
        result
            .cleanup
            .iter()
            .any(|s| s.block == error && s.place.0 == source && s.kind == CleanupKind::Conditional)
    );
    compare(
        "struct S { u8 n }\nfn take(s: S) -> bool!u8 { return ok(true) }\nfn f(s: S, b: bool) -> void!u8 { if b && take(s)? {}\n_ = s\nreturn ok() }",
        Some("moved"),
        &["s"],
    );
    compare(
        "struct S { u8 n }\nfn get() -> S!u8 { return ok(S{n: 1}) }\nfn f(s: S) -> void!u8 { s = get()?\n_ = s\nreturn ok() }",
        None,
        &[],
    );
    compare(
        "struct S { u8 n }\nfn get() -> u8!u8 { return ok(1) }\nfn f(s: S) -> void!u8 { s.n = get()?\nreturn ok() }",
        None,
        &[],
    );
}

#[test]
fn unsupported_payloads_patterns_and_guards_discard_the_whole_graph() {
    for (source, reason) in [
        ("enum E { V(&u8) }\nfn f(e: E) {}", "type is outside"),
        ("fn f(o: Option<&u8>) {}", "type is outside"),
        ("fn f(r: u8!&u8) {}", "type is outside"),
        (
            "struct S { u8 n\nfn drop(&mut self) {} }\nfn f(o: Option<S>) {}",
            "custom destructors",
        ),
        ("enum E { R(E) }\nfn f(e: E) {}", "recursive"),
        ("enum E<T> { A }\nfn f(e: E) {}", "generic"),
        ("enum E { A, A }\nfn f(e: E) {}", "duplicate enum variants"),
        ("enum E { A(u8) }\nfn f() { _ = E.A() }", "arity"),
        (
            "fn f(o: Option<u8>) { match o { some() => {}, none => {} } }",
            "arity",
        ),
        (
            "fn f(o: Option<u8>) { match o { some(n) | none => {} } }",
            "identical binding",
        ),
        (
            "fn f(n: u8) { match n { 256 => {}, _ => {} } }",
            "target range",
        ),
        (
            "fn f(n: u8) { match n { 10..1 => {}, _ => {} } }",
            "reversed range",
        ),
        (
            "fn f(o: Option<u8>) { match o { some(n) if { n = 1\ntrue } => {}, _ => {} } }",
            "assignments in pattern guards",
        ),
        (
            "struct S { u8 n }\nfn take(s: S) -> bool { return true }\nfn f(o: Option<S>) { match o { some(s) if take(s) => {}, _ => {} } }",
            "moves in pattern guards",
        ),
        (
            "fn f(o: Option<u8>) { match o { some(n) if { r := &mut n\ntrue } => {}, _ => {} } }",
            "mutable borrows in pattern guards",
        ),
        (
            "fn f(r: u8!u8) -> u8!u16 { return ok(r?) }",
            "type mismatch",
        ),
        ("fn f(r: u8!u8) -> u8 { return r? }", "Result return type"),
        (
            "fn f(o: Option<u8>) -> u8!u8 { return ok(o?) }",
            "Result operand",
        ),
    ] {
        let program = parser::parse(&format!("package experiment\n{source}")).unwrap();
        let error = flow::lower(&program, "f").expect_err(source);
        assert!(error.reason.contains(reason), "{error:?}\n{source}");
    }
    // A late restriction must erase earlier initialization findings even after
    // the adapter has constructed pattern branches and Result return blocks.
    let program = parser::parse("package experiment\nfn get() -> u8!u8 { return ok(1) }\nfn f() -> u8!u8 { u8 x\n_ = x\nn := get()?\n_ = n as u32\nreturn ok(n) }").unwrap();
    let comparison = flow::compare_checkers(&program, 64);
    assert!(comparison.flow.diagnostics.is_empty());
    let body = comparison
        .flow
        .coverage
        .unwrap()
        .bodies
        .into_iter()
        .find(|b| b.name == "f")
        .unwrap();
    assert!(matches!(body.status, flow::BodyStatus::Skipped { .. }));
    assert_eq!(comparison.combined.diagnostics.len(), 1);
    assert!(
        comparison.combined.diagnostics[0]
            .message
            .contains("uninitialized")
    );
}

#[test]
fn guards_retry_alternatives_before_consuming_payloads() {
    let body = compare(
        "struct S { u8 n }\nenum E { A(S), B(S) }\nfn observe(s: &S) -> bool { return s.n == 1 }\nfn f(e: E) { match e { E.A(s) | E.B(s) if observe(&s) => { _ = s }, _ => {} } }",
        None,
        &[],
    );
    let result = body.initialization();
    let mut previews = 0;
    let mut guards = 0;
    for (id, block) in body.blocks.iter().enumerate() {
        for instruction in &block.instructions {
            match &instruction.operation {
                Operation::PatternBind {
                    source: Operand::Copy(source),
                    mode: PatternMode::Preview,
                    ..
                } => {
                    previews += 1;
                    assert!(result.incoming[id].as_ref().unwrap()[source.0].initialized);
                }
                Operation::Call { function, .. } if function == "observe" => guards += 1,
                _ => (),
            }
        }
    }
    assert_eq!(previews, 2);
    assert_eq!(guards, 2);
    // Each guard-failure edge keeps the subject available for the next test.
    for block in &body.blocks {
        if block.instructions.iter().any(
            |i| matches!(&i.operation, Operation::Call { function, .. } if function == "observe"),
        ) {
            let Terminator::Branch { no, .. } = block.terminator else {
                panic!("{block:?}")
            };
            let Terminator::Goto(next) = body.blocks[no].terminator else {
                panic!("missing retry edge")
            };
            let subject = body
                .locals
                .iter()
                .position(|l| l.name == "$value" && l.ty == dodoc::ast::Type::Named("E".into()))
                .unwrap();
            assert!(result.incoming[next].as_ref().unwrap()[subject].initialized);
        }
    }
}

#[test]
fn payload_slots_are_initialized_only_on_selected_arms() {
    let body = compare(
        "struct S { u8 n }\nfn f(o: Option<S>) { match o { some(s) => {}, none => {} } }",
        None,
        &[],
    );
    let result = body.initialization();
    let payload = body.locals.iter().position(|l| l.name == "s").unwrap();
    let selected = body
        .blocks
        .iter()
        .position(|b| {
            b.instructions
                .iter()
                .any(|i| matches!(i.operation, Operation::Cleanup(p) if p.0 == payload))
        })
        .unwrap();
    assert!(result.incoming[selected].as_ref().unwrap()[payload].initialized);
    let none = body.blocks.iter().position(|b| b.instructions.iter().any(|i| matches!(&i.operation, Operation::PatternTest { test: flow::PatternTest::Variant(v), .. } if v == "none"))).unwrap();
    assert!(!result.incoming[none].as_ref().unwrap()[payload].maybe_initialized);
    assert!(
        result
            .cleanup
            .iter()
            .any(|s| s.place.0 == payload && s.kind == CleanupKind::Always)
    );
}

#[test]
fn pattern_limits_and_enum_fallbacks_remain_body_local() {
    let alternatives = (0..4097).map(|_| "false").collect::<Vec<_>>().join(" | ");
    let program = parser::parse(&format!(
        "package experiment\nfn f(b: bool) {{ match b {{ {alternatives} => {{}}, _ => {{}} }} }}"
    ))
    .unwrap();
    assert!(
        flow::lower(&program, "f")
            .unwrap_err()
            .reason
            .contains("expansion limit")
    );
    let source = "package experiment\nenum E { Borrowed(&u8) }\nfn f(e: E) {}\nfn independent() {}";
    let program = parser::parse(source).unwrap();
    let comparison = flow::compare_checkers(&program, 64);
    let coverage = comparison.flow.coverage.unwrap();
    let dependent = coverage.bodies.iter().find(|b| b.name == "f").unwrap();
    let flow::BodyStatus::Skipped { scope, limitation } = &dependent.status else {
        panic!("{dependent:?}")
    };
    assert_eq!(*scope, flow::SkipScope::Body);
    assert_eq!(limitation.kind, flow::LimitationKind::Enums);
    assert!(source[limitation.span.start..limitation.span.end].contains("&u8"));
    assert_eq!(
        coverage
            .bodies
            .iter()
            .find(|b| b.name == "independent")
            .unwrap()
            .status,
        flow::BodyStatus::Analyzed
    );
}

#[test]
fn range_loops_combine_patterns_with_result_exits_and_bound_cleanup() {
    let body = compare(
        "fn limit() -> u8!u8 { return ok(2) }\nfn f() -> u8!u8 { total := 0u8\nfor i in 0..limit()? { let some(n) = some(i) else { continue }\ntotal += n\n_ = limit()? }\nreturn ok(total) }",
        None,
        &[],
    );
    let error = body
        .blocks
        .iter()
        .position(|block| {
            block.instructions.iter().any(|instruction|
        matches!(&instruction.operation, Operation::Tagged { variant, .. } if variant == "err"))
        })
        .unwrap();
    let initialization = body.initialization();
    for (name, kind) in [
        ("$range.index", CleanupKind::Always),
        ("$range.end", CleanupKind::Never),
    ] {
        let slot = body
            .locals
            .iter()
            .position(|local| local.name == name)
            .unwrap();
        assert!(
            initialization
                .cleanup
                .iter()
                .any(|site| site.block == error && site.place.0 == slot && site.kind == kind)
        );
    }
    compare(
        "fn limit() -> u8!u8 { return ok(2) }\nfn f() { for i in 0..limit()! { _ = i } }",
        None,
        &[],
    );
}
