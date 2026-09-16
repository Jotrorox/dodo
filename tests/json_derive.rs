//! JSON derive expansion remains ordinary syntax and checked generic dispatch.
use dodoc::{ast::Type, format, package, parser, sema};

#[test]
fn derives_checked_methods_and_a_hygienic_import() {
    let program = parser::parse("package demo\nconst __dodo_json_derive: i32 = 1\n@derive(Json)\nstruct Record {\nname: &str\ncount: u16\nactive: bool\nvalues: [2]Option<i32>\n}\n").unwrap();
    assert_eq!(program.import_aliases[0].1, "__dodo_json_derive_");
    assert_eq!(program.functions.len(), 2);
    let encode = &program.functions[0];
    assert_eq!(encode.name, "Record.encode_json");
    assert_eq!(encode.generics, ["__JsonWriter"]);
    assert!(matches!(&encode.params[0].ty, Type::Ref(false, _)));
    let decode = &program.functions[1];
    assert_eq!(decode.name, "Record.decode_json");
    assert!(
        decode.from.is_empty(),
        "borrow checker infers the sole borrowed parameter only when the return type borrows"
    );
}

#[test]
fn reuse_explicit_json_import_alias_and_preserve_public_visibility() {
    let program = parser::parse("package demo\nimport \"std/encoding/json\" as wire\n@derive(Json)\npub struct Record { name: wire.String }\n").unwrap();
    assert_eq!(program.imports.len(), 1);
    assert!(program.functions.iter().all(|function| function.public));
    assert!(format!("{:?}", program.functions).contains("wire.Encoder"));
}

#[test]
fn attributes_and_unsupported_fields_fail_at_the_declaration() {
    for (source, expected) in [
        ("@derive(Json)\nfn sample() {}", "apply only to structs"),
        ("@derive(Other)\nstruct Record {}", "supported derive"),
        (
            "@derive(Json)\n@derive(Json)\nstruct Record {}",
            "duplicate @derive",
        ),
        ("@json_deny_unknown\nstruct Record {}", "requires @derive"),
        (
            "struct Record { @json_name(\"x\")\nx: i32 }",
            "applies only to fields",
        ),
        (
            "@derive(Json)\nstruct Record { data: &[u8] }",
            "does not support field type",
        ),
        (
            "@derive(Json)\nstruct Record { pointer: *const u8 }",
            "does not support field type",
        ),
        (
            "@derive(Json)\nstruct Record<T> { value: T }",
            "requires a concrete struct",
        ),
        (
            "@derive(Json)\nstruct Record { data: [N]u8 }",
            "integer literals",
        ),
        (
            "@derive(Json)\nstruct Record { data: [4096][4096]u8 }",
            "exceeds the supported total size",
        ),
        (
            "@derive(Json)\nstruct Record { @json_name(\"x\")\na: i32\nx: i32 }",
            "duplicate JSON field name",
        ),
        (
            "@derive(Json)\nstruct Record { fn encode_json() {} }",
            "conflicts with existing",
        ),
    ] {
        let source = format!("package demo\n{source}\n");
        let error = parser::parse(&source).unwrap_err();
        assert!(
            error.message.contains(expected),
            "expected {expected:?}, got {}",
            error.message
        );
    }
}

#[test]
fn editor_retains_declarations_when_a_derive_is_invalid() {
    let (program, diagnostics) = parser::parse_recovering(
        "package demo\n@derive(Json)\nstruct Record { raw: *const u8 }\nfn later() -> i32 { return 1 }\n",
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(program.structs.len(), 1);
    assert!(
        program
            .functions
            .iter()
            .any(|function| function.name == "later")
    );
}

#[test]
fn derived_arrays_and_custom_codecs_typecheck() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/stdlib/json_derive.dodo");
    let mut loaded = package::load(&path).unwrap_or_else(|error| panic!("{error}"));
    sema::check(&mut loaded.program).unwrap_or_else(|error| panic!("{}", loaded.render(&error)));
}

