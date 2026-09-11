//! Comment-preserving formatting and migration to canonical Dodo syntax.
use std::collections::HashSet;

use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::lexer::{Token, TokenKind, lex};
use crate::parser::parse;

/// Format a complete file without resolving imports or requiring type checking.
/// Comments, literal spellings, declaration order, and explicit types survive
/// migration. Parsed syntax is checked before returning the replacement source.
pub fn format_source(source: &str) -> Result<String, Diagnostic> {
    let original = parse(source)?;
    let tokens = lex(source)?;
    let mut plan = Plan::new(source, &tokens);
    plan.program(&original);
    let migrated = apply_edits(source, plan.edits)?;
    let migrated_program = parse(&migrated).map_err(|error| {
        format_error(format!(
            "syntax migration could not be parsed: {}",
            error.message
        ))
    })?;
    if !same_syntax(&original, &migrated_program) {
        return Err(format_error("syntax migration would change the program"));
    }
    let tokens = lex(&migrated)?;
    let mut layout = Plan::new(&migrated, &tokens);
    layout.program(&migrated_program);
    let formatted = render(&migrated, &tokens, &layout);
    let formatted_program = parse(&formatted).map_err(|error| {
        format_error(format!(
            "formatted output could not be parsed: {}",
            error.message
        ))
    })?;
    if !same_syntax(&original, &formatted_program) {
        return Err(format_error("formatting would change the program"));
    }
    Ok(formatted)
}

fn format_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(Span::default(), message)
}

struct Edit {
    span: Span,
    replacement: String,
}

fn apply_edits(source: &str, mut edits: Vec<Edit>) -> Result<String, Diagnostic> {
    edits.sort_by_key(|edit| (edit.span.start, edit.span.end));
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in edits {
        if edit.span.start < cursor {
            return Err(format_error("overlapping syntax migrations"));
        }
        output.push_str(&source[cursor..edit.span.start]);
        output.push_str(&edit.replacement);
        cursor = edit.span.end;
    }
    output.push_str(&source[cursor..]);
    Ok(output)
}

struct Plan<'a> {
    source: &'a str,
    tokens: &'a [Token],
    edits: Vec<Edit>,
    binary: HashSet<usize>,
    literal_braces: HashSet<usize>,
    header_semicolons: HashSet<usize>,
}

