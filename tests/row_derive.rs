//! `@derive(Row)` and the derive registry: generated methods are ordinary
//! checked syntax, derive-owned attributes are validated generically, and
//! zero-copy row decoding cannot outlive the row it borrows.
use dodoc::{format, package, parser, sema};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-row-derive-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn check(path: &Path) -> Result<(), String> {
    let mut loaded = package::load(path).map_err(|error| error.to_string())?;
    sema::check(&mut loaded.program).map_err(|error| loaded.render(&error))
}

fn check_source(source: &str) -> Result<(), String> {
    let directory = Directory::new();
    let path = directory.0.join("main.dodo");
    std::fs::write(&path, source).unwrap();
    check(&path)
}

#[test]
fn generates_decode_row_and_params_with_hygienic_imports() {
    let program = parser::parse(
        "package demo\nconst __dodo_sql_derive: i32 = 1\n@derive(Row)\nstruct User {\nid: i64\n@db_name(\"display_name\")\nname: &str\nemail: Option<&str>\n}\n",
    )
    .unwrap();
    let names: Vec<_> = program.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["User.decode_row", "User.params"]);
    assert!(program.functions.iter().all(|function| function.public));
    assert_eq!(program.functions[0].bounds.len(), 1);
    assert!(program.imports.iter().any(|import| import == "std/db"));
    assert!(program.imports.iter().any(|import| import == "std/db/sql"));
    assert!(
        program
            .import_aliases
            .iter()
            .any(|(import, alias)| import == "std/db/sql" && alias == "__dodo_sql_derive_")
    );
}

#[test]
fn reuses_explicit_imports_and_combines_derives() {
    for derives in ["@derive(Json, Row)", "@derive(Row)\n@derive(Json)"] {
        let program = parser::parse(&format!(
            "package demo\nimport \"std/db\" as store\nimport \"std/db/sql\"\n{derives}\nstruct User {{\n@json_name(\"n\")\n@db_name(\"user_name\")\nname: &str\n}}\n"
        ))
        .unwrap();
        assert_eq!(program.imports.len(), 3, "{derives}");
        let mut names: Vec<_> = program.functions.iter().map(|f| f.name.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "User.decode_json",
                "User.decode_row",
                "User.encode_json",
                "User.params"
            ]
        );
        let generated = format!("{:?}", program.functions);
        assert!(generated.contains("store.Value"));
        assert!(generated.contains("\"sql.Row\""), "{generated}");
    }
}

#[test]
fn flattened_structs_only_decode() {
    let program = parser::parse(
        "package demo\n@derive(Row)\nstruct Team { name: &str }\n@derive(Row)\nstruct Member {\n@db_flatten\nteam: Team\nrole: &str\n}\n",
    )
    .unwrap();
    let names: Vec<_> = program.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        ["Team.decode_row", "Team.params", "Member.decode_row"]
    );
}

#[test]
fn derive_attributes_and_unsupported_fields_fail_at_the_declaration() {
    for (source, expected) in [
        (
            "@derive(Other)\nstruct Record {}",
            "unknown derive `Other`; supported derives: @derive(Json), @derive(Row)",
        ),
        (
            "@derive(Row, Row)\nstruct Record { a: i32 }",
            "duplicate @derive(Row)",
        ),
        (
            "@derive(Row)\n@derive(Row)\nstruct Record { a: i32 }",
            "duplicate @derive(Row)",
        ),
        ("@derive(Row)\nfn sample() {}", "apply only to structs"),
        (
            "@db_name(\"x\")\nfn sample() {}",
            "@db_name applies only to fields of @derive(Row) structs",
        ),
        (
            "@db_flatten\nstruct Record { a: i32 }",
            "@db_flatten applies only to fields",
        ),
        (
            "struct Record {\n@db_name(\"x\")\na: i32\n}",
            "@db_name applies only to fields of @derive(Row) structs",
        ),
        (
            "@derive(Json)\nstruct Record {\n@db_name(\"x\")\na: i32\n}",
            "@db_name applies only to fields of @derive(Row) structs",
        ),
        (
            "@derive(Row)\nstruct Record {\n@json_name(\"x\")\na: i32\n}",
            "@json_name applies only to fields of @derive(Json) structs",
        ),
        (
            "@derive(Row)\nstruct Record {\n@json_deny_unknown\na: i32\n}",
            "@json_deny_unknown applies only to @derive(Json) structs",
        ),
        (
            "@derive(Row)\n@json_deny_unknown\nstruct Record { a: i32 }",
            "@json_deny_unknown requires @derive(Json)",
        ),
        (
            "@derive(Row)\nstruct Record {\n@db_name(\"x\")\n@db_name(\"y\")\na: i32\n}",
            "duplicate @db_name attribute",
        ),
        (
            "@derive(Row)\nstruct Record {\n@db_name(x)\na: i32\n}",
            "@db_name expects a string column name",
        ),
        (
            "@derive(Row)\nstruct Record {\n@db_name(\"\")\na: i32\n}",
            "non-empty column name",
        ),
        (
            "@derive(Row)\nstruct Record {\n@db_name(\"a\")\nb: i32\na: i32\n}",
            "duplicate column name `a`",
        ),
        ("@derive(Row)\nstruct Record {}", "at least one field"),
        (
            "@derive(Row)\nstruct Record { a: u64 }",
            "SQL integers are signed 64-bit",
        ),
        (
            "@derive(Row)\nstruct Record { a: usize }",
            "SQL integers are signed 64-bit",
        ),
        (
            "@derive(Row)\nstruct Record { a: Option<Option<i32>> }",
            "nested options",
        ),
        (
            "@derive(Row)\nstruct Record { a: [2]i32 }",
            "does not support field type",
        ),
        (
            "@derive(Row)\nstruct Record { a: &[i32] }",
            "does not support field type",
        ),
        (
            "@derive(Row)\nstruct Record { a: &mut [u8] }",
            "does not support field type",
        ),
        (
            "@derive(Row)\nstruct Record {\n@db_flatten\na: i32\n}",
            "@db_flatten needs a struct type",
        ),
        (
            "@derive(Row)\nstruct Record {\n@db_flatten\n@db_name(\"x\")\na: Inner\n}",
            "remove @db_name",
        ),
        (
            "@derive(Row)\nstruct Record<T> { a: T }",
            "@derive(Row) currently requires a concrete struct",
        ),
        (
            "@derive(Row)\nstruct Record {\na: i32\nfn params(&self) {}\n}",
            "@derive(Row) conflicts with existing `params` method",
        ),
        (
            "@unknown\nstruct Record {}",
            "@db_name(\"...\"), @db_flatten",
        ),
    ] {
        let source = format!("package demo\n{source}\n");
        let error = parser::parse(&source).unwrap_err();
        assert!(
            error.message.contains(expected),
            "{source}: expected {expected:?}, got {}",
            error.message
        );
    }
}

