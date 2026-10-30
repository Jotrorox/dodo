//! Ownership checking through the control-flow analysis. Expected outcomes
//! are specified independently for each program: production checking must
//! report the expected first error (or none), and the analysis of `f` must
//! report exactly the listed uninitialized or moved bindings.
use dodoc::{parser, sema};
use sema::flow;

#[path = "flow_ownership/loans.rs"]
mod loans;

fn compare(source: &str, production_error: Option<&str>, uninitialized: &[&str]) {
    let program = parser::parse(&format!("package experiment\n{source}")).unwrap();
    let report = flow::check_with_report(&mut program.clone(), 64);
    match (report.diagnostics.first(), production_error) {
        (None, None) => (),
        (Some(error), Some(expected)) => {
            assert!(error.message.contains(expected), "{error:?}\n{source}")
        }
        (actual, expected) => panic!("mismatch: {actual:?}, expected {expected:?}\n{source}"),
    }
    let bodies = report.bodies.as_ref().expect("bodies analyzed");
    let body = bodies
        .iter()
        .find(|body| body.name == "f")
        .unwrap_or_else(|| panic!("`f` was not analyzed\n{source}"));
    let names: Vec<_> = body
        .uninitialized
        .iter()
        .map(|issue| issue.name.as_str())
        .collect();
    assert_eq!(names, uninitialized, "{source}");
    // Also lock down the normal fail-fast entry point.
    let mut checked = program.clone();
    match (sema::check(&mut checked), production_error) {
        (Ok(()), None) => (),
        (Err(error), Some(expected)) => assert!(error.message.contains(expected), "{error:?}"),
        (actual, expected) => {
            panic!("production mismatch: {actual:?}, expected {expected:?}\n{source}")
        }
    }
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
fn range_loop_moves_follow_back_edges_and_nested_exits() {
    let prefix = "struct S { u8 n }\nfn take(s: S) {}\n";
    for (body, error, issues) in [
        (
            "fn f(s: S) { for i in 0..2 { take(s) } }",
            Some("moved in a loop"),
            vec!["s"],
        ),
        (
            "fn f(s: S, b: bool) { for i in 0..2 { if b { take(s)\ncontinue } } }",
            Some("moved in a loop"),
            vec!["s"],
        ),
        (
            "fn f(s: S) { for i in 0..2 { take(s)\ns = S{n: 1}\ncontinue } }",
            None,
            vec![],
        ),
        (
            "fn f(s: S) { for i in 0..2 { take(s)\nbreak } }",
            None,
            vec![],
        ),
        (
            "fn f(s: S) { for i in 0..2 { take(s)\nreturn } }",
            None,
            vec![],
        ),
        (
            "fn f(s: S) { for i in 0..2 { take(s)\nbreak }\ntake(s) }",
            Some("moved"),
            vec!["s"],
        ),
        (
            "fn f(s: S) { for i in 0..2 { for j in 0..2 { take(s)\nbreak } } }",
            Some("moved in a loop"),
            vec!["s"],
        ),
    ] {
        compare(&format!("{prefix}{body}"), error, &issues);
    }
}

#[test]
fn range_bound_type_inference_supports_integer_expressions() {
    for source in [
        "fn f(n: u8) { for i in 0..n + 1 { _ = i } }",
        "fn f(values: &[u8]) { for i in 0..values.len / 2 { _ = values[i] } }",
        "struct S { i16 n }\nfn f(s: &S) { for i in -1..s.n { _ = i } }",
        "fn f(n: &i16) { for i in -1..*n { _ = i } }",
        "fn f(values: &[i16]) { for i in -1..values[0] { _ = i } }",
    ] {
        compare(source, None, &[]);
    }
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
fn array_loop_moves_reinitialization_and_exits_have_independent_outcomes() {
    let prefix = "fn take(a: [1]u8) {}\n";
    for (body, error, issues) in [
        (
            "fn f(a: [1]u8, b: bool) { for b { take(a) } }",
            Some("moved in a loop"),
            vec!["a"],
        ),
        (
            "fn f(a: [1]u8, b: bool) { for b { take(a)\na = [1]u8{1}\ncontinue } }",
            None,
            vec![],
        ),
        (
            "fn f(a: [1]u8) { for { take(a)\nbreak }\n_ = a[0] }",
            Some("moved"),
            vec!["a"],
        ),
        (
            "fn f(a: [1]u8) { for { take(a)\na = [1]u8{1}\nbreak }\n_ = a[0] }",
            None,
            vec![],
        ),
        ("fn f(a: [1]u8) { for { take(a)\nreturn } }", None, vec![]),
        (
            "fn f() { [1]u8 a\nfor { a = [1]u8{1}\nbreak }\n_ = a[0] }",
            None,
            vec![],
        ),
        (
            "fn f(b: bool) { [1]u8 a\nfor b { a = [1]u8{1}\nbreak }\n_ = a[0] }",
            Some("uninitialized"),
            vec!["a"],
        ),
    ] {
        compare(&format!("{prefix}{body}"), error, &issues);
    }
}

#[test]
fn slice_availability_does_not_establish_borrow_safety() {
    compare("fn f(a: &[u8]) -> u8 { return a[0] }", None, &[]);
    compare("fn f(a: &mut[u8]) { a[0] += 1 }", None, &[]);
    compare("fn f(a: &mut [2]u8) { a[0] = 1 }", None, &[]);
    compare(
        "fn f() { &[u8] a\n_ = a.len }",
        Some("uninitialized"),
        &["a"],
    );
    compare(
        "fn f() { [2]u8 a\n_ = &a[..] }",
        Some("uninitialized"),
        &["a"],
    );
    compare(
        "fn f(a: [2]u8) { usize end\n_ = &a[..end] }",
        Some("uninitialized"),
        &["end"],
    );
    compare(
        "fn f(a: [2]u8) { s := &mut a[..]\ns[0] = 1\n_ = s.len }",
        None,
        &[],
    );
    compare(
        "fn observe(s: &[u8]) {}\nfn f(a: [2]u8) { s := &a[..]\na[0] = 1\nobserve(s) }",
        Some("borrow"),
        &[],
    );
    compare(
        "fn take(a: [2]u8) {}\nfn f(a: [2]u8) { a[0] = { take(a)\na = [2]u8{1,2}\n3 } }",
        Some("borrow"),
        &[],
    );
    compare(
        "fn take(a: [2]u8) {}\nfn f(a: [2]u8) { _ = &a[..{ take(a)\na = [2]u8{1,2}\n1usize }] }",
        Some("borrow"),
        &[],
    );
}

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
fn result_obligations_are_tracked_per_binding() {
    compare(
        "fn get() -> u8!u8 { return ok(1) }\nfn f() { r := get() }",
        Some("never handled"),
        &[],
    );
    // Discarding a Result is rejected by type checking, before ownership.
    let mut program = parser::parse("package experiment\nfn f(r: u8!u8) { _ = r }").unwrap();
    let error = sema::check(&mut program).unwrap_err();
    assert!(error.message.contains("cannot be discarded"), "{error:?}");
    let mut program =
        parser::parse("package experiment\nfn f(r: u8!u8) { if let ok(n) = r { _ = n } }").unwrap();
    let error = sema::check(&mut program).unwrap_err();
    assert!(error.message.contains("Result"), "{error:?}");
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
fn collection_forms_borrow_owners_and_initialize_index_and_element_bindings() {
    for source in [
        "fn f(a: [2]u8) { for i, value in a { _ = i\n_ = *value }\n_ = a[0] }",
        "fn f(a: [2]u8) { for i, &value in &a { _ = i\n_ = value }\n_ = a[0] }",
        "fn f(a: &[u8]) { for i, &value in a { _ = i\n_ = value }\n_ = a.len }",
        "fn f(a: &mut[u8]) { for value in a { *value += 1 }\na[0] = 2 }",
        "fn f(a: [2]u8) { for value in &mut a { *value += 1 }\n_ = a[0] }",
        "fn f(a: &[2]u8) { for value in a { _ = *value } }",
        "fn f(a: &mut[2]u8) { for &value in &*a { _ = value } }",
        "fn f(a: [2]u8) { for &value in a[1..] { _ = value } }",
        "fn f(a: [2]u8) { s := &mut a[..]\nfor value in s { *value = 3 } }",
        "struct S { u8 n }\nfn f(a: [2]S) { for value in a { _ = value.n }\n_ = a[0].n }",
        "fn f(a: [2][2]u8) { for row in a { for &value in row { _ = value } } }",
        "fn f(a: [0]u8) { for &_ in a {}\n_ = a.len }",
    ] {
        compare(source, None, &[]);
    }
}

#[test]
fn collection_evaluation_checks_availability_and_preserves_source_order() {
    for (source, names) in [
        ("fn f() { [2]u8 a\nfor value in a {} }", vec!["a"]),
        ("fn f() { &[u8] a\nfor &value in a {} }", vec!["a"]),
        (
            "fn take(a: [2]u8) {}\nfn f(a: [2]u8) { take(a)\nfor value in a {} }",
            vec!["a"],
        ),
        (
            "fn f(a: [2]u8) { usize end\nfor value in a[..end] {} }",
            vec!["end"],
        ),
        (
            "fn f() { [2]u8 a\nfor value in a[..{ a = [2]u8{1,2}\n1usize }] {} }",
            vec!["a"],
        ),
        (
            "fn f() { [2][2]u8 a\nfor value in a[{ a = [2][2]u8{[2]u8{1,2},[2]u8{3,4}}\n0usize }] {} }",
            vec!["a"],
        ),
    ] {
        compare(source, Some("uninitialized"), &names);
    }
    // Evaluation is before the zero-iteration path, so its assignments survive.
    compare(
        "fn f(a: [2]u8) { usize end\nfor &value in a[..{ end = 1\nend }] {}\n_ = end }",
        None,
        &[],
    );
    compare(
        "fn f(a: &[u8]) { usize n\nfor &value in { n = 1\na } { _ = value }\n_ = n\n_ = a[0] }",
        None,
        &[],
    );
}

#[test]
fn foreach_joins_zero_iterations_breaks_and_continue_back_edges() {
    let prefix = "struct S { u8 n }\nfn take(s: S) {}\n";
    for (body, error, issues) in [
        (
            "fn f(a: &[u8], s: S) { for value in a { take(s) } }",
            Some("moved in a loop"),
            vec!["s"],
        ),
        (
            "fn f(a: &[u8], s: S, b: bool) { for value in a { if b { take(s)\ncontinue } } }",
            Some("moved in a loop"),
            vec!["s"],
        ),
        (
            "fn f(a: &[u8], s: S) { for value in a { take(s)\ns = S{n: 1}\ncontinue }\ntake(s) }",
            None,
            vec![],
        ),
        (
            "fn f(a: &[u8], s: S) { for value in a { take(s)\nbreak } }",
            None,
            vec![],
        ),
        (
            "fn f(a: &[u8], s: S) { for value in a { take(s)\nreturn }\ntake(s) }",
            None,
            vec![],
        ),
        (
            "fn f(a: &[u8], s: S, b: bool) { for value in a { if b { take(s)\nbreak } }\ntake(s) }",
            Some("moved"),
            vec!["s"],
        ),
        (
            "fn f(a: [2]u8) { u8 x\nfor &value in a { x = value\nbreak }\n_ = x }",
            Some("uninitialized"),
            vec!["x"],
        ),
        (
            "fn f(a: &[u8], s: S) { for value in a { for inner in a { take(s)\nbreak } } }",
            Some("moved in a loop"),
            vec!["s"],
        ),
    ] {
        compare(&format!("{prefix}{body}"), error, &issues);
    }
}

#[test]
fn range_and_collection_loops_preserve_nested_exit_targets() {
    for source in [
        "fn f(a: &[u8], b: bool) { for &value in a { for i in 0usize..a.len { if b { continue }\n_ = i }\nif b { continue }\n_ = value } }",
        "fn f(a: &[u8], b: bool) { for i in 0usize..a.len { for value in a { if b { continue }\nbreak }\nif b { continue }\n_ = i } }",
    ] {
        compare(source, None, &[]);
    }
    for loops in [
        "for value in a { for i in 0..2 { take(s)\nbreak } }",
        "for i in 0..2 { for value in a { take(s)\nbreak } }",
    ] {
        compare(
            &format!("struct S {{ u8 n }}\nfn take(s: S) {{}}\nfn f(a: &[u8], s: S) {{ {loops} }}"),
            Some("moved in a loop"),
            &["s"],
        );
    }
}

#[test]
fn foreach_keeps_its_collection_borrowed_across_iterations() {
    compare(
        "fn f(a: [2]u8) { for &value in a { a[0] = value } }",
        Some("live shared borrow held by `$foreach.collection"),
        &[],
    );
    compare(
        "fn f(a: [2]u8) { for value in &mut a { a[0] = 1 } }",
        Some("live mutable borrow held by `$foreach.collection"),
        &[],
    );
    // `&mut a[..]` is a mutable slice; its elements are written through it.
    compare(
        "fn f(a: [2]u8) { for value in &mut a[..] { *value = 3 } }",
        None,
        &[],
    );
    // The value block moves the exclusive slice out of `a`.
    compare(
        "fn f(a: &mut[u8]) { for value in { a } { *value = 2 }\na[0] = 3 }",
        Some("moved"),
        &["a"],
    );
}

#[test]
fn integer_casts_compose_with_ranges_projections_patterns_and_result_exits() {
    for source in [
        "fn f(n: u8) { for i in 0..n as u32 { u32 value = i } }",
        "fn f(n: u8) { for i in n as i16..3 { i16 value = i } }",
        "fn f(n: u8) { for i in 0..(n as u32) + 1 { u32 value = i } }",
        "fn f(a: [2]u8) { for &value in a { _ = value as u32 } }",
        "struct S { u8 n }\nfn f(s: &S, a: &[u8]) -> u32 { return (s.n as u32) + (a[0] as u32) }",
        "fn f(n: u8) -> u32 { return match n { x if (x as u32) > 0 => x as u32, _ => 0 } }",
        "fn f(r: u8!u8) -> u32!u8 { return ok(r? as u32) }",
        "fn f(r: u8!u8) -> u32 { return r! as u32 }",
        "fn f(n: u8) -> i64 { return n as u32 as i64 }",
    ] {
        compare(source, None, &[]);
    }
    compare(
        "fn f() { u8 n\nfor i in 0..n as u32 {} }",
        Some("uninitialized"),
        &["n"],
    );
}
