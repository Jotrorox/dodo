//! Native filesystem ownership, byte/wide paths, I/O, mode and failure contracts.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-fs-{}-{}",
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
fn success(output: Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn compile(source: &std::path::Path, output: &std::path::Path, optimization: &str) {
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
            .args(["-O", optimization, "-o"])
            .arg(output)
            .output()
            .unwrap(),
        "compile native filesystem fixture",
    );
}

#[test]
fn filesystem_native_fixtures_at_o0_and_o3() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Workspace::new();
    let mut fixtures = vec!["fs_checks", "fs_modes", "fs_path_checks"];
    if cfg!(target_os = "linux") {
        fixtures.push("fs_linux_checks");
    }
    if cfg!(target_os = "windows") {
        fixtures.push("fs_windows_checks");
    }
    for fixture in fixtures {
        for optimization in ["0", "3"] {
            let source = root.join(format!("tests/os/{fixture}.dodo"));
            let executable = scratch.0.join(format!(
                "{fixture}-O{optimization}{}",
                std::env::consts::EXE_SUFFIX
            ));
            compile(&source, &executable, optimization);
            success(
                Command::new(executable)
                    .current_dir(&scratch.0)
                    .output()
                    .unwrap(),
                &format!("execute {fixture} at O{optimization}"),
            );
            assert!(!scratch.0.join("fs space Ω").exists());
            assert!(!scratch.0.join("symbolic link").exists());
        }
    }
}

#[test]
fn filesystem_windows_adapters_compile_at_o0_and_o3() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Workspace::new();
    for fixture in [
        "fs_checks",
        "fs_windows_checks",
        "fs_modes",
        "fs_path_checks",
    ] {
        for optimization in ["0", "3"] {
            let object = scratch.0.join(format!("{fixture}-{optimization}.obj"));
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(root.join(format!("tests/os/{fixture}.dodo")))
                    .args([
                        "--target",
                        "x86_64-pc-windows-msvc",
                        "--emit",
                        "obj",
                        "-O",
                        optimization,
                        "-o",
                    ])
                    .arg(&object)
                    .output()
                    .unwrap(),
                "compile real Win64 filesystem object",
            );
            assert!(fs::metadata(object).unwrap().len() > 0);
        }
    }
}

#[test]
fn lexical_paths_remain_freestanding() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Workspace::new();
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join("tests/os/fs_path_checks.dodo"))
                .args(["--target", target, "--emit", "obj", "-O", "3", "-o"])
                .arg(scratch.0.join(format!("{target}.o")))
                .output()
                .unwrap(),
            "lexical paths cross compile without OS imports",
        );
    }
}

#[test]
fn filesystem_borrows_and_results_are_checked() {
    let scratch = Workspace::new();
    let cases = [
        (
            "escape-path",
            "fn escape() -> native.NativeString!error.Error from(static) { b := [0u8; 64]\n w := [0u16; 64]\n return native.from_utf8(\"local\", &mut b, &mut w) }",
            "borrow",
        ),
        (
            "mutate-native-path",
            "fn main() { b := [0u8; 64]\n w := [0u16; 64]\n match native.from_utf8(\"path\", &mut b, &mut w) { ok(path) => { b[0] = 3\n match fs.metadata(&path) { ok(_) => {} err(_) => {} } } err(_) => {} } }",
            "borrow",
        ),
        (
            "ignore-open-result",
            "fn main() { b := [0u8; 64]\n w := [0u16; 64]\n match native.from_utf8(\"path\", &mut b, &mut w) { ok(path) => { options := fs.OpenOptions.read_only()\n fs.File.open(&path, &options) } err(_) => {} } }",
            "Result",
        ),
        (
            "private-handle",
            "fn main() { handle := native.Handle { value: 0 } }",
            "private",
        ),
        (
            "directory-entry-borrow",
            "fn bad(directory: &mut fs.Directory, b: &mut[u8], w: &mut[u16]) -> void!error.Error { match directory.next(b, w)? { some(name) => { b[0] = 0\n match fs.metadata(&name) { ok(_) => {} err(_) => {} } } none => {} } return ok() }",
            "borrow",
        ),
    ];
    for (name, body, expected) in cases {
        let source = scratch.0.join(format!("{name}.dodo"));
        fs::write(
            &source,
            format!(
                "package rejected\nimport \"std/fs\"\nimport \"std/platform/native\"\nimport \"std/platform/error\"\n{body}\n"
            ),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {name}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains(expected), "{name}: {diagnostic}");
    }
}

#[test]
fn filesystem_hosted_dependencies_reject_freestanding_and_wrong_abis() {
    let scratch = Workspace::new();
    let source = scratch.0.join("hosted.dodo");
    fs::write(
        &source,
        "package hosted\nimport \"std/fs\"\nfn main() -> i32 { return 0 }\n",
    )
    .unwrap();
    for target in [
        "wasm32-unknown-unknown",
        "thumbv6m-none-eabi",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&source)
            .args(["--target", target, "--emit", "obj", "-o"])
            .arg(scratch.0.join("hosted.o"))
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted unsupported {target}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(
            diagnostic.contains("target") || diagnostic.contains("supported"),
            "{target}: {diagnostic}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn cross_filesystem_rename_preserves_source_and_destination() {
    use std::os::unix::fs::MetadataExt;
    let scratch = Workspace::new();
    let Ok(shared_memory) = fs::metadata("/dev/shm") else {
        eprintln!("cross-device fixture unavailable: /dev/shm absent");
        return;
    };
    if shared_memory.dev() == fs::metadata(&scratch.0).unwrap().dev() {
        eprintln!("cross-device fixture unavailable: temporary storage shares device");
        return;
    }
    let destination = PathBuf::from("/dev/shm").join(format!(
        "dodo-fs-cross-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&destination, "destination").unwrap();
    struct RemoveFile(PathBuf);
    impl Drop for RemoveFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let cleanup = RemoveFile(destination.clone());
    let source = scratch.0.join("cross.dodo");
    fs::write(
        &source,
        format!(
            r#"package cross
import "std/fs"
import "std/platform/native"
import "std/platform/error"
fn run() -> i32!error.Error {{
    source := native.NativeString.new(b"source.txt\x00")?
    destination := native.NativeString.new(b"{}\x00")?
    match fs.rename(&source, &destination, true) {{
        ok() => {{ return ok(1) }}
        err(reason) => {{ if reason.kind != error.Kind.CrossDevice {{ return ok(2) }} }}
    }}
    return ok(0)
}}
fn main() -> i32 {{ match run() {{ ok(code) => {{ return code }} err(_) => {{ return 99 }} }} }}
"#,
            destination.display()
        ),
    )
    .unwrap();
    for optimization in ["0", "3"] {
        fs::write(scratch.0.join("source.txt"), "source").unwrap();
        let executable = scratch.0.join(format!("cross-{optimization}"));
        compile(&source, &executable, optimization);
        success(
            Command::new(executable)
                .current_dir(&scratch.0)
                .output()
                .unwrap(),
            "cross-filesystem rename must fail without fallback",
        );
        assert_eq!(fs::read(scratch.0.join("source.txt")).unwrap(), b"source");
        assert_eq!(fs::read(&destination).unwrap(), b"destination");
    }
    drop(cleanup);
}
