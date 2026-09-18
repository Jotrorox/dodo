//! End-to-end typed JSON, independent malformed inputs, and portable emission.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-encoding-json-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn execute_and_cross_compile(source: &Path, scratch: &Scratch, stem: &str) {
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "{stem}-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(source)
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .current_dir(&scratch.0)
                .output()
                .unwrap(),
            &format!("compile {stem} at O{optimization}"),
        );
        success(
            Command::new(&executable).output().unwrap(),
            &format!("execute {stem} at O{optimization}"),
        );
        for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
            let object = scratch.0.join(format!("{stem}-{target}-O{optimization}.o"));
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(source)
                    .args([
                        "--emit",
                        "obj",
                        "--target",
                        target,
                        "-O",
                        optimization,
                        "-o",
                    ])
                    .arg(&object)
                    .output()
                    .unwrap(),
                &format!("emit {stem} for {target} at O{optimization}"),
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
}

#[test]
fn public_json_api_and_example_execute_and_cross_compile() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new();
    for (source, stem) in [
        ("tests/stdlib/encoding_json_checks.dodo", "json-api"),
        ("tests/stdlib/json_values.dodo", "json-values"),
        ("tests/stdlib/json_strings.dodo", "json-strings"),
        ("tests/stdlib/json_indexed.dodo", "json-indexed"),
        ("tests/stdlib/json_derive.dodo", "json-derive"),
        ("tests/stdlib/json_structs.dodo", "json-structs"),
        ("tests/stdlib/json_encoder.dodo", "json-encoder"),
        ("examples/json.dodo", "json-example"),
    ] {
        execute_and_cross_compile(&root.join(source), &scratch, stem);
    }
}

#[test]
fn wide_derived_structs_track_required_and_optional_fields() {
    use std::fmt::Write;

    let scratch = Scratch::new();
    let source = scratch.0.join("wide.dodo");
    let mut program =
        "package wide\nimport \"std/encoding/json\"\n@derive(Json)\nstruct Wide {\n".to_owned();
    for i in 0..70 {
        let ty = if i % 2 == 0 { "Option<u32>" } else { "u32" };
        writeln!(program, "field{i}: {ty}").unwrap();
    }
    program.push_str("}\nfn main() {\n");
    for omit_optional in [false, true] {
        let members = (0..70)
            .rev()
            .filter(|i| !omit_optional || i % 2 != 0)
            .map(|i| format!("\"field{i}\":{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let input = format!("{{{members}}}");
        writeln!(program, "{{\nrecord := json.decode::<Wide>(b{input:?})!").unwrap();
        for i in 0..70 {
            if i % 2 != 0 {
                writeln!(program, "assert_eq(record.field{i}, {i}u32)").unwrap();
            } else if omit_optional {
                writeln!(
                    program,
                    "match &record.field{i} {{ some(_) => {{ assert(false) }}, none => {{}}, }}"
                )
                .unwrap();
            } else {
                writeln!(program, "match &record.field{i} {{ some(value) => {{ assert_eq(*value, {i}u32) }}, none => {{ assert(false) }}, }}").unwrap();
            }
        }
        program.push_str("}\n");
    }
    // Missing required fields at either end and in the middle must be found.
    for missing in [1, 35, 69] {
        let members = (0..70)
            .filter(|i| *i != missing)
            .map(|i| format!("\"field{i}\":{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let input = format!("{{{members}}}");
        writeln!(program, "match json.decode::<Wide>(b{input:?}) {{ ok(_) => {{ assert(false) }}, err(reason) => {{ assert_eq(reason.kind, json.ErrorKind.MissingField) }}, }}").unwrap();
    }
    program.push_str("}\n");
    fs::write(&source, program).unwrap();
    execute_and_cross_compile(&source, &scratch, "json-wide-struct");
}

#[test]
fn decoded_json_views_preserve_source_lifetimes() {
    let scratch = Scratch::new();
    let source = scratch.0.join("rejected.dodo");
    for program in [
        "package bad\nimport \"std/encoding/json\"\nfn escape() -> json.Value!json.Error from(static) { data := [49u8]\nreturn json.parse(&data) }\n",
        "package bad\nimport \"std/encoding/json\"\n@derive(Json)\nstruct Entry { name: json.String }\nfn escape() -> Entry!json.Error from(static) { data := [123u8,34,110,97,109,101,34,58,34,120,34,125]\nreturn json.decode<Entry>(&data) }\n",
        "package bad\nimport \"std/encoding/json\"\nfn main() -> i32 { data := [49u8]\nvalue := json.parse(&data)!\ndata[0] = 50\nreturn value.as_u64()! as i32 }\n",
    ] {
        fs::write(&source, program).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source)
            .output()
            .unwrap();
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "accepted source: {program}");
        assert!(error.contains("borrow"), "expected borrow error: {error}");
        assert!(!error.contains("panicked at"), "{error}");
    }
}
