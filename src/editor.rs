//! Source hovers and a small, full-document-sync Language Server Protocol server.
//!
//! Positions use UTF-16 code units, as required by the default LSP encoding.
//! Checking uses in-memory document overlays; editor changes never write files.
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::lexer::{self, Token, TokenKind};
use crate::{package, parser, sema};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct HoverEntry {
    span: Span,
    markdown: String,
}

/// Analysis of a document, retaining successfully inferred types even if checking fails.
pub struct Document {
    text: String,
    start: usize,
    entries: Vec<HoverEntry>,
    diagnostic: Option<Diagnostic>,
    sources: Vec<package::Source>,
}

impl Document {
    /// Analyze a standalone document. The language server additionally resolves imports.
    pub fn new(text: String) -> Self {
        Self::standalone(text)
    }

    fn standalone(text: String) -> Self {
        let (program, diagnostic) = match parser::parse(&text) {
            Ok(program) => (program, None),
            Err(error) => (Program::default(), Some(error)),
        };
        Self::analyze(text, 0, program, vec![], diagnostic)
    }

    fn analyze(
        text: String,
        start: usize,
        mut program: Program,
        sources: Vec<package::Source>,
        diagnostic: Option<Diagnostic>,
    ) -> Self {
        let diagnostic = diagnostic.or_else(|| sema::check(&mut program).err());
        let tokens = lexer::lex(&text).unwrap_or_default();
        let mut index = HoverIndex {
            program: &program,
            tokens: &tokens,
            start,
            entries: vec![],
        };
        index.program();
        Self {
            text,
            start,
            entries: index.entries,
            diagnostic,
            sources,
        }
    }

    fn load(uri: &str, text: String, overrides: &BTreeMap<PathBuf, String>) -> Self {
        if let Some(path) = file_path(uri).and_then(|p| p.canonicalize().ok()) {
            match package::load_with_overrides(&path, overrides) {
                Ok(loaded) => {
                    if let Some(source) = loaded.sources.iter().find(|s| s.path == path) {
                        return Self::analyze(
                            text,
                            source.start,
                            loaded.program,
                            loaded.sources,
                            None,
                        );
                    }
                }
                Err(error) => {
                    let mut document = Self::standalone(text);
                    // A syntax error in this buffer has a better range than the loader's text.
                    if parser::parse(&document.text).is_ok() {
                        document.diagnostic = Some(
                            Diagnostic::new(Span::default(), "could not load document package")
                                .note(error),
                        );
                    }
                    return document;
                }
            }
        }
        Self::standalone(text)
    }

    /// Return an LSP Markdown hover and range for a zero-based UTF-16 position.
    pub fn hover(&self, line: u32, character: u32) -> Option<Value> {
        let offset = byte_offset(&self.text, line, character)? + self.start;
        let entry = self
            .entries
            .iter()
            .filter(|entry| entry.span.start <= offset && offset < entry.span.end)
            .min_by_key(|entry| entry.span.end.saturating_sub(entry.span.start))?;
        Some(json!({
            "contents": {"kind": "markdown", "value": entry.markdown},
            "range": range(&self.text, local_span(entry.span, self.start))
        }))
    }

    fn location(&self, span: Span, uri: &str) -> Value {
        if let Some(source) = self.sources.iter().find(|source| {
            source.start <= span.start && span.start <= source.start + source.text.len()
        }) {
            json!({"uri": file_uri(&source.path), "range": range(&source.text, local_span(span, source.start))})
        } else {
            json!({"uri": uri, "range": range(&self.text, local_span(span, self.start))})
        }
    }

    fn diagnostics(&self, uri: &str) -> Vec<Value> {
        let Some(diagnostic) = &self.diagnostic else {
            return vec![];
        };
        let mut related: Vec<_> = diagnostic.labels.iter().map(|label| {
            json!({"location": self.location(label.span, uri), "message": label.message})
        }).collect();
        let primary = self.location(diagnostic.span, uri);
        let local = diagnostic.span.start >= self.start
            && diagnostic.span.start <= self.start + self.text.len();
        if !local {
            related.insert(
                0,
                json!({"location": primary, "message": diagnostic.message}),
            );
        }
        let mut message = diagnostic.message.clone();
        for note in &diagnostic.notes {
            message.push('\n');
            message.push_str(note);
        }
        vec![json!({
            "range": if local { primary["range"].clone() } else { range(&self.text, Span::default()) },
            "severity": 1,
            "source": "dodo",
            "message": message,
            "relatedInformation": related
        })]
    }
}

