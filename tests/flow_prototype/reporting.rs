use dodoc::{ast::Program, package, parser, sema};
use sema::flow::{self, BodyStatus, LimitationKind, SkipScope};

fn parse(source: &str) -> Program {
    parser::parse(&format!("package experiment\n{source}")).unwrap()
}

fn status<'a>(report: &'a flow::CheckReport, name: &str) -> &'a BodyStatus {
    &report
        .coverage
        .as_ref()
        .expect("production flow integration reached")
        .bodies
        .iter()
        .find(|body| body.name == name)
        .unwrap()
        .status
}

fn messages(report: &flow::CheckReport) -> Vec<&str> {
    report
        .diagnostics
        .iter()
        .map(|d| d.message.as_ref())
        .collect()
}

#[test]
fn coverage_distinguishes_success_findings_and_body_fallback() {
    let source =
        "package experiment\nfn clean() {}\nfn bad() { u8 x\n_ = x }\nfn cast() { _ = 1u8 as u32 }";
    let mut program = parser::parse(source).unwrap();
    let report = flow::check_with_report(&mut program, 64);
    assert_eq!(status(&report, "clean"), &BodyStatus::Analyzed);
    let BodyStatus::Diagnostics(issues) = status(&report, "bad") else {
        panic!("{report:?}")
    };
    assert_eq!(issues.len(), 1);
    assert_eq!(&source[issues[0].span.start..issues[0].span.end], "x");
    assert!(source[issues[0].declaration.start..issues[0].declaration.end].contains("u8 x"));
    let BodyStatus::Skipped { scope, limitation } = status(&report, "cast") else {
        panic!("{report:?}")
    };
    assert_eq!(*scope, SkipScope::Body);
    assert_eq!(limitation.kind, LimitationKind::Expression);
    assert!(limitation.reason.contains("expression outside subset"));
    assert_eq!(
        &source[limitation.span.start..limitation.span.end],
        "1u8 as u32"
    );
    assert_eq!(report.diagnostics.len(), 1);
    assert!(report.diagnostics[0].message.contains("uninitialized"));
}

#[test]
fn unrelated_declarations_do_not_exclude_supported_bodies() {
    for declaration in [
        "import \"core/mem\"",
        "const u8 N = 1",
        "enum E { A }",
        "struct S { &u8 value }",
        "struct S { u8 n\nfn drop(&mut self) {} }",
        "fn borrowed(x: &u8) -> &u8 { return x }",
        "unsafe fn dangerous() {}",
        "extern \"C\" fn external()",
    ] {
        let source =
            format!("package experiment\n{declaration}\nfn f() {{}}\nfn g() {{ u8 x\n_ = x }}");
        let program = parser::parse(&source).unwrap();
        assert!(flow::lower(&program, "f").is_ok(), "{declaration}");
        let comparison = flow::compare_checkers(&program, 64);
        assert_eq!(status(&comparison.flow, "f"), &BodyStatus::Analyzed);
        assert!(matches!(
            status(&comparison.flow, "g"),
            BodyStatus::Diagnostics(_)
        ));
        assert_eq!(comparison.flow.diagnostics.len(), 1);
        assert_eq!(messages(&comparison.combined), messages(&comparison.ast));
        assert_eq!(comparison.combined.diagnostics.len(), 1);
        assert!(
            comparison.combined.diagnostics[0]
                .message
                .contains("uninitialized")
        );
        let coverage = comparison.flow.coverage.as_ref().unwrap();
        assert!(
            coverage
                .skip_counts()
                .keys()
                .all(|(scope, _)| *scope == SkipScope::Body)
        );
    }
}

