//! Exercise the real compiler process with independently framed JSON-RPC.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use url::Url;

static NEXT: AtomicU64 = AtomicU64::new(0);
const VALID: &str = "package app\nfn main() -> i32 { return 0 }\n";
const INVALID: &str = "package app\nfn main() -> i32 { return missing }\n";

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        loop {
            let path = std::env::temp_dir().join(format!(
                "dodo-lsp-{}-{} space # é 😀",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => panic!("create workspace: {error}"),
            }
        }
    }

    fn uri(&self, name: &str) -> String {
        Url::from_file_path(self.0.join(name)).unwrap().into()
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
                let message: Value = serde_json::from_slice(&body).unwrap();
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
        let body = serde_json::to_vec(&message).unwrap();
        let input = self.input.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.write_all(&body).unwrap();
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
        let response = client.request(json!("hover"), "textDocument/hover", json!({}));
        assert_eq!(response["error"]["code"], -32601);
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
    client.open("untitled:Untitled-1", VALID, 1);
    assert!(client.diagnostics().is_empty());
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
