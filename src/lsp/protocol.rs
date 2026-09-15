//! The JSON-RPC and LSP subset used by Dodo, implemented directly over JSON.
//!
//! Unknown fields are ignored for protocol extensions. Required fields and the
//! optional fields we consume are validated before any server state changes.
//! No external protocol definitions or serialization derives are needed.
//! Wire formats: https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/
use crate::json::{Value, json};
use crate::{editor, file_uri};
use std::io::{self, Write};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Copy)]
pub(super) enum ErrorCode {
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    ServerNotInitialized = -32002,
    RequestFailed = -32803,
}

#[derive(Debug)]
pub(super) struct Request {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

#[derive(Debug)]
pub(super) struct Notification {
    pub method: String,
    pub params: Value,
}

#[derive(Debug)]
pub(super) enum Message {
    Request(Request),
    Notification(Notification),
    Response { id: Value, error: Option<String> },
}

fn request_id(id: &Value) -> bool {
    id.is_string() || id.as_i64().is_some_and(|n| i32::try_from(n).is_ok())
}

impl Message {
    /// Invalid envelopes return the usable request ID, or null if it is invalid.
    /// LSP restricts numeric IDs to signed 32-bit integers and excludes null IDs
    /// from requests. A response can have a null ID after a peer's parse error.
    pub fn parse(mut value: Value) -> std::result::Result<Self, Value> {
        let invalid_id = value
            .get("id")
            .filter(|id| request_id(id))
            .cloned()
            .unwrap_or(Value::Null);
        let invalid = || invalid_id.clone();
        let object = value.as_object_mut().ok_or_else(invalid)?;
        if object.get("jsonrpc") != Some(&json!("2.0")) {
            return Err(Value::Null);
        }
        if let Some(method) = object.get("method") {
            let method = method.as_str().ok_or_else(invalid)?.to_owned();
            if object.contains_key("result") || object.contains_key("error") {
                return Err(invalid());
            }
            // Parameter shape is checked by the method handler. That gives bad
            // requests InvalidParams and keeps bad notifications response-free.
            let params = object.remove("params").unwrap_or(Value::Null);
            return match object.remove("id") {
                Some(id) if request_id(&id) => Ok(Self::Request(Request { id, method, params })),
                Some(_) => Err(Value::Null),
                None => Ok(Self::Notification(Notification { method, params })),
            };
        }
        let id = object.get("id").ok_or_else(invalid)?;
        if (!id.is_null() && !request_id(id)) || object.contains_key("params") {
            return Err(invalid());
        }
        match (object.get("result"), object.get("error")) {
            (Some(_), None) => Ok(Self::Response {
                id: id.clone(),
                error: None,
            }),
            (None, Some(error))
                if error.is_object()
                    && error["code"]
                        .as_i64()
                        .is_some_and(|n| i32::try_from(n).is_ok())
                    && error["message"].is_string() =>
            {
                Ok(Self::Response {
                    id: id.clone(),
                    error: Some(error["message"].as_str().unwrap().to_owned()),
                })
            }
            _ => Err(invalid()),
        }
    }
}

pub(super) struct Response(Value);

impl Response {
    pub fn new_ok(id: Value, result: Value) -> Self {
        Self(json!({"jsonrpc":"2.0", "id":id, "result":result}))
    }

    pub fn new_err(id: Value, code: i32, message: String) -> Self {
        Self(json!({"jsonrpc":"2.0", "id":id, "error":{"code":code, "message":message}}))
    }

