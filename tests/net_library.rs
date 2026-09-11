//! Portable network codecs, owned adapters and independent loopback peers.
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-net-{}-{}",
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
fn compile(source: &Path, output: &Path, optimization: &str) {
    success(
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("build")
            .arg(source)
            .args(["-O", optimization, "-o"])
            .arg(output)
            .output()
            .unwrap(),
        &format!("compile {} at O{optimization}", source.display()),
    );
}
#[test]
fn network_fixtures_at_o0_and_o3() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = Workspace::new();
    for fixture in [
        "stdlib/net_checks",
        "stdlib/net_operations",
        "net/native_checks",
        "net/backpressure",
    ] {
        for optimization in ["0", "3"] {
            let binary = workspace.0.join(format!(
                "{}-{optimization}{}",
                fixture.replace('/', "-"),
                std::env::consts::EXE_SUFFIX
            ));
            compile(
                &root.join(format!("tests/{fixture}.dodo")),
                &binary,
                optimization,
            );
            success(
                Command::new(binary).output().unwrap(),
                &format!("run {fixture} at O{optimization}"),
            );
        }
    }
}
#[test]
fn portable_network_packages_cross_compile_without_providers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = Workspace::new();
    for fixture in ["net_checks", "net_operations"] {
        for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(root.join(format!("tests/stdlib/{fixture}.dodo")))
                    .args(["--target", target, "--emit", "obj", "-O", "3", "-o"])
                    .arg(workspace.0.join(format!("{fixture}-{target}.o")))
                    .output()
                    .unwrap(),
                "portable net + DNS + operations compile freestanding",
            );
        }
    }
}
#[test]
fn windows_network_adapter_cross_compiles_at_o0_and_o3() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = Workspace::new();
    for fixture in ["native_checks", "backpressure", "exhaustion"] {
        for optimization in ["0", "3"] {
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(root.join(format!("tests/net/{fixture}.dodo")))
                    .args([
                        "--target",
                        "x86_64-pc-windows-msvc",
                        "--emit",
                        "obj",
                        "-O",
                        optimization,
                        "-o",
                    ])
                    .arg(workspace.0.join(format!("{fixture}-{optimization}.obj")))
                    .output()
                    .unwrap(),
                "Win64 network adapter object",
            );
        }
    }
}
#[cfg(target_os = "linux")]
#[test]
fn descriptor_exhaustion_unwinds_all_socket_owners() {
    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let binary = workspace.0.join(format!("exhaustion-{optimization}"));
        compile(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/net/exhaustion.dodo"),
            &binary,
            optimization,
        );
        success(
            Command::new("bash")
                .args(["-c", "ulimit -n 64; exec \"$1\"", "dodo-exhaustion"])
                .arg(binary)
                .output()
                .unwrap(),
            "descriptor exhaustion returns typed error and cleanup restores capacity",
        );
    }
}
#[test]
fn network_borrows_and_owned_handles_are_checked() {
    let workspace = Workspace::new();
    let cases = [
        (
            "private-handle",
            "fn main() { socket := native.TcpConnection { handle: 0, connecting: false, failed: false } }",
            "private",
        ),
        (
            "double-owner",
            "fn bad(address: &net.SocketAddress) -> void!net.Error { socket := native.TcpConnection.connect(address)?\n moved := socket\n socket.close()?\n return ok() }",
            "moved",
        ),
        (
            "resolver-borrow",
            "fn bad() -> void!net.Error { storage := [0u8; 128]\n addresses := native.resolve(b\"localhost\", 80, &mut storage)?\n storage[0] = 0\n address := addresses.get(0)?\n return ok() }",
            "borrow",
        ),
        (
            "resolver-escape",
            "fn bad() -> native.Addresses!net.Error from(static) { storage := [0u8; 128]\n return native.resolve(b\"localhost\", 80, &mut storage) }",
            "borrow",
        ),
        (
            "dns-borrow",
            "fn bad(packet: &[u8]) -> void!dns.Error { reader := dns.Reader.new(packet)?\n name := [0u8; 255]\n match reader.next_record(&mut name)? { some(record) => { name[0] = 0\n length := record.name.len } none => {} }\n return ok() }",
            "borrow",
        ),
    ];
    for (name, body, diagnostic) in cases {
        let source = workspace.0.join(format!("{name}.dodo"));
        fs::write(
            &source,
            format!(
                "package rejected\nimport \"std/net\"\nimport \"std/net/native\"\nimport \"std/net/dns\"\n{body}\n"
            ),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {name}");
        let text = String::from_utf8_lossy(&output.stderr);
        assert!(text.contains(diagnostic), "{name}: {text}");
    }
}
#[test]
fn independent_rust_tcp_peer_interoperates_with_fragmented_dodo_client() {
    let workspace = Workspace::new();
    for optimization in ["0", "3"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let source = workspace.0.join(format!("client-{optimization}.dodo"));
        fs::write(
            &source,
            format!(
                r#"package interop
import "std/net"
import "std/net/native"
import "std/net/operations"
fn run() -> i32!net.Error {{
 address := net.parse_socket(b"{address}")?
 clock := native.MonotonicClock {{}}
 cancel := net.NeverCancel {{}}
 operation := net.Operation.until(clock.now_ms() + 3000)
 client := native.connect_with(&address, &operation, &mut clock, &cancel)?
 match operations.write_all(&mut client, b"ping", &operation, &mut clock, &cancel) {{ ok(count) => {{ if count != 4 {{ return ok(1) }} }} err(_) => {{ return ok(2) }} }}
 buffer := [0u8; 1]
 for expected in b"pong" {{
  match operations.read(&mut client, &mut buffer, &operation, &mut clock, &cancel) {{ ok(count) => {{ if count != 1 || buffer[0] != *expected {{ return ok(3) }} }} err(_) => {{ return ok(4) }} }}
 }}
 match operations.read(&mut client, &mut buffer, &operation, &mut clock, &cancel) {{ ok(count) => {{ if count != 0 {{ return ok(5) }} }} err(_) => {{ return ok(6) }} }}
 return ok(0)
}}
fn main() -> i32 {{ match run() {{ ok(code) => {{ return code }} err(_) => {{ return 99 }} }} }}
"#
            ),
        )
        .unwrap();
        let binary = workspace.0.join(format!("client-{optimization}"));
        compile(&source, &binary, optimization);
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0_u8; 4];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"ping");
            for byte in b"pong" {
                stream.write_all(&[*byte]).unwrap();
                std::thread::sleep(Duration::from_millis(5));
            }
            stream.shutdown(Shutdown::Write).unwrap();
        });
        success(
            Command::new(binary).output().unwrap(),
            "Rust/Dodo TCP interop",
        );
        peer.join().unwrap();
    }
}
