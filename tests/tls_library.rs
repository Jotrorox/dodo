//! TLS credentials are generated locally; no public network or checked-in key.
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
            "dodo-tls-{}-{}",
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
            "generate local TLS credentials",
        );
    }
}
fn fixture(directory: &Path, source: &str) -> PathBuf {
    let mut contents = source.to_owned();
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
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "TLS fixture exceeded 30s: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn verified_tls_fragmentation_backpressure_mutual_auth_rejections_and_cleanup() {
    let workspace = Workspace::new();
    credentials(&workspace.0);
    let source = fixture(&workspace.0, include_str!("tls/engine_checks.dodo"));
    for level in ["0", "3"] {
        let executable = workspace.0.join(format!("engine-{level}"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["-O", level, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile TLS fixture",
        );
        success(run_bounded(&executable), "run TLS fixture");
    }
}
#[test]
fn tls_portable_contract_has_no_backend_or_host_dependency() {
    let workspace = Workspace::new();
    let source = workspace.0.join("portable.dodo");
    fs::write(&source, "package portable\nimport \"std/tls\"\nimport \"std/tls/stream\"\npub fn state() -> tls.State { return tls.State.NeedInput }\n").unwrap();
    for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["--target", target, "--emit", "obj", "-o"])
                .arg(workspace.0.join("portable.o"))
                .output()
                .unwrap(),
            "cross compile portable TLS contract",
        );
    }
}
#[test]
fn tls_stream_keeps_transport_exclusively_borrowed() {
    let workspace = Workspace::new();
    let source = workspace.0.join("escape.dodo");
    fs::write(&source, "package escape\nimport \"std/tls/stream\"\nimport \"std/tls\"\nstruct Engine {}\nstruct Transport {}\nfn escape() -> stream.Stream<Engine, Transport>!tls.Error { transport := Transport {}\ninput := [0u8; 16]\noutput := [0u8; 16]\nreturn stream.Stream.new(Engine {}, &mut transport, &mut input[..], &mut output[..]) }\nfn main() {}\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("check")
        .arg(&source)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("borrow"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for mutation in ["input[0] = 7", "output[0] = 7", "transport = Transport {}"] {
        fs::write(
            &source,
            format!(
                r#"package reuse
import "std/tls/stream"
pub struct Engine {{ pub fn abort(&mut self) {{}} }}
struct Transport {{}}
fn main() {{
    transport := Transport {{}}
    input := [0u8; 16]
    output := [0u8; 16]
    match stream.Stream.new(Engine {{}}, &mut transport, &mut input[..], &mut output[..]) {{
        ok(connection) => {{ {mutation}
            connection.abort()
        }}
        err(_) => {{}}
    }}
}}
"#
            ),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "accepted live TLS scratch/transport mutation: {mutation}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("borrow"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn tls_stream_interoperates_with_python_ssl_over_loopback() {
    let workspace = Workspace::new();
    credentials(&workspace.0);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for level in ["0", "3"] {
        success(
            Command::new("python3")
                .arg(root.join("tests/tls/interop.py"))
                .arg(env!("CARGO_BIN_EXE_dodo"))
                .arg(&workspace.0)
                .arg(root.join("tests/tls/loopback_client.dodo"))
                .arg(level)
                .output()
                .unwrap(),
            "independent Python TLS loopback interoperability",
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn tls_owned_staging_allocation_failure_unwinds_cleanly() {
    let workspace = Workspace::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for level in ["-O0", "-O3"] {
        let executable = workspace.0.join(format!("allocation{level}"));
        success(
            Command::new("cc")
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", level])
                .arg(root.join("stdlib/std/tls/runtime.c"))
                .arg(root.join("tests/tls/allocation_failure.c"))
                .args(["-Wl,--wrap=CRYPTO_zalloc", "-lssl", "-lcrypto", "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
            "compile TLS allocation failure injection",
        );
        success(run_bounded(&executable), "recover TLS allocation failure");
    }
}
