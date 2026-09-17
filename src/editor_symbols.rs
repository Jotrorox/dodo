//! Source identities and lexical scopes survive semantic lowering. The index is
//! rebuilt with each full check and shared by documents in the checked package.
use super::{byte_offset, local_span, range, short_name, source_names};
use crate::ast::*;
use crate::json::{Value, json};
use crate::lexer::{self, Token, TokenKind};
use crate::package::{Loaded, Source};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key(pub PathBuf, pub usize, pub usize);

pub(crate) struct Symbol {
    pub key: Key,
    pub name: String,
    pub span: Span,
    pub kind: u32,
    pub detail: String,
    pub parameters: Option<Vec<String>>,
    ty: Type,
    // Globals use their fully qualified name; locals have a visibility interval.
    global: Option<String>,
    visible: Span,
    public: bool,
    receiver: bool,
    renameable: bool,
}

pub(crate) struct Occurrence {
    pub span: Span,
    pub symbol: usize,
    pub declaration: bool,
}

pub(crate) struct Index {
    pub sources: Vec<Source>,
    pub symbols: Vec<Symbol>,
    pub occurrences: Vec<Occurrence>,
    tokens: Vec<Token>,
    globals: BTreeMap<String, usize>,
    namespaces: BTreeMap<PathBuf, BTreeMap<String, String>>,
    types: BTreeMap<(usize, usize), Type>,
    declarations: BTreeMap<usize, usize>,
    roles: BTreeMap<usize, Option<usize>>,
}

impl Index {
    pub fn new(loaded: &Loaded, original: &Program) -> Self {
        let mut index = Self {
            sources: loaded.sources.clone(),
            symbols: vec![],
            occurrences: vec![],
            tokens: vec![],
            globals: BTreeMap::new(),
            namespaces: loaded.namespaces.clone(),
            types: BTreeMap::new(),
            declarations: BTreeMap::new(),
            roles: BTreeMap::new(),
        };
        for source in &index.sources {
            let (tokens, _) = lexer::lex_recovering(&source.text);
            index
                .tokens
                .extend(tokens.into_iter().filter_map(|mut token| {
                    if matches!(token.kind, TokenKind::Newline | TokenKind::Eof) {
                        return None;
                    }
                    token.span.start += source.start;
                    token.span.end += source.start;
                    Some(token)
                }));
        }
        index.tokens.sort_by_key(|token| token.span.start);
        // Read inferred types from the checked tree; declarations and uses are
        // indexed from original tokens, never compiler-generated variable names.
        for function in &loaded.program.functions {
            if let Some(body) = &function.body {
                walk_block(
                    body,
                    &mut |statement| {
                        if let StmtKind::Let { ty, .. } = &statement.kind {
                            index
                                .types
                                .entry((statement.span.start, statement.span.end))
                                .or_insert_with(|| ty.clone());
                        }
                    },
                    &mut |_| {},
                );
                walk_block(body, &mut |_| {}, &mut |expr| {
                    if expr.ty != Type::Unknown {
                        index
                            .types
                            .entry((expr.span.start, expr.span.end))
                            .or_insert_with(|| expr.ty.clone());
                    }
                });
            }
        }
        for structure in &original.structs {
            index.global(
                &structure.name,
                structure.span,
                22,
                Type::Named(structure.name.clone()),
                structure.public,
                None,
            );
            for field in &structure.fields {
                index.global(
                    &format!("{}.{}", structure.name, field.name),
                    field.span,
                    5,
                    field.ty.clone(),
                    field.public,
                    None,
                );
            }
        }
        for enumeration in &original.enums {
            index.global(
                &enumeration.name,
                enumeration.span,
                13,
                Type::Named(enumeration.name.clone()),
                enumeration.public,
                None,
            );
            for variant in &enumeration.variants {
                index.global(
                    &format!("{}.{}", enumeration.name, variant.name),
                    variant.span,
                    20,
                    Type::Named(enumeration.name.clone()),
                    enumeration.public,
                    None,
                );
            }
        }
        for constant in &original.constants {
            index.global(
                &constant.name,
                constant.span,
                21,
                constant.ty.clone(),
                constant.public,
                None,
            );
        }
        for function in &original.functions {
            let checked = loaded
                .program
                .functions
                .iter()
                .find(|f| f.span == function.span)
                .unwrap_or(function);
            index.global(
                &function.name,
                function.span,
                3,
                checked.ret.clone(),
                function.public,
                Some(checked),
            );
        }
        for function in &original.functions {
            for parameter in &function.params {
                index.local(
                    &parameter.name,
                    parameter.span,
                    parameter.ty.clone(),
                    function.span,
                    parameter.span.end,
                    true,
                );
            }
            if let Some(body) = &function.body {
                walk_block(
                    body,
                    &mut |statement| index.binding(statement, function.span),
                    &mut |_| {},
                );
            }
        }
        for function in &original.functions {
            for parameter in &function.params {
                index.type_roles(parameter.span);
            }
            index.type_roles(function.ret_span);
            if let Some(body) = &function.body {
                walk_block(body, &mut |_| {}, &mut |expr| index.expression_roles(expr));
            }
        }
        for structure in &original.structs {
            for field in &structure.fields {
                index.type_roles(field.span);
            }
        }
        for constant in &original.constants {
            index.type_roles(Span {
                start: constant.span.start,
                end: constant.value.span.start,
            });
            walk_expr(&constant.value, &mut |_| {}, &mut |e| {
                index.expression_roles(e)
            });
        }
        for function in &original.functions {
            index.generic_roles(function.span, &function.generics);
        }
        for structure in &original.structs {
            index.generic_roles(structure.span, &structure.generics);
        }
        for enumeration in &original.enums {
            index.generic_roles(enumeration.span, &enumeration.generics);
        }
        index.resolve_tokens();
        index
    }

