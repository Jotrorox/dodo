//! QUIC credentials are generated locally; all traffic stays on IPv4 loopback.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "dodo-quic-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        Self(directory)
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
fn credentials(directory: &Path) {
    let commands = [
        vec![
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-noenc",
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
            "-subj",
            "/CN=Dodo local test CA",
            "-days",
            "3650",
        ],
        vec![
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-noenc",
            "-keyout",
            "key.pem",
            "-out",
            "leaf.csr",
            "-subj",
            "/CN=localhost",
        ],
        vec![
            "x509",
            "-req",
            "-in",
            "leaf.csr",
            "-CA",
            "ca.pem",
            "-CAkey",
            "ca.key",
            "-CAcreateserial",
            "-out",
            "cert.pem",
            "-days",
            "2",
            "-extfile",
            "extensions.cnf",
        ],
    ];
    fs::write(directory.join("extensions.cnf"), "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1\nextendedKeyUsage=serverAuth,clientAuth\nbasicConstraints=critical,CA:FALSE\n").unwrap();
    for arguments in commands {
        success(
            Command::new("openssl")
                .args(arguments)
                .current_dir(directory)
                .output()
                .unwrap(),
            "generate local QUIC credentials",
        );
    }
}
fn fixture(directory: &Path) -> PathBuf {
    let mut contents = include_str!("quic/checks.dodo").to_owned();
    for (name, file) in [("CA", "ca.pem"), ("CERT", "cert.pem"), ("KEY", "key.pem")] {
        let data = fs::read_to_string(directory.join(file)).unwrap();
        contents = contents.replace(
            &format!("@@{name}@@"),
            &data
                .replace('\\', "\\\\")
                .replace('\n', "\\n")
                .replace('"', "\\\""),
        );
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    contents = contents.replace("const NOW: i64 = 0", &format!("const NOW: i64 = {now}"));
    let path = directory.join("checks.dodo");
    fs::write(&path, contents).unwrap();
    path
}
fn run_bounded(path: &Path) -> Output {
    let mut child = Command::new(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "QUIC fixture exceeded 60s: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
// A nonzero exit N below 99 names the failed fixture check 100 + N.
#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verified_quic_streams_backpressure_resets_rejections_and_idle_timeout() {
    let workspace = Workspace::new();
    credentials(&workspace.0);
    let source = fixture(&workspace.0);
    for level in ["0", "3"] {
        let executable = workspace.0.join(format!("checks-{level}"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["-O", level, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile QUIC fixture",
        );
        success(run_bounded(&executable), "run QUIC fixture");
    }
}
#[test]
fn quic_portable_vocabulary_has_no_backend_or_host_dependency() {
    let workspace = Workspace::new();
    let source = workspace.0.join("portable.dodo");
    fs::write(&source, "package portable\nimport \"std/quic\"\npub fn state() -> quic.State { return quic.State.Pending }\npub fn reason() -> quic.Error { return quic.failure(quic.ErrorKind.StreamReset, 0) }\n").unwrap();
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["--target", target, "--emit", "obj", "-o"])
                .arg(workspace.0.join("portable.o"))
                .output()
                .unwrap(),
            "cross compile portable QUIC vocabulary",
        );
    }
}
#[test]
fn quic_backend_requires_a_hosted_target() {
    let workspace = Workspace::new();
    let source = workspace.0.join("hosted.dodo");
    fs::write(&source, "package hosted\nimport \"std/quic/openssl\"\npub fn port(listener: &openssl.Listener) -> bool { return listener.timeout_ms() == none }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("build")
        .arg(&source)
        .args(["--target", "wasm32-unknown-unknown", "--emit", "obj", "-o"])
        .arg(workspace.0.join("hosted.o"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("std/quic/native"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn windows_quic_backend_cross_compiles() {
    let workspace = Workspace::new();
    let mut source = include_str!("quic/checks.dodo").to_owned();
    for name in ["CA", "CERT", "KEY"] {
        source = source.replace(&format!("@@{name}@@"), "");
    }
    let path = workspace.0.join("checks.dodo");
    fs::write(&path, source).unwrap();
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(&path)
            .args(["--target", "x86_64-pc-windows-msvc", "--emit", "obj", "-o"])
            .arg(workspace.0.join("checks.obj"))
            .output()
            .unwrap(),
        "Win64 QUIC adapter object",
    );
}
