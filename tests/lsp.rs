//! Exercise the real compiler process with independently framed JSON-RPC.
use dodoc::json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
const VALID: &str = "package app\nfn main() -> i32 { return 0 }\n";
const INVALID: &str = "package app\nfn main() -> i32 { return missing }\n";

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        loop {
            let path = std::env::temp_dir().join(format!(
                "dodo-lsp-{}-{} space # %20 + é 😀",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                // Overlay keys use canonical paths, including Windows' verbatim
                // prefix; URI fixtures below independently encode that path.
                Ok(()) => return Self(fs::canonicalize(path).unwrap()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => panic!("create workspace: {error}"),
            }
        }
    }

    fn uri(&self, name: &str) -> String {
        // Encode fixtures independently of the server's conversion, so URI
        // assertions also catch bugs in URIs generated for imported sources.
        let path = self.0.join(name);
        #[cfg(not(windows))]
        let bytes = path.as_os_str().as_encoded_bytes();
        #[cfg(windows)]
        let path = path.to_str().unwrap();
        #[cfg(windows)]
        let path = path
            .strip_prefix(r"\\?\")
            .unwrap_or(path)
            .replace('\\', "/");
        #[cfg(windows)]
        let bytes = path.as_bytes();
        let mut uri = String::from(if cfg!(windows) { "file:///" } else { "file://" });
        for &byte in bytes {
            if byte.is_ascii_alphanumeric() || b"/-._~:+".contains(&byte) {
                uri.push(char::from(byte));
            } else {
                use std::fmt::Write;
                write!(uri, "%{byte:02X}").unwrap();
            }
        }
        uri
    }

    fn file(&self, name: &str, text: &str) -> String {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        self.uri(name)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    messages: Receiver<Value>,
}

impl Client {
    fn start(arg: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg(arg)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || {
            loop {
                let mut header = String::new();
                if output.read_line(&mut header).unwrap() == 0 {
                    break;
                }
                let length: usize = header
                    .strip_prefix("Content-Length: ")
                    .expect("stdout must contain only LSP frames")
                    .trim()
                    .parse()
                    .unwrap();
                header.clear();
                output.read_line(&mut header).unwrap();
                assert_eq!(header, "\r\n");
                let mut body = vec![0; length];
                output.read_exact(&mut body).unwrap();
                let message: Value = dodoc::json::from_slice(&body).unwrap();
                assert_eq!(message["jsonrpc"], "2.0");
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            messages,
        }
    }

    fn send(&mut self, message: Value) {
        let body = dodoc::json::to_vec(&message);
        self.send_body(&body);
    }

    fn send_body(&mut self, body: &[u8]) {
        let input = self.input.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.write_all(body).unwrap();
        input.flush().unwrap();
    }

    fn receive(&self) -> Value {
        self.messages
            .recv_timeout(Duration::from_secs(15))
            .expect("server response")
    }

    fn request(&mut self, id: Value, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let response = self.receive();
        assert_eq!(response["id"], id, "unexpected response: {response}");
        response
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn initialize(&mut self, mode: &str) -> Value {
        let response = self.request(
            json!(1),
            "initialize",
            json!({
                "processId": null,
                "rootUri": null,
                "capabilities": {},
                "initializationOptions": { "checkMode": mode }
            }),
        );
        assert!(response.get("error").is_none(), "{response}");
        self.notify("initialized", json!({}));
        response
    }

    fn open(&mut self, uri: &str, text: &str, version: i32) {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": "dodo", "version": version, "text": text
            }}),
        );
    }

    fn change(&mut self, uri: &str, text: &str, version: i32) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": uri, "version": version},
                "contentChanges": [{"text": text}]
            }),
        );
    }

    fn close(&mut self, uri: &str) {
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        );
    }

    fn save(&mut self, uri: &str) {
        self.notify(
            "textDocument/didSave",
            json!({"textDocument": {"uri": uri}}),
        );
    }

    fn disk_change(&mut self, uri: &str, kind: i32) {
        self.notify(
            "workspace/didChangeWatchedFiles",
            json!({
                "changes": [{"uri": uri, "type": kind}]
            }),
        );
    }

    // An unsupported request acts as a barrier after synchronous notifications.
    // This catches unexpected replies to notifications without timed sleeps.
    fn diagnostics(&mut self) -> BTreeMap<String, Value> {
        self.send(json!({"jsonrpc": "2.0", "id": "barrier", "method": "dodo/testBarrier"}));
        let mut diagnostics = BTreeMap::new();
        loop {
            let message = self.receive();
            if message["id"] == "barrier" {
                assert_eq!(message["error"]["code"], -32601);
                return diagnostics;
            }
            assert_eq!(
                message["method"], "textDocument/publishDiagnostics",
                "{message}"
            );
            let params = message["params"].clone();
            diagnostics.insert(params["uri"].as_str().unwrap().to_owned(), params);
        }
    }

    fn wait_for_exit(&mut self, code: i32) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(code));
                let mut stderr = String::new();
                self.child
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut stderr)
                    .unwrap();
                return stderr;
            }
            assert!(Instant::now() < deadline, "server did not exit");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn shutdown(&mut self) -> String {
        let response = self.request(json!("stop"), "shutdown", Value::Null);
        assert_eq!(response.get("result"), Some(&Value::Null));
        self.notify("exit", Value::Null);
        // Deliberately keep stdin open: exiting must not wait for EOF.
        self.wait_for_exit(0)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn lsp_lifecycle_capabilities_and_request_errors() {
    let workspace = Workspace::new();
    let uri = workspace.uri("new.dodo");
    for arg in ["--lsp", "lsp"] {
        let mut client = Client::start(arg);
        client.open(&uri, INVALID, 1); // Notifications before initialization are ignored.
        let response = client.request(json!("early"), "shutdown", Value::Null);
        assert_eq!(response["error"]["code"], -32002);
        let response = client.request(json!(2), "initialize", json!({"capabilities": 5}));
        assert_eq!(response["error"]["code"], -32602);
        let response = client.initialize("file");
        let capabilities = &response["result"]["capabilities"];
        assert_eq!(capabilities["textDocumentSync"]["change"], 1);
        assert_eq!(capabilities["textDocumentSync"]["openClose"], true);
        assert_eq!(
            capabilities["textDocumentSync"]["save"]["includeText"],
            false
        );
        assert_eq!(capabilities["positionEncoding"], "utf-16");
        assert_eq!(response["result"]["serverInfo"]["name"], "dodo");
        assert!(client.diagnostics().is_empty());
        let response = client.request(json!(3), "initialize", json!({"capabilities": {}}));
        assert_eq!(response["error"]["code"], -32600);
        client.notify("$/cancelRequest", json!({"id": 123}));
        client.notify("unknown/notification", Value::Null);
        assert_eq!(capabilities["hoverProvider"], true);
        assert_eq!(capabilities["inlayHintProvider"], true);
        assert_eq!(
            capabilities["codeActionProvider"]["codeActionKinds"],
            json!(["quickfix"])
        );
        let response = client.request(json!("hover"), "textDocument/hover", json!({}));
        assert_eq!(response["error"]["code"], -32602);
        assert!(client.shutdown().is_empty());
    }
}

#[test]
fn lsp_exit_eof_and_shutdown_do_not_hang() {
    let mut client = Client::start("--lsp");
    client.notify("exit", Value::Null);
    assert!(client.wait_for_exit(1).is_empty());

    let mut client = Client::start("--lsp");
    client.input.take();
    assert!(client.wait_for_exit(1).is_empty());

    let mut client = Client::start("--lsp");
    client.initialize("file");
    assert!(
        client
            .request(json!(2), "shutdown", Value::Null)
            .get("result")
            .is_some()
    );
    client.notify("textDocument/didOpen", json!({}));
    let response = client.request(json!(3), "unknown", Value::Null);
    assert_eq!(response["error"]["code"], -32600);
    client.input.take();
    assert!(client.wait_for_exit(0).is_empty());
}

#[test]
fn lsp_open_change_save_close_uses_unsaved_text_and_clears_errors() {
    let workspace = Workspace::new();
    let uri = workspace.file("main.dodo", VALID);
    let mut client = Client::start("--lsp");
    client.initialize("file");
    client.open(&uri, INVALID, 1);
    let diagnostics = client.diagnostics();
    let report = &diagnostics[&uri];
    assert_eq!(report["version"], 1);
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["source"], "dodo");
    assert_eq!(diagnostic["severity"], 1);
    assert_eq!(diagnostic["message"], "unknown binding `missing`");
    assert_eq!(
        diagnostic["range"],
        json!({
            "start": {"line": 1, "character": 26}, "end": {"line": 1, "character": 33}
        })
    );
    client.save(&uri);
    assert_eq!(client.diagnostics()[&uri], *report); // Disk still contains valid text.
    client.change(&uri, VALID, 2);
    let report = client.diagnostics();
    assert_eq!(report[&uri]["version"], 2);
    assert_eq!(report[&uri]["diagnostics"], json!([]));
    client.change(&uri, INVALID, 1); // Stale versions cannot restore old errors.
    assert!(client.diagnostics().is_empty());
    client.change(&uri, INVALID, 3);
    assert_eq!(
        client.diagnostics()[&uri]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    client.close(&uri);
    let report = client.diagnostics();
    assert_eq!(report[&uri]["diagnostics"], json!([]));
    assert!(report[&uri].get("version").is_none());
    assert_eq!(
        fs::read_to_string(workspace.0.join("main.dodo")).unwrap(),
        VALID
    );
    assert!(!workspace.0.join("build").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_reports_lexer_parser_and_unicode_semantic_errors_in_new_files() {
    let workspace = Workspace::new();
    let uri = workspace.uri("new é 😀.dodo");
    let mut client = Client::start("--lsp");
    client.initialize("file");
    let cases = [
        ("package app\n\"", "unterminated"),
        ("package app\n@", "expected an identifier"),
        ("package app\nfn main(", "expected an identifier"),
        (
            "package app\nfn main() -> i32 { return true }",
            "expected `i32`",
        ),
    ];
    for (index, (source, expected)) in cases.iter().enumerate() {
        if index == 0 {
            client.open(&uri, source, 1);
        } else {
            client.change(&uri, source, index as i32 + 1);
        }
        let diagnostics = client.diagnostics();
        assert!(
            diagnostics[&uri]["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains(expected),
            "{diagnostics:?}"
        );
    }
    let source = "package app\r\nfn main() -> i32 { text := \"😀é\"; return missing }\r\n";
    client.change(&uri, source, 5);
    let diagnostics = client.diagnostics();
    let line = source.lines().nth(1).unwrap();
    let column = line[..line.find("missing").unwrap()].encode_utf16().count();
    let diagnostic = &diagnostics[&uri]["diagnostics"][0];
    assert_eq!(diagnostic["message"], "unknown binding `missing`");
    assert_eq!(
        diagnostic["range"]["start"],
        json!({"line": 1, "character": column})
    );
    assert_eq!(
        diagnostic["range"]["end"],
        json!({"line": 1, "character": column + 7})
    );
    assert!(!workspace.0.join("new é 😀.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_keeps_borrow_diagnostic_notes() {
    let workspace = Workspace::new();
    let uri = workspace.uri("borrow.dodo");
    let mut client = Client::start("--lsp");
    client.initialize("file");
    client.open(
        &uri,
        "package app\nfn main() -> i32 {\n x := 1i32\n r := &x\n x = 2\n return *r\n}\n",
        1,
    );
    let diagnostics = client.diagnostics();
    let message = diagnostics[&uri]["diagnostics"][0]["message"]
        .as_str()
        .unwrap();
    assert!(message.contains("borrow"), "{message}");
    assert!(message.contains("\nnote: "), "{message}");
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_import_errors_are_published_at_the_dependency_and_cleared_on_save() {
    let workspace = Workspace::new();
    let uri = workspace.file(
        "main.dodo",
        "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n",
    );
    let dependency = workspace.file(
        "lib/value.dodo",
        "package lib\npub fn value() -> i32 { return missing }\n",
    );
    let mut client = Client::start("--lsp");
    client.initialize("file");
    client.open(
        &uri,
        &fs::read_to_string(workspace.0.join("main.dodo")).unwrap(),
        1,
    );
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&uri]["diagnostics"], json!([]));
    let diagnostic = &diagnostics[&dependency]["diagnostics"][0];
    assert_eq!(diagnostic["message"], "unknown binding `missing`");
    assert_eq!(
        diagnostic["range"]["start"],
        json!({"line": 1, "character": 31})
    );
    assert!(diagnostics[&dependency].get("version").is_none());
    workspace.file(
        "lib/value.dodo",
        "package lib\npub fn value() -> i32 { return 7 }\n",
    );
    client.save(&uri);
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&dependency]["diagnostics"], json!([]));
    assert_eq!(diagnostics[&uri]["diagnostics"], json!([]));
    // Parser failures retain the dependency's own source and local byte span.
    workspace.file("lib/value.dodo", "package lib\npub fn value(");
    client.save(&uri);
    let diagnostics = client.diagnostics();
    assert_eq!(
        diagnostics[&dependency]["diagnostics"][0]["range"]["start"]["line"],
        1
    );
    assert!(
        !diagnostics[&dependency]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .starts_with("error:")
    );
    client.close(&uri);
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&dependency]["diagnostics"], json!([]));
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_imports_use_open_buffers_and_recheck_callers_after_changes_and_close() {
    let workspace = Workspace::new();
    let root = "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n";
    let original = "package lib\npub fn value() -> i32 { return 1 }\n";
    let uri = workspace.file("main.dodo", root);
    let dependency = workspace.file("lib/value.dodo", original);
    let mut client = Client::start("--lsp");
    client.initialize("file");
    client.open(&uri, root, 1);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    client.open(
        &dependency,
        "package lib\npub fn value() -> bool { return true }\n",
        1,
    );
    let diagnostics = client.diagnostics();
    assert!(
        diagnostics[&uri]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("expected `i32`")
    );
    assert_eq!(diagnostics[&dependency]["diagnostics"], json!([]));
    client.change(&dependency, original, 2);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    client.change(
        &dependency,
        "package lib\npub fn value() -> bool { return true }\n",
        3,
    );
    assert!(
        !client.diagnostics()[&uri]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    client.close(&dependency);
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&uri]["diagnostics"], json!([]));
    assert_eq!(diagnostics[&dependency]["diagnostics"], json!([]));
    assert_eq!(
        fs::read_to_string(workspace.0.join("lib/value.dodo")).unwrap(),
        original
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_package_mode_includes_unsaved_sibling_files() {
    let workspace = Workspace::new();
    let root = "package app\nfn main() -> i32 { return value() }\n";
    let uri = workspace.file("main.dodo", root);
    let sibling = workspace.uri("value.dodo");
    let mut client = Client::start("--lsp");
    client.initialize("package");
    client.open(&uri, root, 1);
    assert!(
        client.diagnostics()[&uri]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("unknown function")
    );
    client.open(&sibling, "package app\nfn value() -> i32 { return 1 }\n", 1);
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&uri]["diagnostics"], json!([]));
    assert_eq!(diagnostics[&sibling]["diagnostics"], json!([]));
    client.change(
        &sibling,
        "package app\nfn value() -> i32 { return missing }\n",
        2,
    );
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&uri]["diagnostics"], json!([]));
    assert_eq!(diagnostics[&sibling]["version"], 2);
    assert_eq!(
        diagnostics[&sibling]["diagnostics"][0]["message"],
        "unknown binding `missing`"
    );
    client.close(&sibling);
    let diagnostics = client.diagnostics();
    assert!(
        !diagnostics[&uri]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(diagnostics[&sibling]["diagnostics"], json!([]));
    assert!(!workspace.0.join("value.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_resolves_new_unsaved_import_files() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.dodo");
    let dependency = workspace.uri("lib.dodo");
    let mut client = Client::start("--lsp");
    client.initialize("file");
    client.open(
        &uri,
        "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n",
        1,
    );
    assert!(
        client.diagnostics()[&uri]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("cannot resolve import")
    );
    client.open(
        &dependency,
        "package lib\npub fn value() -> i32 { return 1 }\n",
        1,
    );
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&uri]["diagnostics"], json!([]));
    assert_eq!(diagnostics[&dependency]["diagnostics"], json!([]));
    client.close(&dependency);
    let diagnostics = client.diagnostics();
    assert!(
        diagnostics[&uri]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("cannot resolve import")
    );
    assert_eq!(diagnostics[&dependency]["diagnostics"], json!([]));
    assert!(!workspace.0.join("lib.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_missing_imports_and_malformed_notifications_do_not_stop_the_server() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.dodo");
    let mut client = Client::start("--lsp");
    client.initialize("file");
    client.open(&uri, "package app\nimport \"missing\"\n", 1);
    let diagnostics = client.diagnostics();
    assert!(
        diagnostics[&uri]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("cannot resolve import")
    );
    client.notify("textDocument/didChange", json!({}));
    client.notify("textDocument/didChange", json!({
        "textDocument": {"uri": uri, "version": 2},
        "contentChanges": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "text": VALID}]
    }));
    assert!(client.diagnostics().is_empty()); // Invalid changes publish nothing.
    client.open("untitled:Untitled-1", VALID, 1);
    assert_eq!(
        client.diagnostics()["untitled:Untitled-1"]["diagnostics"],
        json!([])
    );
    client.change(&uri, VALID, 2); // Rejected edits did not consume this version.
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    let nested = format!(
        "package app\nfn main() -> i32 {{ return {}1{} }}",
        "(".repeat(300),
        ")".repeat(300)
    );
    client.change(&uri, &nested, 3);
    assert!(
        client.diagnostics()[&uri]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("nesting")
    );
    client.change(&uri, VALID, 4);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    let stderr = client.shutdown();
    assert!(
        stderr.contains("expected a full document change"),
        "{stderr}"
    );
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn lsp_rejects_invalid_file_uris_and_recovers() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.dodo");
    let mut client = Client::start("--lsp");
    client.initialize("file");
    for invalid in [
        format!("{uri}?query"),
        format!("{uri}#"),
        format!("{uri}%00"),
        uri.replacen("file:", "https:", 1),
        uri.replacen("file:///", "file://user@localhost/", 1),
        uri.replacen("file:///", "file://localhost:80/", 1),
        "file:relative.dodo".into(),
        "file:///tmp/%".into(),
        "file:///tmp/%0g".into(),
    ] {
        client.open(&invalid, VALID, 1);
        assert!(client.diagnostics().is_empty(), "accepted {invalid}");
    }
    client.open(&uri, INVALID, 1);
    assert_eq!(
        client.diagnostics()[&uri]["diagnostics"][0]["message"],
        "unknown binding `missing`"
    );
    client.change(&uri, VALID, 2);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    assert!(!client.shutdown().contains("panicked"));
}

