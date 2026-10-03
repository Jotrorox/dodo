//! Collection captures, fresh iteration slots, and loop lifetime cleanup.
use super::{Operation, Terminator, compare, flow, parser};
use dodoc::ast::Type;
use flow::{CleanupKind, Operand};

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
fn collection_indices_and_bounds_execute_once_before_any_iteration() {
    for (parameters, iterable) in [
        ("a: [2]u8", "a[start()..end()]"),
        ("a: [2][2]u8", "a[index()]"),
        ("a: [2][2]u8", "&mut a[index()]"),
    ] {
        let body = compare(
            &format!(
                "fn start() -> usize {{ return 0 }}\nfn end() -> usize {{ return 1 }}\nfn index() -> usize {{ return 0 }}\nfn f({parameters}) {{ for i, value in {iterable} {{ _ = i\n_ = *value\ncontinue }} }}"
            ),
            None,
            &[],
        );
        let entry = &body.blocks[0];
        let root_read = entry
            .instructions
            .iter()
            .position(|i| matches!(i.operation, Operation::Capture { .. }))
            .unwrap();
        let calls: Vec<_> = entry
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(i, instruction)| {
                if let Operation::Call { function, .. } = &instruction.operation {
                    Some((i, function.as_str()))
                } else {
                    None
                }
            })
            .collect();
        let expected = if iterable.contains("index") {
            vec!["index"]
        } else {
            vec!["start", "end"]
        };
        assert_eq!(
            calls.iter().map(|(_, name)| *name).collect::<Vec<_>>(),
            expected
        );
        assert!(calls.iter().all(|(i, _)| root_read < *i));
        assert!(
            body.blocks[1..]
                .iter()
                .flat_map(|block| &block.instructions)
                .all(|i| !matches!(i.operation, Operation::Call { .. }))
        );
        let result = body.initialization();
        let call_cleanup: Vec<_> = result
            .cleanup
            .iter()
            .filter(|site| body.locals[site.place.0].name == "$call")
            .collect();
        assert_eq!(call_cleanup.len(), expected.len());
        for site in call_cleanup {
            assert_eq!(site.block, 0);
            assert_eq!(site.kind, CleanupKind::Always);
        }
    }
}

