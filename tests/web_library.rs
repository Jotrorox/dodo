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
        "web_request_response_checks",
        "http_connection_checks",
        "http_fast_checks",
        "web_application_checks",
        "web_registration_checks",
        "web_developer_checks",
        "web_fast_checks",
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
            "fluent-pattern",
            "fn bad() { path := [47u8]\n routes := application.builder().get(&path, web.text(b\"hello\"))\n path[0] = 65\n built := routes.build()! }",
        ),
        (
            "fluent-handler",
            "fn bad() { body := [65u8]\n routes := application.builder().get(b\"/\", web.text(&body))\n body[0] = 66\n built := routes.build()! }",
        ),
        (
            "required-parameter-view",
            "pub struct Bad { data: &[u8]\n pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure { self.data = request.param(b\"id\")?\n return ok() } }",
        ),
        (
            "registration-pattern",
            "fn bad() { path := [47u8]\n routes := application.new().get(&path, hosted.Text.new(b\"hello\"))!\n path[0] = 65\n count := routes.table.routes().len }",
        ),
        (
            "registration-handler",
            "fn bad() { body := [65u8]\n routes := application.new().get(b\"/\", hosted.Text.new(&body))!\n body[0] = 66\n count := routes.table.routes().len }",
        ),
        (
            "application-storage",
            "fn bad() { workspace := [0u8; hosted.WORKSPACE_BYTES]\n input := [0u8; 8]\n output := [0u8; 8]\n storage := app.Storage.new(&mut workspace, &mut input, &mut output)\n input[0] = 1\n routes := application.new().get(b\"/\", hosted.Text.new(b\"hello\"))!\n server := app.Server.new(routes)\n match server.serve_in(b\"invalid\", &mut storage) { ok(_) => {} err(_) => {} } }",
        ),
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
            "response-storage",
            "fn bad() { headers := [0u8; 64]\n data := [0u8; 64]\n response := web.Response.new(&mut headers, 4, &mut data)\n data[0] = 99\n count := response.body().len }",
        ),
        (
            "response-view",
            "fn bad(response: &mut web.Response) { view := response.body()\n match response.bytes(b\"new\") { ok() => {} err(_) => {} }\n count := view.len }",
        ),
        (
            "request-view",
            "pub struct Bad { data: &[u8]\n pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure { self.data = request.body()\n return ok() } }",
        ),
        (
            "validated-path",
            "fn bad() { data := [47u8]\n path := web.Path.new(&data)!\n data[0] = 65\n count := path.bytes().len }",
        ),
        (
            "owned-route-index",
            "fn bad() { small := [0usize; 2]\n index := hosted.RouteIndex.new(1025, &mut small)!\n storage := index.storage()\n core.drop(index)\n count := storage.len }",
        ),
        (
            "sequential-header-view",
            "fn bad(response: &mut web.Response) { cursor := 0usize\n view := response.header_next(&mut cursor)\n response.header(b\"X\", b\"new\")!\n match view { some(field) => { count := field.value.len } none => {} } }",
        ),
        (
            "request-header-view",
            "pub struct Bad { data: &[u8]\n pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure { match request.header(b\"X\", 0) { some(value) => { self.data = value } none => {} }\n return ok() } }",
        ),
        (
            "request-query-view",
            "pub struct Bad { data: &[u8]\n pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure { match request.query(b\"q\", 0) { some(value) => { self.data = value } none => {} }\n return ok() } }",
        ),
        (
            "request-parameter-view",
            "pub struct Bad { data: &[u8]\n pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure { match request.parameter(b\"id\") { some(value) => { self.data = value } none => {} }\n return ok() } }",
        ),
        (
            "response-header-view",
            "fn bad(response: &mut web.Response) { view := response.header_at(0)\n match response.header(b\"X\", b\"new\") { ok() => {} err(_) => {} }\n match view { some(field) => { count := field.value.len } none => {} } }",
        ),
        (
            "response-escape",
            "fn escape() -> web.Response from(static) { headers := [0u8; 64]\n data := [0u8; 64]\n return web.Response.new(&mut headers, 4, &mut data) }",
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
        fs::write(&source, format!("package rejected\nimport \"std/web\"\nimport \"std/web/stream\"\nimport \"std/web/application\"\nimport \"std/web/app\"\nimport \"std/web/hosted\"\nimport \"std/http/connection\"\n{body}\n")).unwrap();
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
fn hosted_route_indices_scale_past_inline_storage() {
    let scratch = Workspace::new();
    let mut source = String::from(
        "package large_routes\nimport \"std/web\"\nimport \"std/web/hosted\"\nimport \"std/http/hosting\"\nimport \"alloc/layout\"\n\
         fn main() {\n routes := [\n",
    );
    for i in (0..1026).rev() {
        let suffix = if i % 2 == 0 { "/:id" } else { "/fixed" };
        source.push_str(&format!(
            "web.Route {{ method: b\"GET\", pattern: b\"/group/{i:04}{suffix}\", id: {i} }},\n"
        ));
    }
    source.push_str(
        "]\n small := [0usize; 2048]\n storage := hosted.RouteIndex.new(routes.len, &mut small)!\n\
         router := web.Router.indexed(&routes, storage.storage())!\n",
    );
    for i in [0, 1, 511, 512, 1023, 1024, 1025] {
        let suffix = if i % 2 == 0 { "/42" } else { "/fixed" };
        source.push_str(&format!(
            "found_{i} := router.find(b\"HEAD\", b\"/group/{i:04}{suffix}\")!\n\
             assert_eq(found_{i}.id, {i}usize)\nassert(found_{i}.head)\n"
        ));
    }
    source.push_str(
        "core.drop(router)\n core.drop(storage)\n\
         counts := [0usize, 1usize, 1024usize, 1025usize, 4096usize]\n for count in counts {\n\
           inline := [0usize; 2048]\n owner := hosted.RouteIndex.new(*count, &mut inline)!\n data := owner.storage()\n\
           assert_eq(data.len, *count * 2)\n\
           for i in 0usize..data.len { assert_eq(data[i], 0usize); data[i] = i }\n\
         }\n\
         match hosted.RouteIndex.new(layout.max_size(), &mut small) {\n\
           ok(_) => { assert(false) }\n\
           err(reason) => { assert(reason.kind == hosting.ErrorKind.Workspace) }\n\
         }\n }\n",
    );
    let path = scratch.0.join("large_routes.dodo");
    fs::write(&path, source).unwrap();
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!("large-routes-{optimization}"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(&path)
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
        );
        success(Command::new(executable).output().unwrap());
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
fn application_examples_cross_compile_for_windows() {
    let scratch = Workspace::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for example in [
        "web_server",
        "web_server_concurrent",
        "web_routes",
        "web_response",
    ] {
        for optimization in ["0", "3"] {
            success(
                Command::new(env!("CARGO_BIN_EXE_dodo"))
                    .arg("compile")
                    .arg(root.join(format!("examples/{example}.dodo")))
                    .args([
                        "-O",
                        optimization,
                        "--target",
                        "x86_64-pc-windows-msvc",
                        "--emit",
                        "obj",
                        "-o",
                    ])
                    .arg(scratch.0.join(format!("{example}-{optimization}.o")))
                    .output()
                    .unwrap(),
            );
        }
    }
}

#[test]
fn buffered_requests_and_responses_interoperate_with_independent_peers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    success(
        Command::new("python3")
            .arg(root.join("scripts/test_web_request_response.py"))
            .arg("--compiler")
            .arg(env!("CARGO_BIN_EXE_dodo"))
            .output()
            .unwrap(),
    );
}

#[test]
fn application_setup_limits_and_shutdown_execute() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Workspace::new();
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!("app-config-{optimization}"));
        success(
            Command::new(env!("CARGO_BIN_EXE_dodo"))
                .arg("build")
                .arg(root.join("tests/http_hosted/app_config.dodo"))
                .args(["-O", optimization, "-o"])
                .arg(&executable)
                .output()
                .unwrap(),
        );
        success(Command::new(executable).output().unwrap());
    }
    success(
        Command::new("python3")
            .arg(root.join("scripts/test_web_app.py"))
            .arg("--compiler")
            .arg(env!("CARGO_BIN_EXE_dodo"))
            .output()
            .unwrap(),
    );
}

#[test]
fn fluent_applications_interoperate_with_independent_peers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    success(
        Command::new("python3")
            .arg(root.join("scripts/test_web_developer.py"))
            .arg("--compiler")
            .arg(env!("CARGO_BIN_EXE_dodo"))
            .output()
            .unwrap(),
    );
}
