//! Formatter migration and filesystem behavior through the public CLI.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-format-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
        path
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_dodo"));
        cmd.current_dir(&self.0);
        cmd
    }
    fn fmt(&self, args: &[&str]) -> Output {
        self.command().arg("fmt").args(args).output().unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn run(workspace: &Workspace, path: &Path) -> i32 {
    let output = workspace.command().arg("run").arg(path).output().unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.status.code().unwrap()
}

#[test]
fn migrates_legacy_syntax_preserving_comments_types_and_native_behavior() {
    let workspace = Workspace::new();
    let legacy = r#"package app
// Keep the literal spelling and explicit narrow type.
fn identity<T>(value: T) -> T { return value }
fn main() -> i32 {
    [2]u8 values = [2]u8{0x14, 22}
    i32 total = 0
    for &value in values { total += value as i32 }
    return identity<i32>(total) // A trailing comment.
}
"#;
    let path = workspace.file("has spaces.dodo", legacy);
    assert_eq!(run(&workspace, &path), 42);
    let output = workspace.fmt(&["--check", "has spaces.dodo"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&path).unwrap(), legacy);
    let preview = workspace.fmt(&["--stdout", "has spaces.dodo"]);
    success(&preview);
    assert_eq!(fs::read_to_string(&path).unwrap(), legacy);
    success(&workspace.fmt(&["has spaces.dodo"]));
    let formatted = fs::read_to_string(&path).unwrap();
    assert_eq!(formatted.as_bytes(), preview.stdout);
    for expected in [
        "values: [2]u8",
        "total: i32",
        "identity::<i32>(total)",
        "0x14",
        "// A trailing comment.",
    ] {
        assert!(
            formatted.contains(expected),
            "missing {expected}:\n{formatted}"
        );
    }
    assert!(!formatted.contains("[2]u8{"));
    assert_eq!(run(&workspace, &path), 42);
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    success(&workspace.fmt(&["--check", "has spaces.dodo"]));
    success(&workspace.fmt(&["has spaces.dodo"]));
    assert_eq!(fs::read_to_string(&path).unwrap(), formatted);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
}

#[test]
fn directory_validation_precedes_all_writes_and_traversal_skips_artifacts() {
    let workspace = Workspace::new();
    let legacy = "package app\nfn main(){i32 value=1}\n";
    let path = workspace.file("a.dodo", legacy);
    let nested = workspace.file("nested/b.dodo", legacy);
    let broken = workspace.file("nested/z.dodo", "package broken\nfn {\n");
    for ignored in [
        "target/ignored.dodo",
        "build/ignored.dodo",
        "dist/ignored.dodo",
        "node_modules/ignored.dodo",
        "vendor/ignored.dodo",
        ".git/ignored.dodo",
    ] {
        workspace.file(ignored, "this is not Dodo");
        workspace.file(&format!("nested/{ignored}"), "this is not Dodo");
    }
    workspace.file("notes.txt", "this is not Dodo");
    let output = workspace.fmt(&[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("z.dodo"));
    assert_eq!(fs::read_to_string(&path).unwrap(), legacy);
    assert_eq!(fs::read_to_string(&nested).unwrap(), legacy);
    fs::remove_file(broken).unwrap();
    success(&workspace.fmt(&[]));
    assert!(fs::read_to_string(path).unwrap().contains("value: i32"));
    assert!(fs::read_to_string(nested).unwrap().contains("value: i32"));
    success(&workspace.fmt(&["--check"]));
    assert!(!workspace.fmt(&["--stdout", "."]).status.success());
}

#[test]
fn excluded_directories_are_untouched_unless_explicitly_selected() {
    let workspace = Workspace::new();
    let legacy = "package app\nfn main(){i32 value=1}\n";
    for directory in [
        "build",
        "target",
        "dist",
        "node_modules",
        "vendor",
        ".hidden",
    ] {
        let filename = format!("{directory}/source.dodo");
        let path = workspace.file(&filename, legacy);
        // Exclusions apply to directory names, not similarly named source files.
        let included = workspace.file(&format!("{directory}_source.dodo"), legacy);
        success(&workspace.fmt(&["."]));
        success(&workspace.fmt(&["--check", "."]));
        assert_eq!(fs::read_to_string(&path).unwrap(), legacy);
        assert!(fs::read_to_string(included).unwrap().contains("value: i32"));

        assert_eq!(
            workspace.fmt(&["--check", &filename]).status.code(),
            Some(1)
        );
        success(&workspace.fmt(&[&filename]));
        assert!(fs::read_to_string(&path).unwrap().contains("value: i32"));

        fs::write(&path, legacy).unwrap();
        success(&workspace.fmt(&[directory]));
        assert!(fs::read_to_string(path).unwrap().contains("value: i32"));
    }
}

#[test]
fn stdin_and_invalid_options_do_not_write_files() {
    let workspace = Workspace::new();
    let mut child = workspace
        .command()
        .args(["fmt", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"package app\nfn main(){i32 value=1}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("value: i32"));
    for args in [
        vec!["--check", "--stdout", "-"],
        vec!["--emit", "obj"],
        vec!["one", "two"],
        vec!["missing.dodo"],
    ] {
        assert!(!workspace.fmt(&args).status.success());
    }
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn replacement_preserves_permissions_and_does_not_follow_directory_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let workspace = Workspace::new();
    let path = workspace.file("main.dodo", "package app\nfn main(){}\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    symlink(&workspace.0, workspace.0.join("cycle")).unwrap();
    success(&workspace.fmt(&["."]));
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn shipped_examples_use_canonical_formatting_and_ergonomics_example_runs() {
    let workspace = Workspace::new();
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    success(
        &workspace
            .command()
            .args(["fmt", "--check"])
            .arg(&examples)
            .output()
            .unwrap(),
    );
    for optimization in ["0", "3"] {
        let output = workspace
            .command()
            .args(["run", "-O", optimization])
            .arg(examples.join("ergonomics.dodo"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(42),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
