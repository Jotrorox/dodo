//! Manifest and CLI behavior through real compiler/linker/program invocations.
#![cfg(feature = "llvm")]
use dodoc::toml::{self, Kind, Value};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "dodo-project-{}-{} space",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn file(&self, path: &str, source: &str) {
        let p = self.0.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, source).unwrap();
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_dodo"));
        c.current_dir(&self.0).env_remove("DODO_CC");
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn basic(&self) {
        self.file("main.dodo", "package app\nfn main() -> i32 { 17 }\n");
        self.file("dodo.toml", "schema=1\n[targets.app]\nentry='main.dodo'\n");
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}
fn success(o: &Output) {
    assert!(o.status.success(), "{}", text(o));
}
fn config(o: &Output) -> Value {
    success(o);
    toml::parse(std::str::from_utf8(&o.stdout).unwrap()).unwrap()
}
fn field<'a>(v: &'a Value, key: &str) -> &'a Kind {
    &v.as_table().unwrap()[key].kind
}
fn path(v: &Value, key: &str) -> PathBuf {
    let Kind::String(s) = field(v, key) else {
        panic!()
    };
    s.into()
}
#[test]
fn configuration_overrides_and_inspection_have_no_side_effects() {
    let w = Workspace::new();
    w.basic();
    w.file("dodo.toml","schema=1\n[build]\nopt-level=1\nlinker='unavailable-project-linker'\nlink-args=['-lbase']\n[profiles.release]\ndebug=true\n[targets.app]\nentry='main.dodo'\nopt-level=2\n[targets.app.run]\nargs=['saved value']\n");
    let c = config(&w.run(&["build", "--release", "--print-config"]));
    assert_eq!(*field(&c, "opt-level"), Kind::Integer(3));
    assert_eq!(*field(&c, "debug"), Kind::Bool(true));
    assert!(!w.0.join("build").exists());
    let o = w
        .command()
        .args([
            "build",
            "--release",
            "--no-debug",
            "-O0",
            "--clear-link-args",
            "--link-arg=-s",
            "--print-config",
        ])
        .env("DODO_CC", "environment-linker")
        .output()
        .unwrap();
    let c = config(&o);
    assert_eq!(
        *field(&c, "linker"),
        Kind::String("environment-linker".into())
    );
    assert_eq!(*field(&c, "debug"), Kind::Bool(false));
    assert_eq!(*field(&c, "opt-level"), Kind::Integer(0));
    let o = w
        .command()
        .args(["build", "--linker=command-linker", "--print-config"])
        .env("DODO_CC", "environment-linker")
        .output()
        .unwrap();
    assert_eq!(
        *field(&config(&o), "linker"),
        Kind::String("command-linker".into())
    );
    success(&w.run(&["check"]));
}
#[test]
fn native_outputs_are_separated_and_run_returns_child_status() {
    let w = Workspace::new();
    w.basic();
    let dev = config(&w.run(&["build", "--print-config"]));
    let release = config(&w.run(&["build", "--release", "--print-config"]));
    assert_ne!(path(&dev, "output"), path(&release, "output"));
    for args in [&["build"][..], &["build", "--release"]] {
        let o = w.run(args);
        success(&o);
        assert!(o.stdout.is_empty());
        assert!(text(&o).contains("Built"));
    }
    assert!(path(&dev, "output").is_file());
    assert!(path(&release, "output").is_file());
    assert_eq!(w.run(&["run"]).status.code(), Some(17));
    let o = w.run(&["check"]);
    success(&o);
    assert!(o.stdout.is_empty());
    assert!(String::from_utf8_lossy(&o.stderr).contains("Checked"));
    let o = w.run(&["check", "-q"]);
    success(&o);
    assert!(o.stdout.is_empty() && o.stderr.is_empty());
}
#[test]
fn explicit_sources_bypass_even_broken_manifests_and_release_is_standalone() {
    let w = Workspace::new();
    w.basic();
    w.file("dodo.toml", "schema = broken");
    assert_eq!(w.run(&["build"]).status.code(), Some(1));
    success(&w.run(&["check", "main.dodo"]));
    success(&w.run(&["check", "--no-manifest"]));
    let c = config(&w.run(&["build", "main.dodo", "--release", "--print-config"]));
    assert_eq!(*field(&c, "opt-level"), Kind::Integer(3));
    assert!(c.as_table().unwrap().get("manifest").is_none());
    let o = w.run(&["help", "run"]);
    success(&o);
    assert!(text(&o).contains("PROGRAM_ARGS"));
    assert!(!text(&o).contains("--output"));
}
#[test]
fn project_paths_stay_relative_to_manifest_and_cli_output_to_caller() {
    let w = Workspace::new();
    w.file("app/src/main.dodo", "package app\nfn main() {}\n");
    w.file(
        "app/dodo.toml",
        "schema=1\n[targets.app]\nentry='src/main.dodo'\n",
    );
    let c = config(&w.run(&["build", "app", "--print-config"]));
    assert_eq!(path(&c, "input"), w.0.join("app/src/main.dodo"));
    assert!(path(&c, "output").starts_with(w.0.join("app/build")));
    success(&w.run(&["build", "app", "--emit=obj", "-o", "custom output.o"]));
    assert!(w.0.join("custom output.o").is_file());
    let o = w
        .command()
        .current_dir(w.0.join("app/src"))
        .args(["check"])
        .output()
        .unwrap();
    success(&o);
    assert!(text(&o).contains("main.dodo"));
    let report = w
        .command()
        .current_dir(w.0.join("app/src"))
        .args(["build", "--print-config"])
        .output()
        .unwrap();
    assert!(
        config(&report)
            .as_table()
            .unwrap()
            .get("manifest")
            .is_none()
    );
}
#[test]
fn selected_platform_checks_and_early_run_rejection() {
    let w = Workspace::new();
    w.file(
        "lib.dodo",
        "package lib\npub fn value() -> usize { 4294967296 }\n",
    );
    w.file(
        "dodo.toml",
        "schema=1\n[targets.wasm]\nentry='lib.dodo'\nemit='obj'\ntriple='wasm32-unknown-unknown'\n",
    );
    let o = w.run(&["check", "-bwasm"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("usize"));
    fs::remove_file(w.0.join("lib.dodo")).unwrap();
    let o = w.run(&["run", "-bwasm"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("host executable"));
    assert!(!text(&o).contains("cannot open"));
    let o = w.run(&["build", "--target=wasm", "--print-config"]);
    assert!(text(&o).contains("use -b wasm"), "{}", text(&o));
}
#[test]
fn all_targets_are_listed_sorted_and_validated_before_any_build() {
    let w = Workspace::new();
    w.basic();
    w.file("dodo.toml","schema=1\n[targets.a]\nentry='main.dodo'\nemit='obj'\n[targets.z]\nentry='missing.dodo'\nemit='obj'\n");
    let o = w.run(&["targets"]);
    success(&o);
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("a\t"));
    assert_eq!(w.run(&["build"]).status.code(), Some(1));
    assert_eq!(w.run(&["build", "--all-targets"]).status.code(), Some(1));
    assert!(!w.0.join("build").exists());
    w.file(
        "missing.dodo",
        "package lib\npub fn add(a:i32,b:i32)->i32 { a+b }\n",
    );
    success(&w.run(&["build", "--all-targets"]));
    let c = config(&w.run(&["build", "--all-targets", "--print-config"]));
    let Kind::Array(a) = field(&c, "resolved") else {
        panic!()
    };
    assert_eq!(a.len(), 2);
}
#[test]
fn failed_builds_preserve_existing_outputs_and_manifest() {
    let w = Workspace::new();
    w.basic();
    w.file("artifact.o", "existing output");
    w.file("main.dodo", "package app\nfn main() { invalid }\n");
    assert_eq!(
        w.run(&["build", "--emit=obj", "-oartifact.o"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        fs::read_to_string(w.0.join("artifact.o")).unwrap(),
        "existing output"
    );
    w.file("main.dodo", "package app\nfn main() {}\n");
    let saved = fs::read(w.0.join("dodo.toml")).unwrap();
    let o = w.run(&["build", "--emit=obj", "-ododo.toml"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("overwrite the project manifest"));
    assert_eq!(fs::read(w.0.join("dodo.toml")).unwrap(), saved);
}
#[test]
fn object_targets_can_clear_shared_linker_arguments() {
    let w = Workspace::new();
    w.basic();
    w.file(
        "dodo.toml",
        "schema=1\n[build]\nlink-args=['-lmissing']\n[targets.app]\nentry='main.dodo'\n",
    );
    assert_eq!(w.run(&["build", "--emit=obj"]).status.code(), Some(1));
    success(&w.run(&["build", "--emit=obj", "--clear-link-args"]));
}
#[test]
fn hosted_tests_use_their_own_root_settings_and_profiles() {
    let w = Workspace::new();
    w.file("dodo.toml","schema=1\n[build]\ntriple='thumbv7em-none-eabi'\nlinker='missing-embedded-linker'\npanic-hook='board'\n[test]\nroot='tests'\ntimeout=3\n[test.build]\ndebug=true\n");
    w.file(
        "tests/suite.dodo",
        "package suite\n@test fn works(){ assert_eq(2+2,4) }\n",
    );
    w.file("elsewhere/broken.dodo", "@test broken");
    let c = config(&w.run(&["test", "--print-config"]));
    assert_eq!(path(&c, "input"), w.0.join("tests"));
    assert_eq!(*field(&c, "debug"), Kind::Bool(true));
    assert_eq!(*field(&c, "linker"), Kind::String("cc".into()));
    success(&w.run(&["test", "--release"]));
    let o = w.run(&["test", "--list", "--linker=missing-linker"]);
    success(&o);
    assert!(text(&o).contains("1 tests listed"));
    let o = w.run(&["test", "-g", "-q"]);
    success(&o);
    assert!(o.stdout.is_empty() && o.stderr.is_empty(), "{}", text(&o));
    success(&w.run(&["test", "tests", "--manifest-path=dodo.toml"]));
}
#[test]
fn usage_errors_are_distinct_from_configuration_failures() {
    let w = Workspace::new();
    for args in [
        &["help", "unknown"][..],
        &["build", "--opt-level=9"],
        &["test", "--list", "--print-config"],
        &["build", "--release", "--profile=dev"],
        &["run", "--port=8080"],
    ] {
        assert_eq!(w.run(args).status.code(), Some(2), "{args:?}");
    }
    w.file("dodo.toml", "schema=9");
    assert_eq!(w.run(&["check"]).status.code(), Some(1));
}
#[test]
fn init_and_completion_generation_do_not_overwrite_files() {
    let w = Workspace::new();
    success(&w.run(&["init", "new"]));
    assert!(w.0.join("new/main.dodo").is_file());
    success(&w.run(&["check", "new"]));
    let old = fs::read(w.0.join("new/dodo.toml")).unwrap();
    assert_eq!(w.run(&["init", "new"]).status.code(), Some(1));
    assert_eq!(fs::read(w.0.join("new/dodo.toml")).unwrap(), old);
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let o = w.run(&["completions", shell]);
        success(&o);
        assert!(text(&o).contains("--build-target"));
    }
}
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[test]
fn saved_arguments_override_and_empty_separator_clear_them() {
    let w = Workspace::new();
    w.file(
        "main.dodo",
        r#"package app
import "std/env"
fn main() -> i32 {
    bytes := [0u8;4096]
    wide := [0u16;4096]
    args := match env.arguments(&mut bytes,&mut wide) { ok(a)=>{a},err(_)=>{return 99} }
    return args.len() as i32
}
"#,
    );
    w.file("dodo.toml","schema=1\n[targets.app]\nentry='main.dodo'\n[targets.app.run]\nargs=['saved','two words']\n");
    assert_eq!(w.run(&["run"]).status.code(), Some(3));
    assert_eq!(w.run(&["run", "--", "replacement"]).status.code(), Some(2));
    assert_eq!(w.run(&["run", "--"]).status.code(), Some(1));
}
#[cfg(unix)]
#[test]
fn manifest_linker_working_directory_and_argv_are_exact() {
    use std::os::unix::fs::PermissionsExt;
    let w = Workspace::new();
    w.file("app/main.dodo", "package app\nfn main() {}\n");
    w.file(
        "app/dodo.toml",
        "schema=1\n[build]\nlinker='./linker'\nlink-args=['argument with spaces']\n",
    );
    w.file("app/linker","#!/bin/sh\npwd > cwd.txt\nprintf '%s\\n' \"$@\" > args.txt\nprevious=''\nfor arg do\n if [ \"$previous\" = '-o' ]; then printf artifact > \"$arg\"; fi\n previous=$arg\ndone\n");
    fs::set_permissions(w.0.join("app/linker"), fs::Permissions::from_mode(0o755)).unwrap();
    success(&w.run(&["build", "app", "-o", "result"]));
    assert!(w.0.join("result").is_file());
    assert_eq!(
        fs::read_to_string(w.0.join("app/cwd.txt")).unwrap().trim(),
        w.0.join("app").to_str().unwrap()
    );
    assert!(
        fs::read_to_string(w.0.join("app/args.txt"))
            .unwrap()
            .lines()
            .any(|s| s == "argument with spaces")
    );
}
#[cfg(unix)]
#[test]
fn dangling_manifest_and_output_symlink_do_not_bypass_validation() {
    use std::os::unix::fs::symlink;
    let w = Workspace::new();
    w.file("main.dodo", "package app\nfn main() {}\n");
    symlink("missing.toml", w.0.join("dodo.toml")).unwrap();
    assert_eq!(w.run(&["check"]).status.code(), Some(1));
    success(&w.run(&["check", "--no-manifest"]));
    fs::remove_file(w.0.join("dodo.toml")).unwrap();
    w.file("dodo.toml", "schema=1");
    symlink("dodo.toml", w.0.join("alias.o")).unwrap();
    assert_eq!(
        w.run(&["build", "--emit=obj", "-oalias.o"]).status.code(),
        Some(1)
    );
}

#[cfg(unix)]
#[test]
fn runtime_working_directory_uses_manifest_root_and_run_override() {
    let w = Workspace::new();
    w.file(
        "app/main.dodo",
        r#"package app
import "core/ptr"
unsafe extern "C" fn access(path:*const u8, mode:i32) -> i32
fn main() -> i32 { unsafe { return access(ptr.as_ptr(b"marker\x00"), 0) } }
"#,
    );
    w.file("app/fixtures/marker", "fixture");
    w.file(
        "app/dodo.toml",
        "schema=1\n[targets.app]\nentry='main.dodo'\n[targets.app.run]\ncwd='fixtures'\n",
    );
    success(&w.run(&["run", "app"]));
}

#[test]
fn manifest_test_rerun_hints_preserve_cleared_features_and_debug_settings() {
    let w = Workspace::new();
    w.file(
        "dodo.toml",
        "schema=1\n[test.build]\nfeatures='+sse2'\ndebug=true\n",
    );
    w.file(
        "suite.dodo",
        "package suite\n@test fn fails(){ assert(false) }\n",
    );
    let o = w.run(&["test", "--features=", "--no-debug", "--release"]);
    assert_eq!(o.status.code(), Some(1));
    let output = text(&o);
    let hint = output
        .split("Rerun the first failure:\n")
        .nth(1)
        .unwrap()
        .lines()
        .next()
        .unwrap();
    assert!(hint.contains("--manifest-path"));
    assert!(hint.contains("--no-debug"));
    assert!(hint.contains("--profile release"));
    assert!(hint.contains("--features ''"), "{hint}");
    assert!(hint.contains("--clear-link-args"));
}
