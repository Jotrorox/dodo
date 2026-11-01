//! Expand JSON derives into ordinary checked Dodo methods.
//!
//! The expansion deliberately uses the regular parser, type checker and borrow
//! checker. No JSON operation receives privileged access to memory or lifetimes.
use super::{Argument, AttributeSpec, Context, Names, Request, Spec, Target};
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;

pub(crate) static SPEC: Spec = Spec {
    name: "Json",
    attributes: &[
        AttributeSpec {
            name: "json_name",
            target: Target::Field,
            argument: Argument::Text("a string field name"),
        },
        AttributeSpec {
            name: "json_deny_unknown",
            target: Target::Struct,
            argument: Argument::Flag,
        },
    ],
    generate,
};

fn generate(
    context: &mut Context,
    structure: &Struct,
    request: &Request,
) -> Result<String, Diagnostic> {
    let alias = context.import("std/encoding/json");
    let json_names: Vec<String> = structure
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            request
                .field_text(index, "json_name")
                .unwrap_or(&field.name)
                .to_owned()
        })
        .collect();
    let deny_unknown = request.flag("json_deny_unknown");
    let mut names = HashSet::new();
    let mut expansion_size = 0usize;
    for (field, name) in structure.fields.iter().zip(&json_names) {
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
        names: context.names(),
        source: String::new(),
    };
    let writer = generator.names.fresh("__JsonWriter");
    let encoder = generator.names.fresh("encoder");
    let value = generator.names.fresh("value");
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
    for (field, name) in structure.fields.iter().zip(&json_names) {
        writeln!(generator.source, "{encoder}.key({name:?})?").unwrap();
        generator.encode(&field.ty, &format!("self.{}", field.name), false);
    }
    writeln!(generator.source, "{encoder}.end_object()?\nreturn ok()\n}}\n{visibility}fn decode_json({value}: {alias}.Value) -> Self!{alias}.Error {{").unwrap();
    let position = generator.local();
    let cursor = generator.local();
    writeln!(
        generator.source,
        "{position} := {value}.position()\n{cursor} := {alias}.ObjectCursor.new({value})?"
    )
    .unwrap();
    // Optional raw views track presence without requiring default values
    // for user types. Decode them in declaration order after dispatch so
    // unknown-field errors, custom codecs and cleanup keep their ordering.
    let mut slots = Vec::new();
    let mut dispatch = BTreeMap::<u32, Vec<(String, String)>>::new();
    for name in &json_names {
        let slot = generator.local();
        writeln!(generator.source, "{slot}: Option<{alias}.Value> = none").unwrap();
        dispatch
            .entry(field_hash(name))
            .or_default()
            .push((name.clone(), slot.clone()));
        slots.push(slot);
    }
    let member_position = generator.local();
    let rest = generator.local();
    let key = generator.local();
    let hash = generator.local();
    let raw = generator.local();
    writeln!(generator.source, "for {cursor}.has_next() {{\n{member_position} := {cursor}.position()\nlet {alias}.ObjectField {{ rest: {rest}, key: {key}, hash: {hash}, value: {raw} }} = {cursor}.take()?\n{cursor} = {rest}").unwrap();
    generator.dispatch_fields(&dispatch.into_iter().collect::<Vec<_>>(), &hash, &key, &raw);
    if deny_unknown {
        writeln!(generator.source, "return err({alias}.Error {{ kind: {alias}.ErrorKind.UnknownField, position: {member_position} }})").unwrap();
    }
    writeln!(generator.source, "}}").unwrap();
    let mut decoded = Vec::new();
    for (field, slot) in structure.fields.iter().zip(&slots) {
        let result = generator.local();
        let raw = generator.local();
        writeln!(
            generator.source,
            "{result}: {} = match {slot} {{\nsome({raw}) => {{",
            field.ty
        )
        .unwrap();
        let item = generator.decode(&field.ty, &raw);
        writeln!(generator.source, "{item}\n}},\nnone => {{").unwrap();
        if matches!(field.ty, Type::Option(_)) {
            writeln!(generator.source, "none").unwrap();
        } else {
            writeln!(generator.source, "return err({alias}.Error {{ kind: {alias}.ErrorKind.MissingField, position: {position} }})").unwrap();
        }
        writeln!(generator.source, "}},\n}}").unwrap();
        decoded.push(format!("{}: {result}", field.name));
    }
    writeln!(
        generator.source,
        "return ok(Self {{ {} }})\n}}\n}}",
        decoded.join(", ")
    )
    .unwrap();
    Ok(generator.source)
}