impl<'a> Plan<'a> {
    fn new(source: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            source,
            tokens,
            edits: Vec::new(),
            binary: HashSet::new(),
            literal_braces: HashSet::new(),
            header_semicolons: HashSet::new(),
        }
    }

    fn between(&self, start: usize, end: usize) -> impl Iterator<Item = &'a Token> + use<'a> {
        let first = self
            .tokens
            .partition_point(|token| token.span.start < start);
        let last = self.tokens.partition_point(|token| token.span.start < end);
        self.tokens[first..last]
            .iter()
            .filter(|token| !matches!(token.kind, TokenKind::Newline | TokenKind::Eof))
    }

    fn replace(&mut self, span: Span, replacement: impl Into<String>) {
        self.edits.push(Edit {
            span,
            replacement: replacement.into(),
        });
    }

    fn declaration(&mut self, name: &str, span: Span, value: Option<&Expr>) {
        let end = value.map_or(span.end, |value| value.span.start);
        let tokens: Vec<_> = self.between(span.start, end).collect();
        if tokens
            .first()
            .is_some_and(|token| matches!(&token.kind, TokenKind::Ident(word) if word == "let"))
        {
            return;
        }
        let Some(first) = tokens.iter().position(|token| !matches!(&token.kind, TokenKind::Ident(word) if matches!(word.as_str(), "pub" | "const" | "static" | "mut"))) else { return };
        if tokens
            .get(first + 1)
            .is_some_and(|token| matches!(token.kind, TokenKind::Symbol(":" | ":=")))
        {
            return;
        }
        let Some(binding) = tokens
            .iter()
            .rev()
            .find(|token| matches!(&token.kind, TokenKind::Ident(word) if word == name))
        else {
            return;
        };
        self.replace(
            Span {
                start: tokens[first].span.start,
                end: tokens[first].span.start,
            },
            format!("{name}: "),
        );
        self.replace(binding.span, "");
    }

    fn program(&mut self, program: &Program) {
        for structure in &program.structs {
            for field in &structure.fields {
                self.declaration(&field.name, field.span, None);
            }
        }
        for enumeration in &program.enums {
            for variant in &enumeration.variants {
                for field in &variant.fields {
                    self.declaration(&field.name, field.span, None);
                }
            }
        }
        for constant in &program.constants {
            self.declaration(&constant.name, constant.span, Some(&constant.value));
            self.expression(&constant.value);
        }
        for function in &program.functions {
            if let Some(body) = &function.body {
                self.block(body);
            }
        }
    }

    fn block(&mut self, block: &Block) {
        for statement in block {
            self.statement(statement);
        }
    }

    fn pattern_braces(&mut self, start: usize, end: usize) {
        for token in self.between(start, end) {
            if matches!(token.kind, TokenKind::Symbol("{")) {
                self.literal_braces.insert(token.span.start);
            }
        }
    }

    fn statement(&mut self, statement: &Stmt) {
        match &statement.kind {
            StmtKind::Let { name, value, .. } => {
                self.declaration(name, statement.span, value.as_ref());
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            StmtKind::Assign { target, value, .. } => {
                self.expression(target);
                self.expression(value);
            }
            StmtKind::Expr(value) | StmtKind::Yield(value) => self.expression(value),
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.expression(value);
                }
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
                self.pattern_braces(statement.span.start, value.span.start);
                self.expression(value);
                self.block(then_block);
                self.block(else_block);
            }
            StmtKind::LetPattern {
                value, else_block, ..
            } => {
                self.pattern_braces(statement.span.start, value.span.start);
                self.expression(value);
                if let Some(body) = else_block {
                    self.block(body);
                }
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                let start = init
                    .as_ref()
                    .map_or(statement.span.start, |init| init.span.end);
                if let Some(first) = self
                    .between(start, statement.span.end)
                    .find(|token| matches!(token.kind, TokenKind::Symbol(";" | "{")))
                    .filter(|token| matches!(token.kind, TokenKind::Symbol(";")))
                {
                    self.header_semicolons.insert(first.span.start);
                    let start = condition
                        .as_ref()
                        .map_or(first.span.end, |condition| condition.span.end);
                    if let Some(second) = self
                        .between(start, statement.span.end)
                        .find(|token| matches!(token.kind, TokenKind::Symbol(";")))
                    {
                        self.header_semicolons.insert(second.span.start);
                    }
                }
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
                    let end = arm.guard.as_ref().map_or_else(
                        || {
                            self.between(arm.span.start, arm.span.end)
                                .find(|token| matches!(token.kind, TokenKind::Symbol("=>")))
                                .map_or(arm.span.start, |token| token.span.start)
                        },
                        |guard| guard.span.start,
                    );
                    self.pattern_braces(arm.span.start, end);
                    if let Some(guard) = &arm.guard {
                        self.expression(guard);
                    }
                    self.block(&arm.body);
                }
            }
            StmtKind::Block(body) | StmtKind::Unsafe(body) => self.block(body),
            StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn expression(&mut self, expression: &Expr) {
        match &expression.kind {
            ExprKind::Array(_, values) => {
                let end = values
                    .first()
                    .map_or(expression.span.end, |value| value.span.start);
                let mut prefix = self.between(expression.span.start, end);
                if let Some(start) =
                    prefix.find(|token| matches!(token.kind, TokenKind::Symbol("[")))
                    && let Some(open) =
                        prefix.find(|token| matches!(token.kind, TokenKind::Symbol("{")))
                {
                    let mut depth = 1;
                    let close = self
                        .between(open.span.end, expression.span.end)
                        .find(|token| {
                            match token.kind {
                                TokenKind::Symbol("{") => depth += 1,
                                TokenKind::Symbol("}") => depth -= 1,
                                _ => {}
                            }
                            depth == 0
                        });
                    if let Some(close) = close {
                        let ty = self.source[start.span.start..open.span.start].trim_end();
                        self.replace(
                            Span {
                                start: start.span.start,
                                end: open.span.end,
                            },
                            "([",
                        );
                        self.replace(close.span, format!("]: {ty})"));
                    }
                }
                for value in values {
                    self.expression(value);
                }
            }
            ExprKind::Call {
                type_args, args, ..
            } => {
                if !type_args.is_empty() {
                    let end = args
                        .first()
                        .map_or(expression.span.end, |arg| arg.span.start);
                    let mut previous = None;
                    for token in self.between(expression.span.start, end) {
                        if matches!(token.kind, TokenKind::Symbol("<")) {
                            if previous != Some("::") {
                                self.replace(
                                    Span {
                                        start: token.span.start,
                                        end: token.span.start,
                                    },
                                    "::",
                                );
                            }
                            break;
                        }
                        previous = match token.kind {
                            TokenKind::Symbol(symbol) => Some(symbol),
                            _ => None,
                        };
                    }
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
            ExprKind::Struct(_, fields) => {
                let end = fields
                    .first()
                    .map_or(expression.span.end, |(_, value)| value.span.start);
                if let Some(open) = self
                    .between(expression.span.start, end)
                    .find(|token| matches!(token.kind, TokenKind::Symbol("{")))
                {
                    self.literal_braces.insert(open.span.start);
                }
                for (_, value) in fields {
                    self.expression(value);
                }
            }
            ExprKind::Binary(_, left, right) => {
                for token in self.between(left.span.end, right.span.start) {
                    if matches!(token.kind, TokenKind::Symbol(_)) {
                        self.binary.insert(token.span.start);
                    }
                }
                self.expression(left);
                self.expression(right);
            }
            ExprKind::Range(left, right) | ExprKind::Index(left, right) => {
                self.expression(left);
                self.expression(right);
            }
            ExprKind::Repeat(value, _)
            | ExprKind::Constant(value, _)
            | ExprKind::Unary(_, value)
            | ExprKind::Field(value, _)
            | ExprKind::Cast(value, _)
            | ExprKind::Try(value) => self.expression(value),
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
            ExprKind::ValueBlock(body) => self.block(body),
            ExprKind::Int(..)
            | ExprKind::Float(..)
            | ExprKind::Bool(..)
            | ExprKind::String(..)
            | ExprKind::Name(..) => {}
        }
    }
}

#[derive(Clone, Copy)]
enum Item<'a> {
    Token(&'a Token, &'a str),
    Comment(&'a str),
    Newline,
}

fn items<'a>(source: &'a str, tokens: &'a [Token]) -> Vec<Item<'a>> {
    let mut result = Vec::new();
    let mut cursor = 0;
    for token in tokens {
        let gap = &source[cursor..token.span.start];
        if let Some(start) = gap.find("//") {
            result.push(Item::Comment(gap[start..].trim_end()));
        }
        match token.kind {
            TokenKind::Newline => result.push(Item::Newline),
            TokenKind::Eof => {}
            _ => result.push(Item::Token(
                token,
                &source[token.span.start..token.span.end],
            )),
        }
        cursor = token.span.end;
    }
    result
}

struct Frame {
    delimiter: &'static str,
    indented: bool,
    block: bool,
    empty: bool,
}

#[derive(Default)]
struct Writer {
    text: String,
    forced_newline: bool,
    source_newlines: usize,
}

impl Writer {
    fn flush(&mut self, indent: usize) -> bool {
        let newlines = self
            .source_newlines
            .max(usize::from(self.forced_newline))
            .min(2);
        let line_start = newlines > 0 || self.text.is_empty();
        if newlines > 0 && !self.text.is_empty() {
            while self.text.ends_with(' ') {
                self.text.pop();
            }
            let existing = self
                .text
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'\n')
                .count();
            for _ in existing..newlines {
                self.text.push('\n');
            }
        }
        if line_start {
            self.text.push_str(&"    ".repeat(indent));
        }
        self.forced_newline = false;
        self.source_newlines = 0;
        line_start
    }
    fn space(&mut self) {
        if !self.text.ends_with([' ', '\n']) && !self.text.is_empty() {
            self.text.push(' ');
        }
    }
}