#[test]
#[cfg(unix)]
fn lsp_reports_imports_in_non_utf8_directories() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let parent = Workspace::new();
    let workspace = Workspace(parent.0.join(OsString::from_vec(b"non-utf8-\xff".to_vec())));
    let text = "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n";
    let root = workspace.file("main.dodo", text);
    let dependency = workspace.file(
        "lib.dodo",
        "package lib\npub fn value() -> i32 { return missing }\n",
    );
    assert!(dependency.contains("non-utf8-%FF"));
    let mut client = Client::start("--lsp");
    client.initialize("file");
    client.open(&root, text, 1);
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&root]["diagnostics"], json!([]));
    assert_eq!(
        diagnostics[&dependency]["diagnostics"][0]["message"],
        "unknown binding `missing`"
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_preserves_client_uri_spelling_for_open_imports() {
    let workspace = Workspace::new();
    let text = "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n";
    let root = workspace.file("main.dodo", text);
    let dependency = workspace.file(
        "lib.dodo",
        "package lib\npub fn value() -> i32 { return 0 }\n",
    );
    let alias = dependency
        .replacen("file:///", "FILE://LOCALHOST/", 1)
        .replace("%C3%A9", "%c3%a9");
    let mut client = Client::start("--lsp");
    client.initialize("file");
    client.open(
        &alias,
        "package lib\npub fn value() -> i32 { return missing }\n",
        1,
    );
    client.diagnostics();
    client.open(&root, text, 1);
    let diagnostics = client.diagnostics();
    assert_eq!(
        diagnostics[&alias]["diagnostics"][0]["message"],
        "unknown binding `missing`"
    );
    assert!(!diagnostics.contains_key(&dependency));
    client.change(
        &alias,
        "package lib\npub fn value() -> i32 { return 7 }\n",
        2,
    );
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&alias]["diagnostics"], json!([]));
    assert_eq!(diagnostics[&root]["diagnostics"], json!([]));
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_truncated_transport_exits_without_panicking() {
    let mut client = Client::start("--lsp");
    client
        .input
        .as_mut()
        .unwrap()
        .write_all(b"Content-Length: 20\r\n\r\n{}")
        .unwrap();
    client.input.take();
    let stderr = client.wait_for_exit(1);
    assert!(stderr.contains("LSP transport failed"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn lsp_package_hovers_include_new_unsaved_sibling_types_and_contracts() {
    let workspace = Workspace::new();
    let root = workspace.uri("main.dodo");
    let sibling = workspace.uri("views.dodo");
    let mut client = Client::start("lsp");
    client.initialize("package");
    client.open(&root, "package app\nfn main() -> i32 {\nnumber := 42i32\nview := borrow(&number)\nreturn *view\n}\n", 1);
    client.diagnostics();
    client.open(
        &sibling,
        "package app\nfn borrow(value: &i32) -> &i32 { return value }\n",
        1,
    );
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&root]["diagnostics"], json!([]));
    assert_eq!(diagnostics[&sibling]["diagnostics"], json!([]));
    for (id, uri, line, character, expected) in [
        (10, &root, 3, 0, "view: &i32"),
        (11, &root, 3, 8, "from(value)` (inferred)"),
        (12, &sibling, 1, 3, "fn borrow(value: &i32) -> &i32"),
    ] {
        let response = client.request(
            json!(id),
            "textDocument/hover",
            json!({"textDocument":{"uri":uri},"position":{"line":line,"character":character}}),
        );
        let hover = &response["result"];
        assert!(
            hover["contents"]["value"]
                .as_str()
                .unwrap()
                .contains(expected),
            "{response}"
        );
        assert_eq!(hover["range"]["start"]["line"], line);
    }
    assert!(!workspace.0.join("main.dodo").exists());
    assert!(!workspace.0.join("views.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_borrow_labels_in_imports_use_their_own_uris_and_ranges() {
    let workspace = Workspace::new();
    let root_text = "package app\nimport \"views\"\nfn main() {}\n";
    let root = workspace.file("main.dodo", root_text);
    let imported = workspace.file(
        "views.dodo",
        "package views\npub fn bad(a: &i32, b: &i32) -> &i32 from(a) {\nreturn b\n}\n",
    );
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&root, root_text, 1);
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&root]["diagnostics"], json!([]));
    let diagnostic = &diagnostics[&imported]["diagnostics"][0];
    assert_eq!(diagnostic["range"]["start"]["line"], 2);
    let labels = diagnostic["relatedInformation"].as_array().unwrap();
    assert!(labels.len() >= 2, "{diagnostic}");
    assert!(
        labels
            .iter()
            .all(|label| label["location"]["uri"] == imported),
        "{diagnostic}"
    );
    assert!(
        labels
            .iter()
            .any(|label| label["location"]["range"]["start"]["line"] == 1
                && label["message"].as_str().unwrap().contains("from")),
        "{diagnostic}"
    );
    assert!(client.shutdown().is_empty());
}

