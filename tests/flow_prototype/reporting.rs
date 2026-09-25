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
        "package experiment\nfn clean() {}\nfn bad() { u8 x\n_ = x }\nfn array() { _ = [1]u8{1} }";
    let mut program = parser::parse(source).unwrap();
    let report = flow::check_with_report(&mut program, 64);
    assert_eq!(status(&report, "clean"), &BodyStatus::Analyzed);
    let BodyStatus::Diagnostics(issues) = status(&report, "bad") else {
        panic!("{report:?}")
    };
    assert_eq!(issues.len(), 1);
    assert_eq!(&source[issues[0].span.start..issues[0].span.end], "x");
    assert!(source[issues[0].declaration.start..issues[0].declaration.end].contains("u8 x"));
    let BodyStatus::Skipped { scope, limitation } = status(&report, "array") else {
        panic!("{report:?}")
    };
    assert_eq!(*scope, SkipScope::Body);
    assert_eq!(limitation.kind, LimitationKind::Expression);
    assert!(limitation.reason.contains("expression outside subset"));
    assert_eq!(
        &source[limitation.span.start..limitation.span.end],
        "[1]u8{1}"
    );
    assert_eq!(report.diagnostics.len(), 1);
    assert!(report.diagnostics[0].message.contains("uninitialized"));
}

#[test]
fn declaration_restrictions_exclude_even_simple_bodies_without_warnings() {
    for (declaration, kind) in [
        ("import \"core/mem\"", LimitationKind::Imports),
        ("const u8 N = 1", LimitationKind::Constants),
        ("enum E { A }", LimitationKind::Enums),
        ("struct S { &u8 value }", LimitationKind::StructDeclaration),
        (
            "fn borrowed(x: &u8) -> &u8 { return x }",
            LimitationKind::FunctionDeclaration,
        ),
    ] {
        let source = format!("package experiment\n{declaration}\nfn f() {{}}\nfn g() {{}}");
        let program = parser::parse(&source).unwrap();
        let report = flow::check_with_report(&mut program.clone(), 64);
        assert!(report.diagnostics.is_empty(), "{report:?}");
        assert!(sema::check(&mut program.clone()).is_ok());
        for name in ["f", "g"] {
            let BodyStatus::Skipped { scope, limitation } = status(&report, name) else {
                panic!("{report:?}")
            };
            assert_eq!(*scope, SkipScope::Program);
            assert_eq!(limitation.kind, kind);
            assert!(!limitation.reason.is_empty());
            if kind != LimitationKind::Imports {
                assert!(limitation.span.end > limitation.span.start);
                assert!(declaration.contains(&source[limitation.span.start..limitation.span.end]));
            }
        }
        let counts = report.coverage.as_ref().unwrap().skip_counts();
        assert_eq!(counts.len(), 1);
        assert!(counts[&(SkipScope::Program, kind)] >= 2);
    }
}

#[test]
fn coverage_measures_preparation_and_instantiation_not_the_raw_parse() {
    // The unused generic template is removed, and the implicit return is
    // prepared before the production adapter is constructed.
    let program = parse("fn unused<T>(x: T) -> T { x }\nfn f() -> u8 { 1 }");
    assert!(flow::lower(&program, "f").is_err());
    let report = flow::check_with_report(&mut program.clone(), 64);
    assert!(report.diagnostics.is_empty(), "{report:?}");
    assert_eq!(status(&report, "f"), &BodyStatus::Analyzed);
    assert_eq!(report.coverage.as_ref().unwrap().bodies.len(), 1);

    // Instantiation adds a specialized body; its signature currently blocks
    // the adapter for the whole program, including f's simple scalar body.
    let mut program = parse("fn identity<T>(x: T) -> T { x }\nfn f() -> u8 { identity(1u8) }");
    let report = flow::check_with_report(&mut program, 64);
    assert!(report.diagnostics.is_empty(), "{report:?}");
    let instance = program
        .functions
        .iter()
        .find(|f| f.generic_instance)
        .unwrap();
    let coverage = report.coverage.as_ref().unwrap();
    assert_eq!(coverage.bodies.len(), 2);
    assert!(
        coverage
            .bodies
            .iter()
            .any(|body| body.name == instance.name)
    );
    for body in &coverage.bodies {
        let BodyStatus::Skipped { scope, limitation } = &body.status else {
            panic!("{body:?}")
        };
        assert_eq!(*scope, SkipScope::Program);
        assert_eq!(limitation.kind, LimitationKind::FunctionDeclaration);
        assert_eq!(limitation.span, instance.span);
    }
}

#[test]
fn failed_lowering_discards_all_partial_findings_and_does_not_poison_other_bodies() {
    // A prefix graph would diagnose x. A late unsupported array must discard
    // that graph, not analyze it or let it contaminate the next function.
    let program = parse("fn f() { u8 x\n_ = x\n_ = [1]u8{1} }\nfn g() {}");
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

    // A program-wide fallback must also leave AST checking active.
    let program = parse("enum E { A }\nfn f() { u8 x\n_ = x }");
    let comparison = flow::compare_checkers(&program, 64);
    assert!(comparison.flow.diagnostics.is_empty());
    assert!(matches!(
        status(&comparison.flow, "f"),
        BodyStatus::Skipped {
            scope: SkipScope::Program,
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
        let mut loaded =
            package::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture)).unwrap();
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
        let coverage = report.coverage.expect(fixture);
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