fn render(source: &str, tokens: &[Token], plan: &Plan<'_>) -> String {
    let items = items(source, tokens);
    let mut writer = Writer::default();
    let mut frames: Vec<Frame> = Vec::new();
    let mut previous: Option<(&Token, &str)> = None;
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::Newline => {
                writer.source_newlines += 1;
            }
            Item::Comment(comment) => {
                if writer.source_newlines == 0 {
                    writer.forced_newline = false;
                }
                let indent = frames.iter().filter(|frame| frame.indented).count();
                let line_start = writer.flush(indent);
                if !line_start {
                    writer.space();
                }
                writer.text.push_str(comment);
                writer.forced_newline = true;
            }
            Item::Token(token, text) => {
                let symbol = match token.kind {
                    TokenKind::Symbol(symbol) => symbol,
                    _ => "",
                };
                if symbol == ";"
                    && !plan.header_semicolons.contains(&token.span.start)
                    && frames.last().is_none_or(|frame| frame.delimiter != "[")
                {
                    writer.text.push(';');
                    writer.forced_newline = true;
                    continue;
                }
                let frame = if matches!(symbol, "}" | ")" | "]") {
                    frames.pop()
                } else {
                    None
                };
                if let Some(frame) = &frame
                    && frame.delimiter == "{"
                    && frame.block
                    && !frame.empty
                {
                    writer.forced_newline = true;
                }
                if *text == "else"
                    && previous.is_some_and(|(_, text)| text == "}")
                    && writer.text.ends_with('}')
                {
                    writer.source_newlines = 0;
                    writer.forced_newline = false;
                }
                let continuation = writer.source_newlines > 0
                    && (symbol == "."
                        || previous.is_some_and(|(token, text)| {
                            plan.binary.contains(&token.span.start)
                                || matches!(text, "=" | ":=" | "->" | ":")
                        }));
                let indent = frames.iter().filter(|frame| frame.indented).count()
                    + usize::from(continuation);
                let line_start = writer.flush(indent);
                if !line_start && needs_space(previous, token, text, plan) {
                    writer.space();
                }
                if symbol == "}"
                    && frame
                        .as_ref()
                        .is_some_and(|frame| !frame.block && !frame.empty)
                    && !line_start
                {
                    writer.space();
                }
                writer.text.push_str(text);
                if matches!(symbol, "{" | "(" | "[") {
                    let next = items.get(index + 1);
                    let empty = matches!(next, Some(Item::Token(_, closing)) if matches!((symbol, *closing), ("{", "}") | ("(", ")") | ("[", "]")));
                    let block = symbol == "{" && !plan.literal_braces.contains(&token.span.start);
                    let indented = block || matches!(next, Some(Item::Newline | Item::Comment(_)));
                    frames.push(Frame {
                        delimiter: symbol,
                        indented,
                        block,
                        empty,
                    });
                    if block && !empty {
                        writer.forced_newline = true;
                    }
                }
                if symbol == "}" && frame.as_ref().is_some_and(|frame| frame.block) {
                    let next = items[index + 1..].iter().find_map(|item| match item {
                        Item::Token(token, text) => Some((*token, *text)),
                        _ => None,
                    });
                    if !next.is_some_and(|(token, text)| {
                        plan.binary.contains(&token.span.start)
                            || matches!(
                                text,
                                "else" | "," | ";" | ")" | "]" | "." | "?" | "as" | "(" | "["
                            )
                    }) {
                        writer.forced_newline = true;
                    }
                }
                previous = Some((token, text));
            }
        }
    }
    let mut output = writer.text.trim_end().to_owned();
    if !output.is_empty() {
        output.push('\n');
    }
    output
}

