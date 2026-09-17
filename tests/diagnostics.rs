//! Source-level ownership diagnostics and the editor's public stdio interface.
use dodoc::diagnostic::DiagnosticKind;
use dodoc::{parser, sema};
#[cfg(feature = "llvm")]
use std::fs;
#[cfg(feature = "llvm")]
use std::io::Write;
#[cfg(feature = "llvm")]
use std::path::PathBuf;
#[cfg(feature = "llvm")]
use std::process::{Command, Stdio};
#[cfg(feature = "llvm")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "llvm")]
static NEXT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "llvm")]
struct Workspace(PathBuf);
#[cfg(feature = "llvm")]
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dodo-diagnostics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn check(&self, name: &str, source: &str) -> std::process::Output {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        Command::new(env!("CARGO_BIN_EXE_dodo"))
            .arg("check")
            .arg(path)
            .output()
            .unwrap()
    }
}
#[cfg(feature = "llvm")]
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(feature = "llvm")]
#[test]
fn borrow_conflict_cli_shows_origin_access_and_live_use() {
    let workspace = Workspace::new();
    let output = workspace.check(
        "borrow_conflict.dodo",
        include_str!("../examples/diagnostics/borrow_conflict.dodo"),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    for text in [
        "view := &value",
        "shared borrow begins here",
        "value = 2",
        "return consume(view)",
        "borrow is used here",
    ] {
        assert!(stderr.contains(text), "missing {text:?}:\n{stderr}");
    }
    assert!(!stderr.contains("source byte"), "{stderr}");
    assert!(stderr.contains("^^^^^"), "{stderr}");
    assert!(stderr.contains("----"), "{stderr}");
}

#[cfg(feature = "llvm")]
#[test]
fn move_cli_shows_move_and_rejected_use() {
    let workspace = Workspace::new();
    let output = workspace.check(
        "moved_value.dodo",
        include_str!("../examples/diagnostics/moved_value.dodo"),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("moved here"), "{stderr}");
    assert_eq!(stderr.matches("consume(value)").count(), 2, "{stderr}");
    assert!(!stderr.contains("source byte"), "{stderr}");
}

#[cfg(feature = "llvm")]
#[test]
fn return_contract_cli_shows_contract_source_and_return() {
    let workspace = Workspace::new();
    let output = workspace.check(
        "return_source.dodo",
        include_str!("../examples/diagnostics/return_source.dodo"),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    for text in ["from(a)", "b: &i32", "return b"] {
        assert!(stderr.contains(text), "missing {text:?}:\n{stderr}");
    }
    assert!(!stderr.contains("source byte"), "{stderr}");
}

#[cfg(feature = "llvm")]
#[test]
fn borrowing_example_checks_and_runs() {
    let workspace = Workspace::new();
    let source = include_str!("../examples/borrowing.dodo");
    let output = workspace.check("borrowing.dodo", source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("run")
        .arg(workspace.0.join("borrowing.dodo"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unresolved_diagnostics_expose_kinds_codes_and_symbol_names() {
    for (source, expected, code) in [
        (
            "package app\nfn main() { missing() }\n",
            DiagnosticKind::UnknownFunction("missing".into()),
            "unknown-function",
        ),
        (
            "package app\nfn main() { _ = missing }\n",
            DiagnosticKind::UnknownBinding("missing".into()),
            "unknown-binding",
        ),
        (
            "package app\nfn main(value: missing.Type) {}\n",
            DiagnosticKind::UnknownType("missing.Type".into()),
            "unknown-type",
        ),
        (
            "package app\nfn main() { _ = missing.Record { value: 1 } }\n",
            DiagnosticKind::UnknownStruct("missing.Record".into()),
            "unknown-struct",
        ),
        (
            "package app\nenum Choice { Yes }\nfn main() { _ = Choice.No() }\n",
            DiagnosticKind::UnknownVariant("Choice.No".into()),
            "unknown-variant",
        ),
        (
            "package app\nfn main() { missing.answer() }\n",
            DiagnosticKind::UnresolvedReceiver(Some("missing".into())),
            "unresolved-receiver",
        ),
    ] {
        let mut program = parser::parse(source).unwrap();
        let diagnostic = sema::check(&mut program).unwrap_err();
        assert_eq!(diagnostic.kind, expected, "{diagnostic:?}");
        assert_eq!(diagnostic.kind.code(), Some(code));
    }
}

#[test]
fn immutable_diagnostics_identify_the_affected_binding_and_usage() {
    for (operation, code) in [
        ("count = 3", "immutable-assignment"),
        ("_ = &mut count", "immutable-borrow"),
    ] {
        let source = format!(
            "package app\nfn main() {{ let count = 1; {{ let count = 2; {operation} }}; _ = count }}\n"
        );
        let mut program = parser::parse(&source).unwrap();
        let diagnostic = sema::check(&mut program).unwrap_err();
        assert_eq!(diagnostic.kind.code(), Some(code));
        let (DiagnosticKind::ImmutableAssignment(Some(binding))
        | DiagnosticKind::ImmutableBorrow(Some(binding))) = diagnostic.kind
        else {
            panic!("missing binding metadata: {diagnostic:?}");
        };
        assert_eq!(binding.name, "count");
        assert_eq!(
            binding.declaration.start,
            source.find("let count = 2").unwrap()
        );
        assert_eq!(&source[binding.usage.start..binding.usage.end], "count");
        assert!(binding.usage.start >= source.find(operation).unwrap());
    }
}

#[test]
fn immutable_indirect_storage_does_not_identify_a_direct_binding() {
    for source in [
        "package app\nfn main() { let value = 1; let view = &value; *view = 2 }\n",
        "package app\nfn main() { let value = 1; let view = &value; _ = &mut *view }\n",
        "package app\nstruct S { n: i32 }\nfn main() { let value = S { n: 1 }; value.n = 2 }\n",
    ] {
        let mut program = parser::parse(source).unwrap();
        let diagnostic = sema::check(&mut program).unwrap_err();
        assert!(
            matches!(
                diagnostic.kind,
                DiagnosticKind::ImmutableAssignment(None) | DiagnosticKind::ImmutableBorrow(None)
            ),
            "{diagnostic:?}"
        );
    }
}

#[test]
fn borrow_labels_retain_exact_expression_ranges() {
    let source = "package example\nfn consume(view: &i32) {}\nfn main() {\nvalue := 1i32\nview := &value\nvalue = 2\nconsume(view)\n}\n";
    let mut program = parser::parse(source).unwrap();
    let diagnostic = sema::check(&mut program).unwrap_err();
    assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], "value");
    assert!(
        diagnostic.labels.iter().any(|label| {
            &source[label.span.start..label.span.end] == "&value"
                && label.message.contains("borrow begins here")
        }),
        "{diagnostic:?}"
    );
    assert!(
        diagnostic.labels.iter().any(|label| {
            &source[label.span.start..label.span.end] == "view"
                && label.message.contains("borrow is used here")
        }),
        "{diagnostic:?}"
    );
}

#[cfg(feature = "llvm")]
fn lsp(messages: &[dodoc::json::Value]) -> Vec<dodoc::json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dodo"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in messages {
        let body = dodoc::json::to_vec(message);
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.write_all(&body).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut bytes = output.stdout.as_slice();
    let mut replies = vec![];
    while !bytes.is_empty() {
        let header_end = bytes
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap();
        let header = std::str::from_utf8(&bytes[..header_end]).unwrap();
        let length: usize = header
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length:")
                    .map(|value| value.trim().parse().unwrap())
            })
            .unwrap();
        bytes = &bytes[header_end + 4..];
        replies.push(dodoc::json::from_slice(&bytes[..length]).unwrap());
        bytes = &bytes[length..];
    }
    replies
}

#[cfg(feature = "llvm")]
#[test]
fn editor_stdio_reports_inferred_types_and_clears_changed_diagnostics() {
    use dodoc::json::json;
    let source =
        "package hover\nfn main() {\nvalue := 1i32\nview := &value\nvalue = 2\ncopy := *view\n}\n";
    let fixed = source.replace("value = 2\ncopy := *view", "copy := *view\nvalue = 2");
    let uri = "untitled:ownership.dodo";
    let replies = lsp(&[
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"dodo","version":1,"text":source}}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":fixed}]}}),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":uri},"position":{"line":3,"character":1}}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":uri}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}),
        json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    let initialized = replies.iter().find(|reply| reply["id"] == 1).unwrap();
    assert_eq!(initialized["result"]["capabilities"]["hoverProvider"], true);
    let publications: Vec<_> = replies
        .iter()
        .filter(|reply| reply["method"] == "textDocument/publishDiagnostics")
        .collect();
    assert_eq!(publications.len(), 3, "{replies:?}");
    let error = &publications[0]["params"]["diagnostics"][0];
    assert_eq!(error["range"]["start"]["line"], 4);
    let related = error["relatedInformation"].as_array().unwrap();
    for line in [3, 5] {
        assert!(
            related
                .iter()
                .any(|label| label["location"]["range"]["start"]["line"] == line),
            "{error}"
        );
    }
    assert_eq!(publications[1]["params"]["diagnostics"], json!([]));
    assert_eq!(publications[2]["params"]["diagnostics"], json!([]));
    let hover = replies.iter().find(|reply| reply["id"] == 2).unwrap();
    assert_eq!(hover["result"]["contents"]["kind"], "markdown");
    assert!(
        hover["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("view: &i32"),
        "{hover}"
    );
}