#[test]
fn referenced_declarations_fall_back_only_for_the_dependent_body() {
    for (declaration, body, kind) in [
        (
            "const u8 N = 1",
            "_ = N",
            LimitationKind::UnsupportedConstruct,
        ),
        ("enum E { A(&u8) }", "E value", LimitationKind::Enums),
        (
            "struct S { &u8 value }",
            "S value",
            LimitationKind::StructDeclaration,
        ),
        (
            "struct S { u8 n\nfn drop(&mut self) {} }",
            "_ = S{n: 1}",
            LimitationKind::StructDeclaration,
        ),
        (
            "fn borrowed(x: &u8) -> &u8 { return x }",
            "x := 1u8\n_ = borrowed(&x)",
            LimitationKind::FunctionDeclaration,
        ),
        (
            "extern \"C\" fn external()",
            "external()",
            LimitationKind::FunctionDeclaration,
        ),
    ] {
        let program = parse(&format!("{declaration}\nfn f() {{ {body} }}\nfn g() {{}}"));
        let comparison = flow::compare_checkers(&program, 64);
        let BodyStatus::Skipped { scope, limitation } = status(&comparison.flow, "f") else {
            panic!("{comparison:?}");
        };
        assert_eq!(*scope, SkipScope::Body);
        assert_eq!(limitation.kind, kind);
        assert!(!limitation.reason.is_empty());
        assert!(limitation.span.end > limitation.span.start);
        assert_eq!(status(&comparison.flow, "g"), &BodyStatus::Analyzed);
        assert_eq!(messages(&comparison.combined), messages(&comparison.ast));
    }
}

#[test]
fn supported_callee_signatures_do_not_require_supported_bodies() {
    let program = parse(
        "fn callee(x: u8) -> u8 { _ = 1u8 as u32\nreturn x }\nfn f() -> u8 { return callee(1) }",
    );
    let comparison = flow::compare_checkers(&program, 64);
    assert!(comparison.combined.diagnostics.is_empty(), "{comparison:?}");
    assert!(matches!(
        status(&comparison.flow, "callee"),
        BodyStatus::Skipped {
            scope: SkipScope::Body,
            ..
        }
    ));
    assert_eq!(status(&comparison.flow, "f"), &BodyStatus::Analyzed);
}

#[test]
fn raw_lowering_validates_every_referenced_struct_and_callee() {
    for declaration in [
        "struct S { &u8 n }",
        "struct S<T> { u8 n }",
        "struct S { u8 n\nu8 n }",
        "struct S { u8 n }\nstruct S { u8 n }",
        "struct S { u8 n\nfn drop(&mut self) {} }",
    ] {
        for function in [
            "fn f(s: S) {}",
            "fn f(s: &S) {}",
            "fn f() -> S { return S{n: 1} }",
            "fn f() { S s }",
            "fn f() { _ = S{n: 1} }",
            "fn make() -> S { return S{n: 1} }\nfn f() { _ = make() }",
        ] {
            let program = parse(&format!("{declaration}\n{function}\nfn independent() {{}}"));
            assert_eq!(
                flow::lower(&program, "f").unwrap_err().kind,
                LimitationKind::StructDeclaration
            );
            assert!(flow::lower(&program, "independent").is_ok());
        }
    }
    for declaration in [
        "unsafe fn callee() {}",
        "extern \"C\" fn callee()",
        "fn callee<T>() {}",
        "fn callee() -> &u8 { u8 x\nreturn &x }",
        "fn callee(x: &[&u8]) {}",
        "fn callee() {}\nfn callee() {}",
        "struct callee {}\nfn callee() {}",
    ] {
        let program = parse(&format!(
            "{declaration}\nfn f() {{ callee() }}\nfn independent() {{}}"
        ));
        let limitation = flow::lower(&program, "f").unwrap_err();
        assert!(matches!(
            limitation.kind,
            LimitationKind::FunctionDeclaration | LimitationKind::Type
        ));
        assert!(flow::lower(&program, "independent").is_ok());
    }
}

#[test]
fn recursive_calls_need_only_supported_signatures() {
    let program = parse("fn f(b: bool) { if b { g(b) } }\nfn g(b: bool) { f(b) }");
    assert!(flow::lower(&program, "f").is_ok());
    assert!(flow::lower(&program, "g").is_ok());
    assert_eq!(
        flow::lower_for_target(&program, "f", 16).unwrap_err().kind,
        LimitationKind::Target
    );
}