fn needs_space(
    previous: Option<(&Token, &str)>,
    current: &Token,
    text: &str,
    plan: &Plan<'_>,
) -> bool {
    let Some((before, previous)) = previous else {
        return false;
    };
    if matches!(
        text,
        "," | ";" | ")" | "]" | ":" | "." | "::" | "?" | ".." | "..="
    ) {
        return false;
    }
    if plan.binary.contains(&current.span.start) || plan.binary.contains(&before.span.start) {
        return true;
    }
    if matches!(previous, "." | "::" | "@" | ".." | "..=") {
        return false;
    }
    if text == "{" {
        return true;
    }
    if text == "}" {
        return false;
    }
    if matches!(previous, "(" | "[") {
        return false;
    }
    if matches!(text, "<" | ">" | ">>") || previous == "<" {
        return false;
    }
    if text == "(" {
        return matches!(
            previous,
            "if" | "for" | "match" | "return" | "in" | "=" | ":=" | "=>" | "," | ":"
        );
    }
    if text == "[" {
        return matches!(
            previous,
            "=" | ":=" | ":" | "," | ";" | "return" | "in" | "->"
        );
    }
    if matches!(previous, "&" | "*" | "!" | "~") {
        return false;
    }
    if previous == "-" && !plan.binary.contains(&before.span.start) {
        return false;
    }
    if previous == "]" && matches!(current.kind, TokenKind::Ident(_)) {
        return matches!(text, "as" | "in");
    }
    if text == "!" {
        return false;
    }
    true
}

