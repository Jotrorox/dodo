//! Recursive-descent declarations and statements with a Pratt expression parser.
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::lexer::{Token, TokenKind, lex};
use std::collections::HashSet;

/// The largest `@align` value, far above any section or MPU region alignment.
const MAX_ALIGNMENT: u64 = 1 << 29;

type ParseResult<T> = Result<T, Diagnostic>;

pub fn parse(source: &str) -> ParseResult<Program> {
    Parser::new(lex(source)?, false).program()
}

/// Parse already-lexed tokens as one complete expression. The formatter uses
/// this for array lengths, which the AST keeps without source spans.
pub(crate) fn parse_expression(tokens: &[Token]) -> Option<Expr> {
    let end = tokens.last()?.span.end;
    let mut tokens: Vec<_> = tokens
        .iter()
        .filter(|token| !matches!(token.kind, TokenKind::Newline | TokenKind::Eof))
        .cloned()
        .collect();
    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span { start: end, end },
    });
    let mut parser = Parser::new(tokens, false);
    let expression = parser.expression(false).ok()?;
    parser.eof().then_some(expression)
}

/// Editor parser: retain sound declarations and statements after syntax errors.
/// Compilation and formatting continue to use the strict `parse` entry point.
pub fn parse_recovering(source: &str) -> (Program, Vec<Diagnostic>) {
    let (tokens, errors) = crate::lexer::lex_recovering(source);
    let mut parser = Parser::new(tokens, true);
    parser.diagnostics = errors;
    let program = match parser.program() {
        Ok(program) => program,
        Err(error) => {
            parser.diagnostics.push(error);
            Program::default()
        }
    };
    (program, parser.diagnostics)
}

impl Parser {
    fn new(tokens: Vec<Token>, recover: bool) -> Self {
        Self {
            tokens,
            recover,
            diagnostics: vec![],
            cursor: 0,
            soft_newlines: 0,
            self_type: None,
            angle_splits: Vec::new(),
            depth: 0,
            slice_first: false,
            discarded_tails: HashSet::new(),
            json_derives: Vec::new(),
            in_interface: false,
            await_operand: false,
        }
    }
}

struct Parser {
    recover: bool,
    diagnostics: Vec<Diagnostic>,
    tokens: Vec<Token>,
    cursor: usize,
    soft_newlines: usize,
    self_type: Option<Type>,
    // A small undo log makes speculative generic parsing linear in the input,
    // without cloning the token stream for every expression statement.
    angle_splits: Vec<(usize, Token)>,
    depth: usize,
    slice_first: bool,
    // Explicit semicolons suppress implicit block values without changing the
    // statement AST used by semantic analysis and the backend.
    discarded_tails: HashSet<usize>,
    json_derives: Vec<crate::json_derive::Derive>,
    // Interface methods are signatures without bodies.
    in_interface: bool,
    // Parsing the operand of `await`: a trailing `?` or `!` applies to the
    // awaited result, not to the future.
    await_operand: bool,
}

#[derive(Default)]
struct Modifiers {
    printing: Option<Printing>,
    // `@compiler(pio)`: rp_pio's compile-time assembler.
    pio: bool,
    test: bool,
    ignore: Option<String>,
    public: bool,
    unsafe_: bool,
    extern_: bool,
    repr_c: bool,
    unsafe_send: bool,
    unsafe_sync: bool,
    derive_json: bool,
    json_name: Option<String>,
    json_deny_unknown: bool,
    interrupt: Option<String>,
    // `@link_name("symbol")`: the C symbol an extern declaration binds.
    link_name: Option<String>,
    placement: Placement,
    inlining: Inlining,
    // `async fn`: a function whose body may `await`; see std/embedded/executor.
    async_: bool,
}

impl Modifiers {
    fn compiler(&self) -> bool {
        self.printing.is_some() || self.pio
    }
}

