use dodoc::toml::{self, Kind, Value};
fn field<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(value, |v, k| &v.as_table().unwrap()[*k])
}
#[test]
fn manifest_strings_and_arrays_preserve_argument_boundaries() {
    let v=toml::parse("schema=1\n[targets.'my-app'.run]\nargs=[\n'--port',\n\"hello world\", # comment\n\"\\u03B1\\t\\U0001F426\",\n]\ncwd='C:\\work'\n").unwrap();
    let Kind::Array(args) = &field(&v, &["targets", "my-app", "run", "args"]).kind else {
        panic!()
    };
    assert_eq!(args[1].kind, Kind::String("hello world".into()));
    assert_eq!(args[2].kind, Kind::String("α\t🐦".into()));
}
#[test]
fn multiline_strings_newlines_quotes_and_continuations() {
    let v = toml::parse(
        "a=\"\"\"\r\nhello \\\r\n  world\"\"\"\nb='''\nraw\\path\n'''\nc=\"\"\"\"quote\"\"\"\"\n",
    )
    .unwrap();
    assert_eq!(field(&v, &["a"]).kind, Kind::String("hello world".into()));
    assert_eq!(field(&v, &["b"]).kind, Kind::String("raw\\path\n".into()));
    assert_eq!(field(&v, &["c"]).kind, Kind::String("\"quote\"".into()));
}
#[test]
fn integers_floats_and_limits() {
    let v=toml::parse("a=-9223372036854775808\nb=0x7fff_ffff_ffff_ffff\nc=+3.25e-2\nd=-inf\ne=nan\nf=0b10_10\ng=0o71\n").unwrap();
    assert_eq!(field(&v, &["a"]).kind, Kind::Integer(i64::MIN));
    assert_eq!(field(&v, &["b"]).kind, Kind::Integer(i64::MAX));
    assert_eq!(field(&v, &["c"]).kind, Kind::Float(0.0325));
    for s in [
        "01",
        "1_",
        "1__2",
        "0x_1",
        "0b2",
        "+0x1",
        "9223372036854775808",
        "1.",
        ".1",
        "1e_2",
        "+nanx",
    ] {
        assert!(toml::parse(&format!("x={s}")).is_err(), "{s}");
    }
}
#[test]
fn dates_are_validated_and_retained_without_timezone_conversion() {
    for s in [
        "2024-02-29",
        "2024-02-29 12:34:56.123+02:30",
        "2024-02-29t12:34:56z",
        "00:00:00.1",
    ] {
        assert!(matches!(
            field(&toml::parse(&format!("x={s}")).unwrap(), &["x"]).kind,
            Kind::DateTime(_)
        ));
    }
    for s in [
        "2023-02-29",
        "2024-13-01",
        "2024-01-32",
        "24:00:00",
        "12:60:00",
        "12:00:00Z",
        "2024-01-01T12:00:00+24:00",
    ] {
        assert!(toml::parse(&format!("x={s}")).is_err(), "{s}");
    }
}
#[test]
fn tables_have_distinct_declaration_rules() {
    for s in [
        "a=1\na=2",
        "[a]\n[a]",
        "a.b=1\n[a]",
        "a={}\na.b=1",
        "a=[]\n[[a]]",
        "a=1\n[a.b]",
        "a={b=1,}",
        "a={b=1\n}",
        "[a.b]\n[a]\nb.c=2",
    ] {
        assert!(toml::parse(s).is_err(), "{s}");
    }
    let v=toml::parse("[a.b]\nx=1\n[a]\ny=2\n[[items]]\nname='first'\n[items.detail]\nx=3\n[[items]]\nname='second'\n").unwrap();
    let Kind::Array(items) = &field(&v, &["items"]).kind else {
        panic!()
    };
    assert_eq!(items.len(), 2);
    assert_eq!(field(&items[0], &["detail", "x"]).kind, Kind::Integer(3));
    assert!(items[1].as_table().unwrap().get("detail").is_none());
}
#[test]
fn malformed_unicode_controls_and_trailing_content_are_errors() {
    for s in [
        "a=\"\\uD800\"",
        "a=\"\\UFFFFFFFF\"",
        "a=\"\\x00\"",
        "a='line\n'",
        "a=true false",
        "# bad\u{7f}",
        "a='\u{0}'",
        "a=1\r",
        "a=\"unterminated",
        "a=[1 2]",
    ] {
        assert!(toml::parse(s).is_err(), "{s:?}");
    }
    let source = "# α\nname = \"\\uD800\"";
    let error = toml::parse(source)
        .unwrap_err()
        .render(source, std::path::Path::new("dodo.toml"));
    assert!(error.starts_with("dodo.toml:2:"), "{error}");
}
#[test]
fn nesting_is_bounded_and_basic_string_encoding_round_trips() {
    assert!(toml::parse(&format!("a={}0{}", "[".repeat(100), "]".repeat(100))).is_err());
    assert!(toml::parse(&format!("{}=1", vec!["a"; 100].join("."))).is_err());
    assert!(
        toml::parse(&format!(
            "[{}]\na={}0{}",
            vec!["a"; 40].join("."),
            "[".repeat(40),
            "]".repeat(40)
        ))
        .is_err()
    );
    let s = "\0\u{1}\u{8}\u{c}\u{7f}\n\r\tα🐦\"\\";
    let v = toml::parse(&format!("s={}", toml::quote(s))).unwrap();
    assert_eq!(field(&v, &["s"]).kind, Kind::String(s.into()));
}
#[test]
fn arbitrary_utf8_input_does_not_panic() {
    let chars = [
        'a', '1', '[', ']', '{', '}', '\'', '"', '\\', '=', '\n', '\r', '\0', 'α', '🐦', '.', ',',
        '#',
    ];
    let mut seed = 7u64;
    for length in 0..128 {
        for _ in 0..100 {
            let mut input = String::new();
            for _ in 0..length {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                input.push(chars[(seed >> 32) as usize % chars.len()]);
            }
            let _ = toml::parse(&input);
        }
    }
}