#[cfg(feature = "llvm")]
#[test]
fn editor_stdio_exposes_receiver_ownership_and_borrowed_return_sources() {
    use dodoc::json::json;
    let source = include_str!("../examples/borrowing.dodo");
    let uri = "untitled:borrowing.dodo";
    let mut messages = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"dodo","version":1,"text":source}}}),
    ];
    for (id, needle, offset) in [
        (10, "value.view()", "value.".len()),
        (11, "value.replace(20)", "value.".len()),
        (12, "value.finish()", "value.".len()),
        (13, "choose(&previous", 0),
    ] {
        let (line, text) = source
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(needle))
            .unwrap();
        let character = text.find(needle).unwrap() + offset;
        messages.push(json!({"jsonrpc":"2.0","id":id,"method":"textDocument/hover","params":{"textDocument":{"uri":uri},"position":{"line":line,"character":character}}}));
    }
    messages.extend([
        json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
        json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    let replies = lsp(&messages);
    for (id, expected) in [
        (10, vec!["shared borrow", "from(self)", "inferred"]),
        (11, vec!["mutable borrow"]),
        (12, vec!["consumes"]),
        (13, vec!["from(a, b)", "explicit"]),
    ] {
        let reply = replies.iter().find(|reply| reply["id"] == id).unwrap();
        let hover = reply["result"]["contents"]["value"].as_str().unwrap_or("");
        for expected in expected {
            assert!(hover.contains(expected), "expected {expected:?}: {reply}");
        }
    }
}