    fn tokens_in(&self, span: Span) -> &[Token] {
        let start = self.tokens.partition_point(|t| t.span.start < span.start);
        let end = self.tokens.partition_point(|t| t.span.start < span.end);
        &self.tokens[start..end]
    }

    fn name_span(&self, span: Span, name: &str) -> Option<Span> {
        self.tokens_in(span).iter().find_map(|token| {
            matches!(&token.kind, TokenKind::Ident(word) if word == name).then_some(token.span)
        })
    }

    pub fn source(&self, span: Span) -> Option<&Source> {
        self.sources
            .iter()
            .find(|s| s.start <= span.start && span.start <= s.start + s.text.len())
    }

    fn global(
        &mut self,
        name: &str,
        span: Span,
        kind: u32,
        ty: Type,
        public: bool,
        function: Option<&Function>,
    ) {
        let Some(span) = self.name_span(span, short_name(name)) else {
            return;
        };
        if self.declarations.contains_key(&span.start) {
            return;
        }
        let Some(source) = self.source(span) else {
            return;
        };
        let parameters = function.map(|f| {
            super::display_parameters(f)
                .iter()
                .map(|p| source_names(p))
                .collect::<Vec<_>>()
        });
        let detail = if let Some(parameters) = &parameters {
            source_names(&format!("fn {name}({}) -> {ty}", parameters.join(", ")))
        } else {
            source_names(&format!("{name}: {ty}"))
        };
        let id = self.symbols.len();
        self.symbols.push(Symbol {
            key: Key(
                source.path.clone(),
                span.start - source.start,
                span.end - source.start,
            ),
            name: short_name(name).into(),
            span,
            kind,
            detail,
            parameters,
            ty,
            global: Some(name.into()),
            visible: Span::default(),
            public,
            receiver: function.is_some_and(|f| f.params.first().is_some_and(|p| p.name == "self")),
            renameable: !source.path.starts_with("<stdlib>"),
        });
        self.globals.insert(name.into(), id);
        self.declarations.insert(span.start, id);
    }

    fn local(
        &mut self,
        name: &str,
        declaration: Span,
        ty: Type,
        scope: Span,
        visible_start: usize,
        renameable: bool,
    ) {
        if name == "_" {
            return;
        }
        let Some(span) = self.name_span(declaration, name) else {
            return;
        };
        if self.declarations.contains_key(&span.start) {
            return;
        }
        let Some(source) = self.source(span) else {
            return;
        };
        let id = self.symbols.len();
        self.symbols.push(Symbol {
            key: Key(
                source.path.clone(),
                span.start - source.start,
                span.end - source.start,
            ),
            name: name.into(),
            span,
            kind: 6,
            detail: source_names(&format!("{name}: {ty}")),
            parameters: None,
            ty,
            global: None,
            visible: Span {
                start: visible_start,
                end: scope.end,
            },
            public: false,
            receiver: false,
            renameable: renameable && name != "self" && !source.path.starts_with("<stdlib>"),
        });
        self.declarations.insert(span.start, id);
    }