#[test]
fn coverage_measures_preparation_and_instantiation_not_the_raw_parse() {
    // The unused generic template no longer blocks raw lowering of f, and is
    // removed before production coverage is collected.
    let program = parse("fn unused<T>(x: T) -> T { x }\nfn f() -> u8 { 1 }");
    assert!(flow::lower(&program, "f").is_ok());
    assert!(flow::lower(&program, "unused").is_err());
    let report = flow::check_with_report(&mut program.clone(), 64);
    assert!(report.diagnostics.is_empty(), "{report:?}");
    assert_eq!(status(&report, "f"), &BodyStatus::Analyzed);
    assert_eq!(report.coverage.as_ref().unwrap().bodies.len(), 1);

    // Instantiation adds a specialized body. It and its caller fall back,
    // while an independent body still lowers.
    let mut program = parse(
        "fn identity<T>(x: T) -> T { x }\nfn f() -> u8 { identity(1u8) }\nfn independent() {}",
    );
    let report = flow::check_with_report(&mut program, 64);
    assert!(report.diagnostics.is_empty(), "{report:?}");
    let instance = program
        .functions
        .iter()
        .find(|f| f.generic_instance)
        .unwrap();
    let coverage = report.coverage.as_ref().unwrap();
    assert_eq!(coverage.bodies.len(), 3);
    assert_eq!(status(&report, "independent"), &BodyStatus::Analyzed);
    assert!(
        coverage
            .bodies
            .iter()
            .any(|body| body.name == instance.name)
    );
    for body in coverage
        .bodies
        .iter()
        .filter(|body| body.name != "independent")
    {
        let BodyStatus::Skipped { scope, limitation } = &body.status else {
            panic!("{body:?}")
        };
        assert_eq!(*scope, SkipScope::Body);
        assert_eq!(limitation.kind, LimitationKind::FunctionDeclaration);
        assert_eq!(limitation.span, instance.span);
    }
}

#[test]
fn failed_lowering_discards_all_partial_findings_and_does_not_poison_other_bodies() {
    // A prefix graph would diagnose x. A late unsupported cast must discard
    // that graph, not analyze it or let it contaminate the next function.
    let program = parse("fn f() { u8 x\n_ = x\n_ = 1u8 as u32 }\nfn g() {}");
    assert!(flow::lower(&program, "f").is_err());
    let comparison = flow::compare_checkers(&program, 64);
    assert!(comparison.flow.diagnostics.is_empty());
    assert!(matches!(
        status(&comparison.flow, "f"),
        BodyStatus::Skipped {
            scope: SkipScope::Body,
            ..
        }
    ));
    assert_eq!(status(&comparison.flow, "g"), &BodyStatus::Analyzed);
    assert_eq!(comparison.flow.coverage, comparison.combined.coverage);
    assert_eq!(comparison.ast.diagnostics.len(), 1);
    assert!(
        comparison.ast.diagnostics[0]
            .message
            .contains("uninitialized")
    );
    assert_eq!(messages(&comparison.combined), messages(&comparison.ast));
    assert!(
        sema::check(&mut program.clone())
            .unwrap_err()
            .message
            .contains("uninitialized")
    );

    // A referenced unsupported declaration also leaves AST checking active,
    // and discards initialization findings from the already lowered prefix.
    let program = parse("enum E { A(&u8) }\nfn f() { u8 x\n_ = x\nE value }");
    let comparison = flow::compare_checkers(&program, 64);
    assert!(comparison.flow.diagnostics.is_empty());
    assert!(matches!(
        status(&comparison.flow, "f"),
        BodyStatus::Skipped {
            scope: SkipScope::Body,
            ..
        }
    ));
    assert_eq!(comparison.combined.diagnostics.len(), 1);
    assert!(
        comparison.combined.diagnostics[0]
            .message
            .contains("uninitialized")
    );
}

