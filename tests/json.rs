//! Independent wire fixtures plus generated coverage for the internal JSON codec.
use dodoc::json::{Value, json};
use std::collections::BTreeMap;

#[test]
fn parses_each_value_kind_and_json_whitespace() {
    for (input, expected) in [
        ("null", Value::Null),
        ("true", Value::Bool(true)),
        ("false", Value::Bool(false)),
        ("42", Value::from(42)),
        ("-17", Value::from(-17)),
        ("1.25", Value::from(1.25)),
        (r#""text""#, Value::String("text".into())),
        ("[]", Value::Array(vec![])),
        ("{}", Value::Object(BTreeMap::new())),
    ] {
        let padded = format!(" \t\r\n{input}\n\r\t ");
        assert_eq!(json::from_str(&padded).unwrap(), expected, "{input}");
        assert_eq!(json::from_slice(padded.as_bytes()).unwrap(), expected);
    }
    let parsed = json::from_str(r#" { "z": [null, true, {"x": -3}], "a": "é😀" } "#).unwrap();
    assert_eq!(parsed["z"][0], Value::Null);
    assert_eq!(parsed["z"][1], true);
    assert_eq!(parsed["z"][2]["x"], -3);
    assert_eq!(parsed["a"], "é😀");
    assert_eq!(
        parsed.to_string(),
        r#"{"a":"é😀","z":[null,true,{"x":-3}]}"#
    );
}

#[test]
fn string_escape_fixtures_decode_and_encode_independently() {
    for (encoded, decoded) in [
        (r#""""#, ""),
        (r#""\"\\\/\b\f\n\r\t""#, "\"\\/\u{08}\u{0c}\n\r\t"),
        (r#""\u0000\u001f\u007f""#, "\0\u{1f}\u{7f}"),
        (r#""\u00e9\u20AC\uD83D\uDE00""#, "é€😀"),
        (r#""\uD800\uDC00\udbff\udfff""#, "\u{10000}\u{10ffff}"),
        (r#""\uD7FF\uE000\uFFFF""#, "\u{d7ff}\u{e000}\u{ffff}"),
        (r#""raw é😀/\u2028\u2029""#, "raw é😀/\u{2028}\u{2029}"),
        (r#""\\u0000""#, "\\u0000"),
    ] {
        assert_eq!(json::from_str(encoded).unwrap(), decoded, "{encoded}");
    }
    for (decoded, encoded) in [
        ("\"\\/\u{08}\u{0c}\n\r\t", r#""\"\\/\b\f\n\r\t""#),
        ("\0\u{01}\u{1f}", r#""\u0000\u0001\u001f""#),
        ("é😀\u{7f}", "\"é😀\u{7f}\""),
    ] {
        let value = Value::from(decoded);
        assert_eq!(value.to_string(), encoded);
        assert_eq!(json::to_vec(&value), encoded.as_bytes());
    }
    let object = Value::Object(BTreeMap::from([("\"\n\0é".into(), Value::Null)]));
    assert_eq!(object.to_string(), r#"{"\"\n\u0000é":null}"#);
}

#[test]
fn integer_boundaries_are_exact_and_float_syntax_is_not_an_integer() {
    for integer in [
        i64::MIN,
        i32::MIN as i64,
        -1,
        0,
        1,
        i32::MAX as i64,
        i64::MAX,
    ] {
        let value = json::from_str(&integer.to_string()).unwrap();
        assert_eq!(value.as_i64(), Some(integer));
        assert_eq!(value.as_u64(), u64::try_from(integer).ok());
        assert_eq!(value.to_string(), integer.to_string());
    }
    for integer in [0, 1, 9_007_199_254_740_993, i64::MAX as u64 + 1, u64::MAX] {
        let value = json::from_str(&integer.to_string()).unwrap();
        assert_eq!(value.as_u64(), Some(integer));
        assert_eq!(value.as_i64(), i64::try_from(integer).ok());
        assert_eq!(value.to_string(), integer.to_string());
    }
    for (input, expected) in [
        ("-0", -0.0),
        ("-0.0", -0.0),
        ("1.0", 1.0),
        ("1e0", 1.0),
        ("1E+2", 100.0),
        ("-1.25e-2", -0.0125),
        ("18446744073709551616", 18_446_744_073_709_551_616.0),
        ("-9223372036854775809", -9_223_372_036_854_775_809.0),
        ("1.7976931348623157e308", f64::MAX),
        ("5e-324", f64::from_bits(1)),
        ("1e-400", 0.0),
        ("-1e-400", -0.0),
    ] {
        let value = json::from_str(input).unwrap();
        assert_eq!(
            value.as_f64().unwrap().to_bits(),
            expected.to_bits(),
            "{input}"
        );
        assert_eq!(value.as_i64(), None, "{input}");
        assert_eq!(value.as_u64(), None, "{input}");
        let reparsed = json::from_slice(&json::to_vec(&value)).unwrap();
        assert_eq!(reparsed.as_i64(), None);
        assert_eq!(reparsed.as_f64().unwrap().to_bits(), expected.to_bits());
    }
    assert_ne!(Value::from(1.0), Value::from(1));
    for nonfinite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(Value::from(nonfinite), Value::Null);
    }
}

#[test]
fn rejects_invalid_grammar_and_unrepresentable_numbers() {
    for input in [
        "",
        " ",
        "nul",
        "NULL",
        "True",
        "falsex",
        "undefined",
        "NaN",
        "Infinity",
        "+1",
        "01",
        "-01",
        "00",
        "0x10",
        "-",
        "--1",
        "-.1",
        ".1",
        "1.",
        "1.e1",
        "1e",
        "1e+",
        "1e-",
        "1e+-2",
        "1e1.0",
        "1_000",
        "1e309",
        "-1e309",
        "9e9999999999999999999999",
        "[",
        "[1",
        "[1,]",
        "[,1]",
        "[1 2]",
        "[1,,2]",
        "[01]",
        "[truefalse]",
        "{",
        "{\"a\"",
        "{\"a\":",
        "{\"a\":1,}",
        "{,}",
        "{a:1}",
        "{1:2}",
        "{\"a\" 1}",
        "{\"a\":1 \"b\":2}",
        "{\"a\":01}",
        "{}{}",
        "null true",
        "[]x",
        "/*comment*/null",
        "//comment\nnull",
        "\u{feff}null",
        "\u{a0}null",
        "\u{0b}null",
        "null\u{0c}",
        "\"unterminated",
        "\"\\",
        r#""\x20""#,
        r#""\a""#,
        r#""\0""#,
        r#""\u""#,
        r#""\u123""#,
        r#""\u123x""#,
        r#""\u１２３４""#,
        r#""\ud800""#,
        r#""\ud800\u0000""#,
        r#""\ud800\ud800""#,
        r#""\ud800x""#,
        r#""\ud800\\udc00""#,
        r#""\udc00""#,
        r#""\udfff\ud800""#,
        r#""\ud800😀""#,
        r#""\é""#,
    ] {
        assert!(json::from_str(input).is_err(), "accepted {input:?}");
    }
    for control in 0..=0x1f {
        assert!(json::from_slice(&[b'"', control, b'"']).is_err());
        assert!(json::from_slice(&[b'{', b'"', control, b'"', b':', b'0', b'}']).is_err());
    }
    for invalid in [
        &b"\xff"[..],
        &b"\"\x80\""[..],
        &b"\"\xc0\xaf\""[..],
        &b"\"\xed\xa0\x80\""[..],
        &b"\"\xf4\x90\x80\x80\""[..],
        &b"\"\xe2\x82\""[..],
        &b"null\xff"[..],
    ] {
        assert!(json::from_slice(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn duplicate_keys_compare_decoded_strings_and_keep_the_last_value() {
    let value = json::from_str(r#"{"x":1,"\u0078":{"kept":true},"a":0}"#).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 2);
    assert_eq!(value.to_string(), r#"{"a":0,"x":{"kept":true}}"#);
}

#[test]
fn errors_report_byte_offsets() {
    let text = "\n{\"é\": true, \"x\": ]}";
    let error = json::from_str(text).unwrap_err();
    assert_eq!(error.offset, text.find(']').unwrap());
    assert_eq!(
        error.to_string(),
        format!("expected a JSON value at byte {}", error.offset)
    );
    let error = json::from_slice(b"\"a\xff\"").unwrap_err();
    assert_eq!(error.offset, 2);
    assert_eq!(error.message, "invalid UTF-8");
}

#[test]
fn nesting_limit_counts_containers_not_siblings_or_string_contents() {
    for (open, close) in [("[", "]"), ("{\"x\":", "}"), ("{\"x\":[", "]}")] {
        let unit_depth = if open.ends_with('[') && open.starts_with('{') {
            2
        } else {
            1
        };
        let count = 128 / unit_depth;
        let valid = format!("{}null{}", open.repeat(count), close.repeat(count));
        let value = json::from_str(&valid).unwrap();
        assert_eq!(value.to_string(), valid);
        for depth in [count + 1, 10_000] {
            let invalid = format!("{}null{}", open.repeat(depth), close.repeat(depth));
            assert_eq!(
                json::from_str(&invalid).unwrap_err().message,
                "JSON nesting limit exceeded"
            );
        }
    }
    let wide = format!("[{}null]", "[],".repeat(10_000));
    assert_eq!(
        json::from_str(&wide).unwrap().as_array().unwrap().len(),
        10_001
    );
    let string = format!("\"{}\"", "[".repeat(10_000));
    assert!(json::from_str(&string).is_ok());
}

#[test]
fn construction_borrows_expressions_and_evaluates_them_once() {
    let text = "hello".to_owned();
    let key = "dynamic".to_owned();
    let values = vec![Value::Null, Value::from(7)];
    let map = BTreeMap::from([("key".to_owned(), values.clone())]);
    let mut calls = 0;
    let value = json!({
        (key): [null, true, false, -2, {"text": text, "empty": [],},],
        "values": values,
        "map": map,
        "some": Some(3),
        "none": None::<i32>,
        "expression": ({ calls += 1; calls + 2 }),
        "borrowed": &text,
    });
    assert_eq!(calls, 1);
    assert_eq!(text, "hello");
    assert_eq!(key, "dynamic");
    assert_eq!(values.len(), 2);
    assert_eq!(map.len(), 1);
    assert_eq!(
        value.to_string(),
        r#"{"borrowed":"hello","dynamic":[null,true,false,-2,{"empty":[],"text":"hello"}],"expression":3,"map":{"key":[null,7]},"none":null,"some":3,"values":[null,7]}"#
    );
    assert_eq!(
        json!([1 + 2, null, [3,], {"a": 4,},]),
        json::from_str("[3,null,[3],{\"a\":4}]").unwrap()
    );
}

#[test]
fn accessors_validate_types_and_missing_fields_are_null() {
    let mut value = json!({"n":1,"float":1.0,"s":"1","b":true,"a":[2],"null":null});
    assert_eq!(value["n"].as_str(), None);
    assert_eq!(value["n"].as_bool(), None);
    assert_eq!(value["float"].as_i64(), None);
    assert_eq!(value["s"].as_i64(), None);
    assert_eq!(value["b"].as_u64(), None);
    assert_eq!(value["n"].as_array(), None);
    assert_eq!(value["a"].as_object(), None);
    assert_eq!(value["s"].as_f64(), None);
    assert_eq!(value.get("missing"), None);
    assert_eq!(value.get("null"), Some(&Value::Null));
    assert!(value["a"][10]["missing"].is_null());
    assert!(value["n"]["missing"][0].is_null());
    value["inserted"]["nested"] = Value::Bool(false);
    value["a"][0] = Value::from(3);
    assert_eq!(value["inserted"]["nested"], false);
    assert_eq!(value["a"][0], 3);
    assert_eq!(
        value.as_object_mut().unwrap().remove("s"),
        Some(Value::from("1"))
    );
}

#[test]
fn all_unicode_scalar_values_round_trip_raw_and_escaped() {
    let mut raw = String::new();
    let mut escaped = String::from("\"");
    use std::fmt::Write;
    for scalar in 0..=0x10ffff {
        let Some(character) = char::from_u32(scalar) else {
            continue;
        };
        raw.push(character);
        if scalar <= 0xffff {
            write!(escaped, "\\u{scalar:04x}").unwrap();
        } else {
            let high = 0xd800 + ((scalar - 0x10000) >> 10);
            let low = 0xdc00 + ((scalar - 0x10000) & 0x3ff);
            write!(escaped, "\\u{high:04x}\\u{low:04x}").unwrap();
        }
    }
    escaped.push('"');
    assert_eq!(
        json::from_str(&escaped).unwrap().as_str(),
        Some(raw.as_str())
    );
    let encoded = json::to_vec(&Value::from(&raw));
    assert_eq!(
        json::from_slice(&encoded).unwrap().as_str(),
        Some(raw.as_str())
    );
}

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn generated_numeric_values_round_trip_without_losing_bits() {
    let mut seed = 0x1234_5678_9abc_def0;
    for _ in 0..20_000 {
        let bits = next(&mut seed);
        for value in [Value::from(bits), Value::from(bits as i64)] {
            assert_eq!(json::from_slice(&json::to_vec(&value)).unwrap(), value);
        }
        let float = f64::from_bits(bits);
        if float.is_finite() {
            let encoded = json::to_vec(&Value::from(float));
            let decoded = json::from_slice(&encoded).unwrap();
            assert_eq!(decoded.as_f64().unwrap().to_bits(), bits);
            assert_eq!(decoded.as_i64(), None);
        }
    }
}

#[test]
fn arbitrary_bytes_and_mutated_documents_never_panic() {
    let fixtures: &[&[u8]] = &[
        br#"{"jsonrpc":"2.0","id":1,"params":{"text":"\ud83d\ude00"}}"#,
        br#"[null,true,false,-123,1.25e-3,[],{}]"#,
        "\"é😀\\n\\u0000\"".as_bytes(),
    ];
    let mut seed = 0xa5a5_1234_cafe_9876;
    for fixture in fixtures {
        for length in 0..fixture.len() {
            assert!(json::from_slice(&fixture[..length]).is_err());
        }
        for index in 0..fixture.len() {
            for byte in 0..=255 {
                let mut input = fixture.to_vec();
                input[index] = byte;
                if let Ok(value) = json::from_slice(&input) {
                    assert_eq!(json::from_slice(&json::to_vec(&value)).unwrap(), value);
                }
            }
        }
    }
    for _ in 0..10_000 {
        let length = (next(&mut seed) % 128) as usize;
        let input: Vec<_> = (0..length).map(|_| next(&mut seed) as u8).collect();
        if let Ok(value) = json::from_slice(&input) {
            assert_eq!(json::from_slice(&json::to_vec(&value)).unwrap(), value);
        }
    }
}

#[test]
fn lsp_wire_fixtures_have_exact_encoding_and_recover_from_parse_errors() {
    use std::io::Write;
    let mut input = Vec::new();
    for body in [
        &br#"{"invalid":"\ud800"}"#[..],
        br#"{"jsonrpc":"2.0","id":"\ud83d\ude00\"\\\n","method":"shutdown"}"#,
        br#"{"jsonrpc":"2.0","id":1e0,"method":"shutdown"}"#,
        br#"{"jsonrpc":"2.0","id":2147483647,"method":"shutdown"}"#,
        br#"{"jsonrpc":"2.0","method":"exit"}"#,
    ] {
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.extend_from_slice(body);
    }
    let mut output = Vec::new();
    assert_eq!(
        dodoc::lsp::run(&mut input.as_slice(), &mut output).unwrap(),
        1
    );
    let mut expected = String::new();
    use std::fmt::Write as _;
    for body in [
        r#"{"error":{"code":-32700,"message":"Parse error"},"id":null,"jsonrpc":"2.0"}"#,
        r#"{"error":{"code":-32002,"message":"server is not initialized"},"id":"😀\"\\\n","jsonrpc":"2.0"}"#,
        r#"{"error":{"code":-32600,"message":"Invalid Request"},"id":null,"jsonrpc":"2.0"}"#,
        r#"{"error":{"code":-32002,"message":"server is not initialized"},"id":2147483647,"jsonrpc":"2.0"}"#,
    ] {
        write!(expected, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    }
    assert_eq!(String::from_utf8(output).unwrap(), expected);
}