impl Parser {
    fn nested<T>(&mut self, operation: impl FnOnce(&mut Self) -> ParseResult<T>) -> ParseResult<T> {
        if self.depth >= 64 {
            return Err(self.error("syntax nesting exceeds the supported limit of 64 levels"));
        }
        self.depth += 1;
        let result = operation(self);
        self.depth -= 1;
        result
    }
    fn token(&self) -> &Token {
        &self.tokens[self.cursor.min(self.tokens.len() - 1)]
    }
    fn span(&self) -> Span {
        self.token().span
    }
    fn previous_start(&self) -> usize {
        self.tokens[self.cursor.saturating_sub(1)].span.start
    }
    fn previous_end(&self) -> usize {
        self.tokens[self.cursor.saturating_sub(1)].span.end
    }
    fn since(&self, start: usize) -> Span {
        Span {
            start,
            end: self.previous_end(),
        }
    }
    fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(self.span(), message)
    }
    fn at(&self, text: &str) -> bool {
        matches!(&self.token().kind, TokenKind::Symbol(s) if *s == text)
            || matches!(&self.token().kind, TokenKind::Ident(s) if s == text)
    }
    fn look(&self, distance: usize, text: &str) -> bool {
        self.tokens
            .get(self.cursor + distance)
            .is_some_and(|token| {
                matches!(&token.kind, TokenKind::Symbol(s) if *s == text)
                    || matches!(&token.kind, TokenKind::Ident(s) if s == text)
            })
    }
    fn bump(&mut self) -> Token {
        let token = self.token().clone();
        if !matches!(token.kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        token
    }
    fn eat(&mut self, text: &str) -> bool {
        if self.at(text) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, text: &str) -> ParseResult<()> {
        if self.eat(text) {
            Ok(())
        } else {
            Err(self.error(format!("expected `{text}`")))
        }
    }
    fn eof(&self) -> bool {
        matches!(self.token().kind, TokenKind::Eof)
    }
    fn newline(&self) -> bool {
        matches!(self.token().kind, TokenKind::Newline)
    }
    fn newlines(&mut self) {
        while self.newline() {
            self.bump();
        }
    }
    fn separators(&mut self) {
        while self.newline() || self.at(";") {
            self.bump();
        }
    }
    fn following_semicolon(&self) -> bool {
        self.tokens[self.cursor..]
            .iter()
            .take_while(|token| matches!(token.kind, TokenKind::Newline | TokenKind::Symbol(";")))
            .any(|token| matches!(token.kind, TokenKind::Symbol(";")))
    }
    fn end_statement(&mut self) -> ParseResult<()> {
        if self.newline() || self.at(";") {
            self.separators();
            Ok(())
        } else if self.at("}") || self.eof() {
            Ok(())
        } else {
            Err(self.error("expected a newline or `;` after the statement"))
        }
    }
    fn identifier(&mut self) -> ParseResult<String> {
        match &self.token().kind {
            TokenKind::Ident(name) if !reserved(name) => {
                let name = name.clone();
                self.bump();
                Ok(name)
            }
            _ => Err(self.error("expected an identifier")),
        }
    }
    fn qualified_name(&mut self) -> ParseResult<String> {
        let mut name = self.identifier()?;
        while self.eat(".") {
            name.push('.');
            name.push_str(&self.identifier()?);
        }
        Ok(name)
    }
    fn program(&mut self) -> ParseResult<Program> {
        self.separators();
        self.expect("package")?;
        let mut program = Program {
            package: self.identifier()?,
            ..Program::default()
        };
        self.end_statement()?;
        while !self.eof() {
            self.separators();
            if self.eof() {
                break;
            }
            let cursor = self.cursor;
            let soft = self.soft_newlines;
            let self_type = self.self_type.clone();
            let lengths = (
                program.functions.len(),
                program.structs.len(),
                program.enums.len(),
                program.constants.len(),
                program.imports.len(),
                program.import_aliases.len(),
                program.interfaces.len(),
                program.aliases.len(),
            );
            if let Err(error) = self.declaration(&mut program) {
                if !self.recover {
                    return Err(error);
                }
                self.diagnostics.push(error);
                // Struct parsing can append methods before reaching a bad field.
                // Discard that incomplete declaration as one recovery unit.
                program.functions.truncate(lengths.0);
                program.structs.truncate(lengths.1);
                program.enums.truncate(lengths.2);
                program.constants.truncate(lengths.3);
                program.imports.truncate(lengths.4);
                program.import_aliases.truncate(lengths.5);
                program.interfaces.truncate(lengths.6);
                program.aliases.truncate(lengths.7);
                self.soft_newlines = soft;
                self.self_type = self_type;
                self.in_interface = false;
                self.synchronize(cursor, true);
            }
        }
        let derives = std::mem::take(&mut self.json_derives);
        if !derives.is_empty() {
            let identifiers = self
                .tokens
                .iter()
                .filter_map(|token| match &token.kind {
                    TokenKind::Ident(name) => Some(name.as_str()),
                    _ => None,
                })
                .collect();
            if let Err(error) = crate::json_derive::expand(&mut program, derives, &identifiers) {
                if !self.recover {
                    return Err(error);
                }
                self.diagnostics.push(error);
            }
        }
        if program
            .functions
            .iter()
            .chain(program.interfaces.iter().flat_map(|i| &i.methods))
            .any(|f| f.async_part.is_some())
        {
            let identifiers = self
                .tokens
                .iter()
                .filter_map(|token| match &token.kind {
                    TokenKind::Ident(name) => Some(name.as_str()),
                    _ => None,
                })
                .collect();
            if let Err(error) = crate::async_desugar::expand(&mut program, &identifiers) {
                if !self.recover {
                    return Err(error);
                }
                self.diagnostics.push(error);
            }
        }
        Ok(program)
    }
    fn declaration(&mut self, program: &mut Program) -> ParseResult<()> {
        if self.eat("import") {
            match self.bump() {
                Token {
                    kind: TokenKind::String(bytes, false),
                    span,
                } => {
                    let path = String::from_utf8(bytes)
                        .map_err(|_| Diagnostic::new(span, "import path must be UTF-8"))?;
                    if path.is_empty() {
                        return Err(Diagnostic::new(span, "import path cannot be empty"));
                    }
                    if self.eat("as") {
                        program
                            .import_aliases
                            .push((path.clone(), self.identifier()?));
                    }
                    program.imports.push(path);
                }
                token => {
                    return Err(Diagnostic::new(token.span, "expected a string import path"));
                }
            }
            self.end_statement()?;
            return Ok(());
        }
        let start = self.span().start;
        let mods = self.modifiers()?;
        if mods.interrupt.is_some() && !self.at("fn") {
            return Err(self.error("@interrupt applies only to functions"));
        }
        if mods.async_ && !self.at("fn") {
            return Err(self.error("`async` applies only to functions"));
        }
        if mods.link_name.is_some() && !self.at("fn") {
            return Err(self.error("@link_name applies only to extern \"C\" functions"));
        }
        if mods.inlining != Inlining::Auto && !self.at("fn") {
            return Err(self.error("@inline and @cold apply only to functions"));
        }
        if !mods.placement.is_empty() && !self.at("fn") && !self.at("static") && !self.at("const") {
            return Err(self.error("@section and @align apply only to functions and statics"));
        }
        if self.at("interface") {
            if mods.unsafe_
                || mods.extern_
                || mods.repr_c
                || mods.unsafe_send
                || mods.unsafe_sync
                || mods.derive_json
                || mods.json_deny_unknown
                || mods.json_name.is_some()
                || mods.compiler()
                || mods.test
                || mods.ignore.is_some()
            {
                return Err(self.error("interface declarations only support the `pub` modifier"));
            }
            program
                .interfaces
                .push(self.interface_decl(mods.public, start)?);
            return Ok(());
        }
        if self.at("type") {
            if mods.unsafe_
                || mods.extern_
                || mods.repr_c
                || mods.unsafe_send
                || mods.unsafe_sync
                || mods.derive_json
                || mods.json_deny_unknown
                || mods.json_name.is_some()
                || mods.compiler()
                || mods.test
                || mods.ignore.is_some()
            {
                return Err(self.error("type aliases only support the `pub` modifier"));
            }
            program.aliases.push(self.type_alias(mods.public, start)?);
            return Ok(());
        }
        if (mods.derive_json || mods.json_deny_unknown) && !self.at("struct") {
            return Err(self.error("@derive(Json) and @json_deny_unknown apply only to structs"));
        }
        if mods.json_name.is_some() {
            return Err(self.error("@json_name applies only to fields of @derive(Json) structs"));
        }
        if mods.compiler() && !self.at("fn") {
            return Err(self.error("@compiler applies only to functions"));
        }
        if (mods.test || mods.ignore.is_some()) && !self.at("fn") {
            return Err(self.error("@test and @ignore apply only to functions"));
        }
        if self.at("struct") {
            if mods.extern_ {
                return Err(self.error("structs cannot be extern"));
            }
            // Thread contracts are unchecked promises, so they need the same
            // explicit opt-in as any other unsafe code.
            let contract = mods.unsafe_send || mods.unsafe_sync;
            if contract && !mods.unsafe_ {
                return Err(self.error(
                    "@unsafe_send and @unsafe_sync are unsafe contracts; declare the struct as `unsafe struct`",
                ));
            }
            if mods.unsafe_ && !contract {
                return Err(self
                    .error("`unsafe` on a struct applies only with @unsafe_send or @unsafe_sync"));
            }
            self.struct_decl(program, mods, start)?;
        } else if self.at("enum") {
            if mods.unsafe_ || mods.extern_ || mods.repr_c || mods.unsafe_send || mods.unsafe_sync {
                return Err(self.error("enum declarations only support the `pub` modifier"));
            }
            program.enums.push(self.enum_decl(mods.public, start)?);
        } else if self.at("fn") {
            if mods.repr_c || mods.unsafe_send || mods.unsafe_sync {
                return Err(self.error("representation and thread contracts apply to structs"));
            }
            program.functions.push(self.function(mods, start)?);
        } else if self.at("const") || self.at("static") {
            if mods.unsafe_ || mods.extern_ || mods.repr_c || mods.unsafe_send || mods.unsafe_sync {
                return Err(self.error("constant and static declarations only support `pub`"));
            }
            if !mods.placement.is_empty() && !self.at("static") {
                return Err(self.error(
                    "@section and @align apply only to statics; a `const` has no storage of its own",
                ));
            }
            program
                .constants
                .push(self.constant(mods.public, mods.placement, start)?);
        } else {
            return Err(self.error(
                "expected `fn`, `struct`, `enum`, `interface`, `type`, `const`, or `static` declaration",
            ));
        }
        Ok(())
    }

    // Resume at a statement separator or the next declaration. Always advance
    // after an error, while retaining a closing brace for the enclosing block.
    fn synchronize(&mut self, cursor: usize, declaration: bool) {
        if self.cursor == cursor && !self.eof() && (declaration || !self.at("}")) {
            self.bump();
        }
        let mut braces = 0usize;
        while !self.eof() {
            if declaration
                && (self.at("fn")
                    || self.at("@")
                    || self.at("pub")
                    || self.at("struct")
                    || self.at("enum")
                    || self.at("interface")
                    || self.at("type")
                    || self.at("const")
                    || self.at("static")
                    || self.at("import"))
            {
                break;
            }
            if !declaration && braces == 0 && (self.newline() || self.at(";") || self.at("}")) {
                break;
            }
            if self.at("{") {
                braces += 1;
            }
            if self.at("}") {
                braces = braces.saturating_sub(1);
            }
            self.bump();
        }
    }
    fn modifiers(&mut self) -> ParseResult<Modifiers> {
        let mut mods = Modifiers::default();
        loop {
            if self.eat("pub") {
                if mods.public {
                    return Err(self.error("duplicate `pub` modifier"));
                }
                mods.public = true;
            } else if self.eat("unsafe") {
                if mods.unsafe_ {
                    return Err(self.error("duplicate `unsafe` modifier"));
                }
                mods.unsafe_ = true;
            } else if self.eat("async") {
                if mods.async_ {
                    return Err(self.error("duplicate `async` modifier"));
                }
                mods.async_ = true;
            } else if self.eat("extern") {
                if mods.extern_ {
                    return Err(self.error("duplicate `extern` modifier"));
                }
                match self.bump() {
                    Token {
                        kind: TokenKind::String(abi, false),
                        ..
                    } if abi == b"C" => mods.extern_ = true,
                    token => {
                        return Err(Diagnostic::new(
                            token.span,
                            "only the `extern \"C\"` ABI is supported",
                        ));
                    }
                }
            } else if self.eat("@") {
                let name = self.identifier()?;
                if name == "derive" {
                    if mods.derive_json {
                        return Err(self.error("duplicate @derive(Json) attribute"));
                    }
                    self.expect("(")?;
                    if !self.eat("Json") {
                        return Err(self.error("supported derive: @derive(Json)"));
                    }
                    self.expect(")")?;
                    mods.derive_json = true;
                    self.newlines();
                    continue;
                }
                if name == "json_deny_unknown" {
                    if mods.json_deny_unknown {
                        return Err(self.error("duplicate @json_deny_unknown attribute"));
                    }
                    mods.json_deny_unknown = true;
                    self.newlines();
                    continue;
                }
                if name == "json_name" {
                    if mods.json_name.is_some() {
                        return Err(self.error("duplicate @json_name attribute"));
                    }
                    self.expect("(")?;
                    let TokenKind::String(value, false) = self.bump().kind else {
                        return Err(self.error("@json_name expects a string field name"));
                    };
                    mods.json_name = Some(
                        String::from_utf8(value)
                            .map_err(|_| self.error("JSON field name must be UTF-8"))?,
                    );
                    self.expect(")")?;
                    self.newlines();
                    continue;
                }
                if name == "compiler" {
                    if mods.compiler() {
                        return Err(self.error("duplicate @compiler attribute"));
                    }
                    self.expect("(")?;
                    match self.identifier()?.as_str() {
                        "print" => mods.printing = Some(Printing::Print),
                        "println" => mods.printing = Some(Printing::Println),
                        "printf" => mods.printing = Some(Printing::Printf),
                        "pio" => mods.pio = true,
                        _ => return Err(self.error("expected print, println, printf, or pio")),
                    }
                    self.expect(")")?;
                    self.newlines();
                    continue;
                }
                if name == "interrupt" {
                    if mods.interrupt.is_some() {
                        return Err(self.error("duplicate @interrupt attribute"));
                    }
                    self.expect("(")?;
                    mods.interrupt = Some(self.identifier().map_err(|_| {
                        self.error(
                            "@interrupt expects a vector name, such as @interrupt(TIMER_IRQ_0)",
                        )
                    })?);
                    self.expect(")")?;
                    self.newlines();
                    continue;
                }
                if name == "link_name" {
                    if mods.link_name.is_some() {
                        return Err(self.error("duplicate @link_name attribute"));
                    }
                    self.expect("(")?;
                    let token = self.bump();
                    let TokenKind::String(value, false) = token.kind else {
                        return Err(Diagnostic::new(
                            token.span,
                            "@link_name expects a string symbol name, such as @link_name(\"objc_msgSend\")",
                        ));
                    };
                    let symbol = String::from_utf8(value)
                        .ok()
                        .filter(|name| {
                            !name.is_empty()
                                && name.bytes().all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
                        })
                        .ok_or_else(|| {
                            Diagnostic::new(
                                token.span,
                                "a link name must be nonempty printable ASCII without spaces, quotes, or backslashes",
                            )
                        })?;
                    mods.link_name = Some(symbol);
                    self.expect(")")?;
                    self.newlines();
                    continue;
                }
                if name == "inline" || name == "cold" {
                    if mods.inlining != Inlining::Auto {
                        return Err(self.error(
                            "duplicate inlining attribute; use one of @inline, @inline(never), or @cold",
                        ));
                    }
                    mods.inlining = if name == "cold" {
                        Inlining::Cold
                    } else if self.eat("(") {
                        if self.identifier().ok().as_deref() != Some("never") {
                            return Err(self.error("expected @inline or @inline(never)"));
                        }
                        self.expect(")")?;
                        Inlining::Never
                    } else {
                        Inlining::Always
                    };
                    self.newlines();
                    continue;
                }
                if name == "section" {
                    if mods.placement.section.is_some() {
                        return Err(self.error("duplicate @section attribute"));
                    }
                    self.expect("(")?;
                    let token = self.bump();
                    let TokenKind::String(value, false) = token.kind else {
                        return Err(Diagnostic::new(
                            token.span,
                            "@section expects a string section name, such as @section(\".ramfunc\")",
                        ));
                    };
                    let section = String::from_utf8(value)
                        .ok()
                        .filter(|name| {
                            !name.is_empty()
                                && name.bytes().all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
                        })
                        .ok_or_else(|| {
                            Diagnostic::new(
                                token.span,
                                "a section name must be nonempty printable ASCII without spaces, quotes, or backslashes",
                            )
                        })?;
                    mods.placement.section = Some(section);
                    self.expect(")")?;
                    self.newlines();
                    continue;
                }
                if name == "align" {
                    if mods.placement.align.is_some() {
                        return Err(self.error("duplicate @align attribute"));
                    }
                    self.expect("(")?;
                    let token = self.bump();
                    let alignment = match token.kind {
                        TokenKind::Int(value, None) => value,
                        _ => {
                            return Err(Diagnostic::new(
                                token.span,
                                "@align expects an integer byte count, such as @align(256)",
                            ));
                        }
                    };
                    if !alignment.is_power_of_two() || alignment > MAX_ALIGNMENT {
                        return Err(Diagnostic::new(
                            token.span,
                            format!(
                                "alignment must be a power of two from 1 to {MAX_ALIGNMENT} bytes"
                            ),
                        ));
                    }
                    mods.placement.align = Some(alignment as u32);
                    self.expect(")")?;
                    self.newlines();
                    continue;
                }
                if name == "test" {
                    if mods.test {
                        return Err(self.error("duplicate @test attribute"));
                    }
                    mods.test = true;
                    self.newlines();
                    continue;
                }
                if name == "ignore" {
                    if mods.ignore.is_some() {
                        return Err(self.error("duplicate @ignore attribute"));
                    }
                    self.expect("(")?;
                    let TokenKind::String(reason, false) = self.bump().kind else {
                        return Err(self.error("@ignore expects a string reason"));
                    };
                    mods.ignore = Some(
                        String::from_utf8(reason)
                            .map_err(|_| self.error("ignore reason must be UTF-8"))?,
                    );
                    self.expect(")")?;
                    self.newlines();
                    continue;
                }
                if matches!(name.as_str(), "unsafe_send" | "unsafe_sync") {
                    let present = if name == "unsafe_send" {
                        &mut mods.unsafe_send
                    } else {
                        &mut mods.unsafe_sync
                    };
                    if *present {
                        return Err(self.error(format!("duplicate @{name} contract")));
                    }
                    *present = true;
                    self.newlines();
                    continue;
                }
                if name != "repr" {
                    return Err(self.error(format!(
                        "unknown attribute `@{name}`; supported: @repr(C), @unsafe_send, @unsafe_sync, @test, @ignore(\"reason\"), @derive(Json), @json_name(\"name\"), @json_deny_unknown, @interrupt(VECTOR), @link_name(\"symbol\"), @section(\"name\"), @align(N), @inline, @inline(never), @cold"
                    )));
                }
                if mods.repr_c {
                    return Err(self.error("duplicate @repr(C) attribute"));
                }
                self.expect("(")?;
                self.expect("C")?;
                self.expect(")")?;
                mods.repr_c = true;
                self.newlines();
            } else {
                break;
            }
        }
        Ok(mods)
    }
    fn generic_params(&mut self) -> ParseResult<(Vec<String>, Vec<Bound>)> {
        let mut bounds = Vec::new();
        if !self.eat("<") {
            return Ok((Vec::new(), bounds));
        }
        self.newlines();
        let mut params = Vec::new();
        loop {
            let start = self.span().start;
            let name = self.identifier()?;
            if params.contains(&name) {
                return Err(self.error(format!("duplicate generic parameter `{name}`")));
            }
            params.push(name.clone());
            self.newlines();
            if self.eat(":") {
                self.newlines();
                let interfaces = self.interface_list()?;
                bounds.push(Bound {
                    param: name,
                    interfaces,
                    span: self.since(start),
                });
                self.newlines();
            }
            if self.eat(",") {
                self.newlines();
                if self.at(">") {
                    break;
                }
            } else {
                break;
            }
        }
        self.close_angle()?;
        Ok((params, bounds))
    }
    // `A + B + ...`, each a possibly package-qualified interface name.
    fn interface_list(&mut self) -> ParseResult<Vec<String>> {
        let mut names = vec![self.qualified_name()?];
        while self.eat("+") {
            self.newlines();
            let name = self.qualified_name()?;
            if names.contains(&name) {
                return Err(self.error(format!("interface `{name}` is listed twice")));
            }
            names.push(name);
        }
        Ok(names)
    }
    fn interface_decl(&mut self, public: bool, start: usize) -> ParseResult<Interface> {
        self.expect("interface")?;
        let name = self.identifier()?;
        if self.at("<") {
            return Err(self.error("interfaces cannot have type parameters"));
        }
        let parents = if self.eat(":") {
            self.newlines();
            self.interface_list()?
        } else {
            Vec::new()
        };
        self.newlines();
        self.expect("{")?;
        self.separators();
        self.self_type = Some(Type::Named("Self".into()));
        self.in_interface = true;
        let mut methods = Vec::new();
        while !self.eat("}") {
            if self.eof() {
                return Err(self.error("unclosed interface declaration; expected `}`"));
            }
            let method_start = self.span().start;
            let mods = self.modifiers()?;
            if mods.public
                || mods.extern_
                || mods.repr_c
                || mods.unsafe_send
                || mods.unsafe_sync
                || mods.derive_json
                || mods.json_deny_unknown
                || mods.json_name.is_some()
                || mods.compiler()
                || mods.test
                || mods.ignore.is_some()
            {
                return Err(self.error(
                    "interface methods only support the `unsafe` modifier; they are public with their interface",
                ));
            }
            if !self.at("fn") {
                return Err(self.error("an interface contains only `fn` signatures"));
            }
            let method = self.function(mods, method_start)?;
            if !method.generics.is_empty() {
                return Err(Diagnostic::new(
                    method.span,
                    "interface methods cannot have type parameters",
                ));
            }
            if !method.stores.is_empty() || !method.requires_plain.is_empty() {
                return Err(Diagnostic::new(
                    method.span,
                    "interface methods cannot declare `stores(...)` or `requires_plain(...)`",
                ));
            }
            if methods.iter().any(|m: &Function| m.name == method.name) {
                return Err(Diagnostic::new(
                    method.span,
                    format!("duplicate interface method `{}`", method.name),
                ));
            }
            methods.push(method);
            self.separators();
        }
        self.self_type = None;
        self.in_interface = false;
        Ok(Interface {
            name,
            public,
            parents,
            methods,
            span: self.since(start),
        })
    }
    fn close_angle(&mut self) -> ParseResult<()> {
        if self.eat(">") {
            return Ok(());
        }
        // Keep the rest of the token available: the second `>` for an enclosing
        // generic argument list, or the `=` of `type Pair<T>=(T, T)`.
        let rest = match &self.token().kind {
            TokenKind::Symbol(">>") => ">",
            TokenKind::Symbol(">=") => "=",
            TokenKind::Symbol(">>=") => ">=",
            _ => return Err(self.error("expected `>` after generic arguments")),
        };
        self.angle_splits.push((self.cursor, self.token().clone()));
        self.tokens[self.cursor].kind = TokenKind::Symbol(rest);
        self.tokens[self.cursor].span.start += 1;
        Ok(())
    }
    fn restore_angles(&mut self, checkpoint: usize) {
        while self.angle_splits.len() > checkpoint {
            if let Some((index, original)) = self.angle_splits.pop() {
                self.tokens[index] = original;
            }
        }
    }
    fn type_args(&mut self) -> ParseResult<Vec<Type>> {
        self.expect("<")?;
        self.newlines();
        let mut types = vec![self.ty()?];
        self.newlines();
        while self.eat(",") {
            self.newlines();
            if self.at(">") || self.at(">>") {
                break;
            }
            types.push(self.ty()?);
            self.newlines();
        }
        self.close_angle()?;
        Ok(types)
    }
    fn ty(&mut self) -> ParseResult<Type> {
        self.nested(Self::type_result)
    }
    fn type_result(&mut self) -> ParseResult<Type> {
        let ty = self.type_atom()?;
        if self.eat("!") {
            Ok(Type::Result(Box::new(ty), Box::new(self.ty()?)))
        } else {
            Ok(ty)
        }
    }
    fn type_atom(&mut self) -> ParseResult<Type> {
        self.nested(Self::type_atom_inner)
    }
    fn type_atom_inner(&mut self) -> ParseResult<Type> {
        if self.eat("&") {
            let mutable = self.eat("mut");
            if !mutable && self.eat("str") {
                return Ok(Type::Str);
            }
            if self.at("[") {
                let saved = self.cursor;
                let angles = self.angle_splits.len();
                if self.slice_first {
                    let slice: ParseResult<Type> = (|| {
                        self.expect("[")?;
                        self.newlines();
                        let inner = self.ty()?;
                        self.newlines();
                        self.expect("]")?;
                        Ok(Type::Slice(mutable, Box::new(inner)))
                    })();
                    if let Ok(slice) = slice {
                        return Ok(slice);
                    }
                    self.cursor = saved;
                    self.restore_angles(angles);
                }
                if let Ok(array) = self.type_atom() {
                    return Ok(Type::Ref(mutable, Box::new(array)));
                }
                self.cursor = saved;
                self.restore_angles(angles);
                self.expect("[")?;
                self.newlines();
                let inner = self.ty()?;
                self.newlines();
                self.expect("]")?;
                return Ok(Type::Slice(mutable, Box::new(inner)));
            }
            return Ok(Type::Ref(mutable, Box::new(self.type_atom()?)));
        }
        if self.eat("*") {
            let mutable = if self.eat("mut") {
                true
            } else {
                self.expect("const")?;
                false
            };
            return Ok(Type::Raw(mutable, Box::new(self.type_atom()?)));
        }
        if self.eat("[") {
            self.newlines();
            let size = self.expression(false)?;
            self.newlines();
            self.expect("]")?;
            let element = Box::new(self.type_atom()?);
            return Ok(match size.kind {
                ExprKind::Int(n, None) => Type::Array(
                    usize::try_from(n).map_err(|_| {
                        self.error("array length exceeds the compiler host's address space")
                    })?,
                    element,
                ),
                _ => Type::ArrayExpr(Box::new(LengthExpr::from_expr(size)?), element),
            });
        }
        if self.eat("(") {
            // `(T)` groups a type; two or more comma-separated types form a tuple.
            self.newlines();
            if self.at(")") {
                return Err(
                    self.error("tuples need at least two elements; use `void` for no value")
                );
            }
            let mut elements = vec![self.ty()?];
            let mut trailing = false;
            self.newlines();
            while self.eat(",") {
                self.newlines();
                trailing = true;
                if self.at(")") {
                    break;
                }
                elements.push(self.ty()?);
                trailing = false;
                self.newlines();
            }
            self.expect(")")?;
            return match elements.len() {
                1 if trailing => Err(self.error("tuples need at least two elements")),
                1 => Ok(elements.remove(0)),
                n => Ok(Type::Generic(tuple_name(n), elements)),
            };
        }
        if self.eat("void") {
            return Ok(Type::Void);
        }
        let mut foreign = false;
        if self.at("extern") {
            let start = self.span();
            self.bump();
            match self.bump() {
                Token {
                    kind: TokenKind::String(abi, false),
                    ..
                } if abi == b"C" => {}
                token => {
                    return Err(Diagnostic::new(
                        token.span,
                        "only the `extern \"C\"` ABI is supported",
                    ));
                }
            }
            if !self.at("fn") {
                return Err(Diagnostic::new(
                    start,
                    "expected `fn` after `extern \"C\"` in a type",
                ));
            }
            foreign = true;
        }
        let callable = self.at("dyn") && self.look(1, "fn");
        if !callable && self.eat("dyn") {
            return Ok(Type::Dyn(self.qualified_name()?));
        }
        if callable {
            self.bump();
        }
        if self.eat("fn") {
            self.expect("(")?;
            self.newlines();
            let mut params = Vec::new();
            while !self.eat(")") {
                params.push(self.ty()?);
                self.newlines();
                if self.eat(",") {
                    self.newlines();
                } else {
                    self.expect(")")?;
                    break;
                }
            }
            let ret = if self.eat("->") {
                self.newlines();
                self.ty()?
            } else {
                Type::Void
            };
            return Ok(if callable {
                Type::DynFn(params, Box::new(ret))
            } else if foreign {
                Type::ExternFn(params, Box::new(ret))
            } else {
                Type::Fn(params, Box::new(ret))
            });
        }
        let start = self.span();
        let name = self.qualified_name()?;
        if name == "Self" {
            return self.self_type.clone().ok_or_else(|| {
                Diagnostic::new(start, "`Self` is only valid inside a struct declaration")
            });
        }
        let primitive = match name.as_str() {
            "bool" => Some(Type::Bool),
            "isize" => Some(Type::isize()),
            "usize" => Some(Type::usize()),
            "u8" => Some(Type::u8()),
            "i8" => Some(Type::Int {
                signed: true,
                bits: 8,
            }),
            "u16" => Some(Type::Int {
                signed: false,
                bits: 16,
            }),
            "i16" => Some(Type::Int {
                signed: true,
                bits: 16,
            }),
            "u32" => Some(Type::Int {
                signed: false,
                bits: 32,
            }),
            "i32" => Some(Type::Int {
                signed: true,
                bits: 32,
            }),
            "u64" => Some(Type::Int {
                signed: false,
                bits: 64,
            }),
            "i64" => Some(Type::Int {
                signed: true,
                bits: 64,
            }),
            "f32" => Some(Type::Float(32)),
            "f64" => Some(Type::Float(64)),
            _ => None,
        };
        if let Some(primitive) = primitive {
            return Ok(primitive);
        }
        if self.at("<") {
            let args = self.type_args()?;
            return match name.as_str() {
                "Result" if args.len() == 2 => Ok(Type::Result(
                    Box::new(args[0].clone()),
                    Box::new(args[1].clone()),
                )),
                "Option" if args.len() == 1 => Ok(Type::Option(Box::new(args[0].clone()))),
                "MaybeUninit" if args.len() == 1 => {
                    Ok(Type::MaybeUninit(Box::new(args[0].clone())))
                }
                "Result" => Err(Diagnostic::new(start, "Result requires two type arguments")),
                "Option" => Err(Diagnostic::new(start, "Option requires one type argument")),
                "MaybeUninit" => Err(Diagnostic::new(
                    start,
                    "MaybeUninit requires one type argument",
                )),
                _ => Ok(Type::Generic(name, args)),
            };
        }
        if name == "str" {
            return Err(Diagnostic::new(
                start,
                "strings use the borrowed type `&str`",
            ));
        }
        Ok(Type::Named(name))
    }
    fn type_alias(&mut self, public: bool, start: usize) -> ParseResult<TypeAlias> {
        self.expect("type")?;
        let name = self.identifier()?;
        let (generics, bounds) = self.generic_params()?;
        if let Some(bound) = bounds.first() {
            return Err(Diagnostic::new(
                bound.span,
                "type alias parameters cannot have interface bounds",
            ));
        }
        self.expect("=")?;
        self.newlines();
        let ty = self.ty()?;
        self.end_statement()?;
        Ok(TypeAlias {
            name,
            public,
            generics,
            ty,
            span: self.since(start),
        })
    }
    fn struct_decl(
        &mut self,
        program: &mut Program,
        mods: Modifiers,
        start: usize,
    ) -> ParseResult<()> {
        self.expect("struct")?;
        if mods.json_deny_unknown && !mods.derive_json {
            return Err(self.error("@json_deny_unknown requires @derive(Json)"));
        }
        let name = self.identifier()?;
        let (generics, bounds) = self.generic_params()?;
        let mut implements = Vec::new();
        if self.eat(":") {
            self.newlines();
            let start = self.span().start;
            for interface in self.interface_list()? {
                implements.push((interface, self.since(start)));
            }
        }
        self.self_type = Some(if generics.is_empty() {
            Type::Named(name.clone())
        } else {
            Type::Generic(
                name.clone(),
                generics.iter().cloned().map(Type::Named).collect(),
            )
        });
        self.newlines();
        self.expect("{")?;
        self.separators();
        let mut fields = Vec::new();
        let mut json_names = Vec::new();
        while !self.eat("}") {
            if self.eof() {
                return Err(self.error("unclosed struct declaration; expected `}`"));
            }
            let field_start = self.span().start;
            let field_mods = self.modifiers()?;
            if field_mods.derive_json || field_mods.json_deny_unknown {
                return Err(
                    self.error("@derive(Json) and @json_deny_unknown apply only to structs")
                );
            }
            if field_mods.json_name.is_some() && (!mods.derive_json || self.at("fn")) {
                return Err(
                    self.error("@json_name applies only to fields of @derive(Json) structs")
                );
            }
            if field_mods.compiler() && !self.at("fn") {
                return Err(self.error("@compiler applies only to functions"));
            }
            if field_mods.test || field_mods.ignore.is_some() {
                return Err(self.error("@test and @ignore apply only to top-level functions"));
            }
            if field_mods.interrupt.is_some() {
                return Err(self.error("@interrupt applies only to top-level functions"));
            }
            if field_mods.inlining != Inlining::Auto && !self.at("fn") {
                return Err(self.error("@inline and @cold apply only to functions"));
            }
            if field_mods.async_ && !self.at("fn") {
                return Err(self.error("`async` applies only to functions"));
            }
            if !field_mods.placement.is_empty() && !self.at("fn") {
                return Err(self.error("@section and @align apply only to functions and statics"));
            }
            if self.at("fn") {
                if field_mods.repr_c || field_mods.unsafe_send || field_mods.unsafe_sync {
                    return Err(self.error("representation and thread contracts apply to structs"));
                }
                let mut function = self.function(field_mods, field_start)?;
                function.name = format!("{name}.{}", function.name);
                let mut all_generics = generics.clone();
                for generic in &function.generics {
                    if all_generics.contains(generic) {
                        return Err(self.error(format!(
                            "method generic parameter `{generic}` shadows its struct parameter"
                        )));
                    }
                    all_generics.push(generic.clone());
                }
                function.generics = all_generics;
                let mut all_bounds = bounds.clone();
                all_bounds.append(&mut function.bounds);
                function.bounds = all_bounds;
                program.functions.push(function);
            } else {
                if field_mods.unsafe_
                    || field_mods.extern_
                    || field_mods.repr_c
                    || field_mods.unsafe_send
                    || field_mods.unsafe_sync
                {
                    return Err(self.error("struct fields only support the `pub` modifier"));
                }
                let field_start = if field_mods.json_name.is_some() {
                    // Attribute text is preserved by the formatter, independently
                    // from the field declaration that follows it.
                    self.span().start
                } else {
                    field_start
                };
                let (field_name, ty) = self.typed_name()?;
                json_names.push(field_mods.json_name.unwrap_or_else(|| field_name.clone()));
                fields.push(Field {
                    name: field_name,
                    ty,
                    public: field_mods.public,
                    span: self.since(field_start),
                });
                if self.eat(",") {
                    self.separators();
                } else {
                    self.end_statement()?;
                }
            }
            self.separators();
        }
        self.self_type = None;
        if mods.derive_json {
            self.json_derives.push(crate::json_derive::Derive {
                name: name.clone(),
                json_names,
                deny_unknown: mods.json_deny_unknown,
            });
        }
        program.structs.push(Struct {
            name,
            public: mods.public,
            generics,
            bounds,
            implements,
            fields,
            span: self.since(start),
            repr_c: mods.repr_c,
            unsafe_send: mods.unsafe_send,
            unsafe_sync: mods.unsafe_sync,
            interior_mutable: false,
            async_future: false,
        });
        Ok(())
    }
    fn enum_decl(&mut self, public: bool, start: usize) -> ParseResult<Enum> {
        self.expect("enum")?;
        let name = self.identifier()?;
        let (generics, bounds) = self.generic_params()?;
        self.newlines();
        self.expect("{")?;
        self.newlines();
        let mut variants = Vec::new();
        while !self.eat("}") {
            if self.eof() {
                return Err(self.error("unclosed enum declaration; expected `}`"));
            }
            let variant_start = self.span().start;
            let variant_name = self.identifier()?;
            let mut fields = Vec::new();
            if self.eat("(") {
                self.newlines();
                while !self.eat(")") {
                    let field_start = self.span().start;
                    let (field_name, ty) = if self.look(1, ":") {
                        let name = self.identifier()?;
                        self.expect(":")?;
                        (name, self.ty()?)
                    } else {
                        let saved = self.cursor;
                        let angles = self.angle_splits.len();
                        if let Ok(named) = self.typed_name() {
                            named
                        } else {
                            self.cursor = saved;
                            self.restore_angles(angles);
                            (fields.len().to_string(), self.ty()?)
                        }
                    };
                    fields.push(Field {
                        name: field_name,
                        ty,
                        public: true,
                        span: self.since(field_start),
                    });
                    self.newlines();
                    if self.eat(",") {
                        self.newlines();
                    } else {
                        self.expect(")")?;
                        break;
                    }
                }
            }
            variants.push(Variant {
                name: variant_name,
                fields,
                span: self.since(variant_start),
            });
            if self.eat(",") || self.newline() {
                self.newlines();
            } else if !self.at("}") {
                return Err(self.error("expected `,` or `}` after enum variant"));
            }
        }
        Ok(Enum {
            name,
            public,
            generics,
            bounds,
            variants,
            span: self.since(start),
        })
    }
    fn function(&mut self, mods: Modifiers, start: usize) -> ParseResult<Function> {
        self.expect("fn")?;
        let name = self.identifier()?;
        let (generics, bounds) = self.generic_params()?;
        self.expect("(")?;
        self.newlines();
        let mut params = Vec::new();
        while !self.eat(")") {
            let param_start = self.span().start;
            let (name, ty) = if self.at("&") || (self.at("self") && !self.look(1, ":")) {
                let borrow = self.eat("&");
                let mutable = borrow && self.eat("mut");
                self.expect("self")?;
                let owner = self.self_type.clone().ok_or_else(|| {
                    self.error("receiver shorthand is only valid inside a struct")
                })?;
                (
                    "self".into(),
                    if borrow {
                        Type::Ref(mutable, Box::new(owner))
                    } else {
                        owner
                    },
                )
            } else {
                let name = self.identifier()?;
                self.expect(":")?;
                self.newlines();
                (name, self.ty()?)
            };
            params.push(Param {
                name,
                ty,
                span: self.since(param_start),
            });
            self.newlines();
            if self.eat(",") {
                self.newlines();
            } else {
                self.expect(")")?;
                break;
            }
        }
        let ret_start = self.span().start;
        let ret = if self.eat("->") {
            self.newlines();
            self.ty()?
        } else {
            Type::Void
        };
        let ret_span = if ret == Type::Void && self.span().start == ret_start {
            Span {
                start: ret_start,
                end: ret_start,
            }
        } else {
            self.since(ret_start)
        };
        let mut from = Vec::new();
        let from_start = self.span().start;
        let mut from_span = None;
        if self.eat("from") {
            self.expect("(")?;
            self.newlines();
            loop {
                from.push(if self.eat("static") {
                    "static".to_owned()
                } else {
                    let mut source = self.identifier()?;
                    if self.eat(".") {
                        self.expect("stored")?;
                        source.push_str(".stored");
                    }
                    source
                });
                self.newlines();
                if self.eat(",") {
                    self.newlines();
                } else {
                    break;
                }
            }
            self.expect(")")?;
            from_span = Some(self.since(from_start));
        }
        let mut stores = Vec::new();
        if self.eat("stores") {
            self.expect("(")?;
            loop {
                self.newlines();
                stores.push(self.identifier()?);
                self.newlines();
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(")")?;
        }
        let mut requires_plain = Vec::new();
        if self.eat("requires_plain") {
            self.expect("(")?;
            loop {
                requires_plain.push(self.ty()?);
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(")")?;
        }
        // Foreign prototypes end at the line; an optional body may start on the next line.
        let saved = self.cursor;
        self.newlines();
        let body = if self.in_interface {
            if !mods.placement.is_empty() || mods.interrupt.is_some() {
                return Err(Diagnostic::new(
                    self.since(start),
                    "interface methods cannot have @interrupt, @section, or @align",
                ));
            }
            if self.at("{") {
                return Err(self.error("interface methods are signatures without a body"));
            }
            self.cursor = saved;
            self.end_statement()?;
            None
        } else if mods.compiler() {
            if self.at("{") || mods.extern_ || mods.unsafe_ || mods.test {
                return Err(self.error("@compiler declarations are safe prototypes without bodies"));
            }
            self.cursor = saved;
            self.end_statement()?;
            None
        } else if self.at("{") {
            let mut body = self.block()?;
            if ret != Type::Void {
                self.lower_tail(&mut body, true);
            }
            Some(body)
        } else if mods.extern_ {
            self.cursor = saved;
            self.end_statement()?;
            None
        } else {
            return Err(self.error("expected a function body enclosed in braces"));
        };
        if mods.ignore.is_some() && !mods.test && !name.starts_with("test_") {
            return Err(Diagnostic::new(
                self.since(start),
                "@ignore requires @test or a test_ function",
            ));
        }
        if mods.interrupt.is_some()
            && (mods.unsafe_
                || mods.extern_
                || mods.public
                || mods.test
                || !generics.is_empty()
                || !params.is_empty()
                || ret != Type::Void
                || body.is_none())
        {
            return Err(Diagnostic::new(
                self.since(start),
                "@interrupt requires a private, safe, non-generic fn name() with a body and no result; the hardware calls it, so it takes no arguments, and code cannot call it",
            ));
        }
        if mods.inlining != Inlining::Auto && body.is_none() {
            return Err(Diagnostic::new(
                self.since(start),
                "@inline and @cold apply only to functions with a body",
            ));
        }
        if mods.link_name.is_some() && (!mods.extern_ || body.is_some()) {
            return Err(Diagnostic::new(
                self.since(start),
                "@link_name applies only to extern \"C\" declarations without a body",
            ));
        }
        if mods.test
            && (mods.unsafe_
                || mods.extern_
                || !generics.is_empty()
                || !params.is_empty()
                || ret != Type::Void
                || self.self_type.is_some())
        {
            return Err(Diagnostic::new(
                self.since(start),
                "@test requires a safe, non-generic, top-level fn name() -> void",
            ));
        }
        Ok(Function {
            imported: false,
            generic_instance: false,
            test: mods.test,
            ignore: mods.ignore,
            name,
            public: mods.public,
            unsafe_: mods.unsafe_,
            extern_: mods.extern_,
            printing: mods.printing,
            pio: mods.pio,
            generics,
            bounds,
            params,
            ret,
            ret_span,
            from,
            from_span,
            stores,
            requires_plain,
            body,
            span: self.since(start),
            environment: None,
            interrupt: mods.interrupt,
            link_name: mods.link_name,
            placement: mods.placement,
            inlining: mods.inlining,
            async_part: mods.async_.then_some(AsyncPart::Body),
        })
    }
    fn constant(
        &mut self,
        public: bool,
        placement: Placement,
        start: usize,
    ) -> ParseResult<Constant> {
        let static_ = self.at("static");
        let mutable = if self.eat("static") {
            self.eat("mut")
        } else {
            self.expect("const")?;
            false
        };
        let (name, ty) = self.typed_name()?;
        self.expect("=")?;
        self.newlines();
        let value = self.expression(true)?;
        let span = self.since(start);
        self.end_statement()?;
        Ok(Constant {
            name,
            ty,
            value,
            public,
            static_,
            mutable,
            placement,
            span,
        })
    }
    fn block(&mut self) -> ParseResult<Block> {
        self.nested(Self::block_inner)
    }
    fn block_inner(&mut self) -> ParseResult<Block> {
        self.expect("{")?;
        let previous_soft = self.soft_newlines;
        self.soft_newlines = 0;
        let mut statements = Vec::new();
        self.separators();
        while !self.eat("}") {
            if self.eof() {
                let error = self.error("unclosed block; expected `}`");
                if !self.recover {
                    return Err(error);
                }
                self.diagnostics.push(error);
                break;
            }
            let cursor = self.cursor;
            let soft = self.soft_newlines;
            match self.statement() {
                Ok(statement) => statements.push(statement),
                Err(error) if self.recover => {
                    self.diagnostics.push(error);
                    self.soft_newlines = soft;
                    self.synchronize(cursor, false);
                }
                Err(error) => return Err(error),
            }
            self.separators();
        }
        self.soft_newlines = previous_soft;
        Ok(statements)
    }
    fn statement(&mut self) -> ParseResult<Stmt> {
        let start = self.span().start;
        let compound;
        let kind = if self.eat("return") {
            compound = false;
            StmtKind::Return(
                if self.newline() || self.at(";") || self.at("}") || self.eof() {
                    None
                } else {
                    Some(self.expression(true)?)
                },
            )
        } else if self.eat("break") {
            compound = false;
            StmtKind::Break
        } else if self.eat("continue") {
            compound = false;
            StmtKind::Continue
        } else if self.eat("if") {
            compound = true;
            self.if_statement()?
        } else if self.eat("for") {
            compound = true;
            self.for_statement()?
        } else if self.eat("match") {
            compound = true;
            self.match_statement(false)?
        } else if self.eat("unsafe") {
            compound = true;
            self.newlines();
            StmtKind::Unsafe(self.block()?)
        } else if self.at("{") {
            compound = true;
            StmtKind::Block(self.block()?)
        } else {
            compound = false;
            self.simple_statement(true)?
        };
        let span = self.since(start);
        if self.following_semicolon() {
            self.discarded_tails.insert(start);
        }
        if !compound {
            self.end_statement()?;
        }
        Ok(Stmt { kind, span })
    }
    fn typed_name(&mut self) -> ParseResult<(String, Type)> {
        if self.look(1, ":") {
            let name = self.identifier()?;
            self.expect(":")?;
            self.newlines();
            Ok((name, self.ty()?))
        } else {
            let saved = self.cursor;
            let angles = self.angle_splits.len();
            let first = (|| {
                let ty = self.ty()?;
                Ok((self.identifier()?, ty))
            })();
            if first.is_ok() {
                return first;
            }
            self.cursor = saved;
            self.restore_angles(angles);
            let previous = self.slice_first;
            self.slice_first = true;
            let alternate: ParseResult<(String, Type)> = (|| {
                let ty = self.ty()?;
                Ok((self.identifier()?, ty))
            })();
            self.slice_first = previous;
            alternate.or(first)
        }
    }
    fn simple_statement(&mut self, allow_struct: bool) -> ParseResult<StmtKind> {
        if self.eat("let") {
            let pattern = self.pattern()?;
            let ty = if self.eat(":") {
                self.newlines();
                self.ty()?
            } else {
                Type::Unknown
            };
            self.expect("=")?;
            self.newlines();
            let value = self.expression(allow_struct)?;
            let saved = self.cursor;
            self.newlines();
            let else_block = if self.eat("else") {
                self.newlines();
                Some(self.block()?)
            } else {
                self.cursor = saved;
                None
            };
            if else_block.is_none()
                && let Pattern::Binding(name) = &pattern
            {
                return Ok(StmtKind::Let {
                    name: name.clone(),
                    ty,
                    value: Some(value),
                    constant: false,
                    mutable: false,
                });
            }
            return Ok(StmtKind::LetPattern {
                pattern,
                ty,
                value,
                else_block,
            });
        }
        if self.eat("const") {
            let (name, ty) = self.typed_name()?;
            self.expect("=")?;
            self.newlines();
            return Ok(StmtKind::Let {
                name,
                ty,
                value: Some(self.expression(allow_struct)?),
                constant: true,
                mutable: false,
            });
        }
        if self.look(1, ":=") {
            let name = self.identifier()?;
            self.expect(":=")?;
            self.newlines();
            return Ok(StmtKind::Let {
                name,
                ty: Type::Unknown,
                value: Some(self.expression(allow_struct)?),
                constant: false,
                mutable: true,
            });
        }
        if self.look(1, ":") {
            let (name, ty) = self.typed_name()?;
            let value = if self.eat("=") {
                self.newlines();
                Some(self.expression(allow_struct)?)
            } else {
                None
            };
            return Ok(StmtKind::Let {
                name,
                ty,
                value,
                constant: false,
                mutable: true,
            });
        }
        // A type-first declaration is unambiguous once followed by its binding name.
        let saved = self.cursor;
        let saved_angles = self.angle_splits.len();
        if let Ok((name, ty)) = self.typed_name() {
            let value = if self.eat("=") {
                self.newlines();
                Some(self.expression(allow_struct)?)
            } else {
                None
            };
            return Ok(StmtKind::Let {
                name,
                ty,
                value,
                constant: false,
                mutable: true,
            });
        }
        self.cursor = saved;
        self.restore_angles(saved_angles);
        let target = self.expression(allow_struct)?;
        if self.eat("=") {
            self.newlines();
            return Ok(StmtKind::Assign {
                target,
                op: None,
                value: self.expression(allow_struct)?,
            });
        }
        let op = match &self.token().kind {
            TokenKind::Symbol("+=") => Some(BinaryOp::Add),
            TokenKind::Symbol("-=") => Some(BinaryOp::Sub),
            TokenKind::Symbol("*=") => Some(BinaryOp::Mul),
            TokenKind::Symbol("/=") => Some(BinaryOp::Div),
            TokenKind::Symbol("%=") => Some(BinaryOp::Rem),
            TokenKind::Symbol("&=") => Some(BinaryOp::BitAnd),
            TokenKind::Symbol("|=") => Some(BinaryOp::BitOr),
            TokenKind::Symbol("^=") => Some(BinaryOp::BitXor),
            TokenKind::Symbol("<<=") => Some(BinaryOp::Shl),
            TokenKind::Symbol(">>=") => Some(BinaryOp::Shr),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            self.newlines();
            Ok(StmtKind::Assign {
                target,
                op: Some(op),
                value: self.expression(allow_struct)?,
            })
        } else {
            Ok(StmtKind::Expr(target))
        }
    }
    fn if_statement(&mut self) -> ParseResult<StmtKind> {
        self.nested(Self::if_statement_inner)
    }
    fn if_statement_inner(&mut self) -> ParseResult<StmtKind> {
        let pattern = if self.eat("let") {
            let pattern = self.pattern()?;
            self.expect("=")?;
            self.newlines();
            Some(pattern)
        } else {
            None
        };
        let condition = self.expression(false)?;
        self.newlines();
        let then_block = self.block()?;
        let saved = self.cursor;
        self.newlines();
        let else_block = if self.eat("else") {
            self.newlines();
            if self.eat("if") {
                let start = self.tokens[self.cursor - 1].span.start;
                let kind = self.if_statement()?;
                vec![Stmt {
                    kind,
                    span: self.since(start),
                }]
            } else {
                self.block()?
            }
        } else {
            self.cursor = saved;
            Vec::new()
        };
        Ok(if let Some(pattern) = pattern {
            StmtKind::IfLet {
                pattern,
                value: condition,
                then_block,
                else_block,
            }
        } else {
            StmtKind::If {
                condition,
                then_block,
                else_block,
            }
        })
    }
    fn for_statement(&mut self) -> ParseResult<StmtKind> {
        let start = self.span().start;
        if self.at("{") {
            return Ok(StmtKind::For {
                init: None,
                condition: None,
                step: None,
                body: self.block()?,
            });
        }
        if self.at("&") || self.look(1, "in") || self.look(1, ",") || self.loop_pattern_ahead() {
            let mut copy = self.eat("&");
            if copy && self.at("mut") {
                return Err(self.error(
                    "`&mut` loop patterns are unsupported; use `for value in &mut values`",
                ));
            }
            let pattern_start = self.span().start;
            let mut element = if self.at("(") {
                Err(self.pattern()?)
            } else {
                Ok(self.identifier()?)
            };
            let index = if self.eat(",") {
                if copy {
                    return Err(self.error(
                        "loop indices are values; put `&` on the element: `for i, &value in values`",
                    ));
                }
                let Ok(index) = element else {
                    return Err(self.error("a loop index must be a name"));
                };
                copy = self.eat("&");
                if copy && self.at("mut") {
                    return Err(self.error(
                        "`&mut` loop patterns are unsupported; use `for i, value in &mut values`",
                    ));
                }
                element = if self.at("(") {
                    Err(self.pattern()?)
                } else {
                    Ok(self.identifier()?)
                };
                Some(index)
            } else {
                None
            };
            let pattern_span = self.since(pattern_start);
            self.expect("in")?;
            let mut iterable = self.expression(false)?;
            if self.eat("..") {
                self.newlines();
                let end = self.expression(false)?;
                iterable = Expr::new(
                    ExprKind::Range(Box::new(iterable), Box::new(end)),
                    self.since(start),
                );
            }
            self.newlines();
            let mut body = self.block()?;
            let name = match element {
                Ok(name) => name,
                Err(pattern) => {
                    // `for (a, b) in pairs` destructures each element reference, so
                    // the bindings borrow its fields. `for &(a, b) in pairs` copies
                    // each bound field out of that borrow instead.
                    let mut pattern = pattern;
                    let mut prefix = Vec::new();
                    if copy {
                        for name in pattern.bindings() {
                            prefix.push(Stmt {
                                kind: StmtKind::Let {
                                    name: name.clone(),
                                    ty: Type::Unknown,
                                    value: Some(Expr::new(
                                        ExprKind::Unary(
                                            UnaryOp::Deref,
                                            Box::new(Expr::new(
                                                ExprKind::Name(format!("${name}")),
                                                pattern_span,
                                            )),
                                        ),
                                        pattern_span,
                                    )),
                                    constant: false,
                                    mutable: false,
                                },
                                span: pattern_span,
                            });
                        }
                        pattern.rename_bindings(&|name| format!("${name}"));
                        copy = false;
                    }
                    prefix.insert(
                        0,
                        Stmt {
                            kind: StmtKind::LetPattern {
                                pattern,
                                ty: Type::Unknown,
                                value: Expr::new(ExprKind::Name("$element".into()), pattern_span),
                                else_block: None,
                            },
                            span: pattern_span,
                        },
                    );
                    prefix.append(&mut body);
                    body = prefix;
                    "$element".into()
                }
            };
            return Ok(StmtKind::ForEach {
                index,
                name,
                copy,
                iterable,
                body,
            });
        }
        let start = self.span().start;
        let first = if self.at(";") {
            None
        } else {
            let kind = self.simple_statement(false)?;
            Some(Stmt {
                kind,
                span: self.since(start),
            })
        };
        if self.eat(";") {
            self.newlines();
            let condition = if self.at(";") {
                None
            } else {
                Some(self.expression(false)?)
            };
            self.expect(";")?;
            self.newlines();
            let step = if self.at("{") {
                None
            } else {
                let start = self.span().start;
                let kind = self.simple_statement(false)?;
                Some(Box::new(Stmt {
                    kind,
                    span: self.since(start),
                }))
            };
            self.newlines();
            let body = self.block()?;
            Ok(StmtKind::For {
                init: first.map(Box::new),
                condition,
                step,
                body,
            })
        } else {
            let condition =
                match first {
                    Some(Stmt {
                        kind: StmtKind::Expr(expression),
                        ..
                    }) => expression,
                    _ => return Err(self.error(
                        "condition loops need an expression; three-part loops need `;` separators",
                    )),
                };
            self.newlines();
            let body = self.block()?;
            Ok(StmtKind::For {
                init: None,
                condition: Some(condition),
                step: None,
                body,
            })
        }
    }
    // `for (a, b) in`, `for &(a, b) in`, and `for i, (a, b) in` destructure
    // elements; any other parenthesis starts a condition loop.
    fn loop_pattern_ahead(&mut self) -> bool {
        let saved = self.cursor;
        let angles = self.angle_splits.len();
        let diagnostics = self.diagnostics.len();
        self.eat("&");
        let found = self.at("(") && self.pattern().is_ok() && self.at("in");
        self.cursor = saved;
        self.restore_angles(angles);
        self.diagnostics.truncate(diagnostics);
        found
    }
    fn match_statement(&mut self, value_mode: bool) -> ParseResult<StmtKind> {
        let value = self.expression(false)?;
        self.newlines();
        self.expect("{")?;
        self.separators();
        let mut arms = Vec::new();
        while !self.eat("}") {
            if self.eof() {
                return Err(self.error("unclosed match statement; expected `}`"));
            }
            let start = self.span().start;
            let pattern = self.pattern()?;
            self.newlines();
            let guard = if self.eat("if") {
                self.newlines();
                Some(self.expression(true)?)
            } else {
                None
            };
            self.newlines();
            self.expect("=>")?;
            self.newlines();
            let body = if self.at("{") {
                let mut body = self.block()?;
                if value_mode {
                    self.lower_tail(&mut body, false);
                }
                body
            } else {
                let value = self.expression(true)?;
                let discarded = self.following_semicolon();
                if discarded {
                    self.discarded_tails.insert(value.span.start);
                }
                if !self.at(",") && !self.at("}") && !self.newline() && !self.at(";") {
                    return Err(
                        self.error("expected `,` or a newline after a match arm expression")
                    );
                }
                vec![Stmt {
                    span: value.span,
                    kind: if value_mode && !discarded {
                        StmtKind::Yield(value)
                    } else {
                        StmtKind::Expr(value)
                    },
                }]
            };
            arms.push(MatchArm {
                pattern,
                guard,
                body,
                span: self.since(start),
            });
            self.eat(",");
            self.separators();
        }
        Ok(StmtKind::Match { value, arms })
    }
    fn pattern(&mut self) -> ParseResult<Pattern> {
        self.nested(Self::pattern_inner)
    }
    fn pattern_inner(&mut self) -> ParseResult<Pattern> {
        self.eat("|");
        self.newlines();
        let first = self.pattern_atom()?;
        let mut alternatives = vec![first];
        loop {
            let saved = self.cursor;
            self.newlines();
            if !self.eat("|") {
                self.cursor = saved;
                break;
            }
            self.newlines();
            alternatives.push(self.pattern_atom()?);
        }
        if alternatives.len() == 1 {
            Ok(alternatives.remove(0))
        } else {
            Ok(Pattern::Or(alternatives))
        }
    }
    fn pattern_integer(&mut self) -> ParseResult<u64> {
        let negative = self.eat("-");
        let token = self.bump();
        if let TokenKind::Int(value, _) = token.kind
            && (!negative || value <= (1u64 << 63))
        {
            Ok(if negative {
                value.wrapping_neg()
            } else {
                value
            })
        } else {
            Err(Diagnostic::new(
                token.span,
                "expected an integer or byte literal in a pattern",
            ))
        }
    }
    fn pattern_name(&mut self) -> ParseResult<String> {
        let start = self.span();
        let mut name = self.qualified_name()?;
        if name == "Self" {
            name = self
                .self_type
                .as_ref()
                .ok_or_else(|| {
                    Diagnostic::new(start, "`Self` is only valid inside a struct declaration")
                })?
                .to_string();
        }
        if self.eat("::") || self.at("<") {
            name = Type::Generic(name, self.type_args()?).to_string();
            while self.eat(".") {
                name.push('.');
                name.push_str(&self.identifier()?);
            }
        }
        Ok(name)
    }
    fn pattern_atom(&mut self) -> ParseResult<Pattern> {
        if self.eat("(") {
            self.newlines();
            let pattern = self.pattern()?;
            self.newlines();
            if !self.eat(",") {
                self.expect(")")?;
                return Ok(pattern);
            }
            let mut elements = vec![pattern];
            self.newlines();
            while !self.eat(")") {
                elements.push(self.pattern()?);
                self.newlines();
                if self.eat(",") {
                    self.newlines();
                } else {
                    self.expect(")")?;
                    break;
                }
            }
            if elements.len() < 2 {
                return Err(self.error("tuple patterns need at least two elements"));
            }
            return Ok(Pattern::Struct(
                tuple_name(elements.len()),
                elements
                    .into_iter()
                    .enumerate()
                    .map(|(index, pattern)| (index.to_string(), pattern))
                    .collect(),
                false,
            ));
        }
        if self.eat("_") {
            return Ok(Pattern::Wildcard);
        }
        if self.eat("true") {
            return Ok(Pattern::Bool(true));
        }
        if self.eat("false") {
            return Ok(Pattern::Bool(false));
        }
        if self.at("-") || matches!(self.token().kind, TokenKind::Int(..)) {
            let value = self.pattern_integer()?;
            let inclusive = self.eat("..=");
            if inclusive || self.eat("..") {
                self.newlines();
                let end = self.pattern_integer()?;
                return Ok(Pattern::Range(value, end, inclusive));
            }
            return Ok(Pattern::Int(value));
        }
        let name = self.pattern_name()?;
        if self.eat("(") {
            let mut fields = Vec::new();
            self.newlines();
            while !self.eat(")") {
                fields.push(self.pattern()?);
                self.newlines();
                if self.eat(",") {
                    self.newlines();
                } else {
                    self.expect(")")?;
                    break;
                }
            }
            return Ok(Pattern::Variant(name, fields));
        }
        if self.eat("{") {
            let mut fields = Vec::new();
            let mut rest = false;
            self.newlines();
            while !self.eat("}") {
                if self.eat("..") {
                    rest = true;
                    self.eat(",");
                    self.newlines();
                    self.expect("}")?;
                    break;
                }
                let field = self.identifier()?;
                let pattern = if self.eat(":") {
                    self.newlines();
                    self.pattern()?
                } else {
                    Pattern::Binding(field.clone())
                };
                fields.push((field, pattern));
                self.newlines();
                if self.eat(",") {
                    self.newlines();
                } else {
                    self.expect("}")?;
                    break;
                }
            }
            return Ok(Pattern::Struct(name, fields, rest));
        }
        Ok(
            if name == "none" || name.contains('.') || name.contains('<') {
                Pattern::Variant(name, Vec::new())
            } else {
                Pattern::Binding(name)
            },
        )
    }
    fn expression(&mut self, allow_struct: bool) -> ParseResult<Expr> {
        self.expr_bp(0, allow_struct)
    }
    fn chain_newlines(&mut self) {
        let mut next = self.cursor;
        while matches!(self.tokens[next].kind, TokenKind::Newline) {
            next += 1;
        }
        // A leading dot continues the preceding expression across blank and
        // comment-only lines. Leave every other newline (and every semicolon)
        // in place so ordinary statement boundaries keep their meaning.
        if matches!(self.tokens[next].kind, TokenKind::Symbol(".")) {
            self.cursor = next;
        }
    }
    fn expr_bp(&mut self, minimum: u8, allow_struct: bool) -> ParseResult<Expr> {
        self.nested(|parser| parser.expr_bp_inner(minimum, allow_struct))
    }
    fn expr_bp_inner(&mut self, minimum: u8, allow_struct: bool) -> ParseResult<Expr> {
        if self.soft_newlines > 0 {
            self.newlines();
        }
        let start = self.span().start;
        // Only the operand chain directly after `await` stops at `?` and `!`.
        let await_operand = std::mem::take(&mut self.await_operand);
        let unary = if self.eat("-") {
            Some(UnaryOp::Neg)
        } else if self.eat("!") {
            Some(UnaryOp::Not)
        } else if self.eat("~") {
            Some(UnaryOp::BitNot)
        } else if self.eat("*") {
            Some(UnaryOp::Deref)
        } else if self.eat("&") {
            Some(if self.eat("mut") {
                UnaryOp::BorrowMut
            } else {
                UnaryOp::Borrow
            })
        } else {
            None
        };
        let mut lhs = if unary.is_none() && self.eat("await") {
            self.newlines();
            self.await_operand = true;
            let operand = self.expr_bp(25, allow_struct);
            self.await_operand = false;
            Expr::new(
                ExprKind::Await {
                    value: Box::new(operand?),
                    poll: String::new(),
                    output: None,
                },
                self.since(start),
            )
        } else if let Some(op) = unary {
            self.newlines();
            let mut rhs = self.expr_bp(23, allow_struct)?;
            if matches!(op, UnaryOp::Borrow | UnaryOp::BorrowMut)
                && let ExprKind::Slice { mutable, .. } = &mut rhs.kind
            {
                *mutable = op == UnaryOp::BorrowMut;
                rhs.span = self.since(start);
                rhs
            } else {
                Expr::new(ExprKind::Unary(op, Box::new(rhs)), self.since(start))
            }
        } else {
            self.primary(allow_struct)?
        };
        loop {
            if self.soft_newlines > 0 {
                self.newlines();
            } else if 25 >= minimum {
                self.chain_newlines();
            }
            if await_operand && (self.at("?") || self.at("!")) {
                break;
            }
            if 25 >= minimum {
                if self.eat("?") {
                    lhs = Expr::new(ExprKind::Try(Box::new(lhs)), self.since(start));
                    continue;
                }
                if self.eat("!") {
                    lhs = Expr::new(ExprKind::Unwrap(Box::new(lhs)), self.since(start));
                    continue;
                }
                if self.eat("[") {
                    self.soft_newlines += 1;
                    self.newlines();
                    let index = if self.at("..") {
                        None
                    } else {
                        Some(Box::new(self.expression(true)?))
                    };
                    let kind = if self.eat("..") {
                        self.newlines();
                        let end = if self.at("]") {
                            None
                        } else {
                            Some(Box::new(self.expression(true)?))
                        };
                        ExprKind::Slice {
                            base: Box::new(lhs),
                            start: index,
                            end,
                            mutable: false,
                        }
                    } else {
                        ExprKind::Index(
                            Box::new(lhs),
                            index.ok_or_else(|| self.error("expected an index"))?,
                        )
                    };
                    self.newlines();
                    self.expect("]")?;
                    self.soft_newlines -= 1;
                    lhs = Expr::new(kind, self.since(start));
                    continue;
                }
                if self.eat(".") {
                    let field = if let TokenKind::Int(index, None) = self.token().kind {
                        self.bump();
                        index.to_string()
                    } else {
                        self.identifier()?
                    };
                    lhs = Expr::new(ExprKind::Field(Box::new(lhs), field), self.since(start));
                    continue;
                }
                if self.at("(") {
                    let args = self.arguments()?;
                    lhs = self.call(lhs, Vec::new(), args, start)?;
                    continue;
                }
                if allow_struct
                    && self.at("{")
                    && let Some(name) = expression_path(&lhs)
                {
                    lhs = self.struct_literal(name, start)?;
                    continue;
                }
                if self.at("::")
                    || (self.at("<") && matches!(lhs.kind, ExprKind::Name(_) | ExprKind::Field(..)))
                {
                    let explicit = self.eat("::");
                    let saved_cursor = self.cursor;
                    let saved_angles = self.angle_splits.len();
                    match self.type_args() {
                        Ok(types) if self.at("(") => {
                            let args = self.arguments()?;
                            lhs = self.call(lhs, types, args, start)?;
                            continue;
                        }
                        Ok(types) if allow_struct && self.at("{") => {
                            let name = expression_path(&lhs)
                                .ok_or_else(|| self.error("expected a named struct type"))?;
                            lhs =
                                self.struct_literal(Type::Generic(name, types).to_string(), start)?;
                            continue;
                        }
                        Ok(_) if explicit => {
                            return Err(self.error("expected `(` after function type arguments"));
                        }
                        Err(error) if explicit => return Err(error),
                        _ => {
                            self.cursor = saved_cursor;
                            self.restore_angles(saved_angles);
                        }
                    }
                }
            }
            if self.at("as") && 21 >= minimum {
                self.bump();
                self.newlines();
                let ty = self.ty()?;
                lhs = Expr::new(ExprKind::Cast(Box::new(lhs), ty), self.since(start));
                continue;
            }
            let Some((op, left, right)) = binary(&self.token().kind) else {
                break;
            };
            if left < minimum {
                break;
            }
            self.bump();
            self.newlines();
            let rhs = self.expr_bp(right, allow_struct)?;
            lhs = Expr::new(
                ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
                self.since(start),
            );
        }
        Ok(lhs)
    }
    fn call(
        &self,
        callee: Expr,
        type_args: Vec<Type>,
        args: Vec<Expr>,
        start: usize,
    ) -> ParseResult<Expr> {
        let kind = match callee.kind {
            ExprKind::Name(name) => ExprKind::Call {
                name,
                type_args,
                args,
            },
            ExprKind::Field(receiver, name) => {
                if !type_args.is_empty() {
                    let Some(prefix) = expression_path(&receiver) else {
                        return Err(Diagnostic::new(
                            callee.span,
                            "generic method calls require a qualified type/function name",
                        ));
                    };
                    ExprKind::Call {
                        name: format!("{prefix}.{name}"),
                        type_args,
                        args,
                    }
                } else {
                    ExprKind::MethodCall {
                        receiver,
                        name,
                        args,
                    }
                }
            }
            _ if type_args.is_empty() => ExprKind::Invoke {
                callee: Box::new(callee),
                args,
            },
            _ => {
                return Err(Diagnostic::new(
                    callee.span,
                    "type arguments require a named generic function",
                ));
            }
        };
        Ok(Expr::new(kind, self.since(start)))
    }
    fn arguments(&mut self) -> ParseResult<Vec<Expr>> {
        self.expect("(")?;
        self.soft_newlines += 1;
        self.newlines();
        let mut args = Vec::new();
        while !self.eat(")") {
            args.push(self.expression(true)?);
            self.newlines();
            if self.eat(",") {
                self.newlines();
            } else {
                self.expect(")")?;
                break;
            }
        }
        self.soft_newlines -= 1;
        Ok(args)
    }
    fn lower_tail(&self, block: &mut Block, function_return: bool) {
        let Some(last) = block.last_mut() else {
            return;
        };
        if self.discarded_tails.contains(&last.span.start) {
            return;
        }
        match &mut last.kind {
            StmtKind::Expr(e) => {
                last.kind = if function_return {
                    StmtKind::Return(Some(e.clone()))
                } else {
                    StmtKind::Yield(e.clone())
                }
            }
            StmtKind::If {
                then_block,
                else_block,
                ..
            }
            | StmtKind::IfLet {
                then_block,
                else_block,
                ..
            } => {
                self.lower_tail(then_block, function_return);
                self.lower_tail(else_block, function_return);
            }
            StmtKind::Match { arms, .. } => {
                for arm in arms {
                    self.lower_tail(&mut arm.body, function_return);
                }
            }
            StmtKind::Block(b) | StmtKind::Unsafe(b) => self.lower_tail(b, function_return),
            _ => (),
        }
    }
    fn primary(&mut self, allow_struct: bool) -> ParseResult<Expr> {
        let start = self.span().start;
        let token = self.bump();
        let kind = match token.kind {
            TokenKind::Int(value, ty) => ExprKind::Int(value, ty),
            TokenKind::Float(value, ty) => ExprKind::Float(value, ty),
            TokenKind::String(value, byte) => ExprKind::String(value, byte),
            TokenKind::Ident(name) if name == "if" || name == "match" || name == "unsafe" => {
                let kind = match name.as_str() {
                    "if" => self.if_statement()?,
                    "match" => self.match_statement(true)?,
                    _ => {
                        self.newlines();
                        StmtKind::Unsafe(self.block()?)
                    }
                };
                let mut body = vec![Stmt {
                    kind,
                    span: self.since(start),
                }];
                self.lower_tail(&mut body, false);
                ExprKind::ValueBlock(body)
            }
            TokenKind::Symbol("{") => {
                self.cursor -= 1;
                let mut body = self.block()?;
                self.lower_tail(&mut body, false);
                ExprKind::ValueBlock(body)
            }
            TokenKind::Ident(name) if name == "true" || name == "false" => {
                ExprKind::Bool(name == "true")
            }
            TokenKind::Ident(name) if name == "fn" => self.closure()?,
            TokenKind::Ident(name) if name == "asm" => self.inline_asm(start)?,
            TokenKind::Ident(name) if !reserved(&name) => {
                if name == "Self" && self.self_type.is_none() {
                    return Err(Diagnostic::new(
                        token.span,
                        "`Self` is only valid inside a struct declaration",
                    ));
                }
                let name = if name == "Self" {
                    match &self.self_type {
                        Some(Type::Named(name) | Type::Generic(name, _)) => name.clone(),
                        _ => name,
                    }
                } else {
                    name
                };
                if allow_struct && self.at("{") {
                    return self.struct_literal(name, start);
                } else {
                    ExprKind::Name(name)
                }
            }
            TokenKind::Symbol("(") => {
                self.soft_newlines += 1;
                self.newlines();
                if self.at(")") {
                    return Err(
                        self.error("tuples need at least two elements; use `void` for no value")
                    );
                }
                let mut value = self.expression(true)?;
                self.newlines();
                if self.eat(",") {
                    let mut elements = vec![value];
                    self.newlines();
                    while !self.at(")") {
                        elements.push(self.expression(true)?);
                        self.newlines();
                        if !self.eat(",") {
                            break;
                        }
                        self.newlines();
                    }
                    self.expect(")")?;
                    self.soft_newlines -= 1;
                    if elements.len() < 2 {
                        return Err(Diagnostic::new(
                            self.since(start),
                            "tuples need at least two elements",
                        ));
                    }
                    let name = tuple_name(elements.len());
                    let fields = elements
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| (index.to_string(), value))
                        .collect();
                    return Ok(Expr::new(ExprKind::Struct(name, fields), self.since(start)));
                }
                if self.eat(":") {
                    self.newlines();
                    let annotation = self.ty()?;
                    if !matches!(annotation, Type::Array(..) | Type::ArrayExpr(..)) {
                        return Err(
                            self.error("array literal annotation requires a fixed-size array type")
                        );
                    }
                    match &mut value.kind {
                        ExprKind::Array(ty, _) if matches!(&*ty, Type::Array(_, element) if **element == Type::Unknown) =>
                        {
                            *ty = annotation;
                        }
                        ExprKind::Array(..) => {
                            return Err(self.error("array literal already has an explicit type"));
                        }
                        ExprKind::Repeat(..) => {
                            return Err(self.error("array repetition annotations are not supported; annotate the binding instead"));
                        }
                        _ => {
                            return Err(self.error(
                                "expression type annotations require a bracket array literal",
                            ));
                        }
                    }
                    self.newlines();
                }
                self.expect(")")?;
                self.soft_newlines -= 1;
                value.span = self.since(start);
                return Ok(value);
            }
            TokenKind::Symbol("[") => return self.array_literal(start),
            _ => return Err(Diagnostic::new(token.span, "expected an expression")),
        };
        Ok(Expr::new(kind, self.since(start)))
    }
    /// `asm("template", ..., in(reg) value, out("r0") u32, clobber("memory"),
    /// options(nomem))` after its `asm` keyword. Consecutive template strings
    /// are joined with newlines, one instruction per string.
    fn inline_asm(&mut self, start: usize) -> ParseResult<ExprKind> {
        self.expect("(")?;
        self.soft_newlines += 1;
        self.newlines();
        let mut lines = Vec::new();
        while let TokenKind::String(..) = self.token().kind {
            let token = self.bump();
            let TokenKind::String(bytes, byte) = token.kind else {
                unreachable!()
            };
            if byte {
                return Err(Diagnostic::new(
                    token.span,
                    "an assembly template is a string, not a byte string",
                ));
            }
            lines.push(
                String::from_utf8(bytes).map_err(|_| {
                    Diagnostic::new(token.span, "an assembly template must be UTF-8")
                })?,
            );
            self.newlines();
            if !self.eat(",") {
                break;
            }
            self.newlines();
        }
        if lines.is_empty() {
            return Err(self.error("asm expects a template string, such as asm(\"wfi\")"));
        }
        let mut asm = InlineAsm {
            template: lines.join("\n"),
            operands: vec![],
            clobbers: vec![],
            pure: false,
            nomem: false,
            tuple: None,
            span: Span::default(),
        };
        let mut options = false;
        while !self.at(")") {
            let operand_start = self.span().start;
            // `in` is a keyword elsewhere; here it names an operand kind.
            let keyword = match &self.token().kind {
                TokenKind::Ident(name) => {
                    let name = name.clone();
                    self.bump();
                    name
                }
                _ => {
                    return Err(self.error(
                        "expected an asm operand: in(reg) value, out(reg) Type, inout(reg) value, clobber(\"name\"), or options(...)",
                    ));
                }
            };
            match keyword.as_str() {
                "in" | "out" | "inout" => {
                    self.expect("(")?;
                    let register = if self.eat("reg") {
                        AsmRegister::Any
                    } else {
                        let token = self.bump();
                        match token.kind {
                            TokenKind::String(name, false)
                                if !name.is_empty()
                                    && name
                                        .iter()
                                        .all(|b| b.is_ascii_alphanumeric() || *b == b'_') =>
                            {
                                AsmRegister::Named(String::from_utf8(name).unwrap())
                            }
                            _ => {
                                return Err(Diagnostic::new(
                                    token.span,
                                    "expected `reg` or a register name such as \"r0\"",
                                ));
                            }
                        }
                    };
                    self.expect(")")?;
                    let kind = match keyword.as_str() {
                        "in" => AsmOperandKind::In(self.expression(true)?),
                        "inout" => AsmOperandKind::InOut(self.expression(true)?),
                        _ => AsmOperandKind::Out(self.ty()?),
                    };
                    asm.operands.push(AsmOperand {
                        kind,
                        register,
                        span: self.since(operand_start),
                    });
                }
                "clobber" => {
                    self.expect("(")?;
                    loop {
                        self.newlines();
                        let token = self.bump();
                        match token.kind {
                            TokenKind::String(name, false)
                                if !name.is_empty()
                                    && name
                                        .iter()
                                        .all(|b| b.is_ascii_alphanumeric() || *b == b'_') =>
                            {
                                asm.clobbers.push(String::from_utf8(name).unwrap());
                            }
                            _ => {
                                return Err(Diagnostic::new(
                                    token.span,
                                    "expected a clobbered register name or \"memory\"",
                                ));
                            }
                        }
                        self.newlines();
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect(")")?;
                }
                "options" => {
                    if options {
                        return Err(self.error("duplicate asm options"));
                    }
                    options = true;
                    self.expect("(")?;
                    loop {
                        self.newlines();
                        match self.identifier()?.as_str() {
                            "pure" => asm.pure = true,
                            "nomem" => asm.nomem = true,
                            _ => {
                                return Err(Diagnostic::new(
                                    self.since(self.previous_start()),
                                    "unknown asm option; supported: pure, nomem",
                                ));
                            }
                        }
                        self.newlines();
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect(")")?;
                }
                _ => {
                    return Err(Diagnostic::new(
                        self.since(operand_start),
                        "expected an asm operand: in(reg) value, out(reg) Type, inout(reg) value, clobber(\"name\"), or options(...)",
                    ));
                }
            }
            self.newlines();
            if !self.eat(",") {
                break;
            }
            self.newlines();
        }
        self.expect(")")?;
        self.soft_newlines -= 1;
        asm.span = self.since(start);
        Ok(ExprKind::Asm(Box::new(asm)))
    }
    /// `fn(a, b: T) -> R { body }` after its `fn` keyword. Omitted parameter
    /// and result types come from the expected type during checking.
    fn closure(&mut self) -> ParseResult<ExprKind> {
        self.expect("(")?;
        self.soft_newlines += 1;
        self.newlines();
        let mut params = Vec::new();
        while !self.eat(")") {
            let param_start = self.span().start;
            let name = self.identifier()?;
            let ty = if self.eat(":") {
                self.newlines();
                self.ty()?
            } else {
                Type::Unknown
            };
            params.push(Param {
                name,
                ty,
                span: self.since(param_start),
            });
            self.newlines();
            if self.eat(",") {
                self.newlines();
            } else {
                self.expect(")")?;
                break;
            }
        }
        self.soft_newlines -= 1;
        let ret_start = self.span().start;
        let ret = if self.eat("->") {
            self.newlines();
            self.ty()?
        } else {
            Type::Unknown
        };
        let ret_span = self.since(ret_start);
        // Closure bodies are statement blocks, even inside a parenthesized
        // argument list where newlines would otherwise be insignificant.
        let soft = std::mem::replace(&mut self.soft_newlines, 0);
        let body = self.block();
        self.soft_newlines = soft;
        let mut body = body?;
        self.lower_tail(&mut body, false);
        Ok(ExprKind::Closure(Box::new(Closure {
            params,
            ret,
            ret_span,
            body,
            function: String::new(),
            captures: Vec::new(),
        })))
    }
    fn array_literal(&mut self, start: usize) -> ParseResult<Expr> {
        self.cursor -= 1;
        let saved = self.cursor;
        let angles = self.angle_splits.len();
        let legacy = self
            .ty()
            .ok()
            .filter(|t| matches!(t, Type::Array(..) | Type::ArrayExpr(..)) && self.at("{"));
        if legacy.is_none() {
            self.cursor = saved;
            self.restore_angles(angles);
            self.expect("[")?;
            self.soft_newlines += 1;
            self.newlines();
            let mut values = Vec::new();
            if !self.at("]") {
                values.push(self.expression(true)?);
                if self.eat(";") {
                    self.newlines();
                    let length = LengthExpr::from_expr(self.expression(false)?)?;
                    self.newlines();
                    self.expect("]")?;
                    self.soft_newlines -= 1;
                    return Ok(Expr::new(
                        ExprKind::Repeat(
                            Box::new(values.remove(0)),
                            Type::ArrayExpr(Box::new(length), Box::new(Type::Unknown)),
                        ),
                        self.since(start),
                    ));
                }
                while self.eat(",") {
                    self.newlines();
                    if self.at("]") {
                        break;
                    }
                    values.push(self.expression(true)?);
                }
            }
            self.newlines();
            self.expect("]")?;
            self.soft_newlines -= 1;
            return Ok(Expr::new(
                ExprKind::Array(Type::Array(values.len(), Box::new(Type::Unknown)), values),
                self.since(start),
            ));
        }
        let ty = legacy.unwrap();
        self.expect("{")?;
        self.soft_newlines += 1;
        self.newlines();
        let mut values = Vec::new();
        while !self.eat("}") {
            values.push(self.expression(true)?);
            self.newlines();
            if self.eat(",") {
                self.newlines();
            } else {
                self.expect("}")?;
                break;
            }
        }
        self.soft_newlines -= 1;
        Ok(Expr::new(ExprKind::Array(ty, values), self.since(start)))
    }
    fn struct_literal(&mut self, name: String, start: usize) -> ParseResult<Expr> {
        self.expect("{")?;
        self.soft_newlines += 1;
        self.newlines();
        let mut fields = Vec::new();
        while !self.eat("}") {
            let field_start = self.span().start;
            let field = self.identifier()?;
            let value = if self.eat(":") {
                self.newlines();
                self.expression(true)?
            } else {
                Expr::new(ExprKind::Name(field.clone()), self.since(field_start))
            };
            fields.push((field, value));
            self.newlines();
            if self.eat(",") {
                self.newlines();
            } else {
                self.expect("}")?;
                break;
            }
        }
        self.soft_newlines -= 1;
        Ok(Expr::new(ExprKind::Struct(name, fields), self.since(start)))
    }
}

fn expression_path(expression: &Expr) -> Option<String> {
    match &expression.kind {
        ExprKind::Name(name) => Some(name.clone()),
        ExprKind::Field(receiver, name) => Some(format!("{}.{name}", expression_path(receiver)?)),
        _ => None,
    }
}

pub(crate) fn reserved(name: &str) -> bool {
    matches!(
        name,
        "package"
            | "import"
            | "pub"
            | "fn"
            | "struct"
            | "enum"
            | "interface"
            | "type"
            | "dyn"
            | "const"
            | "let"
            | "return"
            | "if"
            | "else"
            | "for"
            | "in"
            | "break"
            | "continue"
            | "match"
            | "unsafe"
            | "extern"
            | "as"
            | "from"
            | "stores"
            | "requires_plain"
            | "static"
            | "void"
            | "mut"
            | "true"
            | "false"
            | "asm"
            | "async"
            | "await"
    )
}

fn binary(token: &TokenKind) -> Option<(BinaryOp, u8, u8)> {
    let (operator, precedence) = match token {
        TokenKind::Symbol("||") => (BinaryOp::Or, 1),
        TokenKind::Symbol("&&") => (BinaryOp::And, 3),
        TokenKind::Symbol("|") => (BinaryOp::BitOr, 5),
        TokenKind::Symbol("^") => (BinaryOp::BitXor, 7),
        TokenKind::Symbol("&") => (BinaryOp::BitAnd, 9),
        TokenKind::Symbol("==") => (BinaryOp::Eq, 11),
        TokenKind::Symbol("!=") => (BinaryOp::Ne, 11),
        TokenKind::Symbol("<") => (BinaryOp::Lt, 13),
        TokenKind::Symbol("<=") => (BinaryOp::Le, 13),
        TokenKind::Symbol(">") => (BinaryOp::Gt, 13),
        TokenKind::Symbol(">=") => (BinaryOp::Ge, 13),
        TokenKind::Symbol("<<") => (BinaryOp::Shl, 15),
        TokenKind::Symbol(">>") => (BinaryOp::Shr, 15),
        TokenKind::Symbol("+") => (BinaryOp::Add, 17),
        TokenKind::Symbol("-") => (BinaryOp::Sub, 17),
        TokenKind::Symbol("*") => (BinaryOp::Mul, 19),
        TokenKind::Symbol("/") => (BinaryOp::Div, 19),
        TokenKind::Symbol("%") => (BinaryOp::Rem, 19),
        _ => return None,
    };
    Some((operator, precedence, precedence + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn body(source: &str) -> Block {
        parse(&format!(
            "package test\nfn main() -> void {{\n{source}\n}}\n"
        ))
        .unwrap()
        .functions
        .remove(0)
        .body
        .unwrap()
    }
    #[test]
    fn precedence_casts_and_postfix() {
        let statements = body("x := 1 + 2 * 3\ny := f(1)? as u32 + 1");
        let StmtKind::Let {
            value: Some(value), ..
        } = &statements[0].kind
        else {
            panic!("expected local");
        };
        assert!(
            matches!(&value.kind, ExprKind::Binary(BinaryOp::Add, _, rhs) if matches!(rhs.kind, ExprKind::Binary(BinaryOp::Mul, ..)))
        );
        let StmtKind::Let {
            value: Some(value), ..
        } = &statements[1].kind
        else {
            panic!("expected local");
        };
        assert!(
            matches!(&value.kind, ExprKind::Binary(BinaryOp::Add, lhs, _) if matches!(&lhs.kind, ExprKind::Cast(inner, _) if matches!(inner.kind, ExprKind::Try(_))))
        );
    }
    #[test]
    fn postfix_unwrap_precedence_and_newlines() {
        let statements = body("f()! as u32 + 1\n!flag()!\nr!!\nleft != right\nr!\n!flag");
        let expressions: Vec<_> = statements
            .iter()
            .map(|statement| match &statement.kind {
                StmtKind::Expr(value) => &value.kind,
                _ => panic!("expected expression"),
            })
            .collect();
        assert_eq!(expressions.len(), 6);
        assert!(
            matches!(expressions[0], ExprKind::Binary(BinaryOp::Add, lhs, _)
            if matches!(&lhs.kind, ExprKind::Cast(value, _) if matches!(value.kind, ExprKind::Unwrap(_))))
        );
        assert!(
            matches!(expressions[1], ExprKind::Unary(UnaryOp::Not, value)
            if matches!(value.kind, ExprKind::Unwrap(_)))
        );
        assert!(matches!(expressions[2], ExprKind::Unwrap(value)
            if matches!(value.kind, ExprKind::Unwrap(_))));
        assert!(matches!(expressions[3], ExprKind::Binary(BinaryOp::Ne, ..)));
        assert!(matches!(expressions[4], ExprKind::Unwrap(_)));
        assert!(matches!(expressions[5], ExprKind::Unary(UnaryOp::Not, _)));
    }
    #[test]
    fn all_for_forms() {
        let statements = body(
            "for { break }\nfor true { continue }\nfor i := 0; i < 10; i += 1 {}\nfor ; ; {}\nfor value in values {}\nfor i, value in &mut values {}",
        );
        assert_eq!(statements.len(), 6);
        assert!(matches!(
            statements[0].kind,
            StmtKind::For {
                condition: None,
                ..
            }
        ));
        assert!(matches!(
            statements[2].kind,
            StmtKind::For {
                init: Some(_),
                condition: Some(_),
                step: Some(_),
                ..
            }
        ));
        assert!(
            matches!(&statements[5].kind, StmtKind::ForEach { index: Some(index), iterable, .. } if index == "i" && matches!(iterable.kind, ExprKind::Unary(UnaryOp::BorrowMut, _)))
        );
    }
    #[test]
    fn structs_methods_and_borrow_contracts() {
        let program = parse("package sample\n@repr(C)\npub struct Sample {\n pub [4]u16 values\n pub fn view(self: &Self) -> &[u16] from(self) { return &self.values }\n}\n").unwrap();
        assert!(program.structs[0].repr_c);
        assert_eq!(program.functions[0].name, "Sample.view");
        assert_eq!(
            program.functions[0].params[0].ty,
            Type::Ref(false, Box::new(Type::Named("Sample".into())))
        );
        assert_eq!(program.functions[0].from, ["self"]);
    }
    #[test]
    fn generic_types_functions_and_nested_closing_angles() {
        let program = parse("package sample\nstruct Box<T> { T value\n fn get(self: &Self) -> &T { return &self.value }\n}\nfn id<T>(value: T) -> T { return value }\nfn demo(x: Option<Result<u8, bool>>) -> void {\n y := id<u8>(1)\n z := id::<Option<u8>>(none)\n}\n").unwrap();
        assert_eq!(program.functions[0].generics, ["T"]);
        assert!(matches!(program.functions[2].params[0].ty, Type::Option(_)));
    }
    #[test]
    fn firmware_attributes_and_inline_assembly() {
        let program = parse(
            "package p\n@interrupt(TIMER_IRQ_0)\n@section(\".ramfunc\") @align(8)\nfn tick() {\n unsafe {\n  value := asm(\"mrs {0}, primask\", \"cpsid i\", out(reg) u32, in(\"r0\") 1u32, inout(reg) x, clobber(\"r1\", \"memory\"), options(pure, nomem),)\n }\n}\n@section(\".noinit\")\nstatic mut RESETS: u32 = 0\n",
        )
        .unwrap();
        let tick = &program.functions[0];
        assert_eq!(tick.interrupt.as_deref(), Some("TIMER_IRQ_0"));
        assert_eq!(tick.placement.section.as_deref(), Some(".ramfunc"));
        assert_eq!(tick.placement.align, Some(8));
        let constant = &program.constants[0];
        assert!(constant.static_ && constant.mutable);
        assert_eq!(constant.placement.section.as_deref(), Some(".noinit"));
        let StmtKind::Unsafe(body) = &tick.body.as_ref().unwrap()[0].kind else {
            panic!("unsafe block");
        };
        let StmtKind::Let {
            value: Some(value), ..
        } = &body[0].kind
        else {
            panic!("binding");
        };
        let ExprKind::Asm(asm) = &value.kind else {
            panic!("asm expression");
        };
        assert_eq!(asm.template, "mrs {0}, primask\ncpsid i");
        assert_eq!(asm.operands.len(), 3);
        assert!(matches!(
            asm.operands[0].kind,
            AsmOperandKind::Out(Type::Int { bits: 32, .. })
        ));
        assert_eq!(asm.operands[1].register, AsmRegister::Named("r0".into()));
        assert!(matches!(asm.operands[2].kind, AsmOperandKind::InOut(_)));
        assert_eq!(asm.clobbers, ["r1", "memory"]);
        assert!(asm.pure && asm.nomem);
        for (source, message) in [
            (
                "@interrupt(T)\nconst A: u32 = 1",
                "@interrupt applies only to functions",
            ),
            ("@align(12)\nfn f() {}", "alignment must be a power of two"),
            (
                "@section(\".x\")\nenum E { A }",
                "apply only to functions and statics",
            ),
            ("fn f() { asm() }", "asm expects a template string"),
            ("fn f() { asm(\"nop\", in(reg)) }", "expected an expression"),
            (
                "fn f() { asm(\"nop\", sideways(reg) 1) }",
                "expected an asm operand",
            ),
        ] {
            let error = parse(&format!("package p\n{source}\n")).unwrap_err();
            assert!(
                error.message.contains(message),
                "{source}: {}",
                error.message
            );
        }
    }
    #[test]
    fn tuples_and_type_aliases() {
        let program = parse(
            "package p\npub type Pair<T>=(T, T)\ntype Id = u32\nfn f(p: (u8, (bool, &str))) -> Pair<u8> {\nlet (a, (b, _)) = p\nfor &(x, y) in pairs { }\nfor (i < 3) { }\nreturn (p.0, p.1.0 as u8)\n}\n",
        )
        .unwrap();
        assert_eq!(program.aliases.len(), 2);
        assert!(program.aliases[0].public);
        assert_eq!(program.aliases[0].generics, ["T"]);
        assert_eq!(program.aliases[0].ty.to_string(), "(T, T)");
        assert_eq!(
            program.functions[0].params[0].ty.to_string(),
            "(u8, (bool, &str))"
        );
        let body = program.functions[0].body.as_ref().unwrap();
        assert!(matches!(
            &body[0].kind,
            StmtKind::LetPattern { pattern: Pattern::Struct(name, fields, false), .. }
                if *name == tuple_name(2) && fields[1].0 == "1"
        ));
        // `&(x, y)` copies each bound element out of the element reference.
        let StmtKind::ForEach {
            name,
            copy: false,
            body: loop_body,
            ..
        } = &body[1].kind
        else {
            panic!("expected a destructuring loop");
        };
        assert_eq!(name, "$element");
        assert_eq!(loop_body.len(), 3);
        assert!(matches!(
            &body[2].kind,
            StmtKind::For {
                condition: Some(_),
                ..
            }
        ));
        let StmtKind::Return(Some(Expr {
            kind: ExprKind::Struct(name, fields),
            ..
        })) = &body[3].kind
        else {
            panic!("expected a tuple literal");
        };
        assert_eq!(*name, tuple_name(2));
        assert!(matches!(&fields[1].1.kind, ExprKind::Cast(inner, _)
            if matches!(&inner.kind, ExprKind::Field(base, index)
                if index == "0" && matches!(&base.kind, ExprKind::Field(_, one) if one == "1"))));
        for source in [
            "package p\ntype = u8",
            "package p\nunsafe type A = u8",
            "package p\ntype A<T: I> = T",
            "package p\nfn f() { x := () }",
            "package p\nfn f() { x := (1,) }",
            "package p\nfn f(x: ()) {}",
            "package p\nfn f() { let (a,) = b }",
            "package p\nfn f() { let type = 1 }",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }
    #[test]
    fn result_binds_outside_borrow_types() {
        let program = parse("package p\nfn f(a: &u8) -> &u8!bool { return ok(a) }\n").unwrap();
        assert_eq!(
            program.functions[0].ret,
            Type::Result(
                Box::new(Type::Ref(false, Box::new(Type::u8()))),
                Box::new(Type::Bool)
            )
        );
    }
    #[test]
    fn foreign_prototypes_and_enum_payload_patterns() {
        let program = parse("package p\nunsafe extern \"C\" fn send(p: *const u8, n: usize) -> i32\npub enum Error { Empty, Invalid(u8 byte), Other(code: u32) }\nfn f(e: Error) -> void { match e { Error.Empty => {} Error.Invalid(byte) => {} _ => {} } }\n").unwrap();
        assert!(program.functions[0].body.is_none());
        assert_eq!(program.enums[0].variants[1].fields[0].name, "byte");
        assert_eq!(program.enums[0].variants[2].fields[0].name, "code");
    }
    #[test]
    fn arrays_strings_nested_blocks_and_multiline_calls() {
        let statements = body(
            "a := [3]u8{1,\n2, 3,}\ns := b\"hi\"\nf(\n 1 +\n 2,\n a[\n0\n],\n)\nif true {\n{ x := 1 }\n} else if false {} else {}\nunsafe {}\nu32 count\nconst u32 LIMIT = 3",
        );
        assert_eq!(statements.len(), 7);
    }
    #[test]
    fn rejects_missing_terminators_and_malformed_declarations() {
        for source in [
            "fn f() -> void {}",
            "package p\nfn f() -> void { x := 1 y := 2 }",
            "package p\nfn f() -> void {",
            "package p\nfn f() -> void { return (1 }",
            "package p\n@unknown fn f() -> void {}",
            "package p\nfn f() -> Self {}",
        ] {
            assert!(parse(source).is_err(), "accepted {source:?}");
        }
    }
    #[test]
    fn malformed_token_sequences_do_not_panic() {
        let pieces = [
            "fn", "struct", "enum", "for", "if", "match", "return", "(", ")", "{", "}", "[", "]",
            "<", ">>", "&", "*", "x", "1", "\n",
        ];
        for first in pieces {
            for second in pieces {
                let source = format!("package p\nfn f() -> void {{ {first} {second} }}");
                assert!(
                    std::panic::catch_unwind(|| parse(&source)).is_ok(),
                    "panicked for {source:?}"
                );
            }
        }
    }
    #[test]
    fn editor_recovery_advances_and_preserves_later_declarations() {
        let source =
            "package app\nfn broken() {\na := ;\nb := ;\n}\nfn good() -> i32 { return 3 }\n";
        let (program, errors) = parse_recovering(source);
        assert_eq!(errors.len(), 2);
        assert_eq!(program.functions.len(), 2);
        assert!(parse(source).is_err());
        let (program, errors) = parse_recovering(
            "package app\nstruct Bad {\nfn partial() {}\nfield: }\nfn good() {}\n",
        );
        assert!(!errors.is_empty());
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "good");
        let pieces = [
            "fn", "{", "}", "(", ")", "<", ">>", "let", ":=", ";", "😀", "\"", "a", "\n",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    let source = format!("package app\nfn main() {{ {a} {b} {c} }}\n");
                    assert!(
                        std::panic::catch_unwind(|| parse_recovering(&source)).is_ok(),
                        "{source}"
                    );
                }
            }
        }
    }

    #[test]
    fn editor_recovery_preserves_test_attributes_after_bad_declarations() {
        let source = "package app\nbroken declaration\n@test\n@ignore(\"needs hardware\")\nfn hardware() {}\n";
        let (program, errors) = parse_recovering(source);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(program.functions.len(), 1);
        let function = &program.functions[0];
        assert_eq!(function.name, "hardware");
        assert!(function.test);
        assert_eq!(function.ignore.as_deref(), Some("needs hardware"));
        assert!(parse(source).is_err());
    }

    #[test]
    fn strict_and_recovering_parsers_reject_test_attributes_on_non_functions() {
        for attribute in ["@test", "@ignore(\"later\")"] {
            for declaration in [
                "struct Invalid {}",
                "enum Invalid { Value }",
                "const invalid: i32 = 1",
                "static invalid: i32 = 1",
            ] {
                let source = format!(
                    "package app\n{attribute} {declaration}\n@test fn good() {{ assert(true) }}\n"
                );
                let expected = "@test and @ignore apply only to functions";
                assert_eq!(
                    parse(&source).unwrap_err().message.as_ref(),
                    expected,
                    "{source}"
                );
                let (program, errors) = parse_recovering(&source);
                assert_eq!(errors.len(), 1, "{source}: {errors:?}");
                assert_eq!(errors[0].message.as_ref(), expected);
                assert_eq!(program.functions.len(), 1);
                assert_eq!(program.functions[0].name, "good");
                assert!(program.functions[0].test);
            }
        }
    }

    #[test]
    fn excessive_nesting_has_a_diagnostic() {
        let source = format!(
            "package p\nfn f() -> void {{ x := {}1{} }}",
            "(".repeat(300),
            ")".repeat(300)
        );
        assert!(parse(&source).unwrap_err().message.contains("nesting"));
    }
    #[test]
    fn complete_worked_examples_from_the_specification_parse() {
        let spec = include_str!("../docs/src/content/docs/language-spec-0.1.md");
        let mut examples = 0;
        for block in spec.split("```dodo\n").skip(1) {
            let source = block.split("```").next().unwrap();
            if !source.starts_with("package ") {
                continue;
            }
            parse(source)
                .unwrap_or_else(|error| panic!("{}", error.render("language-spec-0.1.md", source)));
            examples += 1;
        }
        assert!(
            examples >= 4,
            "expected the package example and all three worked examples"
        );
    }
    #[test]
    fn qualified_generic_struct_literals_and_negative_patterns() {
        let statements =
            body("x := lib.Box<Option<u8>>{value: none}\nmatch value { -1 => {} _ => {} }");
        assert!(
            matches!(&statements[0].kind, StmtKind::Let { value: Some(Expr { kind: ExprKind::Struct(name, _), .. }), .. } if name == "lib.Box<Option<u8>>")
        );
        assert!(
            matches!(&statements[1].kind, StmtKind::Match { arms, .. } if matches!(arms[0].pattern, Pattern::Int(u64::MAX)))
        );
    }
    #[test]
    fn expression_arms_work_in_statement_and_value_matches() {
        let statements = body(
            "match result { ok(value) => consume(value), err(error) => report(error), }\n\
             answer := match optional { some(value) => value, none => { log(); 0 } }",
        );
        let StmtKind::Match { arms, .. } = &statements[0].kind else {
            panic!("expected statement match");
        };
        assert!(
            arms.iter()
                .all(|arm| matches!(arm.body[0].kind, StmtKind::Expr(_)))
        );
        let StmtKind::Let {
            value:
                Some(Expr {
                    kind: ExprKind::ValueBlock(block),
                    ..
                }),
            ..
        } = &statements[1].kind
        else {
            panic!("expected value match");
        };
        let StmtKind::Match { arms, .. } = &block[0].kind else {
            panic!("expected value match arms");
        };
        assert!(
            arms.iter()
                .all(|arm| matches!(arm.body.last().unwrap().kind, StmtKind::Yield(_)))
        );
    }
    #[test]
    fn function_types_closures_and_value_calls() {
        let program = parse(
            "package p\nfn f(g: fn(i32, &u8) -> bool, h: &mut dyn fn(), k: &fn() -> u8!E) {\n\
             c := fn(a, b: u32) -> u32 { a + b }\n\
             d := fn() { total += 1; }\n\
             (table[0])(1)\n\
             c(1, 2)\n\
             }\n",
        )
        .unwrap();
        let params = &program.functions[0].params;
        assert_eq!(params[0].ty.to_string(), "fn(i32, &u8) -> bool");
        assert_eq!(params[1].ty.to_string(), "&mut dyn fn()");
        // A function pointer's result type includes `!E`; `&fn` is a plain reference.
        assert_eq!(params[2].ty.to_string(), "&fn() -> Result<u8, E>");
        let body = program.functions[0].body.as_ref().unwrap();
        let StmtKind::Let { value: Some(c), .. } = &body[0].kind else {
            panic!("expected a binding");
        };
        let ExprKind::Closure(closure) = &c.kind else {
            panic!("expected a closure");
        };
        assert_eq!(closure.params[0].ty, Type::Unknown);
        assert_eq!(closure.params[1].ty.to_string(), "u32");
        assert!(matches!(closure.body[0].kind, StmtKind::Yield(_)));
        let StmtKind::Let { value: Some(d), .. } = &body[1].kind else {
            panic!("expected a binding");
        };
        let ExprKind::Closure(discarded) = &d.kind else {
            panic!("expected a closure");
        };
        assert_eq!(discarded.ret, Type::Unknown);
        assert!(matches!(discarded.body[0].kind, StmtKind::Assign { .. }));
        assert!(matches!(
            &body[2].kind,
            StmtKind::Expr(Expr {
                kind: ExprKind::Invoke { .. },
                ..
            })
        ));
        assert!(matches!(
            &body[3].kind,
            StmtKind::Expr(Expr {
                kind: ExprKind::Call { .. },
                ..
            })
        ));
        assert!(parse("package p\nfn f() { (table[0])::<u8>(1) }\n").is_err());
    }
    #[test]
    fn function_tails_return_values_and_semicolons_discard_them() {
        let program = parse(
            "package p\n\
             fn add(a: u32, b: u32) -> u32 { a + b }\n\
             fn discard() -> u32 { 1; }\n\
             fn choose(b: bool) -> u32 { if b { 1 } else { return 2 } }\n\
             fn discard_match(b: bool) -> u32 { match b { true => 1, false => 2 }; }\n\
             fn side_effect() { consume() }",
        )
        .unwrap();
        assert!(matches!(
            program.functions[0].body.as_ref().unwrap()[0].kind,
            StmtKind::Return(Some(_))
        ));
        assert!(matches!(
            program.functions[1].body.as_ref().unwrap()[0].kind,
            StmtKind::Expr(_)
        ));
        let StmtKind::If {
            then_block,
            else_block,
            ..
        } = &program.functions[2].body.as_ref().unwrap()[0].kind
        else {
            panic!("expected conditional tail");
        };
        assert!(matches!(then_block[0].kind, StmtKind::Return(Some(_))));
        assert!(matches!(else_block[0].kind, StmtKind::Return(Some(_))));
        let StmtKind::Match { arms, .. } = &program.functions[3].body.as_ref().unwrap()[0].kind
        else {
            panic!("expected discarded match");
        };
        assert!(
            arms.iter()
                .all(|arm| matches!(arm.body[0].kind, StmtKind::Expr(_)))
        );
        assert!(matches!(
            program.functions[4].body.as_ref().unwrap()[0].kind,
            StmtKind::Expr(_)
        ));

        let statements =
            body("x := { 1; }\ny := { 2 }\nz := match true { true => { 3; }, false => 4 }");
        let StmtKind::Let {
            value:
                Some(Expr {
                    kind: ExprKind::ValueBlock(block),
                    ..
                }),
            ..
        } = &statements[0].kind
        else {
            panic!("expected value block")
        };
        assert!(matches!(block[0].kind, StmtKind::Expr(_)));
        let StmtKind::Let {
            value:
                Some(Expr {
                    kind: ExprKind::ValueBlock(block),
                    ..
                }),
            ..
        } = &statements[1].kind
        else {
            panic!("expected value block")
        };
        assert!(matches!(block[0].kind, StmtKind::Yield(_)));
        let StmtKind::Let {
            value:
                Some(Expr {
                    kind: ExprKind::ValueBlock(block),
                    ..
                }),
            ..
        } = &statements[2].kind
        else {
            panic!("expected match")
        };
        let StmtKind::Match { arms, .. } = &block[0].kind else {
            panic!("expected match")
        };
        assert!(matches!(arms[0].body[0].kind, StmtKind::Expr(_)));
    }
    #[test]
    fn immutable_runtime_bindings_are_distinct_from_constants_and_mutable_locals() {
        let statements = body(
            "let limit = read_limit()\nlet size: u32 = read_limit()\ncount := 0u32\nconst CAPACITY: usize = 256",
        );
        assert!(matches!(
            &statements[0].kind,
            StmtKind::Let {
                ty: Type::Unknown,
                constant: false,
                mutable: false,
                ..
            }
        ));
        assert!(matches!(
            &statements[1].kind,
            StmtKind::Let {
                ty: Type::Int { bits: 32, .. },
                constant: false,
                mutable: false,
                ..
            }
        ));
        assert!(matches!(
            &statements[2].kind,
            StmtKind::Let {
                constant: false,
                mutable: true,
                ..
            }
        ));
        assert!(matches!(
            &statements[3].kind,
            StmtKind::Let {
                constant: true,
                mutable: false,
                ..
            }
        ));
    }
    #[test]
    fn recursive_patterns_ranges_alternatives_and_guards_share_a_grammar() {
        let statements = body(
            "match optional {\n\
             some(some(Point { x: 1..=9, y, .. })) | some(some(Point { x: 20..30, y, .. })) if y > 0 => consume(y),\n\
             some(none) | none => {},\n\
             _ => {},\n}\n\
             match byte { b'0'..=b'9' => {}, -3..-1 => {}, _ => {} }",
        );
        let StmtKind::Match { arms, .. } = &statements[0].kind else {
            panic!("expected match")
        };
        assert!(matches!(arms[0].pattern, Pattern::Or(_)));
        assert_eq!(arms[0].pattern.bindings(), vec!["y"]);
        assert!(arms[0].guard.is_some());
        let Pattern::Or(alternatives) = &arms[0].pattern else {
            panic!("expected alternatives")
        };
        let Pattern::Variant(_, outer) = &alternatives[0] else {
            panic!("expected variant")
        };
        let Pattern::Variant(_, inner) = &outer[0] else {
            panic!("expected nested variant")
        };
        let Pattern::Struct(_, fields, rest) = &inner[0] else {
            panic!("expected nested struct")
        };
        assert!(*rest);
        assert!(matches!(fields[0].1, Pattern::Range(1, 9, true)));
        let StmtKind::Match { arms, .. } = &statements[1].kind else {
            panic!("expected match")
        };
        assert!(matches!(arms[0].pattern, Pattern::Range(48, 57, true)));
        assert!(
            matches!(arms[1].pattern, Pattern::Range(start, end, false) if start == (-3i64 as u64) && end == u64::MAX)
        );
    }
    #[test]
    fn conditional_and_early_exit_destructuring_use_recursive_patterns() {
        let statements = body(
            "if let some(some(value)) = optional { consume(value) } else if let some(value) = other { consume(value) }\n\
             let some(Point { x: value, .. }): Option<Point> = optional else { return }\n\
             let Point { x, y } = point",
        );
        let StmtKind::IfLet {
            pattern,
            else_block,
            ..
        } = &statements[0].kind
        else {
            panic!("expected if let")
        };
        assert_eq!(pattern.bindings(), vec!["value"]);
        assert!(matches!(else_block[0].kind, StmtKind::IfLet { .. }));
        assert!(matches!(
            &statements[1].kind,
            StmtKind::LetPattern {
                ty: Type::Option(_),
                else_block: Some(_),
                ..
            }
        ));
        assert!(matches!(
            &statements[2].kind,
            StmtKind::LetPattern {
                pattern: Pattern::Struct(_, _, false),
                else_block: None,
                ..
            }
        ));
    }
    #[test]
    fn malformed_patterns_and_bindings_report_errors() {
        for source in [
            "let limit",
            "let limit := 3",
            "if let some(value) optional {}",
            "match x { 1..= => {} }",
            "match x { some(some(value) => {} }",
            "match x { Point { .., x } => {} }",
            "match x { true => f() false => g() }",
            "let some(x) = optional else return",
        ] {
            assert!(
                parse(&format!("package p\nfn f() {{ {source} }}")).is_err(),
                "accepted {source:?}"
            );
        }
        let source = format!(
            "package p\nfn f() {{ match x {{ {}v{} => {{}} }} }}",
            "some(".repeat(100),
            ")".repeat(100)
        );
        assert!(parse(&source).unwrap_err().message.contains("nesting"));
    }
    #[test]
    fn qualified_generic_pattern_names_preserve_nested_type_arguments() {
        let statements = body(
            "match value {\n\
             lib.Choice<Option<u8>>.Value(lib.Box<Option<u8>> { value: some(inner) }) => consume(inner),\n\
             lib.Choice.Value::<Option<u8>>(lib.Box::<Option<u8>> { value: none }) => {},\n\
             _ => {},\n}",
        );
        let StmtKind::Match { arms, .. } = &statements[0].kind else {
            panic!("expected match")
        };
        let Pattern::Variant(name, fields) = &arms[0].pattern else {
            panic!("expected variant")
        };
        assert_eq!(name, "lib.Choice<Option<u8>>.Value");
        assert!(matches!(&fields[0], Pattern::Struct(name, _, _) if name == "lib.Box<Option<u8>>"));
        let Pattern::Variant(name, fields) = &arms[1].pattern else {
            panic!("expected variant")
        };
        assert_eq!(name, "lib.Choice.Value<Option<u8>>");
        assert!(matches!(&fields[0], Pattern::Struct(name, _, _) if name == "lib.Box<Option<u8>>"));
    }
    #[test]
    fn semicolons_on_compound_tails_suppress_all_implicit_exits() {
        for statement in [
            "if true { 1 } else { 2 }",
            "if let some(x) = optional { x } else { 2 }",
            "match true { true => 1, false => 2 }",
            "{ 1 }",
            "unsafe { 1 }",
        ] {
            let source = format!("package p\nfn f() -> u32 {{ {statement}\n; }}");
            let block = parse(&source).unwrap().functions.remove(0).body.unwrap();
            let assert_discarded = |tail: &Stmt| match &tail.kind {
                StmtKind::If {
                    then_block,
                    else_block,
                    ..
                }
                | StmtKind::IfLet {
                    then_block,
                    else_block,
                    ..
                } => {
                    assert!(matches!(then_block[0].kind, StmtKind::Expr(_)));
                    assert!(matches!(else_block[0].kind, StmtKind::Expr(_)));
                }
                StmtKind::Match { arms, .. } => {
                    assert!(
                        arms.iter()
                            .all(|arm| matches!(arm.body[0].kind, StmtKind::Expr(_)))
                    );
                }
                StmtKind::Block(block) | StmtKind::Unsafe(block) => {
                    assert!(matches!(block[0].kind, StmtKind::Expr(_)));
                }
                _ => panic!("expected compound tail"),
            };
            assert_discarded(&block[0]);
            let statements = body(&format!("x := {{ {statement}; }}"));
            let StmtKind::Let {
                value:
                    Some(Expr {
                        kind: ExprKind::ValueBlock(block),
                        ..
                    }),
                ..
            } = &statements[0].kind
            else {
                panic!("expected value block")
            };
            assert_discarded(&block[0]);
        }
    }
    #[test]
    fn if_let_expression_tails_yield_and_function_tails_return() {
        let statements = body(
            "let x = if let some(x) = optional { x } else if let some(y) = other { y } else { 0 }",
        );
        let StmtKind::Let {
            value:
                Some(Expr {
                    kind: ExprKind::ValueBlock(block),
                    ..
                }),
            ..
        } = &statements[0].kind
        else {
            panic!("expected if let value")
        };
        let StmtKind::IfLet {
            then_block,
            else_block,
            ..
        } = &block[0].kind
        else {
            panic!("expected if let")
        };
        assert!(matches!(then_block[0].kind, StmtKind::Yield(_)));
        let StmtKind::IfLet {
            then_block,
            else_block,
            ..
        } = &else_block[0].kind
        else {
            panic!("expected else if let")
        };
        assert!(matches!(then_block[0].kind, StmtKind::Yield(_)));
        assert!(matches!(else_block[0].kind, StmtKind::Yield(_)));
        let block =
            parse("package p\nfn f() -> u32 { if let some(x) = optional { x } else { 0 } }")
                .unwrap()
                .functions
                .remove(0)
                .body
                .unwrap();
        let StmtKind::IfLet {
            then_block,
            else_block,
            ..
        } = &block[0].kind
        else {
            panic!("expected if let")
        };
        assert!(matches!(then_block[0].kind, StmtKind::Return(Some(_))));
        assert!(matches!(else_block[0].kind, StmtKind::Return(Some(_))));
    }
    #[test]
    fn reserved_let_and_malformed_generic_patterns_report_errors() {
        for source in [
            "package p\nfn let() {}",
            "package p\nstruct let {}",
            "package p\nfn f(let: u8) {}",
            "package p\nfn f() { match x { some(let) => {} } }",
            "package p\nfn f() { let let = 1 }",
            "package p\nfn f() { match x { Box<> { value } => {} } }",
            "package p\nfn f() { match x { Box::<u8 { value } => {} } }",
            "package p\nfn f() { match x { Choice<u8>. => {} } }",
            "package p\nfn f() { match x { Self {} => {} } }",
        ] {
            assert!(parse(source).is_err(), "accepted {source:?}");
        }
        let source = format!(
            "package p\nfn f() {{ match x {{ Box<{}u8{}> {{value}} => {{}} }} }}",
            "Option<".repeat(100),
            ">".repeat(100)
        );
        assert!(parse(&source).unwrap_err().message.contains("nesting"));
    }
}
