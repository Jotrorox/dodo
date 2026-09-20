//! Synchronous stdio LSP server using the compiler's parser and semantic checker.
//!
//! Open documents are authoritative in-memory overlays, including when imported
//! by another document. Editor recovery retains independent valid syntax/bodies.
use crate::diagnostic::{Diagnostic, Severity};
use crate::{editor, file_uri, format, package, sema};
mod bundled;
mod project_config;
mod protocol;
mod watch;
use crate::json::{Value, json};
use protocol::{
    DocumentChange, ErrorCode, InitializeParams, Location, Message, Notification, OpenDocument,
    Position, PublishDiagnosticsParams, QueryParams, Range, RangeParams, Request, Response,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Default, PartialEq, Eq)]
enum State {
    #[default]
    Uninitialized,
    Running,
    Shutdown,
}

struct Document {
    uri: String,
    path: Option<PathBuf>,
    text: String,
    version: i32,
    analysis: Option<editor::Document>,
    watch_roots: BTreeSet<PathBuf>,
}

#[derive(Default)]
struct Server {
    state: State,
    documents: BTreeMap<String, Document>,
    published: BTreeMap<String, String>,
    check_packages: bool,
    target: String,
    project_config: project_config::ProjectConfig,
    project_error: Option<String>,
    pointer_bits: u32,
    document_changes: bool,
    inlay_refresh: bool,
    next_inlay_refresh: u64,
    watches: watch::Watches,
}

/// Serve LSP messages until `exit` or EOF. Stdout contains only framed JSON-RPC.
/// The returned process status follows LSP: success requires a prior shutdown.
pub fn run(input: &mut impl BufRead, output: &mut impl Write) -> io::Result<i32> {
    let mut server = Server::default();
    while let Some(body) = editor::read_message(input)? {
        let value: Value = match crate::json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                write_error(output, Value::Null, -32700, "Parse error")?;
                continue;
            }
        };
        let message = match Message::parse(value) {
            Ok(message) => message,
            Err(id) => {
                write_error(output, id, -32600, "Invalid Request")?;
                continue;
            }
        };
        match message {
            Message::Request(request) => {
                server.request(request).write(output)?;
            }
            Message::Notification(notification) if notification.method == "exit" => break,
            Message::Notification(notification) if server.state == State::Running => {
                match server.notification(notification) {
                    Ok(true) => server.publish(output)?,
                    Ok(false) => (),
                    Err(error) => eprintln!("LSP: {error}"),
                }
                server.sync_watches(output)?;
            }
            Message::Response { id, error } if server.state == State::Running => {
                server.watches.response(&id, error.as_deref());
                if error.is_some()
                    && id
                        .as_str()
                        .is_some_and(|id| id.starts_with("dodo/inlayHint/"))
                {
                    server.inlay_refresh = false;
                }
            }
            // Unknown notifications, responses, and notifications outside the
            // initialized lifetime never receive a JSON-RPC response.
            _ => (),
        }
    }
    Ok(i32::from(server.state != State::Shutdown))
}

