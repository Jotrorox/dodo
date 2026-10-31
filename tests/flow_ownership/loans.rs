//! Loans, escapes, destruction order, Result obligations, and coverage. Every
//! body the type checker accepts is analyzed; there is no fallback checker.
use super::compare;
use dodoc::{parser, sema};
use sema::flow;

#[test]
fn every_checked_body_is_analyzed_including_generics_methods_and_closures() {
    let source = "package experiment\n\
        struct Cell<T> { value: T\nfn get(&self) -> &T { return &self.value } }\n\
        fn id<T>(value: T) -> T { return value }\n\
        fn apply(callback: &dyn fn(u8) -> u8) -> u8 { return callback(1) }\n\
        fn f() -> u8 {\n cell := Cell<u8> { value: id(2u8) }\n n := *cell.get()\n\
        offset := 3u8\n return apply(fn(x: u8) -> u8 { return x + offset + n })\n}\n";
    let mut program = parser::parse(source).unwrap();
    let report = flow::check_with_report(&mut program, 64);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let names: Vec<_> = report
        .bodies
        .as_ref()
        .unwrap()
        .iter()
        .map(|body| body.name.as_str())
        .collect();
    for expected in ["f", "apply"] {
        assert!(names.contains(&expected), "{names:?}");
    }
    assert!(names.iter().any(|n| n.starts_with("id$")), "{names:?}");
    assert!(
        names
            .iter()
            .any(|n| n.starts_with("Cell$") && n.ends_with(".get")),
        "{names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("f#closure")),
        "{names:?}"
    );
}

#[test]
fn live_loans_conflict_and_end_at_their_last_use() {
    compare(
        "fn f() -> u8 {\n u8 x = 1\n r := &x\n x = 2\n return *r\n}",
        Some("access conflicts with a live shared borrow held by `r`"),
        &[],
    );
    compare(
        "fn f() -> u8 {\n u8 x = 1\n r := &x\n n := *r\n x = 2\n return n + x\n}",
        None,
        &[],
    );
    compare(
        "fn f() -> u8 {\n u8 x = 1\n r := &mut x\n n := x\n *r = 2\n return n\n}",
        Some("live mutable borrow held by `r`"),
        &[],
    );
    // A loan stays live across a loop back edge.
    compare(
        "fn f() {\n u8 x = 1\n r := &x\n for i := 0; i < 2; i += 1 {\n n := *r\n x = n\n }\n}",
        Some("live shared borrow"),
        &[],
    );
}

#[test]
fn disjoint_fields_and_conservative_indices() {
    compare(
        "struct P { u8 a\nu8 b }\nfn f() -> u8 {\n p := P{a: 1, b: 2}\n a := &mut p.a\n b := &mut p.b\n *a = 3\n *b = 4\n return p.a + p.b\n}",
        None,
        &[],
    );
    compare(
        "fn f() -> u8 {\n v := [2]u8{1, 2}\n a := &mut v[0]\n b := &mut v[1]\n *a = 3\n *b = 4\n return v[0]\n}",
        Some("live mutable borrow held by `a`"),
        &[],
    );
}

#[test]
fn arguments_and_reservations_overlap_within_an_expression() {
    compare(
        "fn both(a: &mut u8, b: &mut u8) {}\nfn f() {\n u8 x = 1\n both(&mut x, &mut x)\n}",
        Some("overlapping borrows within the same expression"),
        &[],
    );
    compare(
        "fn both(a: &mut u8, b: &u8) {}\nfn f() {\n u8 x = 1\n r := &mut x\n both(r, r)\n}",
        Some("overlapping borrows within the same expression"),
        &[],
    );
    // Reborrowing a mutable reference twice in sequence is fine.
    compare(
        "fn set(x: &mut u8) { *x = 2 }\nfn f() -> u8 {\n u8 x = 1\n r := &mut x\n set(r)\n set(r)\n return *r\n}",
        None,
        &[],
    );
    // The captured destination is reserved while the right-hand side runs.
    compare(
        "fn f() {\n v := [2]u8{1, 2}\n v[0] = { v = [2]u8{3, 4}\n 5u8 }\n}",
        Some("overlapping borrows within the same expression"),
        &[],
    );
}