struct HoverIndex<'a> {
    program: &'a Program,
    tokens: &'a [Token],
    start: usize,
    entries: Vec<HoverEntry>,
}

impl HoverIndex<'_> {
    fn add(&mut self, span: Span, markdown: String) {
        if span.start >= self.start && span.start < span.end {
            self.entries.push(HoverEntry { span, markdown });
        }
    }

    fn name_span(&self, span: Span, name: &str) -> Option<Span> {
        self.tokens.iter().find_map(|token| {
            let global = Span {
                start: token.span.start + self.start,
                end: token.span.end + self.start,
            };
            (global.start >= span.start
                && global.end <= span.end
                && matches!(&token.kind, TokenKind::Ident(word) if word == name))
            .then_some(global)
        })
    }

    fn callee_span(&self, span: Span) -> Option<Span> {
        let tokens: Vec<_> = self
            .tokens
            .iter()
            .filter(|token| {
                token.span.start + self.start >= span.start
                    && token.span.end + self.start <= span.end
            })
            .collect();
        // Match the outer call's closing parenthesis, skipping calls in its receiver
        // and arguments. The immediately preceding name is the actual callee.
        let close = tokens
            .iter()
            .rposition(|token| token.kind == TokenKind::Symbol(")"))?;
        let mut depth = 1;
        let open = (0..close).rev().find(|&index| {
            match tokens[index].kind {
                TokenKind::Symbol(")") => depth += 1,
                TokenKind::Symbol("(") => depth -= 1,
                _ => {}
            }
            depth == 0
        })?;
        let mut index = open.checked_sub(1)?;
        let mut angles = 0;
        loop {
            match tokens[index].kind {
                TokenKind::Symbol(">") => angles += 1,
                TokenKind::Symbol(">>") => angles += 2,
                TokenKind::Symbol("<") if angles > 0 => angles -= 1,
                TokenKind::Symbol("::") if angles == 0 => {}
                TokenKind::Ident(_) if angles == 0 => {
                    let token = tokens[index];
                    return Some(Span {
                        start: token.span.start + self.start,
                        end: token.span.end + self.start,
                    });
                }
                _ if angles == 0 => return None,
                _ => {}
            }
            index = index.checked_sub(1)?;
        }
    }

    fn binding(&mut self, span: Span, name: &str, ty: &Type) {
        if *ty != Type::Unknown
            && let Some(span) = self.name_span(span, name)
        {
            self.add(span, code(&format!("{name}: {ty}")));
        }
    }

    fn program(&mut self) {
        for function in &self.program.functions {
            let markdown = signature(function);
            if let Some(span) = self.name_span(function.span, short_name(&function.name)) {
                self.add(span, markdown.clone());
            }
            self.add(function.ret_span, markdown.clone());
            if let Some(span) = function.from_span {
                self.add(span, markdown);
            }
            for parameter in &function.params {
                self.binding(parameter.span, &parameter.name, &parameter.ty);
            }
            if let Some(body) = &function.body {
                self.block(body);
            }
        }
        for constant in &self.program.constants {
            self.binding(constant.span, &constant.name, &constant.ty);
            self.expression(&constant.value);
        }
        for structure in &self.program.structs {
            if let Some(span) = self.name_span(structure.span, short_name(&structure.name)) {
                self.add(span, code(&format!("struct {}", structure.name)));
            }
            for field in &structure.fields {
                self.binding(field.span, &field.name, &field.ty);
            }
        }
    }

    fn block(&mut self, block: &Block) {
        for statement in block {
            self.statement(statement);
        }
    }

    fn statement(&mut self, statement: &Stmt) {
        match &statement.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                self.binding(statement.span, name, ty);
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            StmtKind::LetPattern {
                value, else_block, ..
            } => {
                self.expression(value);
                if let Some(block) = else_block {
                    self.block(block);
                }
            }
            StmtKind::Assign { target, value, .. } => {
                self.expression(target);
                self.expression(value);
            }
            StmtKind::Expr(value) | StmtKind::Yield(value) | StmtKind::Return(Some(value)) => {
                self.expression(value)
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expression(condition);
                self.block(then_block);
                self.block(else_block);
            }
            StmtKind::IfLet {
                value,
                then_block,
                else_block,
                ..
            } => {
                self.expression(value);
                self.block(then_block);
                self.block(else_block);
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                if let Some(init) = init {
                    self.statement(init);
                }
                if let Some(condition) = condition {
                    self.expression(condition);
                }
                if let Some(step) = step {
                    self.statement(step);
                }
                self.block(body);
            }
            StmtKind::ForEach { iterable, body, .. } => {
                self.expression(iterable);
                self.block(body);
            }
            StmtKind::Match { value, arms } => {
                self.expression(value);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.expression(guard);
                    }
                    self.block(&arm.body);
                }
            }
            StmtKind::Block(body) | StmtKind::Unsafe(body) => self.block(body),
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn expression(&mut self, expression: &Expr) {
        // Index children before parents so identical generated spans favor source nodes.
        match &expression.kind {
            ExprKind::Call { name, args, .. } => {
                if let Some(function) = self.program.functions.iter().find(|f| f.name == *name)
                    && let Some(span) = self.callee_span(expression.span)
                {
                    self.add(span, signature(function));
                }
                for arg in args {
                    self.expression(arg);
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                self.expression(receiver);
                for arg in args {
                    self.expression(arg);
                }
            }
            ExprKind::Array(_, values) => {
                for value in values {
                    self.expression(value);
                }
            }
            ExprKind::Struct(_, fields) => {
                for (_, value) in fields {
                    self.expression(value);
                }
            }
            ExprKind::Repeat(value, _)
            | ExprKind::Constant(value, _)
            | ExprKind::Unary(_, value)
            | ExprKind::Field(value, _)
            | ExprKind::Cast(value, _)
            | ExprKind::Try(value) => self.expression(value),
            ExprKind::Binary(_, left, right)
            | ExprKind::Index(left, right)
            | ExprKind::Range(left, right) => {
                self.expression(left);
                self.expression(right);
            }
            ExprKind::Slice {
                base, start, end, ..
            } => {
                self.expression(base);
                if let Some(start) = start {
                    self.expression(start);
                }
                if let Some(end) = end {
                    self.expression(end);
                }
            }
            ExprKind::ValueBlock(block) => self.block(block),
            ExprKind::Int(..)
            | ExprKind::Float(..)
            | ExprKind::Bool(..)
            | ExprKind::String(..)
            | ExprKind::Name(..) => {}
        }
        if expression.ty != Type::Unknown {
            let display = match &expression.kind {
                ExprKind::Name(name) => format!("{name}: {}", expression.ty),
                _ => expression.ty.to_string(),
            };
            self.add(expression.span, code(&display));
        }
    }
}