    // Find the enclosing source braces, including empty/incomplete blocks. AST
    // Block is a Vec and does not retain its delimiter spans.
    fn scope(&self, at: usize, function: Span) -> Span {
        let mut stack = Vec::new();
        for token in self.tokens_in(function) {
            match token.kind {
                TokenKind::Symbol("{") => stack.push(token.span.start),
                TokenKind::Symbol("}") => {
                    if let Some(start) = stack.pop()
                        && start <= at
                        && at < token.span.end
                    {
                        return Span {
                            start,
                            end: token.span.end,
                        };
                    }
                }
                _ => (),
            }
        }
        function
    }

    fn binding(&mut self, statement: &Stmt, function: Span) {
        let scope = self.scope(statement.span.start, function);
        match &statement.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                let ty = self
                    .types
                    .get(&(statement.span.start, statement.span.end))
                    .unwrap_or(ty)
                    .clone();
                self.local(name, statement.span, ty, scope, statement.span.end, true);
                self.type_roles(Span {
                    start: statement.span.start,
                    end: value.as_ref().map_or(statement.span.end, |v| v.span.start),
                });
            }
            StmtKind::For {
                init: Some(init), ..
            } => {
                if let StmtKind::Let { name, ty, .. } = &init.kind {
                    let ty = self
                        .types
                        .get(&(init.span.start, init.span.end))
                        .unwrap_or(ty)
                        .clone();
                    self.local(name, init.span, ty, statement.span, init.span.end, true);
                }
            }
            StmtKind::LetPattern { pattern, value, .. } => {
                self.pattern_rename(pattern);
                for name in pattern.bindings() {
                    self.local(
                        &name,
                        Span {
                            start: statement.span.start,
                            end: value.span.start,
                        },
                        Type::Unknown,
                        scope,
                        statement.span.end,
                        false,
                    );
                }
            }
            StmtKind::IfLet { pattern, value, .. } => {
                self.pattern_rename(pattern);
                let start = self
                    .tokens_in(Span {
                        start: value.span.end,
                        end: statement.span.end,
                    })
                    .iter()
                    .find(|t| t.kind == TokenKind::Symbol("{"))
                    .map_or(value.span.end, |t| t.span.end);
                let scope = self.scope(start, function);
                for name in pattern.bindings() {
                    self.local(
                        &name,
                        Span {
                            start: statement.span.start,
                            end: value.span.start,
                        },
                        Type::Unknown,
                        scope,
                        value.span.end,
                        false,
                    );
                }
            }
            StmtKind::ForEach {
                index,
                name,
                iterable,
                ..
            } => {
                let scope = statement.span;
                for name in index.iter().chain(std::iter::once(name)) {
                    self.local(
                        name,
                        Span {
                            start: statement.span.start,
                            end: iterable.span.start,
                        },
                        Type::Unknown,
                        scope,
                        iterable.span.end,
                        true,
                    );
                }
            }
            StmtKind::Match { arms, .. } => {
                for arm in arms {
                    self.pattern_rename(&arm.pattern);
                    let end = arm
                        .guard
                        .as_ref()
                        .map(|g| g.span.start)
                        .or_else(|| arm.body.first().map(|s| s.span.start))
                        .unwrap_or(arm.span.end);
                    for name in arm.pattern.bindings() {
                        self.local(
                            &name,
                            Span {
                                start: arm.span.start,
                                end,
                            },
                            Type::Unknown,
                            arm.span,
                            end,
                            false,
                        );
                    }
                }
            }
            _ => (),
        }
    }

    // A field label and a value with the same spelling are distinct references.
    // Shorthand uses one token for both roles; refuse renames requiring expansion.
    fn expression_roles(&mut self, expression: &Expr) {
        if let ExprKind::Cast(value, _) = &expression.kind {
            self.type_roles(Span {
                start: value.span.end,
                end: expression.span.end,
            });
        }
        if let ExprKind::Struct(name, fields) = &expression.kind {
            let owner = name.split('<').next().unwrap_or(name);
            let mut start = self
                .tokens_in(expression.span)
                .iter()
                .find(|t| t.kind == TokenKind::Symbol("{"))
                .map_or(expression.span.start, |t| t.span.end);
            for (field, value) in fields {
                if let Some(span) = self.name_span(
                    Span {
                        start,
                        end: value.span.end,
                    },
                    field,
                ) {
                    let id = self.globals.get(&format!("{owner}.{field}")).copied();
                    if span == value.span {
                        if let Some(id) = id {
                            self.symbols[id].renameable = false;
                        }
                        if let Some(id) = self.local_at(field, span.start) {
                            self.symbols[id].renameable = false;
                        }
                    } else {
                        self.roles.insert(span.start, id);
                    }
                }
                start = value.span.end;
            }
        }
        if let ExprKind::Name(name) = &expression.kind
            && !name.contains('.')
            && let Some(id) = self.local_at(name, expression.span.start)
            && self.symbols[id].ty == Type::Unknown
            && let Some(ty) = self
                .types
                .get(&(expression.span.start, expression.span.end))
        {
            self.symbols[id].ty = ty.clone();
            self.symbols[id].detail = source_names(&format!("{name}: {ty}"));
        }
    }

    fn pattern_rename(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Struct(owner, fields, _) => {
                let owner = owner.split('<').next().unwrap_or(owner);
                for (field, pattern) in fields {
                    if let Some(id) = self.globals.get(&format!("{owner}.{field}")) {
                        self.symbols[*id].renameable = false;
                    }
                    self.pattern_rename(pattern);
                }
            }
            Pattern::Variant(_, patterns) | Pattern::Or(patterns) => {
                for pattern in patterns {
                    self.pattern_rename(pattern);
                }
            }
            _ => (),
        }
    }

    fn generic_roles(&mut self, span: Span, generics: &[String]) {
        if generics.is_empty() {
            return;
        }
        let spans: Vec<_> = self
            .tokens_in(span)
            .iter()
            .filter_map(|t| {
                matches!(&t.kind, TokenKind::Ident(name) if generics.contains(name))
                    .then_some(t.span.start)
            })
            .collect();
        for start in spans {
            self.roles.insert(start, None);
        }
    }

    fn type_roles(&mut self, span: Span) {
        let tokens = self.tokens_in(span).to_vec();
        let mut resolved = BTreeMap::new();
        for token in tokens {
            if self.declarations.contains_key(&token.span.start) {
                continue;
            }
            if let TokenKind::Ident(name) = &token.kind {
                let i = self
                    .tokens
                    .partition_point(|t| t.span.start < token.span.start);
                let id = if i > 0 && self.tokens[i - 1].kind == TokenKind::Symbol(".") {
                    self.resolve(i, &resolved)
                } else {
                    self.globals
                        .get(&self.qualified(name, token.span.start))
                        .copied()
                };
                if let Some(id) = id {
                    resolved.insert(i, id);
                }
                self.roles.insert(token.span.start, id);
            }
        }
    }

    pub(super) fn namespace(&self, at: usize) -> Option<&BTreeMap<String, String>> {
        self.namespaces
            .get(&self.source(Span { start: at, end: at })?.path)
    }

    fn qualified(&self, name: &str, at: usize) -> String {
        let Some(namespace) = self.namespace(at) else {
            return name.into();
        };
        let (head, tail) = name.split_once('.').unwrap_or((name, ""));
        if let Some(prefix) = namespace.get(head) {
            return [prefix.as_str(), tail]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(".");
        }
        let prefix = namespace.get("").map_or("", String::as_str);
        if prefix.is_empty() {
            name.into()
        } else {
            format!("{prefix}.{name}")
        }
    }

    fn local_at(&self, name: &str, at: usize) -> Option<usize> {
        self.symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.global.is_none() && s.name == name && s.visible.start <= at && at < s.visible.end
            })
            .max_by_key(|(_, s)| s.visible.start)
            .map(|(id, _)| id)
    }

    fn member_type(ty: &Type) -> Option<String> {
        match ty {
            Type::Named(n) | Type::Generic(n, _) => Some(n.split(['<', '$']).next()?.into()),
            Type::Ref(_, t) | Type::Raw(_, t) => Self::member_type(t),
            _ => None,
        }
    }

    fn resolve_tokens(&mut self) {
        let mut resolved = BTreeMap::new();
        for (i, token) in self.tokens.iter().enumerate() {
            let TokenKind::Ident(name) = &token.kind else {
                continue;
            };
            let declaration = self.declarations.get(&token.span.start).copied();
            let ignored = i > 0
                && (matches!(&self.tokens[i - 1].kind, TokenKind::Ident(n) if n == "package")
                    || (i > 1
                        && matches!(&self.tokens[i - 1].kind, TokenKind::Ident(n) if n == "as")
                        && matches!(self.tokens[i - 2].kind, TokenKind::String(..)))
                    || self.tokens[i - 1].kind == TokenKind::Symbol("@"));
            let id = if ignored {
                None
            } else {
                declaration.or_else(|| {
                    self.roles
                        .get(&token.span.start)
                        .copied()
                        .unwrap_or_else(|| self.resolve(i, &resolved))
                })
            };
            if let Some(id) = id {
                // Do not interpret keyword tokens as ordinary symbol uses.
                if declaration.is_none() && crate::parser::reserved(name) {
                    continue;
                }
                resolved.insert(i, id);
                self.occurrences.push(Occurrence {
                    span: token.span,
                    symbol: id,
                    declaration: declaration.is_some(),
                });
            }
        }
    }

    fn resolve(&self, i: usize, resolved: &BTreeMap<usize, usize>) -> Option<usize> {
        let token = &self.tokens[i];
        let TokenKind::Ident(name) = &token.kind else {
            return None;
        };
        if i >= 2 && self.tokens[i - 1].kind == TokenKind::Symbol(".") {
            if let Some(id) = resolved.get(&(i - 2)) {
                let parent = &self.symbols[*id];
                let prefix = if matches!(parent.kind, 22 | 13) {
                    parent.global.clone()
                } else {
                    Self::member_type(&parent.ty)
                };
                return prefix
                    .and_then(|prefix| self.globals.get(&format!("{prefix}.{name}")).copied());
            }
            // Qualified package paths and aliases are resolved per importing file.
            let mut start = i;
            while start >= 2
                && self.tokens[start - 1].kind == TokenKind::Symbol(".")
                && matches!(self.tokens[start - 2].kind, TokenKind::Ident(_))
            {
                start -= 2;
            }
            let path = self.tokens[start..=i]
                .iter()
                .filter_map(|t| {
                    if let TokenKind::Ident(n) = &t.kind {
                        Some(n.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(".");
            if let Some(id) = self.globals.get(&self.qualified(&path, token.span.start)) {
                return Some(*id);
            }
            let end = self.tokens[i - 2].span.end;
            return self
                .types
                .iter()
                .filter(|((_, e), _)| *e == end)
                .find_map(|(_, ty)| {
                    self.globals
                        .get(&format!("{}.{name}", Self::member_type(ty)?))
                        .copied()
                });
        }
        self.local_at(name, token.span.start).or_else(|| {
            self.globals
                .get(&self.qualified(name, token.span.start))
                .copied()
        })
    }

    pub fn symbol_at(&self, path: &PathBuf, line: u32, character: u32) -> Option<(&Symbol, Span)> {
        let source = self.sources.iter().find(|s| s.path == *path)?;
        let at = byte_offset(&source.text, line, character)? + source.start;
        let occurrence = self
            .occurrences
            .iter()
            .find(|o| o.span.start <= at && at < o.span.end)?;
        Some((&self.symbols[occurrence.symbol], occurrence.span))
    }

    pub fn occurrences(
        &self,
        key: &Key,
        include_declaration: bool,
    ) -> impl Iterator<Item = Span> + '_ {
        let id = self.symbols.iter().position(|s| s.key == *key);
        self.occurrences
            .iter()
            .filter(move |o| Some(o.symbol) == id && (include_declaration || !o.declaration))
            .map(|o| o.span)
    }

    pub fn can_rename(&self, symbol: &Symbol, new_name: &str) -> bool {
        if !symbol.renameable
            || self
                .symbols
                .iter()
                .any(|s| s.key == symbol.key && !s.renameable)
        {
            return false;
        }
        // Conservatively reject possible capture, including a global renamed to
        // a local in a caller. A refused rename is preferable to changing binding.
        !self
            .symbols
            .iter()
            .any(|s| s.key != symbol.key && s.name == new_name)
    }

    pub fn prepare_rename(&self, symbol: &Symbol, span: Span) -> Option<Value> {
        if !symbol.renameable {
            return None;
        }
        let source = self.source(span)?;
        Some(
            json!({"range": range(&source.text, local_span(span, source.start)), "placeholder": symbol.name}),
        )
    }

    fn code_position(&self, source: &Source, at: usize) -> bool {
        if self.tokens.iter().any(|t| {
            matches!(t.kind, TokenKind::String(..)) && t.span.start < at && at < t.span.end
        }) {
            return false;
        }
        let before = self.tokens.partition_point(|t| t.span.end <= at);
        let start = before
            .checked_sub(1)
            .map_or(source.start, |i| self.tokens[i].span.end.max(source.start));
        !source.text[start - source.start..at - source.start]
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .contains("//")
    }

    fn context(&self, source: &Source, at: usize) -> Option<(usize, String, usize)> {
        let before = self.tokens.partition_point(|t| t.span.end <= at);
        let mut start = at;
        while start > source.start
            && (source.text.as_bytes()[start - source.start - 1].is_ascii_alphanumeric()
                || source.text.as_bytes()[start - source.start - 1] == b'_')
        {
            start -= 1;
        }
        let prefix = source
            .text
            .get(start - source.start..at - source.start)?
            .to_owned();
        Some((before, prefix, start))
    }

    pub fn completion(&self, path: &PathBuf, line: u32, character: u32) -> Option<Value> {
        let source = self.sources.iter().find(|s| s.path == *path)?;
        let at = byte_offset(&source.text, line, character)? + source.start;
        if !self.code_position(source, at) {
            return None;
        }
        let (_, prefix, start) = self.context(source, at)?;
        let before = self.tokens.partition_point(|t| t.span.end <= start);
        let member = before > 0 && self.tokens[before - 1].kind == TokenKind::Symbol(".");
        let mut candidates = BTreeMap::new();
        if member && before >= 2 {
            let receiver = &self.tokens[before - 2];
            let occurrence = self.occurrences.iter().find(|o| o.span == receiver.span);
            let owner = occurrence
                .and_then(|o| {
                    let s = &self.symbols[o.symbol];
                    if matches!(s.kind, 22 | 13) {
                        s.global.clone()
                    } else {
                        Self::member_type(&s.ty)
                    }
                })
                .or_else(|| {
                    if let TokenKind::Ident(name) = &receiver.kind {
                        self.namespace(at)?.get(name).cloned()
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    self.types
                        .iter()
                        .filter(|((_, end), _)| *end == receiver.span.end)
                        .find_map(|(_, ty)| Self::member_type(ty))
                });
            if let Some(owner) = owner {
                for (name, id) in &self.globals {
                    if name.rsplit_once('.').is_some_and(|(p, _)| p == owner) {
                        candidates.insert(self.symbols[*id].name.clone(), *id);
                    }
                }
            }
        } else {
            for (id, symbol) in self
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, s)| s.global.is_some())
            {
                if self.qualified(&symbol.name, at) == *symbol.global.as_ref().unwrap() {
                    candidates.insert(symbol.name.clone(), id);
                }
            }
            let mut locals: Vec<_> = self
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, s)| s.global.is_none() && s.visible.start <= at && at < s.visible.end)
                .collect();
            locals.sort_by_key(|(_, s)| s.visible.start);
            for (id, symbol) in locals {
                candidates.insert(symbol.name.clone(), id);
            }
        }
        let edit_range = range(
            &source.text,
            Span {
                start: start - source.start,
                end: self
                    .tokens
                    .get(self.tokens.partition_point(|t| t.span.start < start))
                    .filter(|t| t.span.start == start && matches!(t.kind, TokenKind::Ident(_)))
                    .map_or(at, |t| t.span.end)
                    - source.start,
            },
        );
        let mut items = Vec::new();
        for (name, id) in candidates {
            let s = &self.symbols[id];
            let same_package = self.namespace(s.span.start).and_then(|ns| ns.get(""))
                == self.namespace(at).and_then(|ns| ns.get(""));
            if name.starts_with(&prefix) && (s.public || same_package) {
                items.push(json!({"label":name,"kind":s.kind,"detail":s.detail,"textEdit":{"range":edit_range,"newText":name}}));
            }
        }
        if !member {
            let mut words: BTreeSet<String> = [
                "fn", "return", "if", "else", "for", "match", "let", "mut", "const", "struct",
                "enum", "import", "pub", "unsafe", "true", "false", "void", "bool", "i8", "i16",
                "i32", "i64", "u8", "u16", "u32", "u64", "usize", "isize", "f32", "f64",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            if let Some(namespace) = self.namespace(at) {
                words.extend(namespace.keys().filter(|key| !key.is_empty()).cloned());
            }
            for name in words.into_iter().filter(|n| n.starts_with(&prefix)) {
                items.push(
                    json!({"label":name,"kind":14,"textEdit":{"range":edit_range,"newText":name}}),
                );
            }
        }
        Some(json!({"isIncomplete":false,"items":items}))
    }

    pub fn signature_help(&self, path: &PathBuf, line: u32, character: u32) -> Option<Value> {
        let source = self.sources.iter().find(|s| s.path == *path)?;
        let at = byte_offset(&source.text, line, character)? + source.start;
        if !self.code_position(source, at) {
            return None;
        }
        let tokens = self.tokens_in(Span {
            start: source.start,
            end: at,
        });
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for (i, token) in tokens.iter().enumerate().filter(|(_, t)| t.span.end <= at) {
            match token.kind {
                TokenKind::Symbol("(" | "[" | "{") => stack.push((i, 0)),
                TokenKind::Symbol("<")
                    if i > 0
                        && (tokens[i - 1].kind == TokenKind::Symbol("::")
                            || stack.last().is_some_and(|(open, _)| {
                                tokens[*open].kind == TokenKind::Symbol("<")
                            })) =>
                {
                    stack.push((i, 0));
                }
                TokenKind::Symbol(">" | ">>") => {
                    let count = if token.kind == TokenKind::Symbol(">>") {
                        2
                    } else {
                        1
                    };
                    for _ in 0..count {
                        if stack
                            .last()
                            .is_some_and(|(open, _)| tokens[*open].kind == TokenKind::Symbol("<"))
                        {
                            stack.pop();
                        }
                    }
                }
                TokenKind::Symbol(")" | "]" | "}") => {
                    stack.pop();
                }
                TokenKind::Symbol(",") => {
                    if let Some((_, commas)) = stack.last_mut() {
                        *commas += 1;
                    }
                }
                _ => (),
            }
        }
        for (open, active) in stack.into_iter().rev() {
            if tokens[open].kind != TokenKind::Symbol("(") || open == 0 {
                continue;
            }
            let mut callee = open - 1;
            // Skip explicit generic arguments before the call's parentheses.
            if matches!(tokens[callee].kind, TokenKind::Symbol(">" | ">>")) {
                let mut depth: i32 = 0;
                loop {
                    match tokens[callee].kind {
                        TokenKind::Symbol(">") => depth += 1,
                        TokenKind::Symbol(">>") => depth += 2,
                        TokenKind::Symbol("<") => depth -= 1,
                        _ => (),
                    }
                    if depth == 0 {
                        callee = callee.checked_sub(1)?;
                        break;
                    }
                    callee = callee.checked_sub(1)?;
                }
                if tokens[callee].kind == TokenKind::Symbol("::") {
                    callee = callee.checked_sub(1)?;
                }
            }
            let Some(occurrence) = self
                .occurrences
                .iter()
                .find(|o| o.span == tokens[callee].span && !o.declaration)
            else {
                continue;
            };
            let symbol = &self.symbols[occurrence.symbol];
            let Some(parameters) = &symbol.parameters else {
                continue;
            };
            let skip = usize::from(
                symbol.receiver
                    && callee >= 2
                    && tokens[callee - 1].kind == TokenKind::Symbol(".")
                    && !self.occurrences.iter().any(|o| {
                        o.span == tokens[callee - 2].span
                            && matches!(self.symbols[o.symbol].kind, 22 | 13)
                    }),
            );
            let parameters: Vec<_> = parameters
                .iter()
                .skip(skip)
                .map(|label| json!({"label":label}))
                .collect();
            let active = active.min(parameters.len().saturating_sub(1));
            return Some(
                json!({"signatures":[{"label":symbol.detail,"parameters":parameters}],"activeSignature":0,"activeParameter":active}),
            );
        }
        None
    }
}