#[test]
fn borrows_cannot_escape_their_sources() {
    compare(
        "fn f() -> u8 {\n &u8 view\n {\n u8 x = 1\n view = &x\n }\n return *view\n}",
        Some("borrow in `view` outlives its source in this scope"),
        &[],
    );
    compare(
        "fn f() -> u8 {\n view := {\n u8 x = 1\n &x\n }\n return *view\n}",
        Some("value block cannot yield a borrow of its local storage"),
        &[],
    );
    compare(
        "fn f() -> &u8 from(static) {\n u8 x = 1\n return &x\n}",
        Some("cannot return a borrow of a local value"),
        &[],
    );
    compare(
        "fn f(a: &u8, b: &u8) -> &u8 from(a) { return b }",
        Some("outside the return contract"),
        &[],
    );
    compare(
        "fn f() {\n values := [2]u8{1, 2}\n &mut u8 escaped\n for item in &mut values { escaped = item }\n *escaped = 3\n}",
        Some("mutable foreach element borrow cannot escape its iteration"),
        // The loop may also run zero times.
        &["escaped"],
    );
}

#[test]
fn destructors_keep_their_loans_live_and_run_in_reverse_order() {
    let view = "struct View {\n &u8 r\n fn drop(self: &mut Self) { n := *self.r }\n}\n";
    compare(
        &format!("{view}fn f() {{\n u8 x = 1\n v := View{{r: &x}}\n x = 2\n}}"),
        Some("live shared borrow held by `v`"),
        &[],
    );
    compare(
        &format!("{view}fn f() {{\n View v\n u8 x = 1\n v = View{{r: &x}}\n}}"),
        Some("destructor of `v` would run after its borrowed source is destroyed"),
        &[],
    );
    compare(
        &format!("{view}fn f() {{\n u8 x = 1\n v := View{{r: &x}}\n core.drop(v)\n x = 2\n}}"),
        None,
        &[],
    );
}

#[test]
fn result_obligations_follow_moves_matches_and_exits() {
    let get = "fn get() -> u8!u8 { return ok(1) }\n";
    compare(
        &format!("{get}fn f() -> u8!u8 {{\n r := get()\n return r\n}}"),
        None,
        &[],
    );
    compare(
        &format!("{get}fn f() -> u8 {{\n r := get()\n return 0\n}}"),
        Some("Result `r` is left unhandled on this exit"),
        &[],
    );
    compare(
        &format!("{get}fn f() {{\n r := get()\n match &r {{ ok(n) => {{}} err(e) => {{}} }}\n}}"),
        None,
        &[],
    );
    compare(
        &format!(
            "{get}fn f(b: bool) {{\n r := get()\n if b {{ match &r {{ ok(n) => {{}} err(e) => {{}} }} }}\n}}"
        ),
        Some("Result `r` is never handled"),
        &[],
    );
    compare(
        &format!(
            "{get}fn f() {{\n r := get()\n r = get()\n match r {{ ok(n) => {{}} err(e) => {{}} }}\n}}"
        ),
        Some("overwriting an unhandled Result"),
        &[],
    );
}

#[test]
fn closures_borrow_their_captures_while_used() {
    compare(
        "fn f() -> u8 {\n u8 x = 1\n read: &dyn fn() -> u8 = fn() -> u8 { return x }\n x = 2\n return read()\n}",
        Some("live shared borrow held by `read`"),
        &[],
    );
    compare(
        "fn f() -> u8 {\n u8 x = 1\n read: &dyn fn() -> u8 = fn() -> u8 { return x }\n n := read()\n x = 2\n return n\n}",
        None,
        &[],
    );
}

#[test]
fn match_guards_cannot_move_or_borrow_mutably() {
    compare(
        "struct S { u8 n }\nfn take(s: S) -> bool { return true }\nfn f(s: S, b: bool) {\n match b { true if take(s) => {} _ => {} }\n}",
        Some("match guards cannot move values"),
        &[],
    );
}