fn write_error(output: &mut impl Write, id: Value, code: i32, message: &str) -> io::Result<()> {
    editor::write_message(
        output,
        &json!({"jsonrpc":"2.0", "id":id, "error":{"code":code, "message":message}}),
    )
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
            let params = match InitializeParams::parse(&request.params) {
                Ok(params) => params,
                Err(_) => return error(ErrorCode::InvalidParams, "invalid initialize parameters"),
            };
            let mut check_packages = false;
            match params.initialization_options.get("checkMode") {
                None | Some(Value::Null) => (),
                Some(Value::String(mode)) if mode == "file" => (),
                Some(Value::String(mode)) if mode == "package" => check_packages = true,
                Some(_) => {
                    return error(
                        ErrorCode::InvalidParams,
                        "checkMode must be 'file' or 'package'",
                    );
                }
            }
            let project_config =
                match project_config::ProjectConfig::from_initialize(&request.params) {
                    Ok(config) => config,
                    Err(message) => return error(ErrorCode::InvalidParams, &message),
                };
            let (target, bits) = match project_config.settings() {
                Ok(settings) => settings,
                Err(message) => return error(ErrorCode::InvalidParams, &message),
            };
            self.watches.manifest = project_config.path.clone();
            self.project_config = project_config;
            self.check_packages = check_packages;
            self.target = target;
            self.pointer_bits = bits;
            self.document_changes = params.document_changes;
            self.inlay_refresh = params.inlay_refresh;
            self.watches.supported = params.dynamic_watches;
            self.watches.relative = params.relative_watches;
            self.state = State::Running;
            return Response::new_ok(
                request.id,
                json!({
                    "capabilities": {
                        "positionEncoding": "utf-16",
                        "hoverProvider": true,
                        "completionProvider": {"triggerCharacters": ["."]},
                        "definitionProvider": true,
                        "referencesProvider": true,
                        "renameProvider": {"prepareProvider": true},
                        "signatureHelpProvider": {"triggerCharacters": ["(", ","], "retriggerCharacters": [","]},
                        "documentFormattingProvider": true,
                        "inlayHintProvider": true,
                        "codeActionProvider": {"codeActionKinds": ["quickfix"]},
                        "experimental": {"dodoStdlibSource": true},
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
        if request.method == "dodo/stdlibSource" {
            let Ok(uri) = protocol::bundled_source_uri(&request.params) else {
                return error(ErrorCode::InvalidParams, "invalid bundled source URI");
            };
            let path = bundled::path(&uri).unwrap();
            return Response::new_ok(request.id, json!(package::bundled_source(&path).unwrap()));
        }
        if request.method == "textDocument/formatting" {
            let Ok(uri) = protocol::formatting_uri(&request.params) else {
                return error(ErrorCode::InvalidParams, "invalid formatting parameters");
            };
            if bundled::path(&uri).is_some() {
                return error(ErrorCode::RequestFailed, "bundled sources are read-only");
            }
            let Some(document) = self.documents.get(uri.as_str()) else {
                return Response::new_ok(request.id, Value::Null);
            };
            return match format::format_source(&document.text) {
                Ok(formatted) => Response::new_ok(
                    request.id,
                    if formatted == document.text {
                        json!([])
                    } else {
                        json!([{"range": Range::new(Position::new(0, 0), position(&document.text, document.text.len())).to_json(), "newText": formatted}])
                    },
                ),
                Err(diagnostic) => error(ErrorCode::RequestFailed, &diagnostic.message),
            };
        }
        if matches!(
            request.method.as_str(),
            "textDocument/inlayHint" | "textDocument/codeAction"
        ) {
            let Ok(params) = RangeParams::parse(&request.params, &request.method) else {
                return error(
                    ErrorCode::InvalidParams,
                    "invalid document range parameters",
                );
            };
            let Some(document) = self.documents.get(&params.uri) else {
                return Response::new_ok(request.id, json!([]));
            };
            let Some(analysis) = &document.analysis else {
                return Response::new_ok(request.id, json!([]));
            };
            let Range { start, end } = params.range;
            if request.method == "textDocument/inlayHint" {
                return Response::new_ok(
                    request.id,
                    json!(analysis.inlay_hints(
                        start.line,
                        start.character,
                        end.line,
                        end.character,
                    )),
                );
            }
            if !params.quick_fixes || bundled::path(&document.uri).is_some() {
                return Response::new_ok(request.id, json!([]));
            }
            let (Some(start), Some(end)) = (
                editor::byte_offset(&document.text, start.line, start.character),
                editor::byte_offset(&document.text, end.line, end.character),
            ) else {
                return Response::new_ok(request.id, json!([]));
            };
            let path = document
                .path
                .clone()
                .unwrap_or_else(|| PathBuf::from(&document.uri));
            let Some(source) = analysis
                .index
                .sources
                .iter()
                .find(|source| source.path == path)
            else {
                return Response::new_ok(request.id, json!([]));
            };
            let overlays = self
                .documents
                .values()
                .filter_map(|document| {
                    let path = document.path.as_ref()?;
                    path.is_absolute()
                        .then(|| (path.clone(), document.text.clone()))
                })
                .collect();
            let fixes = editor::actions::quick_fixes(
                &document.text,
                source.start,
                &analysis.index,
                &analysis.diagnostics,
                crate::ast::Span { start, end },
                &self.target,
                &overlays,
            );
            let actions: Vec<_> = fixes.into_iter().filter_map(|fix| {
                let mut changes: BTreeMap<String, Vec<Value>> = BTreeMap::new();
                for edit in fix.edits {
                    let source = analysis.index.sources.iter().find(|source| source.path == edit.path)?;
                    let uri = self.source_uri(&edit.path)?;
                    if bundled::path(&uri).is_some() {
                        return None;
                    }
                    changes.entry(uri).or_default().push(json!({
                        "range": Range::new(position(&source.text, edit.span.start), position(&source.text, edit.span.end)).to_json(),
                        "newText": edit.new_text,
                    }));
                }
                let edit = if self.document_changes {
                    json!({"documentChanges": changes.into_iter().map(|(uri, edits)| {
                        let version = self.documents.get(&uri).map(|document| document.version);
                        json!({"textDocument":{"uri":uri,"version":version},"edits":edits})
                    }).collect::<Vec<_>>()})
                } else {
                    json!({"changes": changes})
                };
                Some(json!({
                    "title": fix.title,
                    "kind": "quickfix",
                    "diagnostics": [to_diagnostic(&fix.diagnostic, &document.text, source.start)],
                    "isPreferred": fix.preferred,
                    "edit": edit,
                }))
            }).collect();
            return Response::new_ok(request.id, json!(actions));
        }
        if !matches!(
            request.method.as_str(),
            "textDocument/hover"
                | "textDocument/definition"
                | "textDocument/prepareRename"
                | "textDocument/completion"
                | "textDocument/signatureHelp"
                | "textDocument/references"
                | "textDocument/rename"
        ) {
            return error(ErrorCode::MethodNotFound, "method is not supported");
        }
        let Ok(params) = QueryParams::parse(&request.params, &request.method) else {
            return error(
                ErrorCode::InvalidParams,
                "invalid document position parameters",
            );
        };
        let new_name = params.new_name;
        if let Some(name) = &new_name
            && !valid_name(name)
        {
            return error(
                ErrorCode::InvalidParams,
                "newName must be a non-reserved Dodo identifier",
            );
        }
        let Some(document) = self.documents.get(params.uri.as_str()) else {
            return Response::new_ok(request.id, Value::Null);
        };
        let Some(analysis) = &document.analysis else {
            return Response::new_ok(request.id, Value::Null);
        };
        let path = document
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from(document.uri.as_str()));
        let (line, character) = (params.position.line, params.position.character);
        let result = match request.method.as_str() {
            "textDocument/hover" => analysis.hover(line, character),
            "textDocument/completion" => analysis.index.completion(&path, line, character),
            "textDocument/signatureHelp" => analysis.index.signature_help(&path, line, character),
            _ => {
                let Some((symbol, span)) = analysis.index.symbol_at(&path, line, character) else {
                    return Response::new_ok(
                        request.id,
                        if request.method == "textDocument/references" {
                            json!([])
                        } else {
                            Value::Null
                        },
                    );
                };
                match request.method.as_str() {
                    "textDocument/definition" => self
                        .location(&analysis.index, symbol.span)
                        .map(|location| location.to_json()),
                    "textDocument/prepareRename" => analysis.index.prepare_rename(symbol, span),
                    "textDocument/references" => Some(json!(
                        self.references(&symbol.key, params.include_declaration)
                            .iter()
                            .map(Location::to_json)
                            .collect::<Vec<_>>()
                    )),
                    "textDocument/rename" => {
                        let name = new_name.as_ref().unwrap();
                        if !self
                            .documents
                            .values()
                            .filter_map(|d| d.analysis.as_ref())
                            .all(|a| a.index.can_rename(symbol, name))
                        {
                            return error(
                                ErrorCode::RequestFailed,
                                "rename is unavailable or could capture another binding",
                            );
                        }
                        let locations = self.references(&symbol.key, true);
                        let mut changes: BTreeMap<String, Vec<Value>> = BTreeMap::new();
                        for location in locations {
                            changes
                                .entry(location.uri.as_str().into())
                                .or_default()
                                .push(json!({"range":location.range.to_json(),"newText":name}));
                        }
                        if self.document_changes {
                            Some(
                                json!({"documentChanges": changes.into_iter().map(|(uri, edits)| {
                                let version = self.documents.get(&uri).map(|d| d.version);
                                json!({"textDocument":{"uri":uri,"version":version},"edits":edits})
                            }).collect::<Vec<_>>()}),
                            )
                        } else {
                            Some(json!({"changes":changes}))
                        }
                    }
                    _ => unreachable!(),
                }
            }
        };
        Response::new_ok(request.id, result.unwrap_or(Value::Null))
    }

    fn location(&self, index: &editor::symbols::Index, span: crate::ast::Span) -> Option<Location> {
        let source = index.source(span)?;
        let uri = self.source_uri(&source.path)?;
        Some(Location {
            uri,
            range: Range::new(
                position(&source.text, span.start.saturating_sub(source.start)),
                position(&source.text, span.end.saturating_sub(source.start)),
            ),
        })
    }

    fn references(&self, key: &editor::symbols::Key, include_declaration: bool) -> Vec<Location> {
        let mut locations = BTreeMap::new();
        let mut visited = BTreeSet::new();
        for analysis in self.documents.values().filter_map(|d| d.analysis.as_ref()) {
            if !visited.insert(Arc::as_ptr(&analysis.index)) {
                continue;
            }
            for span in analysis.index.occurrences(key, include_declaration) {
                if let Some(location) = self.location(&analysis.index, span) {
                    let key = (
                        location.uri.as_str().to_owned(),
                        location.range.start.line,
                        location.range.start.character,
                    );
                    locations.insert(key, location);
                }
            }
        }
        locations.into_values().collect()
    }

    fn notification(&mut self, notification: Notification) -> Result<bool, String> {
        match notification.method.as_str() {
            "initialized" => {
                if !notification.params.is_object() {
                    return Err("invalid initialized parameters".into());
                }
                self.watches.initialized = true;
                return Ok(false);
            }
            "textDocument/didOpen" => {
                let mut document = OpenDocument::parse(&notification.params)?;
                let path = if let Some(path) = bundled::path(&document.uri) {
                    document.text = package::bundled_source(&path).unwrap().to_owned();
                    Some(path)
                } else if document.uri.as_str().starts_with("untitled:") {
                    None
                } else {
                    Some(uri_path(&document.uri)?)
                };
                self.documents.insert(
                    document.uri.as_str().to_owned(),
                    Document {
                        uri: document.uri,
                        path,
                        text: document.text,
                        version: document.version,
                        analysis: None,
                        watch_roots: BTreeSet::new(),
                    },
                );
            }
            "textDocument/didChange" => {
                let params = DocumentChange::parse(&notification.params)?;
                if bundled::path(&params.uri).is_some() {
                    return Ok(false);
                }
                let Some(document) = self.documents.get_mut(params.uri.as_str()) else {
                    return Ok(false);
                };
                if params.version <= document.version || params.text.is_none() {
                    return Ok(false);
                }
                if let Some(text) = params.text {
                    document.text = text;
                    document.version = params.version;
                }
            }
            "textDocument/didSave" => {
                let uri = protocol::saved_uri(&notification.params)?;
                if bundled::path(&uri).is_some() {
                    return Ok(false);
                }
                if !self.documents.contains_key(&uri) {
                    return Ok(false);
                }
                // A save refreshes on-disk dependencies. Open buffers remain
                // authoritative until didChange/didClose, per LSP synchronization.
            }
            "textDocument/didClose" => {
                let uri = protocol::document_uri(&notification.params)?;
                if self.documents.remove(&uri).is_none() {
                    return Ok(false);
                }
            }
            "workspace/didChangeWatchedFiles" => {
                let paths = protocol::watched_paths(&notification.params)?;
                if self
                    .project_config
                    .path
                    .as_ref()
                    .is_some_and(|manifest| paths.contains(&package::source_path(manifest)))
                {
                    match self.project_config.settings() {
                        Ok((target, bits)) => {
                            self.target = target;
                            self.pointer_bits = bits;
                            self.project_error = None;
                        }
                        Err(message) => self.project_error = Some(message),
                    }
                    return Ok(true);
                }
                return Ok(paths.iter().any(|path| {
                    self.watches.contains(path)
                        && !self
                            .documents
                            .values()
                            .any(|document| document.path.as_ref() == Some(path))
                }));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn root<'a>(&self, path: &'a Path) -> &'a Path {
        if self.check_packages && path.is_absolute() {
            path.parent().unwrap_or(path)
        } else {
            path
        }
    }

    fn sync_watches(&mut self, output: &mut impl Write) -> io::Result<()> {
        let mut roots = BTreeSet::new();
        if let Some(parent) = self.project_config.path.as_ref().and_then(|p| p.parent()) {
            roots.insert(parent.to_path_buf());
        }
        for document in self.documents.values() {
            if let Some(path) = &document.path
                && path.is_absolute()
                && let Some(parent) = path.parent()
            {
                roots.insert(parent.to_path_buf());
            }
            roots.extend(document.watch_roots.iter().cloned());
        }
        self.watches.sync(roots, output)
    }

    fn publish(&mut self, output: &mut impl Write) -> io::Result<()> {
        if let Some(message) = self.project_error.take() {
            protocol::write_notification(
                output,
                "window/showMessage",
                json!({"type":1,"message":format!("Dodo project configuration: {message}. Keeping the last valid target.")}),
            )?;
        }
        let overlays = self
            .documents
            .values()
            .filter_map(|document| {
                let path = document.path.as_ref()?;
                path.is_absolute()
                    .then(|| (path.clone(), document.text.clone()))
            })
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
        let mut analyses = BTreeMap::new();
        let mut watch_roots = BTreeMap::new();
        for (key, document) in &self.documents {
            let Some(path) = &document.path else {
                let analysis = editor::Document::standalone(
                    document.text.clone(),
                    self.pointer_bits,
                    PathBuf::from(document.uri.as_str()),
                );
                for diagnostic in &analysis.diagnostics {
                    self.add_diagnostic(&mut diagnostics, &document.uri, None, &[], diagnostic);
                }
                analyses.insert(key.clone(), analysis);
                continue;
            };
            let root = self.root(path);
            if !checked.insert(root) {
                continue;
            }
            let loaded = if bundled::uri(root).is_some() {
                package::load_bundled_for_editor(root, &self.target)
            } else {
                package::load_for_editor(root, &overlays, &self.target)
            };
            match loaded {
                Ok(mut loaded) => {
                    let original = loaded.program.clone();
                    let semantic = sema::check_recovering(&mut loaded.program, self.pointer_bits);
                    loaded.diagnostics.extend(semantic);
                    let index = Arc::new(editor::symbols::Index::new(&loaded, &original));
                    for diagnostic in &loaded.diagnostics {
                        self.add_diagnostic(
                            &mut diagnostics,
                            &document.uri,
                            loaded.source(diagnostic.span),
                            &loaded.sources,
                            diagnostic,
                        );
                    }
                    // Package mode checks a directory once and indexes every open
                    // sibling against that same typed program and source offsets.
                    for (key, target) in &self.documents {
                        let Some(target_path) = &target.path else {
                            continue;
                        };
                        let target_root = self.root(target_path);
                        if target_root == root
                            && let Some(source) = loaded
                                .sources
                                .iter()
                                .find(|source| source.path == *target_path)
                        {
                            watch_roots.insert(
                                key.clone(),
                                loaded
                                    .sources
                                    .iter()
                                    .filter(|source| source.path.is_absolute())
                                    .filter_map(|source| {
                                        source.path.parent().map(Path::to_path_buf)
                                    })
                                    .collect(),
                            );
                            analyses.insert(
                                key.clone(),
                                editor::Document::from_checked(
                                    target.text.clone(),
                                    source.start,
                                    &loaded.program,
                                    loaded.diagnostics.clone(),
                                    Arc::clone(&index),
                                ),
                            );
                        }
                    }
                }
                Err(error) => {
                    self.add_diagnostic(
                        &mut diagnostics,
                        &document.uri,
                        error.source.as_deref(),
                        &[],
                        &error.diagnostic,
                    );
                    // Keep declarations and successfully checked local bindings
                    // available while an import or sibling has a syntax error.
                    for (key, target) in &self.documents {
                        let Some(target_path) = &target.path else {
                            continue;
                        };
                        let target_root = self.root(target_path);
                        if target_root == root {
                            analyses.insert(
                                key.clone(),
                                editor::Document::standalone(
                                    target.text.clone(),
                                    self.pointer_bits,
                                    target_path.clone(),
                                ),
                            );
                        }
                    }
                }
            }
        }
        for (key, analysis) in analyses {
            let document = self.documents.get_mut(&key).unwrap();
            document.analysis = Some(analysis);
            if let Some(roots) = watch_roots.remove(&key) {
                document.watch_roots = roots;
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
            protocol::write_notification(
                output,
                "textDocument/publishDiagnostics",
                params.to_json(),
            )?;
        }
        self.published = current;
        // A dependency edit can change hints in an unchanged open buffer. Let
        // capable clients invalidate their cached hints after each fresh check.
        if self.inlay_refresh && self.watches.initialized {
            let id = format!("dodo/inlayHint/{}", self.next_inlay_refresh);
            self.next_inlay_refresh += 1;
            editor::write_message(
                output,
                &json!({
                    "jsonrpc":"2.0", "id":id, "method":"workspace/inlayHint/refresh"
                }),
            )?;
        }
        Ok(())
    }

    fn add_diagnostic(
        &self,
        diagnostics: &mut BTreeMap<String, PublishDiagnosticsParams>,
        fallback: &str,
        source: Option<&package::Source>,
        sources: &[package::Source],
        diagnostic: &Diagnostic,
    ) {
        let uri = source
            .and_then(|source| self.source_uri(&source.path))
            .unwrap_or_else(|| fallback.to_owned());
        let (text, offset) = source
            .map(|source| (source.text.as_str(), source.start))
            .unwrap_or_else(|| (self.documents[fallback].text.as_str(), 0));
        let mut converted = to_diagnostic(diagnostic, text, offset);
        if !diagnostic.labels.is_empty() {
            converted["relatedInformation"] = json!(
                diagnostic
                    .labels
                    .iter()
                    .map(|label| {
                        let source = sources
                            .iter()
                            .find(|source| {
                                source.start <= label.span.start
                                    && label.span.start <= source.start + source.text.len()
                            })
                            .or(source);
                        let label_uri = source
                            .and_then(|source| self.source_uri(&source.path))
                            .unwrap_or_else(|| fallback.to_owned());
                        let (text, offset) = source
                            .map(|source| (source.text.as_str(), source.start))
                            .unwrap_or((text, offset));
                        json!({
                            "location": Location {
                                uri: label_uri,
                                range: Range::new(
                                    position(text, label.span.start.saturating_sub(offset)),
                                    position(text, label.span.end.saturating_sub(offset)),
                                ),
                            }.to_json(),
                            "message": label.message,
                        })
                    })
                    .collect::<Vec<_>>()
            );
        }
        let diagnostic = converted;
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

    fn source_uri(&self, path: &Path) -> Option<String> {
        self.documents
            .values()
            .find(|document| {
                document.path.as_deref() == Some(path)
                    || (document.path.is_none() && Path::new(document.uri.as_str()) == path)
            })
            .map(|document| document.uri.clone())
            .or_else(|| bundled::uri(path))
            .or_else(|| file_uri::from_path(path).ok())
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name != "_"
        && name != "self"
        && !crate::parser::reserved(name)
}

pub(crate) fn uri_path(uri: &str) -> Result<PathBuf, String> {
    let path = file_uri::to_path(uri)?;
    Ok(package::source_path(&path))
}

fn to_diagnostic(diagnostic: &Diagnostic, text: &str, offset: usize) -> Value {
    let start = diagnostic.span.start.saturating_sub(offset);
    let end = diagnostic.span.end.saturating_sub(offset).max(start);
    let mut message = diagnostic.message.to_string();
    for note in &diagnostic.notes {
        message.push_str("\nnote: ");
        message.push_str(note);
    }
    let mut result = json!({
        "range": Range::new(position(text, start), position(text, end)).to_json(),
        "severity": match diagnostic.severity {
            Severity::Error => 1,
            Severity::Warning => 2,
        },
        "source": "dodo",
        "message": message,
    });
    if let Some(code) = diagnostic.kind.code() {
        result["code"] = json!(code);
    }
    result
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
        assert_eq!(result["severity"], 2);
        assert_eq!(
            result["range"],
            json!({"start":{"line":0,"character":2},"end":{"line":0,"character":3}})
        );
        assert_eq!(
            result["message"],
            "example warning\nnote: use this information"
        );
        assert!(warning.render("test.dodo", "😀é").starts_with("warning: "));
    }
}