fn code(text: &str) -> String {
    format!("```dodo\n{}\n```", source_names(text))
}

// Semantic specialization encodes type arguments as hexadecimal after `$`.
// Present the source notation in hovers, including nested specialized types.
fn source_names(text: &str) -> String {
    let Some(start) = text.find('$') else {
        return text.to_owned();
    };
    let suffix = &text[start + 1..];
    let end = suffix
        .find(|ch: char| !ch.is_ascii_hexdigit() && ch != '_')
        .unwrap_or(suffix.len());
    let arguments: Option<Vec<_>> = suffix[..end]
        .split('_')
        .map(|argument| {
            if argument.is_empty() || !argument.len().is_multiple_of(2) {
                return None;
            }
            let bytes: Option<Vec<_>> = (0..argument.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&argument[i..i + 2], 16).ok())
                .collect();
            String::from_utf8(bytes?)
                .ok()
                .map(|argument| source_names(&argument))
        })
        .collect();
    match arguments {
        Some(arguments) => format!(
            "{}<{}>{}",
            &text[..start],
            arguments.join(", "),
            source_names(&suffix[end..])
        ),
        None => format!("{}${}", &text[..start], source_names(suffix)),
    }
}

fn short_name(name: &str) -> &str {
    name.rsplit('.')
        .next()
        .unwrap_or(name)
        .split(['<', '$'])
        .next()
        .unwrap_or(name)
}