// Keep in sync with ObjectCursor.take: FNV-1a over decoded Unicode scalars,
// using u32 on every target. Hash collisions always receive a full name check.
fn field_hash(name: &str) -> u32 {
    name.chars().fold(2_166_136_261, |hash, scalar| {
        (hash ^ scalar as u32).wrapping_mul(16_777_619)
    })
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
    names: Names,
    source: String,
}

impl Generator<'_> {
    fn dispatch_fields(
        &mut self,
        buckets: &[(u32, Vec<(String, String)>)],
        hash: &str,
        key: &str,
        value: &str,
    ) {
        if buckets.len() > 1 {
            // A balanced tree bounds dispatch even without LLVM optimization.
            let middle = buckets.len() / 2;
            writeln!(self.source, "if {hash} < {}u32 {{", buckets[middle].0).unwrap();
            self.dispatch_fields(&buckets[..middle], hash, key, value);
            writeln!(self.source, "}} else {{").unwrap();
            self.dispatch_fields(&buckets[middle..], hash, key, value);
            writeln!(self.source, "}}").unwrap();
        } else if let Some((expected, fields)) = buckets.first() {
            writeln!(self.source, "if {hash} == {expected}u32 {{").unwrap();
            for (name, slot) in fields {
                writeln!(
                    self.source,
                    "if {key}.equals({name:?}) {{\n{slot} = some({value})\ncontinue\n}}"
                )
                .unwrap();
            }
            writeln!(self.source, "}}").unwrap();
        }
    }

    fn local(&mut self) -> String {
        self.names.local("__json")
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
                writeln!(
                    self.source,
                    "{array} := {}.ArrayCursor.new({value})?",
                    self.json
                )
                .unwrap();
                // Primitive elements can be initialized and replaced in a loop.
                // Keep individual locals for borrowed/custom values so ordinary
                // ownership checking and failure cleanup still apply to them.
                let initial = match inner.as_ref() {
                    Type::Bool => Some("false".to_owned()),
                    Type::Int { .. } | Type::Float(_) => Some(format!("0 as {inner}")),
                    _ => None,
                };
                if let Some(initial) = initial {
                    let index = self.local();
                    writeln!(
                        self.source,
                        "{output}: {ty} = [{initial}; {size}]\nfor {index} in 0usize..{size}usize {{"
                    )
                    .unwrap();
                    let raw = self.take_array_element(&array);
                    let item = self.decode(inner, &raw);
                    writeln!(
                        self.source,
                        "{output}[{index}] = {item}\n}}\n{array}.finish()?"
                    )
                    .unwrap();
                    return output;
                }
                let mut items = Vec::new();
                for _ in 0..*size {
                    let raw = self.take_array_element(&array);
                    items.push(self.decode(inner, &raw));
                }
                writeln!(
                    self.source,
                    "{array}.finish()?\n{output} := ([{}]: {ty})",
                    items.join(", ")
                )
                .unwrap();
            }
            _ => unreachable!("types validated before expansion"),
        }
        output
    }

    fn take_array_element(&mut self, array: &str) -> String {
        let raw = self.local();
        let rest = self.local();
        writeln!(self.source, "let {}.ArrayElement {{ rest: {rest}, value: {raw} }} = {array}.take()?\n{array} = {rest}", self.json).unwrap();
        raw
    }
}
