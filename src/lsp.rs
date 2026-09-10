//! Synchronous stdio LSP server using the compiler's parser and semantic checker.
//!
//! Open documents are authoritative in-memory overlays, including when imported
//! by another document. Each check stops at the compiler's first error.
use crate::diagnostic::{Diagnostic, Severity};
use crate::{package, sema};
use lsp_server::{ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializeParams, Position,
    PublishDiagnosticsParams, Range, Uri,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Default, PartialEq, Eq)]
enum State {
    #[default]
    Uninitialized,
    Running,
    Shutdown,
}

struct Document {
    uri: Uri,
    path: PathBuf,
    text: String,
    version: i32,
}

#[derive(Default)]
struct Server {
    state: State,
    documents: BTreeMap<String, Document>,
    published: BTreeMap<String, Uri>,
    check_packages: bool,
}

/// Serve LSP messages until `exit` or EOF. Stdout contains only framed JSON-RPC.
/// The returned process status follows LSP: success requires a prior shutdown.
pub fn run(input: &mut impl BufRead, output: &mut impl Write) -> io::Result<i32> {
    let mut server = Server::default();
    while let Some(message) = Message::read(input)? {
        match message {
            Message::Request(request) => {
                Message::Response(server.request(request)).write(output)?;
            }
            Message::Notification(notification) if notification.method == "exit" => break,
            Message::Notification(notification) if server.state == State::Running => {
                match server.notification(notification) {
                    Ok(true) => server.publish(output)?,
                    Ok(false) => (),
                    Err(error) => eprintln!("LSP: {error}"),
                }
            }
            // Unknown notifications, responses, and notifications outside the
            // initialized lifetime never receive a JSON-RPC response.
            _ => (),
        }
    }
    Ok(i32::from(server.state != State::Shutdown))
}

impl Server {
    fn request(&mut self, request: Request) -> Response {
        let error = |code: ErrorCode, message: &str| {
            Response::new_err(request.id.clone(), code as i32, message.to_owned())
        };
        if self.state == State::Shutdown {
            return error(ErrorCode::InvalidRequest, "server has shut down");
        }
        if request.method == "initialize" {
            if self.state != State::Uninitialized {
                return error(ErrorCode::InvalidRequest, "server is already initialized");
            }
            let params = match serde_json::from_value::<InitializeParams>(request.params.clone()) {
                Ok(params) => params,
                Err(_) => return error(ErrorCode::InvalidParams, "invalid initialize parameters"),
            };
            match params
                .initialization_options
                .as_ref()
                .and_then(|options| options.get("checkMode"))
            {
                None | Some(Value::Null) => (),
                Some(Value::String(mode)) if mode == "file" => (),
                Some(Value::String(mode)) if mode == "package" => self.check_packages = true,
                Some(_) => {
                    return error(
                        ErrorCode::InvalidParams,
                        "checkMode must be 'file' or 'package'",
                    );
                }
            }
            self.state = State::Running;
            return Response::new_ok(
                request.id,
                json!({
                    "capabilities": {
                        "positionEncoding": "utf-16",
                        "textDocumentSync": {
                            "openClose": true,
                            "change": 1,
                            "save": { "includeText": false }
                        }
                    },
                    "serverInfo": { "name": "dodo", "version": env!("CARGO_PKG_VERSION") }
                }),
            );
        }
        if self.state == State::Uninitialized {
            return error(ErrorCode::ServerNotInitialized, "server is not initialized");
        }
        if request.method == "shutdown" {
            if !request.params.is_null() {
                return error(ErrorCode::InvalidParams, "shutdown takes no parameters");
            }
            self.state = State::Shutdown;
            return Response::new_ok(request.id, Value::Null);
        }
        error(ErrorCode::MethodNotFound, "method is not supported")
    }