#[test]
fn iteration_bindings_are_fresh_and_shadowing_preserves_outer_places() {
    let body = compare(
        "fn f(a: &mut[u8], b: bool) { for i, value in a { r := value\n_ = *r\ni = 99\nif b { continue } } }",
        None,
        &[],
    );
    let value = body
        .locals
        .iter()
        .position(|local| local.name == "value")
        .unwrap();
    let result = body.initialization();
    let (run, _) = body
        .blocks
        .iter()
        .enumerate()
        .find(|(_, block)| {
            block
                .instructions
                .iter()
                .any(|i| matches!(i.operation, Operation::Iteration { .. }))
        })
        .unwrap();
    assert!(!result.incoming[run].as_ref().unwrap()[value].maybe_initialized);
    let value_cleanup: Vec<_> = result
        .cleanup
        .iter()
        .filter(|site| site.place.0 == value)
        .collect();
    assert_eq!(value_cleanup.len(), 2);
    assert!(
        value_cleanup
            .iter()
            .all(|site| site.kind == CleanupKind::Never)
    );
    compare(
        "fn f(a: [2]u8) { u8 value\nfor &value in a { value = 3\n_ = value }\n_ = value }",
        Some("uninitialized"),
        &["value"],
    );
    compare(
        "fn f(a: &[u8], b: bool) { for &value in a { u8 x\nif b { x = value }\n_ = x } }",
        Some("uninitialized"),
        &["x"],
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
fn foreach_cleanup_respects_collection_and_iteration_lifetimes() {
    for exit in ["break", "continue", "return"] {
        let body = compare(
            &format!(
                "struct S {{ u8 n }}\nfn f(a: &[u8]) {{ for i, &value in a {{ outer := S{{n: 1}}\n{{ inner := S{{n: 2}}\n{exit} }} }} }}"
            ),
            None,
            &[],
        );
        let result = body.initialization();
        let cleanup: Vec<_> = result
            .cleanup
            .iter()
            .filter(|site| {
                matches!(
                    body.locals[site.place.0].name.as_str(),
                    "inner" | "outer" | "value" | "i" | "$collection"
                )
            })
            .collect();
        let exit_block = cleanup
            .iter()
            .find(|site| body.locals[site.place.0].name == "inner")
            .unwrap()
            .block;
        let iteration_cleanup: Vec<_> = cleanup
            .iter()
            .filter(|site| site.block == exit_block)
            .map(|site| {
                assert_eq!(site.kind, CleanupKind::Always);
                body.locals[site.place.0].name.as_str()
            })
            .collect();
        let mut expected = vec!["inner", "outer", "value", "i"];
        if exit == "return" {
            expected.push("$collection");
        }
        assert_eq!(iteration_cleanup, expected);
        if exit != "return" {
            assert_eq!(
                cleanup
                    .iter()
                    .filter(|site| body.locals[site.place.0].name == "$collection")
                    .count(),
                1
            );
        }
        let collection = body
            .locals
            .iter()
            .position(|local| local.name == "$collection")
            .unwrap();
        if let Terminator::Goto(target) = body.blocks[exit_block].terminator {
            assert!(result.incoming[target].as_ref().unwrap()[collection].initialized);
            for name in ["i", "value", "inner", "outer"] {
                let local = body
                    .locals
                    .iter()
                    .position(|local| local.name == name)
                    .unwrap();
                assert!(!result.incoming[target].as_ref().unwrap()[local].maybe_initialized);
            }
        }
    }
}

#[test]
fn mutable_element_moves_are_cleaned_and_reset_without_consuming_the_collection() {
    let body = compare(
        "fn f(a: &mut[u8]) { for value in a { r := value\n*r = 1\ncontinue }\na[0] = 2 }",
        None,
        &[],
    );
    let result = body.initialization();
    for (name, kind) in [
        ("value", CleanupKind::Never),
        ("r", CleanupKind::Always),
        ("$collection", CleanupKind::Always),
    ] {
        assert!(
            result
                .cleanup
                .iter()
                .any(|site| body.locals[site.place.0].name == name && site.kind == kind)
        );
    }
    let source = body
        .locals
        .iter()
        .position(|local| local.name == "a")
        .unwrap();
    assert!(body.blocks.iter().flat_map(|block| &block.instructions).all(|i| !matches!(i.operation, Operation::Assign { value: Operand::Move(place), .. } if place.0 == source)));
    assert_eq!(
        body.locals
            .iter()
            .find(|local| local.name == "value")
            .unwrap()
            .ty,
        Type::Ref(true, Box::new(Type::u8()))
    );
    let body = compare(
        "fn f(a: &mut[u8], b: bool) { for value in a { if b { r := value\n*r = 1 } } }",
        None,
        &[],
    );
    assert!(
        body.initialization()
            .cleanup
            .iter()
            .any(|site| body.locals[site.place.0].name == "value"
                && site.kind == CleanupKind::Conditional)
    );
}

#[test]
fn foreach_borrow_safety_stays_with_the_ast_checker() {
    compare(
        "fn f(a: [2]u8) { for &value in a { a[0] = value } }",
        Some("borrow"),
        &[],
    );
    // The AST checker also retains its existing temporary-loan restrictions.
    compare(
        "fn f(a: [2]u8) { for value in &mut a[..] { *value = 3 } }",
        Some("overlapping borrows"),
        &[],
    );
    compare(
        "fn f(a: &mut[u8]) { for value in { a } { *value = 2 }\na[0] = 3 }",
        Some("overlapping borrows"),
        &[],
    );
}

#[test]
fn unsupported_foreach_forms_and_late_body_failures_discard_the_graph() {
    for (source, reason) in [
        ("fn f() { for i in 0..2 {} }", "range foreach iteration"),
        (
            "struct S { u8 n }\nfn f(a: [2]S) { for &value in a {} }",
            "copyable",
        ),
        ("fn f(a: &mut[u8]) { for &value in a {} }", "shared"),
        ("fn f(a: &mut[2]u8) { for &value in a {} }", "shared"),
        ("fn f(a: [2]u8) { for &value in &mut a {} }", "shared"),
        (
            "fn f(a: [2]u8) { for &value in a { _ = value as u32 } }",
            "cast",
        ),
        (
            "fn f(a: [2]u8) { for &value in a {}\n_ = 1u8 as u32 }",
            "cast",
        ),
    ] {
        let program = parser::parse(&format!("package experiment\n{source}")).unwrap();
        let limitation = flow::lower(&program, "f").expect_err(source);
        assert!(
            limitation.reason.contains(reason),
            "{limitation:?}: {source}"
        );
    }
}