fn at(text: &str, needle: &str) -> Value {
    let byte = text
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?}"));
    let prefix = &text[..byte];
    json!({"line":prefix.bytes().filter(|b| *b == b'\n').count(),
        "character":prefix.rsplit('\n').next().unwrap().encode_utf16().count()})
}

fn full_range(text: &str) -> Value {
    json!({"start":{"line":0,"character":0}, "end": {
        "line": text.bytes().filter(|b| *b == b'\n').count(),
        "character": text.rsplit('\n').next().unwrap().trim_end_matches('\r').encode_utf16().count()
    }})
}

fn range_query(
    client: &mut Client,
    method: &str,
    uri: &str,
    range: Value,
    context: Value,
) -> Value {
    client.request(
        json!("range"),
        &format!("textDocument/{method}"),
        json!({"textDocument":{"uri":uri},"range":range,"context":context}),
    )
}

#[test]
fn lsp_inlay_hints_follow_unsaved_types_ranges_and_package_offsets() {
    let workspace = Workspace::new();
    let uri = workspace.uri("z.dodo");
    workspace.file("a.dodo", "package app\nfn number() -> i32 { return 42 }\n");
    let source = "package app\r\nfn main() -> i32 {\r\n_ = \"😀\"; let answer = number()\r\nlet explicit: i32 = answer\r\nreturn explicit\r\n}\r\n";
    let mut client = Client::start("lsp");
    client.initialize("package");
    client.open(&uri, source, 1);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    let response = range_query(
        &mut client,
        "inlayHint",
        &uri,
        full_range(source),
        Value::Null,
    );
    let hints = response["result"].as_array().unwrap();
    assert_eq!(hints.len(), 1, "{response}");
    assert_eq!(hints[0]["label"], ": i32");
    assert_eq!(hints[0]["kind"], 1);
    assert_eq!(hints[0]["position"], at(source, " = number()"));
    let response = range_query(
        &mut client,
        "inlayHint",
        &uri,
        json!({"start":{"line":3,"character":0},"end":{"line":4,"character":0}}),
        Value::Null,
    );
    assert_eq!(response["result"], json!([]));
    let changed = source
        .replace("number()", "true")
        .replace("let explicit: i32 = answer", "let explicit: i32 = 1");
    client.change(&uri, &changed, 2);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    let response = range_query(
        &mut client,
        "inlayHint",
        &uri,
        full_range(&changed),
        Value::Null,
    );
    assert_eq!(response["result"][0]["label"], ": bool");
    assert!(!workspace.0.join("z.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_quick_fixes_apply_imports_and_mutability_and_clear_diagnostics() {
    let workspace = Workspace::new();
    workspace.file(
        "numbers.dodo",
        "package numbers\npub fn value() -> i32 { return 42 }\n",
    );
    let uri = workspace.uri("main.dodo");
    let mut client = Client::start("lsp");
    client.initialize("file");
    for (version, source, title) in [
        (
            1,
            "// 😀 keep this header\r\npackage app\r\nfn main() -> i32 { return numbers.value() }\r\n",
            "import",
        ),
        (
            3,
            "package app\nfn main() -> i32 {\n_ = \"😀\"; let count = 1i32\ncount = 2\nreturn count\n}\n",
            "mutable",
        ),
    ] {
        if version == 1 {
            client.open(&uri, source, version);
        } else {
            client.change(&uri, source, version);
        }
        let diagnostics = client.diagnostics()[&uri]["diagnostics"].clone();
        assert!(!diagnostics.as_array().unwrap().is_empty());
        let response = range_query(
            &mut client,
            "codeAction",
            &uri,
            full_range(source),
            json!({"diagnostics":diagnostics}),
        );
        let actions = response["result"].as_array().unwrap();
        let action = actions
            .iter()
            .find(|action| {
                action["title"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains(title)
            })
            .expect("actionable fix");
        assert_eq!(action["kind"], "quickfix");
        assert!(!action["diagnostics"].as_array().unwrap().is_empty());
        let fixed = apply_text_edits(source, &action["edit"]["changes"][&uri]);
        assert_ne!(fixed, source);
        if title == "import" {
            assert!(
                fixed.starts_with("// 😀 keep this header\r\npackage app\r\n"),
                "{fixed}"
            );
            assert!(fixed.contains("import \"numbers\"\r\n"), "{fixed}");
        }
        client.change(&uri, &fixed, version + 1);
        assert_eq!(
            client.diagnostics()[&uri]["diagnostics"],
            json!([]),
            "{fixed}"
        );
        let stale = range_query(
            &mut client,
            "codeAction",
            &uri,
            full_range(&fixed),
            json!({"diagnostics":diagnostics}),
        );
        assert_eq!(
            stale["result"],
            json!([]),
            "stale client diagnostics must not create edits"
        );
    }
    assert!(!workspace.0.join("main.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_quick_fixes_use_versions_and_unsaved_import_candidates() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.dodo");
    let imported = workspace.uri("values.dodo");
    let source = "package app\nfn main() -> i32 { return values.answer() }\n";
    let mut client = Client::start("lsp");
    let response = client.request(
        json!(1),
        "initialize",
        json!({"capabilities":{"workspace":{"workspaceEdit":{"documentChanges":true}}}}),
    );
    assert!(response.get("error").is_none());
    client.open(
        &imported,
        "package values\npub fn answer() -> i32 { return 42 }\n",
        1,
    );
    client.diagnostics();
    client.open(&uri, source, 7);
    let diagnostics = client.diagnostics()[&uri]["diagnostics"].clone();
    let response = range_query(
        &mut client,
        "codeAction",
        &uri,
        full_range(source),
        json!({"diagnostics":diagnostics,"only":["quickfix"]}),
    );
    let edit = &response["result"][0]["edit"]["documentChanges"][0];
    assert_eq!(
        edit["textDocument"],
        json!({"uri":uri,"version":7}),
        "{response}"
    );
    let fixed = apply_text_edits(source, &edit["edits"]);
    client.change(&uri, &fixed, 8);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    assert!(!workspace.0.join("values.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_range_requests_validate_parameters_and_filter_action_kinds() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.dodo");
    let source = "package app\nfn main() { let count = 1i32; count = 2 }\n";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&uri, source, 1);
    client.diagnostics();
    for method in ["inlayHint", "codeAction"] {
        for range in [
            Value::Null,
            json!({"start":{"line":-1,"character":0},"end":{"line":0,"character":0}}),
            json!({"start":{"line":1,"character":0},"end":{"line":0,"character":0}}),
            json!({"start":{"line":0,"character":false},"end":{"line":0,"character":0}}),
        ] {
            let response = range_query(&mut client, method, &uri, range, json!({"diagnostics":[]}));
            assert_eq!(response["error"]["code"], -32602, "{response}");
        }
    }
    for context in [
        Value::Null,
        json!({}),
        json!({"diagnostics":false}),
        json!({"diagnostics":[{}]}),
        json!({"diagnostics":[],"only":[1]}),
        json!({"diagnostics":[],"triggerKind":3}),
    ] {
        let response = range_query(&mut client, "codeAction", &uri, full_range(source), context);
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
    for only in [
        json!([]),
        json!(["source.organizeImports"]),
        json!(["refactor"]),
    ] {
        let response = range_query(
            &mut client,
            "codeAction",
            &uri,
            full_range(source),
            json!({"diagnostics":[],"only":only}),
        );
        assert_eq!(response["result"], json!([]));
    }
    let response = range_query(
        &mut client,
        "codeAction",
        &uri,
        json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}}),
        json!({"diagnostics":[]}),
    );
    assert_eq!(response["result"], json!([]));
    let response = range_query(
        &mut client,
        "inlayHint",
        "untitled:unopened",
        full_range(source),
        Value::Null,
    );
    assert_eq!(response["result"], json!([]));
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_untitled_quick_fix_updates_inferred_hint_without_saving() {
    let uri = "untitled:quick-fix";
    let source = "package app\nfn main() { let value = 1u32; value = 2 }\n";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(uri, source, 1);
    let diagnostics = client.diagnostics()[uri]["diagnostics"].clone();
    let response = range_query(
        &mut client,
        "codeAction",
        uri,
        diagnostics[0]["range"].clone(),
        json!({"diagnostics": diagnostics}),
    );
    let fixed = apply_text_edits(source, &response["result"][0]["edit"]["changes"][uri]);
    client.change(uri, &fixed, 2);
    assert_eq!(client.diagnostics()[uri]["diagnostics"], json!([]));
    let hints = range_query(
        &mut client,
        "inlayHint",
        uri,
        full_range(&fixed),
        Value::Null,
    );
    assert_eq!(hints["result"][0]["label"], ": u32");
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_inlay_refresh_rechecks_dependents_and_honors_client_support() {
    fn receive_refresh(client: &mut Client) -> Value {
        loop {
            let message = client.receive();
            if message["method"] == "workspace/inlayHint/refresh" {
                return message["id"].clone();
            }
            assert_eq!(
                message["method"], "textDocument/publishDiagnostics",
                "{message}"
            );
        }
    }
    let workspace = Workspace::new();
    let root = workspace.uri("main.dodo");
    let dependency = workspace.file(
        "values.dodo",
        "package values\npub fn answer() -> i32 { return 42 }\n",
    );
    let source = "package app\nimport \"values\"\nfn main() { let answer = values.answer() }\n";
    let mut client = Client::start("lsp");
    let initialized = client.request(
        json!(1),
        "initialize",
        json!({"capabilities":{"workspace":{"inlayHint":{"refreshSupport":true}}}}),
    );
    assert!(initialized.get("error").is_none());
    client.notify("initialized", json!({}));
    client.open(&root, source, 1);
    let id = receive_refresh(&mut client);
    client.send(json!({"jsonrpc":"2.0","id":id,"result":null}));
    let before = range_query(
        &mut client,
        "inlayHint",
        &root,
        full_range(source),
        Value::Null,
    );
    assert_eq!(before["result"][0]["label"], ": i32");
    client.open(
        &dependency,
        "package values\npub fn answer() -> bool { return true }\n",
        1,
    );
    let next = receive_refresh(&mut client);
    assert_ne!(id, next);
    client.send(json!({"jsonrpc":"2.0","id":next,"result":null}));
    let after = range_query(
        &mut client,
        "inlayHint",
        &root,
        full_range(source),
        Value::Null,
    );
    assert_eq!(after["result"][0]["label"], ": bool");
    client.change(
        &dependency,
        "package values\npub fn answer() -> u8 { return 1 }\n",
        2,
    );
    let id = receive_refresh(&mut client);
    client.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"unsupported"}}));
    client.change(
        &dependency,
        "package values\npub fn answer() -> i32 { return 2 }\n",
        3,
    );
    client.diagnostics(); // Rejected refresh requests are not repeatedly sent.
    assert!(client.shutdown().is_empty());
}