#[test]
fn ast_diagnostics_win_without_duplicates_even_when_flow_also_finds_errors() {
    let program = parse(
        "struct S { u8 n }\nfn take(s: S) {}\nfn f(s: S, b: bool) { for b { take(s) } }\nfn g() { u8 x\n_ = x }",
    );
    let comparison = flow::compare_checkers(&program, 64);
    assert_eq!(comparison.ast.diagnostics.len(), 2);
    assert_eq!(comparison.flow.diagnostics.len(), 2);
    assert_eq!(comparison.combined.diagnostics.len(), 2);
    assert!(
        comparison.ast.diagnostics[0]
            .message
            .contains("moved in a loop")
    );
    assert!(comparison.flow.diagnostics[0].message.contains("moved"));
    assert_ne!(
        comparison.ast.diagnostics[0].message,
        comparison.flow.diagnostics[0].message
    );
    assert_eq!(messages(&comparison.combined), messages(&comparison.ast));
    assert_eq!(
        sema::check(&mut program.clone()).unwrap_err().message,
        comparison.ast.diagnostics[0].message
    );
    let normal = sema::check_recovering(&mut program.clone(), 64);
    assert_eq!(
        normal
            .iter()
            .map(|d| d.message.as_ref())
            .collect::<Vec<&str>>(),
        messages(&comparison.combined)
    );
}

#[test]
fn borrowing_rejection_is_not_an_initialization_finding() {
    let program = parse("fn observe(x: &u8) {}\nfn f() { x := 1u8\nr := &x\nx = 2\nobserve(r) }");
    let comparison = flow::compare_checkers(&program, 64);
    assert_eq!(status(&comparison.flow, "f"), &BodyStatus::Analyzed);
    assert!(comparison.flow.diagnostics.is_empty());
    for report in [&comparison.ast, &comparison.combined] {
        assert_eq!(report.diagnostics.len(), 1);
        assert!(report.diagnostics[0].message.contains("live shared borrow"));
    }
}

#[test]
fn coverage_uses_the_selected_target_width() {
    let program = parse("fn f() { _ = 4294967296usize }");
    let wide = flow::check_with_report(&mut program.clone(), 64);
    assert!(wide.diagnostics.is_empty());
    assert_eq!(status(&wide, "f"), &BodyStatus::Analyzed);
    let narrow = flow::check_with_report(&mut program.clone(), 32);
    let BodyStatus::Skipped { scope, limitation } = status(&narrow, "f") else {
        panic!("{narrow:?}")
    };
    assert_eq!(*scope, SkipScope::Body);
    assert!(limitation.reason.contains("target range"));
    // The supplementary checker skipping this literal does not permit it.
    assert_eq!(narrow.diagnostics.len(), 1);
}

#[test]
fn absent_coverage_is_not_a_successful_empty_analysis() {
    let mut invalid = parse("fn f(x: Missing) {}");
    let report = flow::check_with_report(&mut invalid, 64);
    assert!(!report.diagnostics.is_empty());
    assert!(report.coverage.is_none());
    let report = flow::check_with_report(&mut parse(""), 64);
    assert!(report.diagnostics.is_empty());
    assert!(report.coverage.unwrap().bodies.is_empty());

    let mut program =
        parse("extern \"C\" fn external()\nfn excluded() requires_plain(&u8) {}\nfn f() {}");
    let report = flow::check_with_report(&mut program, 64);
    assert!(report.diagnostics.is_empty(), "{report:?}");
    assert_eq!(status(&report, "external"), &BodyStatus::NoBody);
    assert_eq!(status(&report, "excluded"), &BodyStatus::Unavailable);
    assert!(!program.functions.iter().any(|f| f.name == "excluded"));
}

