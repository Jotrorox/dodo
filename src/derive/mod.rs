//! Compile-time derives.
//!
//! `@derive(Json, Row)` on a struct expands into ordinary Dodo methods. Each
//! expansion is generated as source text and then goes through the regular
//! parser, type checker and borrow checker: no derive receives privileged
//! access to memory or lifetimes, and a hand-written method with the same
//! signature is interchangeable with a generated one.
//!
//! Every derive is one [`Spec`] in [`REGISTRY`]. A spec names the attributes
//! it owns, so the parser collects and validates `@json_name("x")` or
//! `@db_flatten` generically, without a hardcoded case per derive. Adding a
//! derive means adding a module with a `SPEC` and listing it here.
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::HashSet;

mod json;
mod row;

/// Where an attribute owned by a derive may appear.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Target {
    /// Before the deriving struct, next to `@derive(...)`.
    Struct,
    /// Before a data field of the deriving struct.
    Field,
}

/// The argument an attribute takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Argument {
    /// No argument: `@json_deny_unknown`.
    Flag,
    /// One UTF-8 string literal: `@json_name("displayName")`. The text
    /// describes the expected value in diagnostics.
    Text(&'static str),
}

pub(crate) struct AttributeSpec {
    pub name: &'static str,
    pub target: Target,
    pub argument: Argument,
}

/// Generates the source of one derive for one struct. The returned text is a
/// `package generated` unit holding a `struct Name { ...methods... }` block;
/// only its methods are kept.
type Generate = fn(&mut Context, &Struct, &Request) -> Result<String, Diagnostic>;

pub(crate) struct Spec {
    /// The name inside `@derive(...)`.
    pub name: &'static str,
    pub attributes: &'static [AttributeSpec],
    generate: Generate,
}

pub(crate) static REGISTRY: &[&Spec] = &[&json::SPEC, &row::SPEC];

pub(crate) fn find(name: &str) -> Option<&'static Spec> {
    REGISTRY.iter().copied().find(|spec| spec.name == name)
}

/// The derive owning an attribute, and the attribute's declaration.
pub(crate) fn attribute(name: &str) -> Option<(&'static Spec, &'static AttributeSpec)> {
    REGISTRY.iter().copied().find_map(|spec| {
        spec.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| (spec, attribute))
    })
}