    fn notification(&mut self, notification: Notification) -> Result<bool, String> {
        match notification.method.as_str() {
            "textDocument/didOpen" => {
                let params = notification
                    .extract::<DidOpenTextDocumentParams>("textDocument/didOpen")
                    .map_err(|error| error.to_string())?;
                let document = params.text_document;
                let path = uri_path(&document.uri)?;
                self.documents.insert(
                    document.uri.as_str().to_owned(),
                    Document {
                        uri: document.uri,
                        path,
                        text: document.text,
                        version: document.version,
                    },
                );
            }
            "textDocument/didChange" => {
                let params = notification
                    .extract::<DidChangeTextDocumentParams>("textDocument/didChange")
                    .map_err(|error| error.to_string())?;
                let Some(document) = self.documents.get_mut(params.text_document.uri.as_str())
                else {
                    return Ok(false);
                };
                if params.text_document.version <= document.version
                    || params.content_changes.is_empty()
                {
                    return Ok(false);
                }
                // Full synchronization is advertised. Reject the whole malformed
                // update, so a ranged edit cannot silently replace the buffer.
                if params
                    .content_changes
                    .iter()
                    .any(|change| change.range.is_some())
                {
                    return Err("expected a full document change".into());
                }
                if let Some(change) = params.content_changes.into_iter().last() {
                    document.text = change.text;
                    document.version = params.text_document.version;
                }
            }
            "textDocument/didSave" => {
                let params = notification
                    .extract::<DidSaveTextDocumentParams>("textDocument/didSave")
                    .map_err(|error| error.to_string())?;
                if !self
                    .documents
                    .contains_key(params.text_document.uri.as_str())
                {
                    return Ok(false);
                }
                // A save refreshes on-disk dependencies. Open buffers remain
                // authoritative until didChange/didClose, per LSP synchronization.
            }
            "textDocument/didClose" => {
                let params = notification
                    .extract::<DidCloseTextDocumentParams>("textDocument/didClose")
                    .map_err(|error| error.to_string())?;
                if self
                    .documents
                    .remove(params.text_document.uri.as_str())
                    .is_none()
                {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn publish(&mut self, output: &mut impl Write) -> io::Result<()> {
        let overlays = self
            .documents
            .values()
            .map(|document| (document.path.clone(), document.text.clone()))
            .collect();
        let mut diagnostics: BTreeMap<String, PublishDiagnosticsParams> = self
            .documents
            .iter()
            .map(|(key, document)| {
                (
                    key.clone(),
                    PublishDiagnosticsParams {
                        uri: document.uri.clone(),
                        diagnostics: vec![],
                        version: Some(document.version),
                    },
                )
            })
            .collect();
        let mut checked = BTreeSet::new();
        for document in self.documents.values() {
            let root = if self.check_packages {
                document.path.parent().unwrap_or(&document.path)
            } else {
                &document.path
            };
            if !checked.insert(root) {
                continue;
            }
            match package::load_with_overlays(root, &overlays) {
                Ok(mut loaded) => {
                    if let Err(diagnostic) =
                        sema::check_for_target(&mut loaded.program, usize::BITS)
                    {
                        let source = loaded.source(diagnostic.span);
                        self.add_diagnostic(&mut diagnostics, &document.uri, source, &diagnostic);
                    }
                }
                Err(error) => self.add_diagnostic(
                    &mut diagnostics,
                    &document.uri,
                    error.source.as_deref(),
                    &error.diagnostic,
                ),
            }
        }
        // Replacing the complete set also clears errors in fixed dependencies,
        // closed documents, and files that are no longer imported.
        let current: BTreeMap<_, _> = diagnostics
            .iter()
            .map(|(key, params)| (key.clone(), params.uri.clone()))
            .collect();
        for (key, uri) in &self.published {
            if !current.contains_key(key) {
                diagnostics.insert(
                    key.clone(),
                    PublishDiagnosticsParams {
                        uri: uri.clone(),
                        diagnostics: vec![],
                        version: None,
                    },
                );
            }
        }
        for params in diagnostics.into_values() {
            Message::Notification(Notification::new(
                "textDocument/publishDiagnostics".into(),
                params,
            ))
            .write(output)?;
        }
        self.published = current;
        Ok(())
    }

    fn add_diagnostic(
        &self,
        diagnostics: &mut BTreeMap<String, PublishDiagnosticsParams>,
        fallback: &Uri,
        source: Option<&package::Source>,
        diagnostic: &Diagnostic,
    ) {
        let uri = source
            .and_then(|source| self.source_uri(&source.path))
            .unwrap_or_else(|| fallback.clone());
        let (text, offset) = source
            .map(|source| (source.text.as_str(), source.start))
            .unwrap_or_else(|| (self.documents[fallback.as_str()].text.as_str(), 0));
        let diagnostic = to_diagnostic(diagnostic, text, offset);
        let version = self
            .documents
            .get(uri.as_str())
            .map(|document| document.version);
        let entries = &mut diagnostics
            .entry(uri.as_str().to_owned())
            .or_insert_with(|| PublishDiagnosticsParams {
                uri,
                diagnostics: vec![],
                version,
            })
            .diagnostics;
        if !entries.contains(&diagnostic) {
            entries.push(diagnostic);
        }
    }

    fn source_uri(&self, path: &Path) -> Option<Uri> {
        self.documents
            .values()
            .find(|document| document.path == path)
            .map(|document| document.uri.clone())
            .or_else(|| Url::from_file_path(path).ok()?.as_str().parse().ok())
    }
}

fn uri_path(uri: &Uri) -> Result<PathBuf, String> {
    let url = Url::parse(uri.as_str()).map_err(|error| error.to_string())?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err("source URI must not have a query or fragment".into());
    }
    let path = url
        .to_file_path()
        .map_err(|()| "only local file URIs are supported")?;
    Ok(package::source_path(&path))
}

fn to_diagnostic(diagnostic: &Diagnostic, text: &str, offset: usize) -> lsp_types::Diagnostic {
    let start = diagnostic.span.start.saturating_sub(offset);
    let end = diagnostic.span.end.saturating_sub(offset).max(start);
    let mut message = diagnostic.message.to_string();
    for note in &diagnostic.notes {
        message.push_str("\nnote: ");
        message.push_str(note);
    }
    lsp_types::Diagnostic {
        range: Range::new(position(text, start), position(text, end)),
        severity: Some(match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        source: Some("dodo".into()),
        message,
        ..Default::default()
    }
}

/// Compiler spans are UTF-8 byte offsets; LSP columns default to UTF-16 units.
fn position(text: &str, byte: usize) -> Position {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    let prefix = &text[..byte];
    let line = prefix.bytes().filter(|&byte| byte == b'\n').count();
    let start = prefix.rfind('\n').map_or(0, |index| index + 1);
    // A position inside CRLF denotes the end of the preceding line.
    let column = prefix[start..]
        .trim_end_matches('\r')
        .encode_utf16()
        .count();
    Position::new(line as u32, column as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Span;

    #[test]
    fn positions_handle_unicode_crlf_eof_and_invalid_spans() {
        let text = "a😀é\r\nnext\n";
        assert_eq!(position(text, 5), Position::new(0, 3));
        assert_eq!(position(text, 6), Position::new(0, 3));
        assert_eq!(position(text, 7), Position::new(0, 4));
        assert_eq!(position(text, 8), Position::new(0, 4));
        assert_eq!(position(text, 9), Position::new(1, 0));
        assert_eq!(position(text, usize::MAX), Position::new(2, 0));
        assert_eq!(position("", 100), Position::default());
    }

    #[test]
    fn diagnostics_preserve_warning_severity_notes_and_source_offsets() {
        let warning = Diagnostic::warning(
            Span {
                start: 104,
                end: 106,
            },
            "example warning",
        )
        .note("use this information");
        let result = to_diagnostic(&warning, "😀é", 100);
        assert_eq!(result.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            result.range,
            Range::new(Position::new(0, 2), Position::new(0, 3))
        );
        assert_eq!(
            result.message,
            "example warning\nnote: use this information"
        );
        assert!(warning.render("test.dodo", "😀é").starts_with("warning: "));
    }
}