fn walk_block(block: &Block, stmt: &mut impl FnMut(&Stmt), expr: &mut impl FnMut(&Expr)) {
    for statement in block {
        walk_stmt(statement, stmt, expr);
    }
}
fn walk_stmt(statement: &Stmt, stmt: &mut impl FnMut(&Stmt), expr: &mut impl FnMut(&Expr)) {
    stmt(statement);
    match &statement.kind {
        StmtKind::Let { value: Some(e), .. } => walk_expr(e, stmt, expr),
        StmtKind::LetPattern {
            value, else_block, ..
        } => {
            walk_expr(value, stmt, expr);
            if let Some(b) = else_block {
                walk_block(b, stmt, expr);
            }
        }
        StmtKind::Assign { target, value, .. } => {
            walk_expr(target, stmt, expr);
            walk_expr(value, stmt, expr);
        }
        StmtKind::Expr(e) | StmtKind::Yield(e) | StmtKind::Return(Some(e)) => {
            walk_expr(e, stmt, expr)
        }
        StmtKind::If {
            condition,
            then_block,
            else_block,
        }
        | StmtKind::IfLet {
            value: condition,
            then_block,
            else_block,
            ..
        } => {
            walk_expr(condition, stmt, expr);
            walk_block(then_block, stmt, expr);
            walk_block(else_block, stmt, expr);
        }
        StmtKind::For {
            init,
            condition,
            step,
            body,
        } => {
            if let Some(s) = init {
                walk_stmt(s, stmt, expr);
            }
            if let Some(e) = condition {
                walk_expr(e, stmt, expr);
            }
            if let Some(s) = step {
                walk_stmt(s, stmt, expr);
            }
            walk_block(body, stmt, expr);
        }
        StmtKind::ForEach { iterable, body, .. } => {
            walk_expr(iterable, stmt, expr);
            walk_block(body, stmt, expr);
        }
        StmtKind::Match { value, arms } => {
            walk_expr(value, stmt, expr);
            for arm in arms {
                if let Some(e) = &arm.guard {
                    walk_expr(e, stmt, expr);
                }
                walk_block(&arm.body, stmt, expr);
            }
        }
        StmtKind::Block(b) | StmtKind::Unsafe(b) => walk_block(b, stmt, expr),
        _ => (),
    }
}
fn walk_expr(e: &Expr, stmt: &mut impl FnMut(&Stmt), expr: &mut impl FnMut(&Expr)) {
    expr(e);
    match &e.kind {
        ExprKind::Call { args, .. } | ExprKind::Array(_, args) => {
            for e in args {
                walk_expr(e, stmt, expr);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            walk_expr(receiver, stmt, expr);
            for e in args {
                walk_expr(e, stmt, expr);
            }
        }
        ExprKind::Struct(_, fields) => {
            for (_, e) in fields {
                walk_expr(e, stmt, expr);
            }
        }
        ExprKind::Repeat(e, _)
        | ExprKind::Constant(e, _)
        | ExprKind::Unary(_, e)
        | ExprKind::Field(e, _)
        | ExprKind::Cast(e, _)
        | ExprKind::Try(e)
        | ExprKind::Unwrap(e) => walk_expr(e, stmt, expr),
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) | ExprKind::Range(a, b) => {
            walk_expr(a, stmt, expr);
            walk_expr(b, stmt, expr);
        }
        ExprKind::Slice {
            base, start, end, ..
        } => {
            walk_expr(base, stmt, expr);
            for e in start.iter().chain(end) {
                walk_expr(e, stmt, expr);
            }
        }
        ExprKind::ValueBlock(b) => walk_block(b, stmt, expr),
        _ => (),
    }
}