fn signature(function: &Function) -> String {
    let params = function
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty))
        .collect::<Vec<_>>()
        .join(", ");
    let from = if function.from.is_empty() {
        String::new()
    } else {
        format!(" from({})", function.from.join(", "))
    };
    let mut markdown = code(&format!(
        "{}fn {}({params}) -> {}{from}",
        if function.unsafe_ { "unsafe " } else { "" },
        function.name,
        function.ret
    ));
    if let Some(receiver) = function.params.first().filter(|p| p.name == "self") {
        markdown.push_str(match receiver.ty {
            Type::Ref(false, _) => "\n\nReceiver: shared borrow (`&self`).",
            Type::Ref(true, _) => "\n\nReceiver: mutable borrow (`&mut self`).",
            _ => "\n\nReceiver: consumes `self` (ownership is transferred).",
        });
    }
    if !function.from.is_empty() {
        markdown.push_str(&format!(
            "\n\nBorrowed return sources: `from({})` ({}).",
            function.from.join(", "),
            if function.from_span.is_some() {
                "explicit"
            } else {
                "inferred"
            }
        ));
    }
    markdown
}

fn local_span(span: Span, start: usize) -> Span {
    Span {
        start: span.start.saturating_sub(start),
        end: span.end.saturating_sub(start),
    }
}

fn byte_offset(text: &str, line: u32, character: u32) -> Option<usize> {
    let mut start = 0;
    for _ in 0..line {
        start += text.get(start..)?.find('\n')? + 1;
    }
    let line = text
        .get(start..)?
        .split('\n')
        .next()?
        .trim_end_matches('\r');
    let mut units = 0;
    for (byte, ch) in line.char_indices() {
        if units == character {
            return Some(start + byte);
        }
        units += ch.len_utf16() as u32;
        if units > character {
            return None;
        }
    }
    (units == character).then_some(start + line.len())
}

fn position(text: &str, byte: usize) -> Value {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    let prefix = &text[..byte];
    let line = prefix.bytes().filter(|b| *b == b'\n').count();
    let last = prefix
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    json!({"line": line, "character": last.encode_utf16().count()})
}

fn range(text: &str, span: Span) -> Value {
    json!({"start": position(text, span.start), "end": position(text, span.end.max(span.start))})
}

