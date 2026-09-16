//! Expand JSON derives into ordinary checked Dodo methods.
//!
//! The expansion deliberately uses the regular parser, type checker and borrow
//! checker. No JSON operation receives privileged access to memory or lifetimes.
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::HashSet;
use std::fmt::Write;

pub(crate) struct Derive {
    pub name: String,
    pub json_names: Vec<String>,
    pub deny_unknown: bool,
}

pub(crate) fn expand(
    program: &mut Program,
    derives: Vec<Derive>,
    identifiers: &HashSet<&str>,
) -> Result<(), Diagnostic> {
    let path = "std/encoding/json";
    let alias = if program.imports.iter().any(|import| import == path) {
        program
            .import_aliases
            .iter()
            .find(|(import, _)| import == path)
            .map_or("json", |(_, alias)| alias.as_str())
            .to_owned()
    } else {
        let mut alias = "__dodo_json_derive".to_owned();
        while identifiers.contains(alias.as_str()) {
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
    };
    for derive in derives {
        let structure = program
            .structs
            .iter()
            .find(|s| s.name == derive.name)
            .unwrap();
        if !structure.generics.is_empty() {
            return Err(Diagnostic::new(
                structure.span,
                "@derive(Json) currently requires a concrete struct; derive concrete nested structs instead",
            ));
        }
        for method in ["encode_json", "decode_json"] {
            if let Some(existing) = program
                .functions
                .iter()
                .find(|f| f.name == format!("{}.{method}", structure.name))
            {
                return Err(Diagnostic::new(
                    existing.span,
                    format!("@derive(Json) conflicts with existing `{method}` method"),
                ));
            }
        }
        let mut names = HashSet::new();
        let mut expansion_size = 0usize;
        for (field, name) in structure.fields.iter().zip(&derive.json_names) {
            if !names.insert(name) {
                return Err(Diagnostic::new(
                    field.span,
                    format!("duplicate JSON field name `{name}`"),
                ));
            }
            validate_type(&field.ty, field.span)?;
            expansion_size = expansion_size.saturating_add(expansion_cost(&field.ty));
            if expansion_size > 16_384 {
                return Err(Diagnostic::new(
                    field.span,
                    "@derive(Json) expansion exceeds the supported total size; reduce nested fixed-array lengths or provide a custom codec",
                ));
            }
        }
        let mut generator = Generator {
            json: &alias,
            encoder: String::new(),
            used_names: identifiers
                .iter()
                .map(|name| (*name).to_owned())
                .chain(std::iter::once(alias.clone()))
                .collect(),
            next: 0,
            source: String::new(),
        };
        let writer = generator.fresh("__JsonWriter");
        let encoder = generator.fresh("encoder");
        let value = generator.fresh("value");
        let key_binding = generator.fresh("__json_keys");
        generator.encoder = encoder.clone();
        // A private type can still participate in a public generic protocol.
        // Its name remains private, while the generic caller can invoke its
        // explicitly public codec methods after specialization.
        let visibility = "pub ";
        writeln!(
            generator.source,
            "package generated\nstruct {} {{",
            structure.name
        )
        .unwrap();
        writeln!(generator.source, "{visibility}fn encode_json<{writer}>(&self, {encoder}: &mut {alias}.Encoder<{writer}>) -> void!{alias}.Error {{\n{encoder}.begin_object()? ").unwrap();
        for (field, name) in structure.fields.iter().zip(&derive.json_names) {
            writeln!(generator.source, "{encoder}.key({name:?})?").unwrap();
            generator.encode(&field.ty, &format!("self.{}", field.name), false);
        }
        writeln!(generator.source, "{encoder}.end_object()?\nreturn ok()\n}}\n{visibility}fn decode_json({value}: {alias}.Value) -> Self!{alias}.Error {{").unwrap();
        let keys = derive
            .json_names
            .iter()
            .map(|name| format!("{name:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            generator.source,
            "{key_binding} := ([{keys}]: [{}]&str)\n{value}.check_fields(&{key_binding}, {})?",
            derive.json_names.len(),
            derive.deny_unknown
        )
        .unwrap();
        let mut decoded = Vec::new();
        for (field, name) in structure.fields.iter().zip(&derive.json_names) {
            let result = if let Type::Option(inner) = &field.ty {
                let output = generator.local();
                let raw = generator.local();
                let rest = generator.local();
                let optional = generator.local();
                writeln!(generator.source, "let {alias}.OptionalField {{ rest: {rest}, value: {optional} }} = {alias}.take_optional_field({value}, {name:?})?\n{value} = {rest}\n{output}: {} = none\nmatch {optional} {{\nsome({raw}) => {{\nif {raw}.kind() != {alias}.Kind.Null {{", field.ty).unwrap();
                let item = generator.decode(inner, &raw);
                writeln!(
                    generator.source,
                    "{output} = some({item})\n}}\n}},\nnone => {{}},\n}}"
                )
                .unwrap();
                output
            } else {
                let raw = generator.local();
                let rest = generator.local();
                writeln!(generator.source, "let {alias}.Field {{ rest: {rest}, value: {raw} }} = {alias}.take_field({value}, {name:?})?\n{value} = {rest}").unwrap();
                generator.decode(&field.ty, &raw)
            };
            decoded.push(format!("{}: {result}", field.name));
        }
        writeln!(
            generator.source,
            "return ok(Self {{ {} }})\n}}\n}}",
            decoded.join(", ")
        )
        .unwrap();
        let mut expanded = crate::parser::parse(&generator.source).map_err(|error| {
            Diagnostic::new(
                structure.span,
                format!("cannot expand @derive(Json): {}", error.message),
            )
            .note(format!("generated source: {}", generator.source))
        })?;
        // Zero-width locations keep generated syntax out of source formatter
        // token ranges while still placing diagnostics on the deriving struct.
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
    Ok(())
}

fn expansion_cost(ty: &Type) -> usize {
    match ty {
        Type::Array(size, inner) => size
            .saturating_mul(expansion_cost(inner).saturating_add(1))
            .saturating_add(1),
        Type::Option(inner) => expansion_cost(inner).saturating_add(1),
        _ => 1,
    }
}

fn validate_type(ty: &Type, span: Span) -> Result<(), Diagnostic> {
    match ty {
        Type::Bool | Type::Int { .. } | Type::Float(_) | Type::Str | Type::Named(_) => Ok(()),
        Type::Option(inner) => validate_type(inner, span),
        Type::Array(size, inner) if *size <= 4096 => validate_type(inner, span),
        Type::Array(..) => Err(Diagnostic::new(
            span,
            "@derive(Json) supports fixed arrays of at most 4096 elements",
        )),
        Type::ArrayExpr(..) => Err(Diagnostic::new(
            span,
            "@derive(Json) array lengths must currently be integer literals",
        )),
        _ => Err(Diagnostic::new(
            span,
            format!(
                "@derive(Json) does not support field type `{ty}`; use a concrete type with encode_json/decode_json methods"
            ),
        )),
    }
}

struct Generator<'a> {
    json: &'a str,
    encoder: String,
    used_names: HashSet<String>,
    next: usize,
    source: String,
}

impl Generator<'_> {
    fn fresh(&mut self, base: &str) -> String {
        let mut name = base.to_owned();
        while !self.used_names.insert(name.clone()) {
            name.push('_');
        }
        name
    }

    fn local(&mut self) -> String {
        loop {
            let name = format!("__json_{}", self.next);
            self.next += 1;
            if self.used_names.insert(name.clone()) {
                return name;
            }
        }
    }

    fn encode(&mut self, ty: &Type, value: &str, borrowed: bool) {
        let encoder = self.encoder.clone();
        let copied = if borrowed {
            format!("(*{value})")
        } else {
            value.to_owned()
        };
        match ty {
            Type::Bool => writeln!(self.source, "{encoder}.boolean({copied})?").unwrap(),
            Type::Int { signed, .. } => writeln!(
                self.source,
                "{encoder}.{}({copied} as {})?",
                if *signed { "signed" } else { "unsigned" },
                if *signed { "i64" } else { "u64" }
            )
            .unwrap(),
            Type::Float(_) => {
                writeln!(self.source, "{encoder}.floating({copied} as f64)?").unwrap()
            }
            Type::Str => writeln!(self.source, "{encoder}.string({copied})?").unwrap(),
            Type::Named(_) => writeln!(self.source, "{value}.encode_json({encoder})?").unwrap(),
            Type::Option(inner) => {
                let item = self.local();
                let reference = if borrowed {
                    value.to_owned()
                } else {
                    format!("&{value}")
                };
                writeln!(self.source, "match {reference} {{\nsome({item}) => {{").unwrap();
                self.encode(inner, &item, true);
                writeln!(self.source, "}},\nnone => {{ {encoder}.null()? }},\n}}").unwrap();
            }
            Type::Array(_, inner) => {
                let item = self.local();
                let reference = if borrowed {
                    value.to_owned()
                } else {
                    format!("&{value}")
                };
                writeln!(
                    self.source,
                    "{encoder}.begin_array()?\nfor {item} in {reference} {{"
                )
                .unwrap();
                self.encode(inner, &item, true);
                writeln!(self.source, "}}\n{encoder}.end_array()?").unwrap();
            }
            _ => unreachable!("types validated before expansion"),
        }
    }

    fn fail(&self, kind: &str, value: &str) -> String {
        format!(
            "return err({}.Error {{ kind: {}.ErrorKind.{kind}, position: {value}.position() }})",
            self.json, self.json
        )
    }

    fn decode(&mut self, ty: &Type, value: &str) -> String {
        let output = self.local();
        match ty {
            Type::Bool => writeln!(self.source, "{output} := {value}.as_bool()?").unwrap(),
            Type::Str => writeln!(self.source, "{output} := {value}.into_str()?").unwrap(),
            Type::Int { signed, bits } => {
                let wide = if *signed { "i64" } else { "u64" };
                let raw = self.local();
                writeln!(self.source, "{raw} := {value}.as_{wide}()?").unwrap();
                // Dodo casts are checked. Test bounds before narrowing so an
                // invalid JSON number returns an Error instead of trapping.
                let outside = if *bits == 0 && *signed {
                    let max = self.local();
                    writeln!(self.source, "{max} := ((~0usize) >> 1usize) as i64").unwrap();
                    Some(format!("{raw} < -{max} - 1i64 || {raw} > {max}"))
                } else if *bits == 0 {
                    Some(format!("{raw} > ((~0usize) as u64)"))
                } else if *bits < 64 && *signed {
                    let min = -(1i64 << (bits - 1));
                    let max = (1i64 << (bits - 1)) - 1;
                    Some(format!("{raw} < {min}i64 || {raw} > {max}i64"))
                } else if *bits < 64 {
                    let max = (1u64 << bits) - 1;
                    Some(format!("{raw} > {max}u64"))
                } else {
                    None
                };
                if let Some(outside) = outside {
                    writeln!(
                        self.source,
                        "if {outside} {{ {} }}",
                        self.fail("NumberRange", value)
                    )
                    .unwrap();
                }
                writeln!(self.source, "{output} := {raw} as {ty}").unwrap();
            }
            Type::Float(bits) => {
                let raw = self.local();
                writeln!(self.source, "{raw} := {value}.as_f64()?").unwrap();
                if *bits == 32 {
                    writeln!(self.source, "if {raw} > 3.4028234663852886e38 || {raw} < -3.4028234663852886e38 {{ {} }}", self.fail("NumberRange", value)).unwrap();
                }
                writeln!(self.source, "{output} := {raw} as {ty}").unwrap();
            }
            Type::Named(name) => {
                writeln!(self.source, "{output} := {name}.decode_json({value})?").unwrap()
            }
            Type::Option(inner) => {
                writeln!(
                    self.source,
                    "{output}: {ty} = none\nif {value}.kind() != {}.Kind.Null {{",
                    self.json
                )
                .unwrap();
                let item = self.decode(inner, value);
                writeln!(self.source, "{output} = some({item})\n}}").unwrap();
            }
            Type::Array(size, inner) => {
                let array = self.local();
                writeln!(self.source, "{array} := {value}").unwrap();
                let value = array.as_str();
                writeln!(self.source, "if {value}.kind() != {}.Kind.Array {{ {} }}\nif {value}.len()? != {size}usize {{ {} }}", self.json, self.fail("TypeMismatch", value), self.fail("TypeMismatch", value)).unwrap();
                let mut items = Vec::new();
                for index in 0..*size {
                    let raw = self.local();
                    let rest = self.local();
                    writeln!(self.source, "let {}.Field {{ rest: {rest}, value: {raw} }} = {}.take_element({value}, {index}usize)?\n{value} = {rest}", self.json, self.json).unwrap();
                    items.push(self.decode(inner, &raw));
                }
                writeln!(self.source, "{output} := ([{}]: {ty})", items.join(", ")).unwrap();
            }
            _ => unreachable!("types validated before expansion"),
        }
        output
    }
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