    pub fn write(&self, output: &mut impl Write) -> io::Result<()> {
        editor::write_message(output, &self.0)
    }
}

pub(super) fn write_notification(
    output: &mut impl Write,
    method: &str,
    params: Value,
) -> io::Result<()> {
    editor::write_message(
        output,
        &json!({"jsonrpc":"2.0", "method":method, "params":params}),
    )
}

fn object(value: &Value) -> Result<&Value> {
    value
        .is_object()
        .then_some(value)
        .ok_or("expected an object")
}

fn string(value: &Value) -> Result<&str> {
    value.as_str().ok_or("expected a string")
}

fn integer(value: &Value) -> Result<i32> {
    value
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .ok_or("expected an LSP integer")
}

fn unsigned(value: &Value) -> Result<u32> {
    integer(value).and_then(|n| u32::try_from(n).map_err(|_| "expected an LSP unsigned integer"))
}

fn boolean(value: &Value) -> Result<bool> {
    value.as_bool().ok_or("expected a boolean")
}

fn optional<T>(value: &Value, parse: impl FnOnce(&Value) -> Result<T>) -> Result<Option<T>> {
    if value.is_null() {
        Ok(None)
    } else {
        parse(value).map(Some)
    }
}

fn optional_object(value: &Value) -> Result<&Value> {
    if value.is_null() {
        Ok(&Value::Null)
    } else {
        object(value)
    }
}

fn uri(value: &Value) -> Result<String> {
    let uri = string(value)?;
    if super::bundled::path(uri).is_none() {
        file_uri::validate(uri)?;
    }
    Ok(uri.to_owned())
}

pub(super) fn document_uri(params: &Value) -> Result<String> {
    uri(&object(&object(params)?["textDocument"])?["uri"])
}

pub(super) struct InitializeParams {
    pub initialization_options: Value,
    pub document_changes: bool,
    pub dynamic_watches: bool,
    pub relative_watches: bool,
}

impl InitializeParams {
    pub fn parse(value: &Value) -> Result<Self> {
        let value = object(value)?;
        // Preserve support for minimal clients that omit capabilities entirely.
        let capabilities = match value.get("capabilities") {
            None => &Value::Null,
            Some(value) => object(value)?,
        };
        let workspace = optional_object(&capabilities["workspace"])?;
        let edits = optional_object(&workspace["workspaceEdit"])?;
        let watches = optional_object(&workspace["didChangeWatchedFiles"])?;
        Ok(Self {
            initialization_options: value["initializationOptions"].clone(),
            document_changes: optional(&edits["documentChanges"], boolean)?.unwrap_or(false),
            dynamic_watches: optional(&watches["dynamicRegistration"], boolean)?.unwrap_or(false),
            relative_watches: optional(&watches["relativePatternSupport"], boolean)?
                .unwrap_or(false),
        })
    }
}

pub(super) fn bundled_source_uri(params: &Value) -> Result<String> {
    let uri = string(&object(params)?["uri"])?;
    super::bundled::path(uri).ok_or("unknown bundled source URI")?;
    Ok(uri.to_owned())
}

pub(super) fn watched_paths(params: &Value) -> Result<Vec<std::path::PathBuf>> {
    let changes = object(params)?["changes"]
        .as_array()
        .ok_or("expected file changes")?;
    // Validate the complete batch before refreshing any analysis.
    changes
        .iter()
        .map(|change| {
            let change = object(change)?;
            if !matches!(integer(&change["type"])?, 1..=3) {
                return Err("invalid file change type");
            }
            let path = file_uri::to_path(string(&change["uri"])?)?;
            Ok(crate::package::source_path(&path))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }

    fn parse(value: &Value) -> Result<Self> {
        let value = object(value)?;
        Ok(Self::new(
            unsigned(&value["line"])?,
            unsigned(&value["character"])?,
        ))
    }

    pub fn to_json(self) -> Value {
        json!({"line":self.line, "character":self.character})
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    fn parse(value: &Value) -> Result<Self> {
        let value = object(value)?;
        Ok(Self::new(
            Position::parse(&value["start"])?,
            Position::parse(&value["end"])?,
        ))
    }

    pub fn to_json(self) -> Value {
        json!({"start":self.start.to_json(), "end":self.end.to_json()})
    }
}

pub(super) struct Location {
    pub uri: String,
    pub range: Range,
}

impl Location {
    pub fn to_json(&self) -> Value {
        json!({"uri":self.uri, "range":self.range.to_json()})
    }
}

pub(super) struct PublishDiagnosticsParams {
    pub uri: String,
    pub diagnostics: Vec<Value>,
    pub version: Option<i32>,
}

impl PublishDiagnosticsParams {
    pub fn to_json(&self) -> Value {
        let mut value = json!({"uri":self.uri, "diagnostics":self.diagnostics});
        if let Some(version) = self.version {
            value["version"] = json!(version);
        }
        value
    }
}

pub(super) struct QueryParams {
    pub uri: String,
    pub position: Position,
    pub include_declaration: bool,
    pub new_name: Option<String>,
}

impl QueryParams {
    pub fn parse(value: &Value, method: &str) -> Result<Self> {
        let uri = document_uri(value)?;
        let position = Position::parse(&value["position"])?;
        let mut result = Self {
            uri,
            position,
            include_declaration: false,
            new_name: None,
        };
        match method {
            "textDocument/references" => {
                result.include_declaration =
                    boolean(&object(&value["context"])?["includeDeclaration"])?;
            }
            "textDocument/rename" => result.new_name = Some(string(&value["newName"])?.to_owned()),
            "textDocument/completion" => {
                let context = optional_object(&value["context"])?;
                if !context.is_null() {
                    unsigned(&context["triggerKind"])?;
                    optional(&context["triggerCharacter"], |v| string(v).map(|_| ()))?;
                }
            }
            "textDocument/signatureHelp" => {
                let context = optional_object(&value["context"])?;
                if !context.is_null() {
                    unsigned(&context["triggerKind"])?;
                    boolean(&context["isRetrigger"])?;
                    optional(&context["triggerCharacter"], |v| string(v).map(|_| ()))?;
                }
            }
            _ => (),
        }
        Ok(result)
    }
}

pub(super) fn formatting_uri(value: &Value) -> Result<String> {
    let uri = document_uri(value)?;
    let options = object(&value["options"])?;
    unsigned(&options["tabSize"])?;
    boolean(&options["insertSpaces"])?;
    for name in [
        "trimTrailingWhitespace",
        "insertFinalNewline",
        "trimFinalNewlines",
    ] {
        optional(&options[name], boolean)?;
    }
    Ok(uri)
}

pub(super) struct OpenDocument {
    pub uri: String,
    pub version: i32,
    pub text: String,
}

impl OpenDocument {
    pub fn parse(value: &Value) -> Result<Self> {
        let uri = document_uri(value)?;
        let document = &value["textDocument"];
        string(&document["languageId"])?;
        Ok(Self {
            uri,
            version: integer(&document["version"])?,
            text: string(&document["text"])?.to_owned(),
        })
    }
}

pub(super) struct DocumentChange {
    pub uri: String,
    pub version: i32,
    pub text: Option<String>,
}

impl DocumentChange {
    pub fn parse(value: &Value) -> Result<Self> {
        let uri = document_uri(value)?;
        let version = integer(&value["textDocument"]["version"])?;
        let changes = value["contentChanges"]
            .as_array()
            .ok_or("expected contentChanges array")?;
        let mut text = None;
        // Validate every entry before selecting the final full-buffer snapshot.
        for change in changes {
            let change = object(change)?;
            if optional(&change["range"], Range::parse)?.is_some() {
                return Err("expected a full document change");
            }
            optional(&change["rangeLength"], unsigned)?;
            text = Some(string(&change["text"])?);
        }
        Ok(Self {
            uri,
            version,
            text: text.map(str::to_owned),
        })
    }
}

pub(super) fn saved_uri(value: &Value) -> Result<String> {
    let uri = document_uri(value)?;
    optional(&value["text"], |v| string(v).map(|_| ()))?;
    Ok(uri)
}