// Debug output is structural and includes every AST field. Erase only its source
// locations so new syntax fields automatically participate in this safety check.
fn same_syntax(left: &Program, right: &Program) -> bool {
    fn key(program: &Program) -> String {
        let debug = format!("{program:?}");
        let mut result = String::with_capacity(debug.len());
        let mut rest = debug.as_str();
        while let Some(start) = rest.find("Span { start: ") {
            result.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}') else {
                break;
            };
            result.push_str("Span {}");
            rest = &rest[start + end + 1..];
        }
        result.push_str(rest);
        result
    }
    key(left) == key(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn formatted(source: &str) -> String {
        let result =
            format_source(source).unwrap_or_else(|error| panic!("{}\n{source}", error.message));
        assert_eq!(
            format_source(&result).unwrap(),
            result,
            "formatter must be idempotent"
        );
        result
    }
    #[test]
    fn migrates_declarations_and_generic_calls() {
        let output = formatted(
            "package p\nconst u32 LIMIT=3\nstruct Box<T>{pub T value}\nenum E{Some(u8 value),Other(code:u32),Plain(u8)}\nfn id<T>(value:T)->T{return value}\nfn main(){u32 total=0;const u8 n=1;result:=id<u32>(total)}\n",
        );
        for text in [
            "const LIMIT: u32 = 3",
            "pub value: T",
            "Some(value: u8)",
            "Plain(u8)",
            "total: u32 = 0;\n",
            "result := id::<u32>(total)",
        ] {
            assert!(output.contains(text), "missing {text}: {output}");
        }
    }
    #[test]
    fn preserves_comments_literals_and_declaration_order() {
        let input = "// leading 🐦\npackage p // package\n\nfn first(){// body\ns:=\"// not a comment \\u{1f426}\" // string\nb:=b'\\n'\nu8 x=0xffu8 // number\n}\n// between\nconst u8 LAST=1 // trailing";
        let output = formatted(input);
        for text in [
            "// leading 🐦",
            "// package",
            "// body",
            "// string",
            "// number",
            "// between",
            "// trailing",
            "\"// not a comment \\u{1f426}\"",
            "b'\\n'",
            "0xffu8",
        ] {
            assert!(output.contains(text), "lost {text:?}: {output}");
        }
        assert!(output.find("fn first").unwrap() < output.find("const LAST").unwrap());
    }
    #[test]
    fn migrates_typed_arrays_without_discarding_types_or_lengths() {
        let output = formatted(
            "package p\nconst [2][0]u8 EMPTY=[2][0]u8{[0]u8{},[0]u8{}}\nfn f(){x:=[2]u16{1,2};y:=take<[2]u16>([2]u16{3,4})}\n",
        );
        for text in [
            "EMPTY: [2][0]u8",
            "([]: [0]u8)",
            "([1, 2]: [2]u16)",
            "take::<[2]u16>",
        ] {
            assert!(output.contains(text), "missing {text}: {output}");
        }
    }
    #[test]
    fn preserves_comments_in_migrated_array_types() {
        let output = formatted("package p\nfn f(){a := [\n // size\n 2]u8{1, // first\n 2}\n}\n");
        assert!(output.contains("// size"));
        assert!(output.contains("// first"));
    }
    #[test]
    fn preserves_precedence_and_statement_boundaries() {
        formatted(
            "package p\nfn f(){x := a < b; y := a > b; z := -x * (*p + 2); a:=1; b:=2;for i:=0;i<10;i+=1{a+=i};for ;true;{}; a=[1;3][0];return}\n",
        );
    }
    #[test]
    fn preserves_immutable_bindings_and_explicit_tail_semicolons() {
        let output = formatted(
            "package p\nfn f()->i32{let result=1;return result;}\nfn g()->i32{1;}\nfn h(){let n:u8=3;let value={1;};}\n",
        );
        assert!(output.contains("let result = 1;"));
        assert!(output.contains("return result;"));
        assert!(output.contains("let n: u8 = 3;"));
    }
    #[test]
    fn preserves_reference_patterns_and_leading_dot_continuations() {
        let output = formatted(
            "package p\nfn f(){for &value in values{sum+=value}\nfor i,&value in values{sum+=value}\nfor &_ in values{}\nfor value in &mut values{*value+=1}\nresult:=source\n.decode()\n.validate()\n}\n",
        );
        assert!(output.contains("for &value in values"));
        assert!(output.contains("for i, &value in values"));
        assert!(output.contains("for &_ in values"));
        assert!(output.contains("result := source\n        .decode()\n        .validate()"));
    }
    #[test]
    fn rejects_invalid_input() {
        assert!(format_source("package p\nfn main( {").is_err());
    }
    #[test]
    fn keeps_destructuring_patterns_with_their_initializers() {
        let output = formatted(
            "package p\nstruct S{x:i32}\nfn f(s:S){let S{x}=s\nif let S{x}=s {return}\nmatch s{S{x} if x>0=>{return} _=>{}}\n}\n",
        );
        assert!(output.contains("let S { x } = s"));
        assert!(output.contains("if let S { x } = s"));
        assert!(output.contains("S { x } if x > 0 =>"));
    }
    #[test]
    fn repository_examples_format_and_remain_idempotent() {
        for source in [
            include_str!("../examples/hello.dodo"),
            include_str!("../examples/samples.dodo"),
            include_str!("../examples/hex.dodo"),
            include_str!("../examples/gpio.dodo"),
            include_str!("../examples/fibonacci.dodo"),
        ] {
            formatted(source);
        }
    }
}