fn query(
    client: &mut Client,
    method: &str,
    uri: &str,
    text: &str,
    needle: &str,
    extra: Value,
) -> Value {
    let mut params = json!({"textDocument":{"uri":uri},"position":at(text, needle)});
    if let Some(extra) = extra.as_object() {
        params.as_object_mut().unwrap().extend(extra.clone());
    }
    client.request(json!(42), &format!("textDocument/{method}"), params)
}

#[test]
fn lsp_symbols_respect_shadowing_and_utf16_and_do_not_edit_comments() {
    let source = "package app\r\nfn main() -> i32 {\r\nvalue := 1i32\r\n{\r\nvalue := value + 1\r\n_ = value\r\n}\r\n_ = \"😀 value\"; return value // value\r\n}\r\n";
    let workspace = Workspace::new();
    let uri = workspace.uri("main.dodo");
    let mut client = Client::start("lsp");
    let capabilities = client.initialize("file")["result"]["capabilities"].clone();
    for provider in [
        "definitionProvider",
        "referencesProvider",
        "documentFormattingProvider",
    ] {
        assert_eq!(capabilities[provider], true);
    }
    assert_eq!(capabilities["renameProvider"]["prepareProvider"], true);
    client.open(&uri, source, 1);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    let definition = query(
        &mut client,
        "definition",
        &uri,
        source,
        "value //",
        json!({}),
    );
    assert_eq!(definition["result"]["uri"], uri);
    assert_eq!(
        definition["result"]["range"]["start"],
        at(source, "value := 1")
    );
    let inner = query(
        &mut client,
        "definition",
        &uri,
        source,
        "value\r\n}",
        json!({}),
    );
    assert_eq!(
        inner["result"]["range"]["start"],
        at(source, "value := value")
    );
    let refs = query(
        &mut client,
        "references",
        &uri,
        source,
        "value //",
        json!({"context":{"includeDeclaration":false}}),
    );
    assert_eq!(refs["result"].as_array().unwrap().len(), 2, "{refs}");
    let rename = query(
        &mut client,
        "rename",
        &uri,
        source,
        "value //",
        json!({"newName":"number"}),
    );
    let edits = rename["result"]["changes"][&uri].as_array().unwrap();
    assert_eq!(edits.len(), 3, "{rename}");
    assert!(edits.iter().all(|e| e["newText"] == "number"));
    assert_eq!(edits[2]["range"]["start"], at(source, "value //"));
    for bad in ["fn", "bad-name", "é", "", "_"] {
        assert_eq!(
            query(
                &mut client,
                "rename",
                &uri,
                source,
                "value //",
                json!({"newName":bad})
            )["error"]["code"],
            -32602
        );
    }
    assert!(!workspace.0.join("main.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_cross_file_alias_navigation_references_rename_and_completion() {
    let workspace = Workspace::new();
    let source =
        "package app\nimport \"lib\" as util\nfn main() -> i32 { return util.sum(1, 2) }\n";
    let library =
        "package lib\npub fn sum(a: i32, b: i32) -> i32 { return a + b }\nfn hidden() {}\n";
    let root = workspace.uri("main.dodo");
    let dependency = workspace.file("lib.dodo", library);
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&root, source, 1);
    assert_eq!(client.diagnostics()[&root]["diagnostics"], json!([]));
    let definition = query(&mut client, "definition", &root, source, "sum(1", json!({}));
    assert_eq!(definition["result"]["uri"], dependency, "{definition}");
    assert_eq!(definition["result"]["range"]["start"], at(library, "sum("));
    // A second analysis of the same dependency must not duplicate its declaration.
    client.open(&dependency, library, 7);
    client.diagnostics();
    let rename = query(
        &mut client,
        "rename",
        &dependency,
        library,
        "sum(",
        json!({"newName":"add"}),
    );
    assert_eq!(
        rename["result"]["changes"][&dependency]
            .as_array()
            .unwrap()
            .len(),
        1,
        "{rename}"
    );
    assert_eq!(
        rename["result"]["changes"][&root].as_array().unwrap().len(),
        1,
        "{rename}"
    );
    let completion = query(&mut client, "completion", &root, source, "sum(1", json!({}));
    let items = completion["result"]["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["label"] == "sum"), "{completion}");
    assert!(
        !items.iter().any(|i| i["label"] == "hidden"),
        "{completion}"
    );
    let signature = query(&mut client, "signatureHelp", &root, source, "2)", json!({}));
    assert_eq!(signature["result"]["activeParameter"], 1, "{signature}");
    assert_eq!(
        signature["result"]["signatures"][0]["parameters"][1]["label"],
        "b: i32"
    );
    assert_eq!(
        fs::read_to_string(workspace.0.join("lib.dodo")).unwrap(),
        library
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_bundled_definitions_open_authoritative_read_only_sources() {
    let workspace = Workspace::new();
    let source =
        "package app\nimport \"std/math\" as numbers\nfn main() { _ = numbers.abs(-1.0) }\n";
    let root = workspace.uri("main.dodo");
    let uri = "dodo-stdlib:/std/math.dodo";
    let library = include_str!("../stdlib/std/math.dodo");
    for mode in ["file", "package"] {
        let mut client = Client::start("lsp");
        assert_eq!(
            client.initialize(mode)["result"]["capabilities"]["experimental"]["dodoStdlibSource"],
            true
        );
        client.open(&root, source, 1);
        assert_eq!(client.diagnostics()[&root]["diagnostics"], json!([]));
        let definition = query(&mut client, "definition", &root, source, "abs(-", json!({}));
        assert_eq!(definition["result"]["uri"], uri, "{definition}");
        assert_eq!(
            definition["result"]["range"]["start"],
            at(library, "abs(value:")
        );
        let content = client.request(json!(2), "dodo/stdlibSource", json!({"uri": uri}));
        assert_eq!(content["result"], library);
        let refs = query(
            &mut client,
            "references",
            &root,
            source,
            "abs(-",
            json!({"context":{"includeDeclaration":true}}),
        );
        assert!(
            refs["result"]
                .as_array()
                .unwrap()
                .iter()
                .any(|location| location["uri"] == uri)
        );
        assert_eq!(
            query(
                &mut client,
                "prepareRename",
                &root,
                source,
                "abs(-",
                json!({})
            )["result"],
            Value::Null
        );
        assert_eq!(
            query(
                &mut client,
                "rename",
                &root,
                source,
                "abs(-",
                json!({"newName":"absolute"})
            )["error"]["code"],
            -32803
        );

        // Opening the virtual source cannot turn it into a mutable local overlay.
        client.open(uri, INVALID, 1);
        assert_eq!(client.diagnostics()[uri]["diagnostics"], json!([]));
        client.change(uri, INVALID, 2);
        client.save(uri);
        assert!(client.diagnostics().is_empty());
        client.close(&root);
        client.diagnostics();
        let definition = query(
            &mut client,
            "definition",
            uri,
            library,
            "from_bits(0x7ff",
            json!({}),
        );
        assert_eq!(definition["result"]["uri"], uri);
        assert_eq!(
            definition["result"]["range"]["start"],
            at(library, "from_bits(value:")
        );
        let hover = query(&mut client, "hover", uri, library, "abs(value:", json!({}));
        assert!(hover["result"].to_string().contains("f64"), "{hover}");
        let hints = range_query(
            &mut client,
            "inlayHint",
            uri,
            full_range(library),
            Value::Null,
        );
        assert!(hints["result"].as_array().is_some(), "{hints}");
        let actions = range_query(
            &mut client,
            "codeAction",
            uri,
            full_range(library),
            json!({"diagnostics":[]}),
        );
        assert_eq!(actions["result"], json!([]));
        // Parameters and local bindings inside library sources are read-only too.
        assert_eq!(
            query(
                &mut client,
                "prepareRename",
                uri,
                library,
                "value: f64",
                json!({})
            )["result"],
            Value::Null
        );
        assert_eq!(
            query(
                &mut client,
                "rename",
                uri,
                library,
                "value: f64",
                json!({"newName":"number"})
            )["error"]["code"],
            -32803
        );
        let formatting = client.request(
            json!(3),
            "textDocument/formatting",
            json!({
                "textDocument":{"uri":uri}, "options":{"tabSize":4,"insertSpaces":true}
            }),
        );
        assert_eq!(formatting["error"]["code"], -32803);
        let trig_uri = "dodo-stdlib:/std/math/trig.dodo";
        let trig = include_str!("../stdlib/std/math/trig.dodo");
        client.open(trig_uri, trig, 1);
        client.diagnostics();
        let definition = query(
            &mut client,
            "definition",
            trig_uri,
            trig,
            "abs(value)",
            json!({}),
        );
        assert_eq!(definition["result"]["uri"], uri);
        assert_eq!(
            definition["result"]["range"]["start"],
            at(library, "abs(value:")
        );
        assert!(client.shutdown().is_empty());
    }
}

#[test]
fn lsp_bundled_source_requests_reject_unknown_or_noncanonical_uris() {
    let mut client = Client::start("lsp");
    client.initialize("file");
    for uri in [
        "dodo-stdlib:/std/missing.dodo",
        "dodo-stdlib:/std/../std/math.dodo",
        "dodo-stdlib:/std/./math.dodo",
        "dodo-stdlib:/std//math.dodo",
        "dodo-stdlib:/std/math.dodo?query",
        "dodo-stdlib:/std/math.dodo#fragment",
        "dodo-stdlib://host/std/math.dodo",
        "dodo-stdlib:///std/math.dodo",
        "dodo-stdlib:/std/%6dath.dodo",
        "dodo-stdlib:/std\\math.dodo",
        "file:///std/math.dodo",
        "untitled:stdlib",
    ] {
        let response = client.request(json!(2), "dodo/stdlibSource", json!({"uri":uri}));
        assert_eq!(response["error"]["code"], -32602, "{uri}: {response}");
    }
    for params in [Value::Null, json!({}), json!({"uri":7})] {
        assert_eq!(
            client.request(json!(2), "dodo/stdlibSource", params)["error"]["code"],
            -32602
        );
    }
    // Sources outside the current import graph are still served by the compiler.
    assert_eq!(
        client.request(
            json!(3),
            "dodo/stdlibSource",
            json!({"uri":"dodo-stdlib:/core/option.dodo"})
        )["result"],
        include_str!("../stdlib/core/option.dodo")
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_watched_imports_refresh_diagnostics_navigation_and_preserve_overlays() {
    let workspace = Workspace::new();
    let source = "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n";
    let good = "package lib\npub fn value() -> i32 { return 7 }\n";
    let root = workspace.uri("main.dodo");
    let dependency = workspace.uri("lib/value.dodo");
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&root, source, 9);
    assert!(
        !client.diagnostics()[&root]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Creation repairs a previously unresolved directory import without a save.
    workspace.file("lib/value.dodo", good);
    client.disk_change(&dependency, 1);
    assert_eq!(client.diagnostics()[&root]["diagnostics"], json!([]));
    let moved = format!("// moved 😀\n{good}");
    workspace.file("lib/value.dodo", &moved);
    client.disk_change(&dependency, 2);
    assert_eq!(client.diagnostics()[&root]["version"], 9);
    let definition = query(
        &mut client,
        "definition",
        &root,
        source,
        "value()",
        json!({}),
    );
    assert_eq!(definition["result"]["uri"], dependency);
    assert_eq!(
        definition["result"]["range"]["start"],
        at(&moved, "value()")
    );
    workspace.file(
        "lib/value.dodo",
        "package lib\npub fn value() -> i32 { return missing }\n",
    );
    client.disk_change(&dependency, 2);
    assert!(
        client.diagnostics()[&dependency]["diagnostics"]
            .to_string()
            .contains("missing")
    );

    client.open(&dependency, good, 3);
    assert_eq!(client.diagnostics()[&dependency]["diagnostics"], json!([]));
    workspace.file(
        "lib/value.dodo",
        "package lib\npub fn value() -> bool { return true }\n",
    );
    client.disk_change(&dependency, 2);
    assert!(client.diagnostics().is_empty());
    let hover = query(&mut client, "hover", &root, source, "value()", json!({}));
    assert!(hover["result"].to_string().contains("i32"), "{hover}");
    client.close(&dependency);
    assert!(
        !client.diagnostics()[&root]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    workspace.file("lib/value.dodo", good);
    client.disk_change(&dependency, 2);
    assert_eq!(client.diagnostics()[&root]["diagnostics"], json!([]));
    fs::remove_file(workspace.0.join("lib/value.dodo")).unwrap();
    client.disk_change(&dependency, 3);
    assert!(
        !client.diagnostics()[&root]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        query(
            &mut client,
            "definition",
            &root,
            source,
            "value()",
            json!({})
        )["result"],
        Value::Null
    );
    workspace.file("lib/value.dodo", good);
    client.disk_change(&dependency, 1);
    assert_eq!(client.diagnostics()[&root]["diagnostics"], json!([]));
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_watched_transitive_imports_and_package_siblings_refresh() {
    let workspace = Workspace::new();
    let root = workspace.uri("main.dodo");
    let source = "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n";
    workspace.file(
        "lib/value.dodo",
        "package lib\nimport \"nested\"\npub fn value() -> i32 { return nested.number() }\n",
    );
    let dependency = workspace.file(
        "lib/nested/number.dodo",
        "package nested\npub fn number() -> i32 { return 1 }\n",
    );
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&root, source, 1);
    assert_eq!(client.diagnostics()[&root]["diagnostics"], json!([]));
    workspace.file(
        "lib/nested/number.dodo",
        "package nested\npub fn number() -> i32 { return absent }\n",
    );
    client.disk_change(&dependency, 2);
    assert!(
        client.diagnostics()[&dependency]["diagnostics"]
            .to_string()
            .contains("absent")
    );
    client.change(&root, VALID, 2);
    assert_eq!(client.diagnostics()[&dependency]["diagnostics"], json!([]));
    assert!(client.shutdown().is_empty());

    let mut client = Client::start("lsp");
    client.initialize("package");
    client.open(
        &root,
        "package app\nfn main() -> i32 { return helper() }\n",
        1,
    );
    client.diagnostics();
    let sibling = workspace.file(
        "helper.dodo",
        "package app\nfn helper() -> i32 { return 1 }\n",
    );
    client.disk_change(&sibling, 1);
    assert_eq!(client.diagnostics()[&root]["diagnostics"], json!([]));
    fs::remove_file(workspace.0.join("helper.dodo")).unwrap();
    client.disk_change(&sibling, 3);
    assert!(
        client.diagnostics()[&root]["diagnostics"]
            .to_string()
            .contains("helper")
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_watched_file_batches_validate_atomically_and_ignore_unrelated_events() {
    let workspace = Workspace::new();
    let other = Workspace::new();
    let root = workspace.uri("main.dodo");
    let dependency = workspace.file(
        "lib.dodo",
        "package lib\npub fn value() -> i32 { return 1 }\n",
    );
    let source = "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&root, source, 1);
    client.diagnostics();
    workspace.file(
        "lib.dodo",
        "package lib\npub fn value() -> i32 { return missing }\n",
    );
    for params in [
        Value::Null,
        json!({}),
        json!({"changes":{}}),
        json!({"changes":[null]}),
    ] {
        client.notify("workspace/didChangeWatchedFiles", params);
    }
    for event in [
        json!({}),
        json!({"uri":dependency,"type":0}),
        json!({"uri":dependency,"type":4}),
        json!({"uri":dependency,"type":2.0}),
        json!({"uri":dependency,"type":"2"}),
        json!({"uri":"https://example.com/lib.dodo","type":2}),
        json!({"uri":"file:///invalid%xx.dodo","type":2}),
        json!({"uri":"dodo-stdlib:/std/math.dodo","type":2}),
    ] {
        client.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes":[{"uri":dependency,"type":2},event]}),
        );
    }
    client.notify("workspace/didChangeWatchedFiles", json!({"changes":[]}));
    client.disk_change(&other.uri("unrelated.dodo"), 2);
    client.disk_change(&workspace.uri("notes.txt"), 2);
    assert!(client.diagnostics().is_empty());
    client.disk_change(&dependency, 2);
    assert!(
        client.diagnostics()[&dependency]["diagnostics"]
            .to_string()
            .contains("missing")
    );
    let stderr = client.shutdown();
    assert!(stderr.contains("LSP:"));
    assert!(!stderr.contains("panicked"));
}

#[cfg(unix)]
#[test]
fn lsp_watches_external_symlink_dependencies_through_load_failures() {
    let workspace = Workspace::new();
    let external = Workspace::new();
    std::os::unix::fs::symlink(&external.0, workspace.0.join("lib")).unwrap();
    let root = workspace.uri("main.dodo");
    let good = "package lib\npub fn value() -> i32 { return 1 }\n";
    let dependency = external.file("value.dodo", good);
    let source = "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&root, source, 1);
    assert_eq!(client.diagnostics()[&root]["diagnostics"], json!([]));
    external.file(
        "value.dodo",
        "package lib\npub fn value() -> i32 { return missing }\n",
    );
    client.disk_change(&dependency, 2);
    assert!(
        client.diagnostics()[&dependency]["diagnostics"]
            .to_string()
            .contains("missing")
    );
    fs::remove_file(external.0.join("value.dodo")).unwrap();
    client.disk_change(&dependency, 3);
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&dependency]["diagnostics"], json!([]));
    assert!(
        !diagnostics[&root]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    external.file("value.dodo", good);
    client.disk_change(&dependency, 1);
    assert_eq!(client.diagnostics()[&root]["diagnostics"], json!([]));
    client.change(&root, VALID, 2);
    client.diagnostics();
    client.disk_change(&dependency, 2);
    assert!(client.diagnostics().is_empty());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_registers_watches_after_initialized_and_releases_them_after_close() {
    for relative in [false, true] {
        let workspace = Workspace::new();
        let root = workspace.uri("main.dodo");
        let sibling = workspace.uri("extra.dodo");
        let mut client = Client::start("lsp");
        let initialized = client.request(
            json!(1),
            "initialize",
            json!({
                "capabilities":{"workspace":{"didChangeWatchedFiles":{
                    "dynamicRegistration":true,"relativePatternSupport":relative
                }}}
            }),
        );
        assert!(initialized.get("error").is_none());
        client.open(&root, VALID, 1);
        client.diagnostics(); // No server requests until the initialized notification.
        client.notify("initialized", json!({}));
        let request = client.receive();
        assert_eq!(request["method"], "client/registerCapability");
        let registration = &request["params"]["registrations"][0];
        assert_eq!(registration["method"], "workspace/didChangeWatchedFiles");
        let watchers = registration["registerOptions"]["watchers"]
            .as_array()
            .unwrap();
        assert_eq!(watchers.len(), 1);
        assert_eq!(watchers[0]["kind"], 7);
        if relative {
            assert_eq!(
                watchers[0]["globPattern"]["baseUri"],
                workspace.uri("").trim_end_matches('/')
            );
            assert_eq!(watchers[0]["globPattern"]["pattern"], "**/*.dodo");
        } else {
            assert!(
                watchers[0]["globPattern"]
                    .as_str()
                    .unwrap()
                    .ends_with("/**/*.dodo")
            );
        }
        client.send(json!({"jsonrpc":"2.0","id":request["id"],"result":null}));
        client.open(&sibling, VALID, 1);
        client.diagnostics(); // Siblings reuse the same directory watch.
        client.close(&root);
        client.diagnostics();
        client.close(&sibling);
        assert_eq!(
            client.receive()["method"],
            "textDocument/publishDiagnostics"
        );
        let unregister = client.receive();
        assert_eq!(unregister["method"], "client/unregisterCapability");
        assert_eq!(
            unregister["params"]["unregisterations"][0]["id"],
            registration["id"]
        );
        client.send(json!({"jsonrpc":"2.0","id":unregister["id"],"result":null}));
        assert!(client.diagnostics().is_empty());
        assert!(client.shutdown().is_empty());
    }
}

#[test]
fn lsp_rejected_watch_registration_keeps_save_refresh_available() {
    let workspace = Workspace::new();
    let root = workspace.uri("main.dodo");
    let dependency = workspace.file(
        "lib.dodo",
        "package lib\npub fn value() -> i32 { return 1 }\n",
    );
    let source = "package app\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n";
    let mut client = Client::start("lsp");
    for capability in [
        json!(false),
        json!({"dynamicRegistration": "yes"}),
        json!({"relativePatternSupport": 1}),
    ] {
        assert_eq!(
            client.request(
                json!(1),
                "initialize",
                json!({
                    "capabilities":{"workspace":{"didChangeWatchedFiles":capability}}
                })
            )["error"]["code"],
            -32602
        );
    }
    assert!(client.request(json!(1), "initialize", json!({
        "capabilities":{"workspace":{"didChangeWatchedFiles":{"dynamicRegistration":true}}}
    })).get("error").is_none());
    client.notify("initialized", json!({}));
    client.open(&root, source, 1);
    assert_eq!(
        client.receive()["method"],
        "textDocument/publishDiagnostics"
    );
    let registration = client.receive();
    assert_eq!(registration["method"], "client/registerCapability");
    client.send(json!({"jsonrpc":"2.0", "id":registration["id"], "error":{"code":-32603,"message":"watching unavailable"}}));
    assert!(client.diagnostics().is_empty());
    workspace.file(
        "lib.dodo",
        "package lib\npub fn value() -> i32 { return missing }\n",
    );
    client.save(&root);
    assert!(
        client.diagnostics()[&dependency]["diagnostics"]
            .to_string()
            .contains("missing")
    );
    client.close(&root);
    client.diagnostics(); // Failed registrations are not retried on every update.
    assert!(client.shutdown().contains("client rejected file watches"));
}

#[test]
fn lsp_completion_and_signatures_work_during_incomplete_calls() {
    let source = "package app\nfn sum(a: i32, b: i32) -> i32 { return a + b }\nfn main() -> i32 {\nnumber := 1i32\nreturn sum(number, \n}\n";
    let uri = "untitled:editing";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(uri, source, 1);
    client.diagnostics();
    let response = query(
        &mut client,
        "signatureHelp",
        uri,
        source,
        "\n}\n",
        json!({}),
    );
    assert_eq!(response["result"]["activeParameter"], 1, "{response}");
    let response = query(&mut client, "completion", uri, source, "\n}\n", json!({}));
    assert!(
        response["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["label"] == "number"),
        "{response}"
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_recovers_multiple_syntax_and_semantic_errors_and_clears_them() {
    let source = "package app\nfn syntax() {\na := ;\nb := ;\n}\nfn first() -> i32 { return missing }\nfn second() -> i32 { return absent }\nfn good() -> i32 { return 42 }\n";
    let uri = "untitled:errors";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(uri, source, 1);
    let diagnostics = client.diagnostics();
    let errors = diagnostics[uri]["diagnostics"].as_array().unwrap();
    assert!(errors.len() >= 4, "{diagnostics:?}");
    let hover = query(&mut client, "hover", uri, source, "42", json!({}));
    assert_eq!(hover["result"]["contents"]["value"], "```dodo\ni32\n```");
    let two = "package app\nfn main() {\n_ = missing\n_ = absent\n}\n";
    client.change(uri, two, 2);
    let diagnostics = client.diagnostics();
    assert_eq!(
        diagnostics[uri]["diagnostics"].as_array().unwrap().len(),
        2,
        "{diagnostics:?}"
    );
    client.change(uri, VALID, 3);
    assert_eq!(client.diagnostics()[uri]["diagnostics"], json!([]));
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_recovers_test_attribute_and_assertion_errors_and_preserves_navigation() {
    let source = "package app\n@test struct Invalid {}\nfn sum(a: i32, b: i32) -> i32 { a + b }\n@test fn checks() {\nassert(1)\nassert_eq(1, true)\n}\n@test @ignore(\"later\") fn valid() { assert_eq(sum(1, 2), 3) }\n";
    let workspace = Workspace::new();
    let uri = workspace.uri("checks.dodo");
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&uri, source, 1);
    let diagnostics = client.diagnostics();
    let errors = diagnostics[&uri]["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 3, "{diagnostics:?}");
    assert!(
        errors.iter().any(|error| error["message"]
            .as_str()
            .unwrap()
            .contains("@test and @ignore apply only to functions")),
        "{diagnostics:?}"
    );
    let definition = query(
        &mut client,
        "definition",
        &uri,
        source,
        "sum(1, 2)",
        json!({}),
    );
    assert_eq!(definition["result"]["uri"], uri, "{definition}");
    assert_eq!(definition["result"]["range"]["start"], at(source, "sum(a:"));
    let fixed = source
        .replace("@test struct Invalid {}\n", "")
        .replace("assert(1)", "assert(true)")
        .replace("assert_eq(1, true)", "assert_eq(1, 1)");
    client.change(&uri, &fixed, 2);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_formatting_uses_the_canonical_formatter_and_unsaved_text() {
    let workspace = Workspace::new();
    let uri = workspace.file("main.dodo", VALID);
    let text = "package app\n// 😀 kept\nfn main()->i32{return 1}\n";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&uri, text, 1);
    client.diagnostics();
    let params = json!({"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":true}});
    let response = client.request(json!(10), "textDocument/formatting", params.clone());
    let expected = dodoc::format::format_source(text).unwrap();
    assert_eq!(response["result"][0]["newText"], expected);
    assert_eq!(
        response["result"][0]["range"]["end"],
        json!({"line":3,"character":0})
    );
    assert_eq!(
        fs::read_to_string(workspace.0.join("main.dodo")).unwrap(),
        VALID
    );
    client.change(&uri, &expected, 2);
    client.diagnostics();
    assert_eq!(
        client.request(json!(11), "textDocument/formatting", params.clone())["result"],
        json!([])
    );
    client.change(&uri, "package app\nfn main( {", 3);
    client.diagnostics();
    assert_eq!(
        client.request(json!(12), "textDocument/formatting", params)["error"]["code"],
        -32803
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_target_controls_pointer_width_and_hosted_imports() {
    let workspace = Workspace::new();
    let uri = workspace.uri("main.dodo");
    let mut client = Client::start("lsp");
    let invalid = client.request(
        json!(0),
        "initialize",
        json!({"capabilities":{},"initializationOptions":{"target":"wasm32-unknown-unknown\0"}}),
    );
    assert_eq!(invalid["error"]["code"], -32602);
    let invalid = client.request(
        json!(1),
        "initialize",
        json!({"capabilities":{},"initializationOptions":{"target":"invalid-dodo-target"}}),
    );
    assert_eq!(invalid["error"]["code"], -32602);
    let response = client.request(
        json!(2),
        "initialize",
        json!({"capabilities":{},"initializationOptions":{"target":"wasm32-unknown-unknown"}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    let text = "package app\nfn big() -> usize { return 4294967296usize }\n";
    client.open(&uri, text, 1);
    assert!(
        !client.diagnostics()[&uri]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    client.open("untitled:target", text, 1);
    assert!(
        !client.diagnostics()["untitled:target"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    client.change(&uri, "package app\nimport \"std/fs/native\"\n", 2);
    let diagnostics = client.diagnostics();
    assert!(
        diagnostics[&uri]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("wasm32-unknown-unknown"),
        "{diagnostics:?}"
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_members_and_field_labels_have_distinct_symbol_identities() {
    let source = "package app\nstruct Point {\n x: i32\n fn plus(self: &Self, amount: i32) -> i32 { return self.x + amount }\n}\nfn main() -> i32 {\nx := 1i32\np := Point { x: x }\nreturn p.plus(2) + p.x\n}\n";
    let workspace = Workspace::new();
    let uri = workspace.uri("main.dodo");
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&uri, source, 1);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    let response = query(
        &mut client,
        "definition",
        &uri,
        source,
        "plus(2)",
        json!({}),
    );
    assert_eq!(
        response["result"]["range"]["start"],
        at(source, "plus(self"),
        "{response}"
    );
    let response = query(
        &mut client,
        "completion",
        &uri,
        source,
        "plus(2)",
        json!({}),
    );
    let items = response["result"]["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["label"] == "x"));
    assert!(items.iter().any(|i| i["label"] == "plus"));
    let response = query(
        &mut client,
        "signatureHelp",
        &uri,
        source,
        "2) +",
        json!({}),
    );
    let parameters = response["result"]["signatures"][0]["parameters"]
        .as_array()
        .unwrap();
    assert_eq!(parameters, &vec![json!({"label":"amount: i32"})]);
    let response = query(
        &mut client,
        "rename",
        &uri,
        source,
        "x :=",
        json!({"newName":"value"}),
    );
    let edits = response["result"]["changes"][&uri].as_array().unwrap();
    assert_eq!(edits.len(), 2, "{response}");
    assert_eq!(edits[1]["range"]["start"], at(source, "x }"));
    let response = query(
        &mut client,
        "rename",
        &uri,
        source,
        "x: i32",
        json!({"newName":"coordinate"}),
    );
    assert_eq!(
        response["result"]["changes"][&uri]
            .as_array()
            .unwrap()
            .len(),
        4,
        "{response}"
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_versioned_rename_and_collision_rejection() {
    let source = "package app\nfn add(a: i32, b: i32) -> i32 { return a + b }\nfn main() -> i32 { return add(1, 2) }\n";
    let workspace = Workspace::new();
    let uri = workspace.uri("main.dodo");
    let mut client = Client::start("lsp");
    client.request(
        json!(1),
        "initialize",
        json!({"capabilities":{"workspace":{"workspaceEdit":{"documentChanges":true}}}}),
    );
    client.open(&uri, source, 9);
    client.diagnostics();
    let response = query(
        &mut client,
        "prepareRename",
        &uri,
        source,
        "a + b",
        json!({}),
    );
    assert_eq!(response["result"]["placeholder"], "a");
    let response = query(
        &mut client,
        "rename",
        &uri,
        source,
        "a + b",
        json!({"newName":"b"}),
    );
    assert_eq!(response["error"]["code"], -32803, "{response}");
    let response = query(
        &mut client,
        "rename",
        &uri,
        source,
        "a + b",
        json!({"newName":"left"}),
    );
    assert_eq!(
        response["result"]["documentChanges"][0]["textDocument"],
        json!({"uri":uri,"version":9})
    );
    assert_eq!(
        response["result"]["documentChanges"][0]["edits"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_recovery_preserves_import_locations_and_does_not_weaken_cli_checks() {
    let workspace = Workspace::new();
    let source = "package app\nimport \"lib\"\nfn main() -> i32 { return lib.good() }\n";
    let library =
        "package lib\npub fn bad() {\na := ;\nb := ;\n}\npub fn good() -> i32 { return 7 }\n";
    let root = workspace.uri("main.dodo");
    let dependency = workspace.file("lib.dodo", library);
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&root, source, 1);
    let diagnostics = client.diagnostics();
    assert_eq!(
        diagnostics[&dependency]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "{diagnostics:?}"
    );
    let response = query(
        &mut client,
        "definition",
        &root,
        source,
        "good()",
        json!({}),
    );
    assert_eq!(response["result"]["uri"], dependency);
    assert_eq!(response["result"]["range"]["start"], at(library, "good()"));
    assert!(dodoc::parser::parse(library).is_err());
    assert!(dodoc::package::load(&workspace.0.join("lib.dodo")).is_err());
    assert!(dodoc::format::format_source(library).is_err());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_completion_replaces_whole_identifier_and_excludes_closed_loop_scopes() {
    let source = "package app\nfn main() -> i32 {\nnumber := 42i32\nfor index := 0; index < 2; index += 1 {}\nreturn number\n}\n";
    let uri = "untitled:completion";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(uri, source, 1);
    assert_eq!(client.diagnostics()[uri]["diagnostics"], json!([]));
    let response = query(&mut client, "completion", uri, source, "mber\n", json!({}));
    let item = response["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == "number")
        .unwrap();
    assert_eq!(item["textEdit"]["range"]["start"], at(source, "number\n"));
    assert_eq!(
        item["textEdit"]["range"]["end"],
        json!({"line":4,"character":13})
    );
    let response = query(
        &mut client,
        "completion",
        uri,
        source,
        "number\n",
        json!({}),
    );
    assert!(
        !response["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["label"] == "index"),
        "{response}"
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_nested_generic_signature_help_and_malformed_requests() {
    let source = "package app\nfn id<A, B>(value: A, ignored: B) -> A { return value }\nfn sum(a: i32, b: i32) -> i32 { return a + b }\nfn main() -> i32 { return sum(id::<i32, u8>(1, 2), 3) }\n";
    let uri = "untitled:signatures";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(uri, source, 1);
    assert_eq!(client.diagnostics()[uri]["diagnostics"], json!([]));
    for needle in ["2),", "3) }"] {
        let response = query(&mut client, "signatureHelp", uri, source, needle, json!({}));
        assert_eq!(response["result"]["activeParameter"], 1, "{response}");
    }
    for method in [
        "completion",
        "definition",
        "references",
        "prepareRename",
        "rename",
        "signatureHelp",
        "formatting",
    ] {
        let response = client.request(json!(80), &format!("textDocument/{method}"), json!({}));
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
    assert!(client.shutdown().is_empty());
}

// Apply edits as an editor would, independently converting UTF-16 positions.
fn apply_text_edits(source: &str, edits: &Value) -> String {
    let byte = |position: &Value| {
        let line = position["line"].as_u64().unwrap() as usize;
        let column = position["character"].as_u64().unwrap() as usize;
        let start: usize = source.split_inclusive('\n').take(line).map(str::len).sum();
        let mut units = 0;
        for (offset, ch) in source[start..].char_indices() {
            if units == column {
                return start + offset;
            }
            assert_ne!(ch, '\n', "edit column exceeds line length");
            units += ch.len_utf16();
            assert!(units <= column, "edit splits a UTF-16 surrogate pair");
        }
        assert_eq!(units, column);
        source.len()
    };
    let mut edits: Vec<_> = edits
        .as_array()
        .unwrap()
        .iter()
        .map(|edit| {
            let range = &edit["range"];
            (
                byte(&range["start"]),
                byte(&range["end"]),
                edit["newText"].as_str().unwrap(),
            )
        })
        .collect();
    edits.sort_by_key(|(start, end, _)| (*start, *end));
    assert!(
        edits.windows(2).all(|pair| pair[0].1 <= pair[1].0),
        "overlapping edits"
    );
    let mut result = source.to_owned();
    for (start, end, text) in edits.into_iter().rev() {
        result.replace_range(start..end, text);
    }
    result
}

#[test]
fn lsp_rename_edits_round_trip_through_package_checking_and_navigation() {
    let workspace = Workspace::new();
    let source = "package app\nimport \"lib\" as util\nfn main() -> i32 {\n_ = \"😀 sum\"; return util.sum(1, 2) // sum stays in this comment\n}\n";
    let library = "package lib\npub fn sum(a: i32, b: i32) -> i32 { return a + b }\n";
    let root = workspace.uri("main.dodo");
    let dependency = workspace.file("lib.dodo", library);
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&root, source, 1);
    client.open(&dependency, library, 1);
    client.diagnostics();
    let response = query(
        &mut client,
        "rename",
        &root,
        source,
        "sum(1",
        json!({"newName":"add"}),
    );
    let changes = &response["result"]["changes"];
    assert_eq!(changes.as_object().unwrap().len(), 2, "{response}");
    let renamed_source = apply_text_edits(source, &changes[&root]);
    let renamed_library = apply_text_edits(library, &changes[&dependency]);
    assert_eq!(renamed_source, source.replace("util.sum(", "util.add("));
    assert_eq!(renamed_library, library.replace("fn sum(", "fn add("));

    let overlays = BTreeMap::from([
        (workspace.0.join("main.dodo"), renamed_source.clone()),
        (workspace.0.join("lib.dodo"), renamed_library.clone()),
    ]);
    let mut loaded =
        dodoc::package::load_with_overlays(&workspace.0.join("main.dodo"), &overlays).unwrap();
    dodoc::sema::check_for_target(&mut loaded.program, usize::BITS).unwrap();
    client.change(&root, &renamed_source, 2);
    client.change(&dependency, &renamed_library, 2);
    let diagnostics = client.diagnostics();
    assert!(
        diagnostics.values().all(|p| p["diagnostics"] == json!([])),
        "{diagnostics:?}"
    );
    let definition = query(
        &mut client,
        "definition",
        &root,
        &renamed_source,
        "add(1",
        json!({}),
    );
    assert_eq!(definition["result"]["uri"], dependency);
    assert_eq!(
        definition["result"]["range"]["start"],
        at(&renamed_library, "add(")
    );
    let references = query(
        &mut client,
        "references",
        &dependency,
        &renamed_library,
        "add(",
        json!({"context":{"includeDeclaration":true}}),
    );
    assert_eq!(
        references["result"].as_array().unwrap().len(),
        2,
        "{references}"
    );
    assert_eq!(
        fs::read_to_string(workspace.0.join("lib.dodo")).unwrap(),
        library
    );
    assert!(!workspace.0.join("main.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_shorthand_rename_restrictions_follow_symbols_across_analysis_roots() {
    let workspace = Workspace::new();
    let source = "package app\nimport \"shapes\"\nfn main() -> i32 {\nx := 1i32\np := shapes.Point { x }\nreturn p.x\n}\n";
    let library = "package shapes\npub struct Point { pub x: i32 }\n";
    let root = workspace.uri("main.dodo");
    let dependency = workspace.file("shapes.dodo", library);
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&root, source, 1);
    client.open(&dependency, library, 1);
    let diagnostics = client.diagnostics();
    assert!(
        diagnostics.values().all(|p| p["diagnostics"] == json!([])),
        "{diagnostics:?}"
    );
    // Query the dependency's own index: the restricting shorthand lives only
    // in the caller's analysis, so checking the current index alone is unsafe.
    let response = query(
        &mut client,
        "rename",
        &dependency,
        library,
        "x: i32",
        json!({"newName":"coordinate"}),
    );
    assert_eq!(response["error"]["code"], -32803, "{response}");
    assert!(response.get("result").is_none());
    let response = query(
        &mut client,
        "prepareRename",
        &root,
        source,
        "x :=",
        json!({}),
    );
    assert_eq!(response.get("result"), Some(&Value::Null));
    let response = query(
        &mut client,
        "rename",
        &root,
        source,
        "x :=",
        json!({"newName":"value"}),
    );
    assert_eq!(response["error"]["code"], -32803, "{response}");
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_queries_ignore_strings_comments_and_invalid_utf16_positions() {
    let source = "package app\nfn echo(text: &str, count: i32) -> i32 { return count }\nfn main() -> i32 { return echo(\"😀 marker\", 1) } // trailing comment\n";
    let uri = "untitled:positions";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(uri, source, 1);
    assert_eq!(client.diagnostics()[uri]["diagnostics"], json!([]));
    for method in ["completion", "signatureHelp", "definition", "prepareRename"] {
        for needle in ["marker", "trailing comment"] {
            let response = query(&mut client, method, uri, source, needle, json!({}));
            assert_eq!(response.get("result"), Some(&Value::Null), "{response}");
        }
        let mut surrogate = at(source, "😀");
        surrogate["character"] = json!(surrogate["character"].as_u64().unwrap() + 1);
        for position in [
            surrogate,
            json!({"line":999,"character":0}),
            json!({"line":0,"character":999}),
        ] {
            let response = client.request(
                json!(50),
                &format!("textDocument/{method}"),
                json!({"textDocument":{"uri":uri},"position":position}),
            );
            assert_eq!(response.get("result"), Some(&Value::Null), "{response}");
        }
    }
    let response = query(&mut client, "signatureHelp", uri, source, "1) }", json!({}));
    assert_eq!(response["result"]["activeParameter"], 1, "{response}");
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_package_recovery_combines_lexical_and_target_errors_in_unsaved_siblings() {
    let workspace = Workspace::new();
    let root = workspace.uri("main.dodo");
    let sibling = workspace.uri("extra.dodo");
    let broken = "package app\nfn bad() {\n😀\n§\n}\nfn big() -> usize { return 4294967296usize }\nfn good() -> i32 { return 7 }\n";
    let fixed = "package app\nfn good() -> i32 { return 7 }\n";
    let mut client = Client::start("lsp");
    let response = client.request(json!(1), "initialize", json!({"capabilities":{},"initializationOptions":{"checkMode":"package","target":"wasm32-unknown-unknown"}}));
    assert!(response.get("error").is_none(), "{response}");
    client.open(&root, VALID, 1);
    client.open(&sibling, broken, 4);
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics[&root]["diagnostics"], json!([]));
    let errors = diagnostics[&sibling]["diagnostics"].as_array().unwrap();
    assert_eq!(errors.len(), 3, "{diagnostics:?}");
    assert_eq!(diagnostics[&sibling]["version"], 4);
    for line in [2, 3, 5] {
        assert!(
            errors.iter().any(|e| e["range"]["start"]["line"] == line),
            "{errors:?}"
        );
    }
    let response = query(&mut client, "hover", &sibling, broken, "7 }", json!({}));
    assert_eq!(response["result"]["contents"]["value"], "```dodo\ni32\n```");
    client.change(&sibling, fixed, 5);
    let diagnostics = client.diagnostics();
    assert!(
        diagnostics.values().all(|p| p["diagnostics"] == json!([])),
        "{diagnostics:?}"
    );
    assert_eq!(diagnostics[&sibling]["version"], 5);
    assert!(!workspace.0.join("extra.dodo").exists());
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_internal_json_rpc_validates_envelopes_and_preserves_request_ids() {
    let mut client = Client::start("lsp");
    for id in [
        Value::Null,
        json!(true),
        json!([]),
        json!({}),
        json!(1.5),
        json!(1.0),
        json!(2147483648_i64),
        json!(-2147483649_i64),
        json!(u64::MAX),
    ] {
        client.send(json!({"jsonrpc":"2.0", "id":id, "method":"initialize", "params":{}}));
        assert_eq!(
            client.receive(),
            json!({
                "jsonrpc":"2.0", "id":null, "error":{"code":-32600,"message":"Invalid Request"}
            }),
            "invalid ID {id}"
        );
    }
    for (message, expected_id) in [
        (json!(null), Value::Null),
        (json!([]), Value::Null),
        (
            json!([{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}]),
            Value::Null,
        ),
        (
            json!({"id":1,"method":"initialize","params":{}}),
            Value::Null,
        ),
        (
            json!({"jsonrpc":"1.0","id":1,"method":"initialize","params":{}}),
            Value::Null,
        ),
        (
            json!({"jsonrpc":"2.0","id":"bad","method":false}),
            json!("bad"),
        ),
        (
            json!({"jsonrpc":"2.0","id":"bad","method":"initialize","params":{},"result":null}),
            json!("bad"),
        ),
        (
            json!({"jsonrpc":"2.0","id":"bad","method":"initialize","params":{},"error":{}}),
            json!("bad"),
        ),
        (json!({"jsonrpc":"2.0","id":"bad"}), json!("bad")),
        (
            json!({"jsonrpc":"2.0","id":"bad","result":null,"error":{"code":1,"message":"bad"}}),
            json!("bad"),
        ),
        (
            json!({"jsonrpc":"2.0","id":"bad","error":{"code":"1","message":"bad"}}),
            json!("bad"),
        ),
    ] {
        client.send(message.clone());
        let response = client.receive();
        assert_eq!(response["id"], expected_id, "{message}");
        assert_eq!(response["error"]["code"], -32600, "{message}");
        assert!(response.get("result").is_none(), "{response}");
    }
    // None of the invalid initialize envelopes may initialize the server.
    client.initialize("file");
    for id in [
        json!(i32::MIN),
        json!(-1),
        json!(0),
        json!(i32::MAX),
        json!(""),
        json!("1"),
        json!("😀\"\\\n"),
    ] {
        let response = client.request(id, "unknown/method", Value::Null);
        assert_eq!(response["error"]["code"], -32601);
        assert!(response.get("result").is_none());
    }
    for response in [
        json!({"jsonrpc":"2.0","id":1,"result":null}),
        json!({"jsonrpc":"2.0","id":"peer","result":{"extension":true}}),
        json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"peer parse error"}}),
        json!({"jsonrpc":"2.0","id":"peer","error":{"code":-32603,"message":"peer failure","data":[]}}),
    ] {
        client.send(response);
    }
    client.notify("unknown/extension", json!({"extension":true}));
    assert!(client.diagnostics().is_empty()); // Responses never elicit responses.
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_internal_json_parser_recovers_on_the_next_frame() {
    let mut client = Client::start("--lsp");
    for body in [
        &b"{"[..],
        b"",
        b"\xff",
        b"{}{}",
        b"[1,",
        b"{\"x\":\"\xff\"}",
    ] {
        client.send_body(body);
        assert_eq!(
            client.receive(),
            json!({
                "jsonrpc":"2.0", "id":null, "error":{"code":-32700,"message":"Parse error"}
            })
        );
    }
    client.initialize("file");
    client.open(
        "untitled:unicode",
        "package app\nfn main() { _ = \"😀é\" }\n",
        1,
    );
    assert_eq!(
        client.diagnostics()["untitled:unicode"]["diagnostics"],
        json!([])
    );
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_internal_parameter_validation_rejects_invalid_queries() {
    let uri = "untitled:validation";
    let mut client = Client::start("lsp");
    for capabilities in [
        Value::Null,
        json!([]),
        json!(false),
        json!({"workspace":false}),
        json!({"workspace":{"workspaceEdit":[]}}),
        json!({"workspace":{"workspaceEdit":{"documentChanges":"true"}}}),
    ] {
        let response = client.request(json!(1), "initialize", json!({"capabilities":capabilities}));
        assert_eq!(response["error"]["code"], -32602, "{capabilities}");
    }
    let response = client.request(json!(1), "initialize", json!({
        "capabilities":{"workspace":{"workspaceEdit":{"documentChanges":true,"futureOption":42}},"experimental":{"anything":[]}},
        "futureParameter":true
    }));
    assert!(response.get("error").is_none(), "{response}");
    client.open(uri, VALID, 1);
    client.diagnostics();
    let base = json!({"textDocument":{"uri":uri},"position":{"line":0,"character":0}});
    for method in [
        "hover",
        "completion",
        "definition",
        "references",
        "prepareRename",
        "rename",
        "signatureHelp",
    ] {
        for position in [
            Value::Null,
            json!([]),
            json!(0),
            json!({}),
            json!({"line":0}),
            json!({"character":0}),
            json!({"line":-1,"character":0}),
            json!({"line":0,"character":1.5}),
            json!({"line":0,"character":"0"}),
            json!({"line":2147483648_i64,"character":0}),
            json!({"line":0,"character":u64::MAX}),
        ] {
            let mut params = base.clone();
            params["position"] = position;
            params["context"] = json!({"includeDeclaration":true});
            params["newName"] = json!("renamed");
            let response =
                client.request(json!(2), &format!("textDocument/{method}"), params.clone());
            assert_eq!(
                response["error"]["code"], -32602,
                "{method}: {params}: {response}"
            );
        }
    }
    for (method, field, value) in [
        ("hover", "textDocument", json!("document")),
        ("hover", "textDocument", json!({"uri":1})),
        ("hover", "textDocument", json!({"uri":"untitled:bad%xy"})),
        ("references", "context", Value::Null),
        ("references", "context", json!({"includeDeclaration":1})),
        ("rename", "newName", Value::Null),
        ("rename", "newName", json!(true)),
        ("completion", "context", json!({})),
        ("completion", "context", json!({"triggerKind":"1"})),
        (
            "completion",
            "context",
            json!({"triggerKind":2,"triggerCharacter":42}),
        ),
        ("signatureHelp", "context", json!({"triggerKind":1})),
        (
            "signatureHelp",
            "context",
            json!({"triggerKind":1,"isRetrigger":"false"}),
        ),
        ("formatting", "options", Value::Null),
        ("formatting", "options", json!({"tabSize":4})),
        (
            "formatting",
            "options",
            json!({"tabSize":-1,"insertSpaces":true}),
        ),
        (
            "formatting",
            "options",
            json!({"tabSize":4,"insertSpaces":"true"}),
        ),
        (
            "formatting",
            "options",
            json!({"tabSize":4,"insertSpaces":true,"insertFinalNewline":1}),
        ),
    ] {
        let mut params = base.clone();
        params[field] = value;
        let response = client.request(json!(3), &format!("textDocument/{method}"), params.clone());
        assert_eq!(
            response["error"]["code"], -32602,
            "{method}: {params}: {response}"
        );
    }
    // Unknown fields and optional nulls do not break supported requests.
    let response = client.request(json!(4), "textDocument/completion", json!({
        "textDocument":{"uri":uri,"future":[]}, "position":{"line":1,"character":3,"future":true},
        "context":null, "workDoneToken":"progress", "future":{}
    }));
    assert!(response.get("result").is_some(), "{response}");
    assert!(client.shutdown().is_empty());
}

#[test]
fn lsp_internal_notification_validation_is_atomic_and_response_free() {
    let uri = "untitled:atomic";
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(uri, INVALID, -2);
    let original = client.diagnostics()[uri].clone();
    let open = json!({"textDocument":{"uri":uri,"languageId":"dodo","version":1,"text":VALID}});
    for (field, value) in [
        ("uri", json!(1)),
        ("languageId", Value::Null),
        ("text", Value::Null),
        ("text", json!(42)),
        ("version", Value::Null),
        ("version", json!(1.5)),
        ("version", json!(2147483648_i64)),
        ("version", json!(-2147483649_i64)),
    ] {
        let mut params = open.clone();
        params["textDocument"][field] = value;
        client.notify("textDocument/didOpen", params);
    }
    for changes in [
        Value::Null,
        json!({"text":VALID}),
        json!([null]),
        json!([{}]),
        json!([{"text":true}]),
        json!([{"text":VALID},{}]),
        json!([{}, {"text":VALID}]),
        json!([{"text":VALID,"rangeLength":"1"}]),
        json!([{"text":VALID,"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}}]),
        json!([{"text":VALID,"range":false}]),
    ] {
        client.notify(
            "textDocument/didChange",
            json!({
                "textDocument":{"uri":uri,"version":1}, "contentChanges":changes
            }),
        );
    }
    for version in [Value::Null, json!(false), json!(1.0), json!(2147483648_i64)] {
        client.notify(
            "textDocument/didChange",
            json!({
                "textDocument":{"uri":uri,"version":version}, "contentChanges":[{"text":VALID}]
            }),
        );
    }
    client.notify(
        "textDocument/didSave",
        json!({"textDocument":{"uri":uri},"text":false}),
    );
    client.notify(
        "textDocument/didClose",
        json!({"textDocument":{"uri":null}}),
    );
    assert!(client.diagnostics().is_empty());
    client.save(uri);
    assert_eq!(client.diagnostics()[uri], original);
    // Invalid updates must not consume the version or apply any partial text.
    client.notify("textDocument/didChange", json!({
        "textDocument":{"uri":uri,"version":1},
        "contentChanges":[{"text":INVALID},{"text":VALID,"range":null,"rangeLength":null,"future":true}]
    }));
    let updated = client.diagnostics();
    assert_eq!(updated[uri]["version"], 1);
    assert_eq!(updated[uri]["diagnostics"], json!([]));
    client.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[]}),
    );
    client.change(uri, INVALID, 0);
    assert!(client.diagnostics().is_empty());
    client.change(uri, INVALID, 2);
    assert_eq!(client.diagnostics()[uri]["version"], 2);
    client.close(uri);
    assert_eq!(
        client.diagnostics()[uri],
        json!({"uri":uri,"diagnostics":[]})
    );
    let stderr = client.shutdown();
    assert!(stderr.contains("LSP:"));
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn lsp_printing_signatures_and_hovers_survive_lowering() {
    let workspace = Workspace::new();
    let source = "package app\nimport \"std/console\" as terminal\nfn main() {\nterminal.printf(\"Answer: {}, enabled: {}\\n\", 42, true)!\noutput := terminal.stderr()\noutput.printf(\"{} {}\", 7, false)!\n}\n";
    let uri = workspace.uri("main.dodo");
    let mut client = Client::start("lsp");
    client.initialize("file");
    client.open(&uri, source, 1);
    assert_eq!(client.diagnostics()[&uri]["diagnostics"], json!([]));
    for needle in ["true)!", "false)!"] {
        let response = query(
            &mut client,
            "signatureHelp",
            &uri,
            source,
            needle,
            json!({}),
        );
        assert_eq!(response["result"]["activeParameter"], 1, "{response}");
        let parameters = &response["result"]["signatures"][0]["parameters"];
        assert_eq!(parameters[0]["label"], "format: &str", "{response}");
        assert_eq!(parameters[1]["label"], "args...", "{response}");
    }
    let hover = query(
        &mut client,
        "hover",
        &uri,
        source,
        "printf(\"Answer",
        json!({}),
    );
    let text = hover["result"]["contents"]["value"].as_str().unwrap();
    assert!(text.contains("printf(format: &str, args...)"), "{hover}");
    assert!(
        !text.contains("$print") && !text.contains("arg0"),
        "{hover}"
    );
    assert!(client.shutdown().is_empty());
}