#[test]
fn editor_retains_declarations_when_a_row_derive_is_invalid() {
    let (program, diagnostics) = parser::parse_recovering(
        "package demo\n@derive(Row)\nstruct Record { raw: u64 }\nfn later() -> i32 { return 1 }\n",
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(program.structs.len(), 1);
    assert!(program.functions.iter().any(|f| f.name == "later"));
}

#[test]
fn formatting_preserves_row_attributes() {
    let source = "package demo\n@derive(Json, Row)\nstruct Record {\n@db_name(\"user_id\")\n@json_name(\"userId\")\nid:i64\n@db_flatten\nteam:Team\n}\n";
    let formatted = format::format_source(source).unwrap();
    for text in [
        "@derive(Json, Row)",
        "@db_name(\"user_id\")",
        "@json_name(\"userId\")",
        "@db_flatten",
    ] {
        assert!(formatted.contains(text), "{formatted}");
    }
    assert_eq!(format::format_source(&formatted).unwrap(), formatted);
}

const ROWS: &str = "package bad\nimport \"std/db\"\nimport \"std/db/sqlite\"\n@derive(Row)\nstruct User { name: &str }\nfn take(name: &str) {}\n";

#[test]
fn decoded_rows_cannot_outlive_the_row_they_borrow() {
    for body in [
        // Returning a decoded row past its cursor.
        "fn escape(conn: &mut sqlite.Connection) -> User!db.Error from(static) {\nnone_: [0]db.Value = []\nrows := conn.select(\"select name from t\", &none_)?\nlet some(row) = rows.next()? else { return err(db.Error.new(db.ErrorKind.Internal, 0)) }\nreturn User.decode_row(&row)\n}\n",
        // Using a decoded row after the cursor advanced.
        "fn stale(conn: &mut sqlite.Connection) -> void!db.Error {\nnone_: [0]db.Value = []\nrows := conn.select(\"select name from t\", &none_)?\nlet some(first) = rows.next()? else { return ok() }\nuser := User.decode_row(&first)?\nlet some(second) = rows.next()? else { return ok() }\ntake(user.name)\nreturn ok()\n}\n",
    ] {
        let error = check_source(&format!("{ROWS}{body}")).expect_err(body);
        assert!(error.contains("borrow"), "{error}");
    }
    let error = check_source("package bad\nimport \"std/db\"\nimport \"core/mem\"\n@derive(Row)\nstruct User { name: &str }\nfn escape() -> [1]db.Value from(static) {\nbuf := [104u8, 105u8]\nuser := User { name: unsafe { mem.str_from_utf8(&buf) } }\nreturn user.params()\n}\n").expect_err("params escaped a local borrow");
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn derived_rows_work_across_packages_and_with_implicit_imports() {
    let directory = Directory::new();
    let models = directory.0.join("models");
    std::fs::create_dir_all(&models).unwrap();
    // Neither model file imports std/db/sql: the derive's hidden imports are
    // renamed per directory and must still resolve in generic bounds.
    std::fs::write(
        models.join("user.dodo"),
        "package models\n@derive(Row)\npub struct User {\npub id: i64\npub name: &str\n}\n",
    )
    .unwrap();
    std::fs::write(
        models.join("team.dodo"),
        "package models\nimport \"std/db\" as value\n@derive(Row)\npub struct Member {\n@db_flatten\npub user: User\npub role: Option<&str>\n}\n",
    )
    .unwrap();
    std::fs::write(
        directory.0.join("main.dodo"),
        "package app\nimport \"models\"\nimport \"std/db\"\nimport \"std/db/sqlite\"\nfn count(conn: &mut sqlite.Connection) -> i64!db.Error {\nnone_: [0]db.Value = []\nrows := conn.select(\"select id, name, role from users\", &none_)?\ntotal := 0i64\nfor {\nlet some(row) = rows.next()? else { break }\nmember := models.Member.decode_row(&row)?\ntotal += member.user.id\n}\nreturn ok(total)\n}\nfn insert(conn: &mut sqlite.Connection, user: &models.User) -> u64!db.Error {\np := user.params()\nreturn conn.execute(\"insert into users (id, name) values (?, ?)\", &p)\n}\n",
    )
    .unwrap();
    check(&directory.0.join("main.dodo")).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn derived_rows_run_against_sqlite() {
    let directory = Directory::new();
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("test")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sqlite/row_derive.dodo"))
        .current_dir(&directory.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