#[test]
fn derived_codecs_work_across_packages_with_import_aliases() {
    let directory =
        std::env::temp_dir().join(format!("dodo-json-derive-imports-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("model.dodo"), "package model\nimport \"std/encoding/json\" as wire\n@derive(Json)\npub struct User {\npub name: wire.String\npub active: bool\n}\n").unwrap();
    std::fs::write(directory.join("main.dodo"), "package app\nimport \"model\" as records\nimport \"std/encoding/json\" as wire\nfn main() -> bool!wire.Error {\nuser := wire.decode<records.User>(b\"{\\\"name\\\":\\\"Ada\\\",\\\"active\\\":true}\")?\nreturn ok(user.name.equals(\"Ada\") && user.active)\n}\n").unwrap();
    let result = package::load(&directory.join("main.dodo"));
    std::fs::remove_dir_all(&directory).unwrap();
    let mut loaded = result.unwrap_or_else(|error| panic!("{error}"));
    sema::check(&mut loaded.program).unwrap_or_else(|error| panic!("{}", loaded.render(&error)));
}

#[test]
fn generated_names_do_not_shadow_import_aliases_or_user_types() {
    let directory =
        std::env::temp_dir().join(format!("dodo-json-derive-hygiene-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.dodo");
    let mut cases = Vec::new();
    for alias in [
        "value",
        "encoder",
        "__JsonWriter",
        "__json_keys",
        "__json_0",
        "self",
    ] {
        cases.push(format!("package audit\nimport \"std/encoding/json\" as {alias}\n@derive(Json)\nstruct Record {{ count: u32 }}\nfn main() -> usize!{alias}.Error {{\nrecord := {alias}.decode<Record>(b\"{{\\\"count\\\":1}}\")?\noutput := [0u8;32]\nreturn {alias}.to_slice(&record, &mut output)\n}}\n"));
    }
    cases.push("package audit\nimport \"std/encoding/json\"\n@derive(Json)\nstruct __JsonWriter { count: u32 }\n@derive(Json)\nstruct __json_0 { inner: __JsonWriter }\n@derive(Json)\nstruct __json_keys { inner: __json_0 }\n@derive(Json)\nstruct value { inner: __json_keys }\nfn main() -> usize!json.Error {\nrecord := json.decode<value>(b\"{\\\"inner\\\":{\\\"inner\\\":{\\\"inner\\\":{\\\"count\\\":1}}}}\")?\noutput := [0u8;128]\nreturn json.to_slice(&record, &mut output)\n}\n".to_owned());
    for source in cases {
        std::fs::write(&path, &source).unwrap();
        let mut loaded = package::load(&path).unwrap_or_else(|error| panic!("{error}"));
        sema::check(&mut loaded.program)
            .unwrap_or_else(|error| panic!("{}", loaded.render(&error)));
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn implicit_derive_imports_coexist_across_sibling_files() {
    let directory =
        std::env::temp_dir().join(format!("dodo-json-derive-siblings-{}", std::process::id()));
    let models = directory.join("models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(directory.join("main.dodo"), "package app\nimport \"models\"\nimport \"std/encoding/json\"\nfn main() -> usize!json.Error {\na := json.decode<models.First>(b\"{\\\"number\\\":1}\")?\nb := json.decode<models.Second>(b\"{\\\"number\\\":2}\")?\noutput := [0u8;32]\njson.to_slice(&a, &mut output)?\nreturn json.to_slice(&b, &mut output)\n}\n").unwrap();
    std::fs::write(
        models.join("second.dodo"),
        "package models\n@derive(Json)\npub struct Second { pub number: u32 }\n",
    )
    .unwrap();
    for imports in [
        "const __dodo_json_derive: u32 = 0\nconst __dodo_import_0: u32 = 0\n",
        "import \"std/encoding/json\" as value\n",
    ] {
        std::fs::write(
            models.join("first.dodo"),
            format!(
                "package models\n{imports}@derive(Json)\npub struct First {{ pub number: u32 }}\n"
            ),
        )
        .unwrap();
        let mut loaded =
            package::load(&directory.join("main.dodo")).unwrap_or_else(|error| panic!("{error}"));
        sema::check(&mut loaded.program)
            .unwrap_or_else(|error| panic!("{}", loaded.render(&error)));
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn derives_do_not_allow_borrowed_strings_to_escape_local_input() {
    let directory =
        std::env::temp_dir().join(format!("dodo-json-derive-borrows-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("main.dodo");
    std::fs::write(&source, "package bad\nimport \"std/encoding/json\"\n@derive(Json)\nstruct User { name: Option<json.String> }\nfn escape() -> User!json.Error from(static) {\ninput := [123u8,34,110,97,109,101,34,58,34,120,34,125]\nreturn json.decode<User>(&input)\n}\n").unwrap();
    let result = package::load(&source);
    std::fs::remove_dir_all(&directory).unwrap();
    let mut loaded = result.unwrap_or_else(|error| panic!("{error}"));
    let error = sema::check(&mut loaded.program)
        .expect_err("derived borrowed field escaped local JSON input");
    assert!(
        error.message.contains("borrow"),
        "{}",
        loaded.render(&error)
    );
}

#[test]
fn formatting_preserves_derive_and_escaped_renames() {
    let source = "package demo\n@derive(Json)\n@json_deny_unknown\nstruct Record {\n@json_name(\"na\\\"me\\n\\u{263a}\")\nvalue:i32\nitems:[2]Option<i64>\n}\n";
    let formatted = format::format_source(source).unwrap();
    assert!(formatted.contains("@derive(Json)"));
    assert!(formatted.contains("@json_deny_unknown"));
    assert!(formatted.contains("@json_name"));
    assert_eq!(format::format_source(&formatted).unwrap(), formatted);
}

#[test]
fn generic_static_method_dispatch_uses_the_concrete_type() {
    let source = "package generic_dispatch\nstruct Number {\nvalue: i32\nfn make(value: i32) -> Self { return Self{value: value} }\n}\nstruct Wrapper<T> {\nvalue: T\nfn make(value: T) -> Self { return Self{value: value} }\n}\nfn make<T, V>(value: V) -> T { return T.make(value) }\nfn main() -> i32 {\na := make<Number, i32>(7)\nb := make<Wrapper<i32>, i32>(9)\nreturn a.value + b.value\n}\n";
    let mut program = parser::parse(source).unwrap();
    sema::check(&mut program)
        .unwrap_or_else(|error| panic!("{}", error.render("generic_dispatch.dodo", source)));
}

#[test]
fn public_protocol_methods_can_use_their_private_owner_type() {
    let source = "package protocol\nstruct Private {\npub fn make() -> Self { return Self{} }\n}\n";
    let mut program = parser::parse(source).unwrap();
    sema::check(&mut program).unwrap();
    for source in [
        "package bad\nstruct Private {}\npub fn expose() -> Private { return Private{} }\n",
        "package bad\nstruct Private {}\nstruct Other { pub fn expose() -> Private { return Private{} } }\n",
    ] {
        let mut program = parser::parse(source).unwrap();
        let error = sema::check(&mut program).unwrap_err();
        assert!(error.message.contains("public API exposes private type"));
    }
}