fn file_path(uri: &str) -> Option<PathBuf> {
    let raw = uri.strip_prefix("file://")?;
    let raw = if raw.starts_with("localhost/") {
        &raw["localhost".len()..]
    } else {
        raw
    };
    if !raw.starts_with('/') || raw.contains(['?', '#']) {
        return None;
    }
    let mut bytes = Vec::new();
    let mut cursor = 0;
    while cursor < raw.len() {
        if raw.as_bytes()[cursor] == b'%' {
            let hex = raw.get(cursor + 1..cursor + 3)?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            cursor += 3;
        } else {
            bytes.push(raw.as_bytes()[cursor]);
            cursor += 1;
        }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    #[cfg(windows)]
    let decoded = decoded.strip_prefix('/').unwrap_or(&decoded).to_owned();
    Some(PathBuf::from(decoded))
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    #[cfg(windows)]
    uri.push('/');
    for byte in path.to_string_lossy().replace('\\', "/").bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            uri.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

struct OpenDocument {
    text: String,
    version: i64,
    analysis: Document,
}

fn publish(
    writer: &mut impl Write,
    uri: &str,
    version: Option<i64>,
    diagnostics: Vec<Value>,
) -> io::Result<()> {
    let mut params = json!({"uri": uri, "diagnostics": diagnostics});
    if let Some(version) = version {
        params["version"] = json!(version);
    }
    write_message(
        writer,
        &json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": params}),
    )
}

fn refresh(
    documents: &mut BTreeMap<String, OpenDocument>,
    writer: &mut impl Write,
) -> io::Result<()> {
    let overrides = documents
        .iter()
        .filter_map(|(uri, doc)| Some((file_path(uri)?.canonicalize().ok()?, doc.text.clone())))
        .collect();
    for (uri, document) in documents {
        document.analysis = Document::load(uri, document.text.clone(), &overrides);
        publish(
            writer,
            uri,
            Some(document.version),
            document.analysis.diagnostics(uri),
        )?;
    }
    Ok(())
}

/// Serve LSP JSON-RPC over framed streams. Returns the process exit status.
/// Only full-document changes are advertised and accepted.
pub fn serve(mut reader: impl BufRead, mut writer: impl Write) -> io::Result<i32> {
    let mut documents = BTreeMap::<String, OpenDocument>::new();
    let mut initialized = false;
    let mut shutdown = false;
    while let Some(body) = read_message(&mut reader)? {
        let message: Value = match serde_json::from_slice(&body) {
            Ok(message) => message,
            Err(_) => {
                error(&mut writer, Value::Null, -32700, "Parse error")?;
                continue;
            }
        };
        if !message.is_object() || message["jsonrpc"] != "2.0" {
            error(&mut writer, Value::Null, -32600, "Invalid Request")?;
            continue;
        }
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            if message.get("result").is_none() && message.get("error").is_none() {
                error(
                    &mut writer,
                    id.unwrap_or(Value::Null),
                    -32600,
                    "Invalid Request",
                )?;
            }
            continue;
        };
        if method == "exit" {
            return Ok(if shutdown { 0 } else { 1 });
        }
        if shutdown {
            if let Some(id) = id {
                error(&mut writer, id, -32600, "Server has shut down")?;
            }
            continue;
        }
        let params = &message["params"];
        if method == "initialize" {
            if let Some(id) = id {
                if initialized {
                    error(&mut writer, id, -32600, "Server is already initialized")?;
                } else {
                    initialized = true;
                    reply(
                        &mut writer,
                        id,
                        json!({"capabilities": {"positionEncoding": "utf-16", "textDocumentSync": {"openClose": true, "change": 1}, "hoverProvider": true}, "serverInfo": {"name": "dodo", "version": env!("CARGO_PKG_VERSION")}}),
                    )?;
                }
            }
            continue;
        }
        if !initialized {
            if let Some(id) = id {
                error(&mut writer, id, -32002, "Server not initialized")?;
            }
            continue;
        }
        match method {
            "shutdown" => {
                if let Some(id) = id {
                    shutdown = true;
                    reply(&mut writer, id, Value::Null)?;
                }
            }
            "textDocument/didOpen" => {
                let doc = &params["textDocument"];
                if let (Some(uri), Some(text), Some(version)) = (
                    doc["uri"].as_str(),
                    doc["text"].as_str(),
                    doc["version"].as_i64(),
                ) {
                    documents.insert(
                        uri.into(),
                        OpenDocument {
                            text: text.into(),
                            version,
                            analysis: Document::new(String::new()),
                        },
                    );
                    refresh(&mut documents, &mut writer)?;
                }
            }
            "textDocument/didChange" => {
                if let (Some(uri), Some(version), Some(changes)) = (
                    params["textDocument"]["uri"].as_str(),
                    params["textDocument"]["version"].as_i64(),
                    params["contentChanges"].as_array(),
                ) && let Some(document) = documents.get_mut(uri)
                    && version > document.version
                    && changes
                        .iter()
                        .all(|change| change.get("range").is_none() && change["text"].is_string())
                    && let Some(text) = changes.last().and_then(|change| change["text"].as_str())
                {
                    document.text = text.into();
                    document.version = version;
                    refresh(&mut documents, &mut writer)?;
                }
            }
            "textDocument/didClose" => {
                if let Some(uri) = params["textDocument"]["uri"].as_str() {
                    documents.remove(uri);
                    publish(&mut writer, uri, None, vec![])?;
                    refresh(&mut documents, &mut writer)?;
                }
            }
            "textDocument/hover" => {
                if let Some(id) = id {
                    let request = params["textDocument"]["uri"]
                        .as_str()
                        .zip(params["position"]["line"].as_u64())
                        .zip(params["position"]["character"].as_u64());
                    if let Some(((uri, line), character)) =
                        request.filter(|((_, line), character)| {
                            *line <= u32::MAX as u64 && *character <= u32::MAX as u64
                        })
                    {
                        let hover = documents
                            .get(uri)
                            .and_then(|doc| doc.analysis.hover(line as u32, character as u32))
                            .unwrap_or(Value::Null);
                        reply(&mut writer, id, hover)?;
                    } else {
                        error(&mut writer, id, -32602, "Invalid hover parameters")?;
                    }
                }
            }
            "initialized" | "$/cancelRequest" | "$/setTrace" => {}
            _ => {
                if let Some(id) = id {
                    error(&mut writer, id, -32601, "Method not found")?;
                }
            }
        }
    }
    Ok(if shutdown { 0 } else { 1 })
}

fn reply(writer: &mut impl Write, id: Value, result: Value) -> io::Result<()> {
    write_message(
        writer,
        &json!({"jsonrpc": "2.0", "id": id, "result": result}),
    )
}
fn error(writer: &mut impl Write, id: Value, code: i32, message: &str) -> io::Result<()> {
    write_message(
        writer,
        &json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    )
}
fn write_message(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}
fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut length = None;
    let mut headers = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            return if headers == 0 {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete LSP headers",
                ))
            };
        }
        headers += header.len();
        if headers > 8192 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LSP headers too large",
            ));
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            if length.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate Content-Length",
                ));
            }
            length = Some(value.trim().parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length")
            })?);
        }
    }
    let length = length
        .filter(|length| *length <= 16 * 1024 * 1024)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "missing or excessive Content-Length",
            )
        })?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hover_at(document: &Document, needle: &str) -> Value {
        let byte = document.text.find(needle).unwrap();
        let position = position(&document.text, byte);
        document
            .hover(
                position["line"].as_u64().unwrap() as u32,
                position["character"].as_u64().unwrap() as u32,
            )
            .unwrap_or_else(|| panic!("missing hover for {needle}"))
    }

    #[test]
    fn inferred_binding_and_expression_types() {
        let document = Document::new(
            "package test\nfn main() -> i32 {\nnumber := 42i32\nreturn number + 1\n}\n".into(),
        );
        assert!(document.diagnostic.is_none(), "{:?}", document.diagnostic);
        assert!(
            hover_at(&document, "number :=")["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("number: i32")
        );
        assert!(
            hover_at(&document, "number +")["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("number: i32")
        );
        assert!(
            hover_at(&document, "42i32")["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("i32")
        );
    }

    #[test]
    fn signatures_describe_receiver_and_return_contracts() {
        let document = Document::new(include_str!("../examples/borrowing.dodo").into());
        assert!(document.diagnostic.is_none(), "{:?}", document.diagnostic);
        for (needle, expected) in [
            ("view()", "shared borrow"),
            ("replace(20)", "mutable borrow"),
            ("finish()", "consumes"),
            ("choose(&", "from(a, b)"),
        ] {
            assert!(
                hover_at(&document, needle)["contents"]["value"]
                    .as_str()
                    .unwrap()
                    .contains(expected)
            );
        }
        let inferred = hover_at(&document, "view()");
        assert!(
            inferred["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("from(self)` (inferred)")
        );
        let explicit = hover_at(&document, "choose(&");
        assert!(
            explicit["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("(explicit)")
        );
    }

    #[test]
    fn invalid_document_retains_checked_bindings_and_contracts() {
        let document = Document::new("package test\nfn borrow(input: &i32) -> &i32 {\nreturn input\n}\nfn main() {\nnumber := 4i32\nbad := missing\n}\n".into());
        assert!(document.diagnostic.is_some());
        assert!(
            hover_at(&document, "number :=")["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("number: i32")
        );
        assert!(
            hover_at(&document, "borrow(")["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("from(input)")
        );
        assert_eq!(document.hover(100, 0), None);
    }

    #[test]
    fn method_name_hover_does_not_replace_receiver_type() {
        let document = Document::new("package test\nstruct Value {\nnumber: i32\nfn view(&self) -> &i32 { return &self.number }\n}\nfn main() {\nview := Value{number: 1}\nresult := view.view()\n}\n".into());
        assert!(document.diagnostic.is_none(), "{:?}", document.diagnostic);
        let receiver = hover_at(&document, "view.view()");
        assert!(
            receiver["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("view: Value")
        );
        let method = hover_at(&document, "view()");
        assert!(
            method["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("shared borrow")
        );
        assert_eq!(
            method["range"]["end"]["character"].as_u64().unwrap()
                - method["range"]["start"]["character"].as_u64().unwrap(),
            4
        );
    }

    #[test]
    fn utf16_positions_and_crlf_ranges() {
        let text = "a😀é\r\nnext";
        assert_eq!(byte_offset(text, 0, 3), Some(5));
        assert_eq!(byte_offset(text, 0, 2), None);
        assert_eq!(byte_offset(text, 0, 4), Some(7));
        assert_eq!(byte_offset(text, 1, 0), Some(9));
        assert_eq!(position(text, 5), json!({"line": 0, "character": 3}));
        let document =
            Document::new("package test\r\nfn main() { text := \"😀\"; count := 1u32 }\r\n".into());
        let hover = hover_at(&document, "count :=");
        assert_eq!(hover["range"]["start"], json!({"line": 1, "character": 26}));
        assert!(
            hover["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("count: u32")
        );
    }

    #[test]
    fn local_uris_round_trip_escaped_characters() {
        let path = PathBuf::from("/tmp/dodo é #?.dodo");
        let uri = file_uri(&path);
        assert_eq!(file_path(&uri), Some(path));
        assert_eq!(
            file_path("file://localhost/tmp/example.dodo"),
            Some(PathBuf::from("/tmp/example.dodo"))
        );
        assert_eq!(file_path("file://other-host/tmp/a.dodo"), None);
        assert_eq!(file_path("file:///tmp/%zz"), None);
        assert_eq!(file_path("untitled:main"), None);
    }

    fn protocol(messages: &[Value]) -> (i32, Vec<Value>) {
        let mut input = Vec::new();
        for message in messages {
            write_message(&mut input, message).unwrap();
        }
        let mut output = Vec::new();
        let status = serve(input.as_slice(), &mut output).unwrap();
        let mut reader = output.as_slice();
        let mut messages = vec![];
        while let Some(body) = read_message(&mut reader).unwrap() {
            messages.push(serde_json::from_slice(&body).unwrap());
        }
        (status, messages)
    }

    #[test]
    fn protocol_lifecycle_unknown_methods_and_unopened_documents() {
        let (status, messages) = protocol(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "textDocument/hover", "params": {}}),
            json!({"jsonrpc": "2.0", "id": "init", "method": "initialize", "params": {}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/hover", "params": {"textDocument": {"uri": "untitled:missing"}, "position": {"line": 0, "character": 0}}}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "unknown"}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "shutdown"}),
            json!({"jsonrpc": "2.0", "method": "exit"}),
        ]);
        assert_eq!(status, 0);
        assert_eq!(messages[0]["error"]["code"], -32002);
        assert_eq!(messages[1]["id"], "init");
        assert_eq!(
            messages[1]["result"]["capabilities"]["positionEncoding"],
            "utf-16"
        );
        assert_eq!(messages[2]["result"], Value::Null);
        assert_eq!(messages[3]["error"]["code"], -32601);
        assert_eq!(messages[4]["result"], Value::Null);
        assert_eq!(
            protocol(&[json!({"jsonrpc": "2.0", "method": "exit"})]).0,
            1
        );
    }

    #[test]
    fn full_sync_rejects_stale_versions_and_incremental_changes() {
        let source = "package test\nfn main() { number := 1u32 }";
        let (_, messages) = protocol(&[
            json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{}}),
            json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{"textDocument":{"uri":"untitled:test", "languageId":"dodo", "version":2, "text":source}}}),
            json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{"textDocument":{"uri":"untitled:test", "version":1}, "contentChanges":[{"text":"broken"}]}}),
            json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{"textDocument":{"uri":"untitled:test", "version":3}, "contentChanges":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}, "text":"broken"}]}}),
            json!({"jsonrpc":"2.0", "id":2, "method":"textDocument/hover", "params":{"textDocument":{"uri":"untitled:test"}, "position":{"line":1,"character":12}}}),
            json!({"jsonrpc":"2.0", "id":3, "method":"shutdown"}),
            json!({"jsonrpc":"2.0", "method":"exit"}),
        ]);
        assert_eq!(messages.len(), 4);
        assert!(
            messages[2]["result"]["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("number: u32")
        );
    }

    #[test]
    fn frame_lengths_count_utf8_bytes_and_incomplete_frames_fail() {
        let mut bytes = vec![];
        let message = json!({"text": "😀"});
        write_message(&mut bytes, &message).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&read_message(&mut bytes.as_slice()).unwrap().unwrap())
                .unwrap(),
            message
        );
        assert!(read_message(&mut &b"Content-Length: 4\r\n\r\n{}"[..]).is_err());
        assert!(read_message(&mut &b"Content-Length: 0\r\nContent-Length: 0\r\n\r\n"[..]).is_err());
    }

    #[test]
    fn invalid_json_rpc_receives_an_error_and_server_keeps_running() {
        let (_, messages) = protocol(&[
            json!([]),
            json!({"jsonrpc": "2.0"}),
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown"}),
            json!({"jsonrpc": "2.0", "method": "exit"}),
        ]);
        assert_eq!(messages[0]["error"]["code"], -32600);
        assert_eq!(messages[1]["error"]["code"], -32600);
        assert_eq!(messages[2]["id"], 1);
    }

    #[test]
    fn imported_buffer_changes_refresh_caller_hovers_without_writing_files() {
        use std::fs;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        struct Workspace(PathBuf);
        impl Drop for Workspace {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let workspace = Workspace(std::env::temp_dir().join(format!(
            "dodo-editor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir(&workspace.0).unwrap();
        let main = "package app\nimport \"views\"\nfn main() {\nvalue := 1i32\nborrowed := views.take(&value)\n}\n";
        let original = "package views\npub fn take(value: &i32) -> &i32 { return value }\n";
        let owned = "package views\npub fn take(value: &i32) -> i32 { return *value }\n";
        let borrowed =
            "package views\npub fn take(value: &i32) -> &i32 from(value) { return value }\n";
        fs::write(workspace.0.join("main.dodo"), main).unwrap();
        fs::write(workspace.0.join("views.dodo"), original).unwrap();
        let root_uri = file_uri(&workspace.0.join("main.dodo"));
        let import_uri = file_uri(&workspace.0.join("views.dodo"));
        let open = |uri: &str, text: &str| json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{"textDocument":{"uri":uri, "languageId":"dodo", "version":1, "text":text}}});
        let hover = |id: i32| json!({"jsonrpc":"2.0", "id":id, "method":"textDocument/hover", "params":{"textDocument":{"uri":root_uri}, "position":{"line":4,"character":0}}});
        let (status, messages) = protocol(&[
            json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{}}),
            open(&root_uri, main),
            hover(2),
            open(&import_uri, owned),
            hover(3),
            json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{"textDocument":{"uri":import_uri, "version":2}, "contentChanges":[{"text":borrowed}]}}),
            hover(4),
            json!({"jsonrpc":"2.0", "method":"textDocument/didClose", "params":{"textDocument":{"uri":import_uri}}}),
            hover(5),
            json!({"jsonrpc":"2.0", "id":6, "method":"shutdown"}),
            json!({"jsonrpc":"2.0", "method":"exit"}),
        ]);
        assert_eq!(status, 0);
        for (id, expected) in [
            (2, "borrowed: &i32"),
            (3, "borrowed: i32"),
            (4, "borrowed: &i32"),
            (5, "borrowed: &i32"),
        ] {
            let reply = messages.iter().find(|message| message["id"] == id).unwrap();
            assert!(
                reply["result"]["contents"]["value"]
                    .as_str()
                    .unwrap()
                    .contains(expected),
                "{reply}"
            );
        }
        assert!(
            messages
                .iter()
                .filter(|message| message["method"] == "textDocument/publishDiagnostics")
                .all(|message| message["params"]["diagnostics"] == json!([]))
        );
        assert_eq!(
            fs::read_to_string(workspace.0.join("views.dodo")).unwrap(),
            original
        );
    }

    #[test]
    fn inferred_generic_call_hover_uses_readable_specialized_types() {
        let document = Document::new("package test\nfn identity<T>(value: T) -> T { return value }\nfn main() { result := identity(1i32) }\n".into());
        assert!(document.diagnostic.is_none(), "{:?}", document.diagnostic);
        assert!(
            hover_at(&document, "result :=")["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("result: i32")
        );
        let hover = hover_at(&document, "identity(1");
        let markdown = hover["contents"]["value"].as_str().unwrap();
        assert!(
            markdown.contains("fn identity<i32>(value: i32) -> i32"),
            "{markdown}"
        );
        assert!(!markdown.contains('$'));
    }
}