#[test]
fn representative_corpus_coverage() {
    use std::collections::BTreeMap;
    use std::path::Path;
    let mut totals = BTreeMap::new();
    let (mut analyzed, mut findings, mut skipped) = (0, 0, 0);
    // Use the package loader (including bundled imports), then the actual
    // recovering production pipeline, not independently lowered raw parses.
    // Pin the corpus target so hosted imports and counts are host-independent.
    for (fixture, expected_error) in [
        ("examples/fibonacci.dodo", None),
        ("examples/patterns.dodo", None),
        ("examples/borrowing.dodo", None),
        ("examples/hello.dodo", None),
        (
            "examples/diagnostics/borrow_conflict.dodo",
            Some("live shared borrow"),
        ),
        ("examples/diagnostics/moved_value.dodo", Some("moved")),
        ("examples/diagnostics/return_source.dodo", Some("return")),
    ] {
        let mut loaded = package::load_for_target(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture),
            "x86_64-unknown-linux-gnu",
        )
        .unwrap();
        let report = flow::check_with_report(&mut loaded.program, 64);
        match expected_error {
            None => assert!(
                report.diagnostics.is_empty(),
                "{fixture}: {:?}",
                report.diagnostics
            ),
            Some(message) => assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|d| d.message.contains(message)),
                "{fixture}: {:?}",
                report.diagnostics
            ),
        }
        if fixture == "examples/patterns.dodo" {
            for name in ["unpack", "category"] {
                assert_eq!(status(&report, name), &BodyStatus::Analyzed);
            }
        }
        if fixture == "examples/hello.dodo" {
            // These loaded dependency bodies include qualified direct calls.
            // Import provenance and package qualification are not limitations.
            for name in [
                "ascii.is_ascii",
                "ascii.is_alphabetic",
                "ascii.to_uppercase",
                "bytes.starts_with",
                "bytes.ends_with",
                "bytes.copy_from",
                "ascii.digit_value",
                "ascii.hex_value",
                "num.checked_sub",
                "num.checked_div",
                "num.checked_rem",
                "num.align_up",
                "io.failure",
                "text.validate",
                "text.encoded_len",
                "text.parse_u64",
            ] {
                assert_eq!(status(&report, name), &BodyStatus::Analyzed);
            }
            let BodyStatus::Skipped { limitation, .. } = status(&report, "io.transfer") else {
                panic!("expected mutable reborrow fallback for io.transfer");
            };
            assert!(limitation.reason.contains("implicit mutable reborrow"));
            // Preserve coverage from the range-loop extension on main.
            for name in [
                "bytes.equal",
                "bytes.compare",
                "bytes.fill",
                "bytes.reverse",
            ] {
                assert_eq!(status(&report, name), &BodyStatus::Analyzed, "{name}");
            }
            assert!(matches!(
                status(&report, "console.Input.read"),
                BodyStatus::Skipped {
                    scope: SkipScope::Body,
                    limitation: flow::Limitation {
                        kind: LimitationKind::FunctionDeclaration,
                        ..
                    },
                }
            ));
            assert!(
                loaded
                    .program
                    .functions
                    .iter()
                    .any(|f| f.name == "ascii.is_ascii" && f.imported)
            );
        }
        let coverage = report.coverage.expect(fixture);
        assert!(
            coverage
                .skip_counts()
                .keys()
                .all(|(scope, _)| *scope == SkipScope::Body)
        );
        for body in &coverage.bodies {
            println!(
                "{fixture}: {} @ {:?}: {:?}",
                body.name, body.span, body.status
            );
            match &body.status {
                BodyStatus::Analyzed => analyzed += 1,
                BodyStatus::Diagnostics(_) => findings += 1,
                BodyStatus::Skipped { .. } => skipped += 1,
                BodyStatus::NoBody | BodyStatus::Unavailable => (),
            }
        }
        for (key, count) in coverage.skip_counts() {
            *totals.entry(key).or_insert(0) += count;
        }
    }
    println!("flow bodies: {analyzed} clean, {findings} with findings, {skipped} skipped");
    let mut ranked: Vec<_> = totals.into_iter().collect();
    ranked.sort_by_key(|(key, count)| (std::cmp::Reverse(*count), *key));
    for (key, count) in ranked {
        println!("{count} skipped bodies: {key:?}");
    }
    assert!(analyzed > 0 && findings > 0 && skipped > 0);
}
