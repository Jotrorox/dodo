//! std/db/sqlite: differential tests against SQLite (Python's sqlite3
//! module, also with UTF-16 databases and under several time zones),
//! crash-recovery, locking, and UTF-16 interoperability on shared files, and
//! the Dodo API checks. The SQLite-based parts are skipped when Python 3 with
//! its sqlite3 module is unavailable.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-sqlite-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn python_with_sqlite() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|python| {
        Command::new(python)
            .args(["-c", "import sqlite3"])
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

fn build_shell(workspace: &Path, optimization: &str) -> PathBuf {
    let shell = workspace.join(format!("shell-O{optimization}"));
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(root().join("tests/sqlite/shell.dodo"))
            .args(["-O", optimization, "-o"])
            .arg(&shell)
            .output()
            .unwrap(),
        "build the sqlite test shell",
    );
    shell
}

#[test]
fn matches_sqlite_on_every_script() {
    let Some(python) = python_with_sqlite() else {
        eprintln!("skipped: python3 with the sqlite3 module is unavailable");
        return;
    };
    let workspace = Workspace::new();
    let shell = build_shell(&workspace.0, "0");
    let generated = workspace.0.join("generated");
    success(
        Command::new(python)
            .arg(root().join("tests/sqlite/generate.py"))
            .arg(&generated)
            .output()
            .unwrap(),
        "generate bulk scripts",
    );
    let mut scripts: Vec<PathBuf> = fs::read_dir(root().join("tests/sqlite/scripts"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "sql"))
        .collect();
    scripts.sort();
    // Every hand-written script also runs against UTF-16 databases.
    for encoding in ["UTF-16le", "UTF-16be"] {
        success(
            Command::new(python)
                .arg(root().join("tests/sqlite/differential.py"))
                .args(["--encoding", encoding])
                .arg(&shell)
                .arg(&workspace.0)
                .args(&scripts)
                .output()
                .unwrap(),
            &format!("differential scripts in {encoding} databases"),
        );
    }
    for entry in fs::read_dir(&generated).unwrap() {
        scripts.push(entry.unwrap().path());
    }
    success(
        Command::new(python)
            .arg(root().join("tests/sqlite/differential.py"))
            .arg(&shell)
            .arg(&workspace.0)
            .args(&scripts)
            .output()
            .unwrap(),
        "differential scripts",
    );
}

// The 'localtime' and 'utc' modifiers follow the OS time zone rules, as
// SQLite's do: compare both under zones with DST gaps and overlaps,
// half-hour offsets, and none. TZ takes IANA names on POSIX systems only.
#[cfg(unix)]
#[test]
fn localtime_matches_sqlite_in_several_time_zones() {
    let Some(python) = python_with_sqlite() else {
        eprintln!("skipped: python3 with the sqlite3 module is unavailable");
        return;
    };
    let workspace = Workspace::new();
    let shell = build_shell(&workspace.0, "0");
    for zone in [
        "America/New_York",
        "Europe/Berlin",
        "Australia/Lord_Howe",
        "Asia/Kolkata",
        "America/St_Johns",
        "UTC",
    ] {
        success(
            Command::new(python)
                .arg(root().join("tests/sqlite/differential.py"))
                .arg(&shell)
                .arg(&workspace.0)
                .arg(root().join("tests/sqlite/localtime.sql"))
                .arg(root().join("tests/sqlite/localtime_zero_offset.sql"))
                .env("TZ", zone)
                .output()
                .unwrap(),
            &format!("localtime scripts with TZ={zone}"),
        );
    }
}

#[cfg(unix)]
#[test]
fn shares_files_with_sqlite() {
    let Some(python) = python_with_sqlite() else {
        eprintln!("skipped: python3 with the sqlite3 module is unavailable");
        return;
    };
    let workspace = Workspace::new();
    let shell = build_shell(&workspace.0, "0");
    success(
        Command::new(python)
            .arg(root().join("tests/sqlite/interop.py"))
            .arg(&shell)
            .arg(&workspace.0)
            .output()
            .unwrap(),
        "crash recovery, locking, and UTF-16 interop",
    );
}

#[test]
fn api_checks_pass() {
    let workspace = Workspace::new();
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("test")
            .arg(root().join("tests/sqlite/api_checks.dodo"))
            .current_dir(&workspace.0)
            .output()
            .unwrap(),
        "dodo test tests/sqlite/api_checks.dodo",
    );
}