/// `@derive(Json), @derive(Row)` for diagnostics.
pub(crate) fn supported_derives() -> String {
    REGISTRY
        .iter()
        .map(|spec| format!("@derive({})", spec.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every derive-owned attribute in source form, for diagnostics.
pub(crate) fn supported_attributes() -> String {
    REGISTRY
        .iter()
        .flat_map(|spec| spec.attributes)
        .map(|attribute| match attribute.argument {
            Argument::Flag => format!("@{}", attribute.name),
            Argument::Text(_) => format!("@{}(\"...\")", attribute.name),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// One derive-owned attribute as written in source.
#[derive(Clone, Debug)]
pub(crate) struct Attribute {
    pub name: &'static str,
    pub value: Option<String>,
    pub span: Span,
}

/// The derives requested for one struct and their attributes.
pub(crate) struct Request {
    pub name: String,
    pub derives: Vec<&'static Spec>,
    /// Attributes before the struct.
    pub attributes: Vec<Attribute>,
    /// Attributes before each data field, parallel to `Struct::fields`.
    pub fields: Vec<Vec<Attribute>>,
}

impl Request {
    pub fn flag(&self, name: &str) -> bool {
        self.attributes
            .iter()
            .any(|attribute| attribute.name == name)
    }

    pub fn field_attribute(&self, field: usize, name: &str) -> Option<&Attribute> {
        self.fields[field]
            .iter()
            .find(|attribute| attribute.name == name)
    }

    pub fn field_flag(&self, field: usize, name: &str) -> bool {
        self.field_attribute(field, name).is_some()
    }

    pub fn field_text(&self, field: usize, name: &str) -> Option<&str> {
        self.field_attribute(field, name)
            .and_then(|attribute| attribute.value.as_deref())
    }
}

/// Shared services for generators: hygienic imports and fresh names.
pub(crate) struct Context<'a> {
    program: &'a mut Program,
    identifiers: &'a HashSet<&'a str>,
}

impl Context<'_> {
    /// The alias under which generated code reaches `path`. An explicit
    /// import is reused with its alias; otherwise a hidden import is added
    /// whose alias cannot collide with any identifier in the file.
    pub fn import(&mut self, path: &str) -> String {
        let program = &mut *self.program;
        if program.imports.iter().any(|import| import == path) {
            return program
                .import_aliases
                .iter()
                .find(|(import, _)| import == path)
                .map_or_else(
                    || path.rsplit('/').next().unwrap_or(path).to_owned(),
                    |(_, alias)| alias.clone(),
                );
        }
        let stem = path.rsplit('/').next().unwrap_or(path);
        let mut alias = format!("__dodo_{stem}_derive");
        while self.identifiers.contains(alias.as_str())
            || program
                .import_aliases
                .iter()
                .any(|(_, existing)| *existing == alias)
        {
            alias.push('_');
        }
        program.imports.push(path.to_owned());
        program
            .import_aliases
            .push((path.to_owned(), alias.clone()));
        program
            .implicit_import_aliases
            .push((path.to_owned(), alias.clone()));
        alias
    }

    /// A name generator that avoids every identifier of the file and every
    /// import alias, so generated locals never shadow user names.
    pub fn names(&self) -> Names {
        Names {
            used: self
                .identifiers
                .iter()
                .map(|name| (*name).to_owned())
                .chain(
                    self.program
                        .import_aliases
                        .iter()
                        .map(|(_, alias)| alias.clone()),
                )
                .collect(),
            next: 0,
        }
    }
}

pub(crate) struct Names {
    used: HashSet<String>,
    next: usize,
}

impl Names {
    /// `base`, or `base` followed by underscores until it is unused.
    pub fn fresh(&mut self, base: &str) -> String {
        let mut name = base.to_owned();
        while !self.used.insert(name.clone()) {
            name.push('_');
        }
        name
    }

    /// A numbered local: `prefix_0`, `prefix_1`, ...
    pub fn local(&mut self, prefix: &str) -> String {
        loop {
            let name = format!("{prefix}_{}", self.next);
            self.next += 1;
            if self.used.insert(name.clone()) {
                return name;
            }
        }
    }
}

/// Expands every request in `program`, appending the generated methods.
pub(crate) fn expand(
    program: &mut Program,
    requests: Vec<Request>,
    identifiers: &HashSet<&str>,
) -> Result<(), Diagnostic> {
    for request in requests {
        let structure = program
            .structs
            .iter()
            .find(|structure| structure.name == request.name)
            .cloned()
            .expect("a derive request names a parsed struct");
        for spec in &request.derives {
            if !structure.generics.is_empty() {
                return Err(Diagnostic::new(
                    structure.span,
                    format!(
                        "@derive({}) currently requires a concrete struct; derive concrete nested structs instead",
                        spec.name
                    ),
                ));
            }
            let source = (spec.generate)(
                &mut Context {
                    program,
                    identifiers,
                },
                &structure,
                &request,
            )?;
            let mut expanded = crate::parser::parse(&source).map_err(|error| {
                Diagnostic::new(
                    structure.span,
                    format!("cannot expand @derive({}): {}", spec.name, error.message),
                )
                .note(format!("generated source: {source}"))
            })?;
            for function in &expanded.functions {
                if let Some(existing) = program.functions.iter().find(|f| f.name == function.name) {
                    let method = function
                        .name
                        .rsplit_once('.')
                        .map_or(function.name.as_str(), |(_, method)| method);
                    return Err(Diagnostic::new(
                        existing.span,
                        format!(
                            "@derive({}) conflicts with existing `{method}` method",
                            spec.name
                        ),
                    ));
                }
            }
            // Zero-width locations keep generated syntax out of source
            // formatter token ranges while still placing diagnostics on the
            // deriving struct.
            let span = Span {
                start: structure.span.start,
                end: structure.span.start,
            };
            for function in &mut expanded.functions {
                function.span = span;
                function.ret_span = span;
                for parameter in &mut function.params {
                    parameter.span = span;
                }
                if let Some(body) = &mut function.body {
                    remap_block(body, span);
                }
            }
            program.functions.extend(expanded.functions);
        }
    }
    Ok(())
}

fn remap_expr(expression: &mut Expr, span: Span) {
    expression.span = span;
    match &mut expression.kind {
        ExprKind::Array(_, values) => {
            for value in values {
                remap_expr(value, span);
            }
        }
        ExprKind::Struct(_, fields) => {
            for (_, value) in fields {
                remap_expr(value, span);
            }
        }
        ExprKind::Unary(_, value)
        | ExprKind::Try(value)
        | ExprKind::Unwrap(value)
        | ExprKind::Field(value, _)
        | ExprKind::Cast(value, _)
        | ExprKind::Constant(value, _)
        | ExprKind::Repeat(value, _) => remap_expr(value, span),
        ExprKind::Binary(_, left, right)
        | ExprKind::Index(left, right)
        | ExprKind::Range(left, right) => {
            remap_expr(left, span);
            remap_expr(right, span);
        }
        ExprKind::Call { args, .. } => {
            for argument in args {
                remap_expr(argument, span);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            remap_expr(receiver, span);
            for argument in args {
                remap_expr(argument, span);
            }
        }
        ExprKind::ValueBlock(body) => remap_block(body, span),
        ExprKind::Slice {
            base, start, end, ..
        } => {
            remap_expr(base, span);
            for value in start.iter_mut().chain(end) {
                remap_expr(value, span);
            }
        }
        ExprKind::Closure(closure) => {
            for param in &mut closure.params {
                param.span = span;
            }
            closure.ret_span = span;
            remap_block(&mut closure.body, span);
        }
        ExprKind::Invoke { callee, args } => {
            remap_expr(callee, span);
            for argument in args {
                remap_expr(argument, span);
            }
        }
        ExprKind::Asm(asm) => {
            asm.span = span;
            for operand in &mut asm.operands {
                operand.span = span;
            }
            for value in asm.inputs_mut() {
                remap_expr(value, span);
            }
        }
        _ => {}
    }
}

fn remap_block(block: &mut Block, span: Span) {
    for statement in block {
        remap_stmt(statement, span);
    }
}

fn remap_stmt(statement: &mut Stmt, span: Span) {
    statement.span = span;
    match &mut statement.kind {
        StmtKind::Let {
            value: Some(value), ..
        }
        | StmtKind::Expr(value)
        | StmtKind::Yield(value)
        | StmtKind::Return(Some(value)) => remap_expr(value, span),
        StmtKind::Assign { target, value, .. } => {
            remap_expr(target, span);
            remap_expr(value, span);
        }
        StmtKind::LetPattern {
            value, else_block, ..
        } => {
            remap_expr(value, span);
            if let Some(body) = else_block {
                remap_block(body, span);
            }
        }
        StmtKind::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            remap_expr(value, span);
            remap_block(then_block, span);
            remap_block(else_block, span);
        }
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            remap_expr(condition, span);
            remap_block(then_block, span);
            remap_block(else_block, span);
        }
        StmtKind::For {
            init,
            condition,
            step,
            body,
        } => {
            if let Some(init) = init {
                remap_stmt(init, span);
            }
            if let Some(condition) = condition {
                remap_expr(condition, span);
            }
            if let Some(step) = step {
                remap_stmt(step, span);
            }
            remap_block(body, span);
        }
        StmtKind::ForEach { iterable, body, .. } => {
            remap_expr(iterable, span);
            remap_block(body, span);
        }
        StmtKind::Match { value, arms } => {
            remap_expr(value, span);
            for arm in arms {
                arm.span = span;
                if let Some(guard) = &mut arm.guard {
                    remap_expr(guard, span);
                }
                remap_block(&mut arm.body, span);
            }
        }
        StmtKind::Block(body) | StmtKind::Unsafe(body) => remap_block(body, span),
        _ => {}
    }
}
