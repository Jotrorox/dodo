use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-web-{}-{}",
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
fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn routing_middleware_and_backpressure_execute_and_remain_portable() {
    let scratch = Workspace::new();
    for fixture in [
        "web_checks",
        "http_connection_checks",
        "http_fast_checks",
        "web_application_checks",
    ] {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/stdlib/{fixture}.dodo"));
        for optimization in ["0", "3"] {
            let executable = scratch.0.join(format!("web-{optimization}"));
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("build")
                    .arg(&source)
                    .args(["-O", optimization, "-o"])
                    .arg(&executable)
                    .output()
                    .unwrap(),
            );
            success(Command::new(executable).output().unwrap());
            for target in ["wasm32-unknown-unknown", "thumbv6m-none-eabi"] {
                success(
                    Command::new(env!("CARGO_BIN_EXE_dodo"))
                        .arg("build")
                        .arg(&source)
                        .args([
                            "-O",
                            optimization,
                            "--target",
                            target,
                            "--emit",
                            "obj",
                            "-o",
                        ])
                        .arg(scratch.0.join(format!("{target}-{optimization}.o")))
                        .output()
                        .unwrap(),
                );
            }
        }
    }
}
#[test]
fn router_context_and_pending_body_borrows_cannot_escape() {
    let scratch = Workspace::new();
    for (name, body) in [
        (
            "router",
            "fn escape() -> web.Router!web.Error from(static) { routes := [web.Route { method: b\"GET\", pattern: b\"/\", id: 1 }]\n return web.Router.new(&routes) }",
        ),
        (
            "router-index",
            "fn bad() { routes := [web.Route { method: b\"GET\", pattern: b\"/\", id: 1 }]\n index := [0usize; 2]\n router := web.Router.indexed(&routes, &mut index)!\n index[0] = 99\n found := router.find(b\"GET\", b\"/\")! }",
        ),
        (
            "pending",
            "fn bad() { bytes := [1u8]\n pending := stream.PendingBody.new(&bytes)\n bytes[0] = 2\n count := pending.remaining() }",
        ),
        (
            "context",
            "fn escape() -> web.Context from(static) { path := [47u8]\n return web.Context { method: b\"GET\", path: &path, route_id: 0, request_id: 0, cancelled: false } }",
        ),
        (
            "body-view",
            "fn bad(driver: &mut connection.Connection) { view := driver.body()\n match driver.consume_body(1) { ok() => {} err(_) => {} }\n count := view.len }",
        ),
        (
            "callback",
            "pub struct Bad { data: &[u8]\n pub fn head(&mut self, context: &mut web.Context) { self.data = context.path } }",
        ),
    ] {
        let source = scratch.0.join(format!("{name}.dodo"));
        fs::write(&source, format!("package rejected\nimport \"std/web\"\nimport \"std/web/stream\"\nimport \"std/http/connection\"\n{body}\n")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(source)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {name}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("borrow"),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn static_files_hold_a_root_and_reject_escape_and_symlinks() {
    let scratch = Workspace::new();
    fs::create_dir(scratch.0.join("public")).unwrap();
    fs::create_dir(scratch.0.join("outside")).unwrap();
    fs::write(scratch.0.join("public/hello.txt"), "hello").unwrap();
    fs::write(scratch.0.join("outside/secret"), "secret").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("../outside/secret", scratch.0.join("public/link")).unwrap();
        std::os::unix::fs::symlink("../outside", scratch.0.join("public/dirlink")).unwrap();
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/web/static_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!("static-{optimization}"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&source)
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
        );
        success(
            Command::new(executable)
                .current_dir(&scratch.0)
                .output()
                .unwrap(),
        );
    }
}

#[test]
fn runnable_examples_interoperate_with_independent_http_peer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    success(
        Command::new("python3")
            .arg(root.join("scripts/test_http_web.py"))
            .arg("--compiler")
            .arg(env!("CARGO_BIN_EXE_dodo"))
            .output()
            .unwrap(),
    );
}

#[test]
fn concurrent_server_keeps_peers_independent_and_bounds_reuse() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    success(
        Command::new("python3")
            .arg(root.join("scripts/test_web_reactor.py"))
            .arg("--compiler")
            .arg(env!("CARGO_BIN_EXE_dodo"))
            .output()
            .unwrap(),
    );
}

#[test]
fn concurrent_server_cross_compiles_for_windows() {
    let scratch = Workspace::new();
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/web_server_concurrent.dodo");
    for optimization in ["0", "3"] {
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("compile")
                .arg(&source)
                .args([
                    "-O",
                    optimization,
                    "--target",
                    "x86_64-pc-windows-msvc",
                    "--emit",
                    "obj",
                    "-o",
                ])
                .arg(scratch.0.join(format!("reactor-{optimization}.o")))
                .output()
                .unwrap(),
        );
    }
}
