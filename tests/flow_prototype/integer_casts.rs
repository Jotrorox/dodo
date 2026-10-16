use super::compare;
use dodoc::{ast::Type, parser, sema};
use sema::flow::{self, BodyStatus, Operation, Terminator};

#[test]
fn integer_casts_support_all_integer_types_at_both_target_widths() {
    let types = [
        "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "isize",
    ];
    for source in types {
        for destination in types {
            let program = parser::parse(&format!(
                "package experiment\nfn f(value: {source}) -> {destination} {{ converted := value as {destination}\n_ = value\nreturn converted }}"
            )).unwrap();
            for bits in [32, 64] {
                let comparison = flow::compare_checkers(&program, bits);
                for report in [&comparison.ast, &comparison.flow, &comparison.combined] {
                    assert!(
                        report.diagnostics.is_empty(),
                        "{source} -> {destination} at {bits}: {report:?}"
                    );
                }
                assert_eq!(comparison.flow.coverage, comparison.combined.coverage);
                assert_eq!(
                    comparison.flow.coverage.as_ref().unwrap().bodies[0].status,
                    BodyStatus::Analyzed
                );
                sema::check_for_target(&mut program.clone(), bits).unwrap();
                let body = flow::lower_for_target(&program, "f", bits).unwrap();
                assert!(body.initialization().issues.is_empty());
                let parameter = &body.locals[0].ty;
                let converted = &body
                    .locals
                    .iter()
                    .find(|l| l.name == "converted")
                    .unwrap()
                    .ty;
                let Terminator::Return(Some(result)) = &body.blocks[0].terminator else {
                    panic!("{body:?}");
                };
                assert_eq!(converted, &program.functions[0].ret);
                assert_eq!(body.operand_type(result), converted);
                assert_eq!(parameter, &program.functions[0].params[0].ty);
            }
        }
    }
}

#[test]
fn integer_casts_preserve_reads_moves_and_diagnostic_spans() {
    let source = "fn f(b: bool) -> u32 { u8 x\nif b { x = 1 }\nreturn x as u32 }";
    let body = compare(source, Some("uninitialized"), &["x"]);
    let issue = &body.initialization().issues[0];
    let full_source = format!("package experiment\n{source}");
    assert_eq!(&full_source[issue.span.start..issue.span.end], "x");
    assert!(full_source[issue.declaration.start..issue.declaration.end].contains("u8 x"));
    compare(
        "fn f() -> u32 { u8 x\nreturn x as u32 }",
        Some("uninitialized"),
        &["x"],
    );
    compare(
        "struct S { u8 n }\nfn number(s: S) -> u8 { return s.n }\nfn f(s: S) { _ = number(s) as u32\n_ = s }",
        Some("moved"),
        &["s"],
    );
    compare(
        "fn f() -> u32 { u8 x\nreturn { x = 1\nx } as u32 }",
        None,
        &[],
    );
    compare(
        "fn f(b: bool) { u8 x\n_ = b && (({ x = 1\nx } as u32) == 1)\n_ = x }",
        Some("uninitialized"),
        &["x"],
    );
}

#[test]
fn integer_cast_operands_execute_once_in_source_order() {
    let body = compare(
        "fn first() -> u8 { return 1 }\nfn second() -> u8 { return 2 }\nfn take(a: u32, b: i64) {}\nfn f() { take(first() as u32, second() as i64) }",
        None,
        &[],
    );
    let calls: Vec<_> = body
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match &instruction.operation {
            Operation::Call { function, .. } => Some(function.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["first", "second", "take"]);
    compare(
        "fn take(a: u32, b: u32) {}\nfn f() { u8 x\ntake({ x = 1\nx } as u32, x as u32) }",
        None,
        &[],
    );
    compare(
        "fn take(a: u32, b: u32) {}\nfn f() { u8 x\ntake(x as u32, { x = 1\nx } as u32) }",
        Some("uninitialized"),
        &["x"],
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

#[test]
fn integer_casts_do_not_infer_operand_types_from_the_destination() {
    // Narrowing and signedness changes do not range-check the converted value.
    for source in [
        "fn f() -> u8 { return 256 as u8 }",
        "fn f() -> u8 { return -1 as u8 }",
        "fn f() -> u64 { return -128i8 as u64 }",
        "fn f() -> i8 { return 255u8 as i8 }",
    ] {
        compare(source, None, &[]);
    }
    let body = compare("fn f() -> u64 { return 1 as u64 }", None, &[]);
    assert!(body.locals.iter().any(|local| local.ty == Type::isize()));
    for (literal, accepted32, accepted64) in [
        ("2147483647", true, true),
        ("2147483648", false, true),
        ("-2147483648", true, true),
        ("-2147483649", false, true),
        ("4294967296usize", false, true),
        ("4294967296u64", true, true),
        ("256u8", false, false),
    ] {
        let program = parser::parse(&format!(
            "package experiment\nfn f() -> u64 {{ return {literal} as u64 }}"
        ))
        .unwrap();
        for (bits, accepted) in [(32, accepted32), (64, accepted64)] {
            let comparison = flow::compare_checkers(&program, bits);
            for report in [&comparison.ast, &comparison.combined] {
                assert_eq!(
                    report.diagnostics.is_empty(),
                    accepted,
                    "{literal} at {bits}: {report:?}"
                );
                if !accepted {
                    assert!(
                        report.diagnostics[0].message.contains("out of range"),
                        "{report:?}"
                    );
                }
            }
            assert_eq!(comparison.flow.coverage, comparison.combined.coverage);
            assert_eq!(
                flow::lower_for_target(&program, "f", bits).is_ok(),
                accepted
            );
            assert_eq!(
                sema::check_for_target(&mut program.clone(), bits).is_ok(),
                accepted
            );
            assert_eq!(
                comparison.flow.coverage.as_ref().unwrap().bodies[0].status == BodyStatus::Analyzed,
                accepted
            );
        }
    }
}

#[test]
fn non_integer_casts_and_invalid_operands_keep_explicit_fallback() {
    for source in [
        "fn f() { _ = true as u8 }",
        "fn f() { _ = 1u8 as bool }",
        "fn f() { _ = 1u8 as f32 }",
        "fn f() { _ = 1.0 as u32 }",
        "fn f() { x := 1u8\n_ = &x as u32 }",
        "fn f() { x := 1u8\n_ = &x as *const u8 }",
        "fn f() { _ = 1usize as *const u8 }",
        "struct S { u8 n }\nfn f(s: S) { _ = s as u32 }",
        "fn f() { _ = -1u8 as i64 }",
        "fn f() { u8 x = 1u8 as u32 }",
        "fn f() { for i in 0u8..2u8 as u32 {} }",
    ] {
        let program = parser::parse(&format!("package experiment\n{source}")).unwrap();
        assert!(flow::lower(&program, "f").is_err(), "{source}");
        let comparison = flow::compare_checkers(&program, 64);
        assert!(
            matches!(
                comparison.flow.coverage.as_ref().unwrap().bodies[0].status,
                BodyStatus::Skipped { .. }
            ),
            "{source}: {comparison:?}"
        );
    }
}
