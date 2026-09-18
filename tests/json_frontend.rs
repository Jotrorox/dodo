//! JSON is available to the frontend without LLVM or hosted dependencies.
use dodoc::{package, sema};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn check_fixture(relative: &str) {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    let mut loaded = package::load(&source).unwrap_or_else(|error| panic!("{error}"));
    sema::check(&mut loaded.program).unwrap_or_else(|error| panic!("{}", loaded.render(&error)));
}

#[test]
fn json_value_fixture_typechecks() {
    check_fixture("tests/stdlib/json_values.dodo");
}

#[test]
fn json_string_fixture_typechecks() {
    check_fixture("tests/stdlib/json_strings.dodo");
}

#[test]
fn json_indexed_fixture_typechecks() {
    check_fixture("tests/stdlib/json_indexed.dodo");
}

#[test]
fn json_independent_checks_typecheck() {
    check_fixture("tests/stdlib/encoding_json_checks.dodo");
}

#[test]
fn json_example_typechecks() {
    check_fixture("examples/json.dodo");
}

#[test]
fn json_encoder_fixture_typechecks() {
    check_fixture("tests/stdlib/json_encoder.dodo");
}

#[test]
fn json_views_preserve_input_and_destination_borrows() {
    let scratch = std::env::temp_dir().join(format!(
        "dodo-json-borrows-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&scratch).unwrap();
    let source = scratch.join("bad.dodo");
    for body in [
        "fn escape() -> json.Value!json.Error from(static) { input := [110u8,117,108,108]\nscratch := [0usize; 5]\nreturn json.parse_indexed(&input, &mut scratch) }",
        "fn invalid() -> usize!json.Error { input := [110u8,117,108,108]\nscratch := [0usize; 5]\nvalue := json.parse_indexed(&input, &mut scratch)?\ninput[0] = 120\nreturn ok(value.raw().len) }",
        "fn escape() -> json.Value!json.Error from(static) { input := [110u8, 117, 108, 108]\nreturn json.parse(&input) }",
        "fn escape() -> json.String!json.Error from(static) { input := [34u8, 120, 34]\nvalue := json.parse(&input)?\nreturn value.as_string() }",
        "fn escape() -> json.String!json.Error from(static) { input := [34u8, 120, 34]\nvalue := json.parse(&input)?\nreturn value.into_string() }",
        "fn escape() -> &[u8]!json.Error from(static) { input := [110u8,117,108,108]\nvalue := json.parse(&input)?\nreturn ok(value.into_raw()) }",
        "fn escape() -> &str!json.Error from(static) { input := json.parse_str(\"\\\"hello\\\"\")?\nvalue := input.as_string()?\noutput := [0u8; 5]\nreturn value.decode(&mut output) }",
        "fn invalid() -> usize!json.Error { input := [110u8,117,108,108]\nvalue := json.parse(&input)?\ninput[0] = 120\nreturn ok(value.raw().len) }",
        "fn invalid() -> usize!json.Error { decoder := json.Decoder.from_str(\"null true\")\nfirst := decoder.next()?\nsecond := decoder.next()?\ncore.drop(second)\nmatch first { some(value) => { return ok(value.position()) }, none => { return ok(0) } } }",
    ] {
        fs::write(
            &source,
            format!("package bad\nimport \"std/encoding/json\"\n{body}\n"),
        )
        .unwrap();
        let mut loaded = package::load(&source).unwrap_or_else(|error| panic!("{error}"));
        let error = sema::check(&mut loaded.program)
            .expect_err("JSON view escaped or invalidated its backing storage");
        assert!(
            error.message.contains("borrow"),
            "{}",
            loaded.render(&error)
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
