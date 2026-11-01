//! `@derive(Row)`: map a struct to SQL result rows and statement parameters.
//!
//! The expansion generates two ordinary methods:
//!
//! - `decode_row<R: sql.Row>(row: &R) -> Self!db.Error` finds each field's
//!   column by name and converts its `db.Value`. Borrowed fields (`&str`,
//!   `&[u8]`) borrow the row, so decoding never copies or allocates. Bounded
//!   generics are specialized, and `R` may be `dyn sql.Row` inside a
//!   `sql.RowSink`.
//! - `params(&self) -> [N]db.Value` returns the fields as statement
//!   parameters in declaration order, borrowing text and bytes from `self`.
//!
//! Conversions go through small helpers in `std/db/sql` (`sql.column`,
//! `sql.int_in`, `sql.text`, ...), so hand-written `decode_row` methods for
//! custom types use exactly the same rules.
use super::{Argument, AttributeSpec, Context, Names, Request, Spec, Target};
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::HashSet;
use std::fmt::Write;

pub(crate) static SPEC: Spec = Spec {
    name: "Row",
    attributes: &[
        AttributeSpec {
            name: "db_name",
            target: Target::Field,
            argument: Argument::Text("a string column name"),
        },
        AttributeSpec {
            name: "db_flatten",
            target: Target::Field,
            argument: Argument::Flag,
        },
    ],
    generate,
};

/// How one field maps onto the row.
enum Mapping {
    /// One column, matched by name.
    Column(String),
    /// A nested `@derive(Row)` struct that reads its own columns from the
    /// same row (for joins).
    Flatten(String),
}

fn generate(
    context: &mut Context,
    structure: &Struct,
    request: &Request,
) -> Result<String, Diagnostic> {
    if structure.fields.is_empty() {
        return Err(Diagnostic::new(
            structure.span,
            "@derive(Row) needs at least one field",
        ));
    }
    let mut mappings = Vec::new();
    let mut columns = HashSet::new();
    for (index, field) in structure.fields.iter().enumerate() {
        let name = request.field_text(index, "db_name");
        if request.field_flag(index, "db_flatten") {
            if let Some(attribute) = request.field_attribute(index, "db_name") {
                return Err(Diagnostic::new(
                    attribute.span,
                    "@db_flatten fields read their own columns; remove @db_name",
                ));
            }
            let Type::Named(nested) = &field.ty else {
                return Err(Diagnostic::new(
                    field.span,
                    format!(
                        "@db_flatten needs a struct type with a `decode_row` method, not `{}`",
                        field.ty
                    ),
                ));
            };
            mappings.push(Mapping::Flatten(nested.clone()));
            continue;
        }
        let column = name.unwrap_or(&field.name).to_owned();
        if column.is_empty() {
            return Err(Diagnostic::new(
                request.field_attribute(index, "db_name").unwrap().span,
                "@db_name needs a non-empty column name",
            ));
        }
        if !columns.insert(column.clone()) {
            return Err(Diagnostic::new(
                field.span,
                format!("duplicate column name `{column}`"),
            ));
        }
        validate_type(&field.ty, field.span, false)?;
        mappings.push(Mapping::Column(column));
    }
    let db = context.import("std/db");
    let sql = context.import("std/db/sql");
    let mut generator = Generator {
        db: &db,
        sql: &sql,
        names: context.names(),
        source: String::new(),
    };
    let source_type = generator.names.fresh("__RowSource");
    let row = generator.names.fresh("row");
    writeln!(
        generator.source,
        "package generated\nstruct {} {{\npub fn decode_row<{source_type}: {sql}.Row>({row}: &{source_type}) -> Self!{db}.Error {{",
        structure.name
    )
    .unwrap();
    let mut decoded = Vec::new();
    for (index, (field, mapping)) in structure.fields.iter().zip(&mappings).enumerate() {
        let output = match mapping {
            Mapping::Column(column) => {
                // The field's position is tried first, so a SELECT listing
                // columns in field order costs one name comparison per field.
                let value = generator.names.local("__row");
                writeln!(
                    generator.source,
                    "{value} := {row}.get({sql}.column({row}, {column:?}, {index}usize)?)?"
                )
                .unwrap();
                generator.decode(&field.ty, &value)
            }
            Mapping::Flatten(nested) => {
                let output = generator.names.local("__row");
                writeln!(generator.source, "{output} := {nested}.decode_row({row})?").unwrap();
                output
            }
        };
        decoded.push(format!("{}: {output}", field.name));
    }
    writeln!(
        generator.source,
        "return ok(Self {{ {} }})\n}}",
        decoded.join(", ")
    )
    .unwrap();
    // The parameter count of a flattened struct is not known while parsing
    // this file, so structs with @db_flatten fields only decode.
    if mappings
        .iter()
        .all(|mapping| matches!(mapping, Mapping::Column(_)))
    {
        generator.params(structure);
    }
    writeln!(generator.source, "}}").unwrap();
    Ok(generator.source)
}

fn validate_type(ty: &Type, span: Span, optional: bool) -> Result<(), Diagnostic> {
    match ty {
        Type::Int {
            signed: false,
            bits: 64 | 0,
        } => Err(Diagnostic::new(
            span,
            format!(
                "@derive(Row) does not support `{ty}`: SQL integers are signed 64-bit; use i64 or a custom column type"
            ),
        )),
        Type::Bool | Type::Int { .. } | Type::Float(_) | Type::Str | Type::Named(_) => Ok(()),
        Type::Slice(false, inner)
            if matches!(
                **inner,
                Type::Int {
                    signed: false,
                    bits: 8
                }
            ) =>
        {
            Ok(())
        }
        Type::Option(inner) if !optional => validate_type(inner, span, true),
        Type::Option(_) => Err(Diagnostic::new(
            span,
            "@derive(Row) maps NULL to `none`, so nested options are not supported",
        )),
        _ => Err(Diagnostic::new(
            span,
            format!(
                "@derive(Row) does not support field type `{ty}`; use a struct with decode_value/encode_value methods"
            ),
        )),
    }
}

struct Generator<'a> {
    db: &'a str,
    sql: &'a str,
    names: Names,
    source: String,
}

impl Generator<'_> {
    /// Emits statements converting the `db.Value` local `value` to `ty` and
    /// returns the local holding the result.
    fn decode(&mut self, ty: &Type, value: &str) -> String {
        let sql = self.sql;
        let output = self.names.local("__row");
        match ty {
            Type::Option(inner) => {
                writeln!(
                    self.source,
                    "{output}: {ty} = none\nif !{sql}.is_null(&{value}) {{"
                )
                .unwrap();
                let item = self.decode(inner, value);
                writeln!(self.source, "{output} = some({item})\n}}").unwrap();
            }
            Type::Named(name) => {
                writeln!(self.source, "{output} := {name}.decode_value({value})?").unwrap();
            }
            _ => {
                let expression = self.scalar(ty, value);
                writeln!(self.source, "{output} := {expression}").unwrap();
            }
        }
        output
    }

    fn scalar(&mut self, ty: &Type, value: &str) -> String {
        let sql = self.sql;
        match ty {
            Type::Bool => format!("{sql}.boolean({value})?"),
            Type::Int {
                signed: true,
                bits: 64,
            } => format!("{sql}.int({value})?"),
            Type::Int {
                signed: true,
                bits: 0,
            } => {
                // Pointer-sized: the bounds depend on the target.
                let max = self.names.local("__row");
                writeln!(self.source, "{max} := ((~0usize) >> 1usize) as i64").unwrap();
                format!("({sql}.int_in({value}, -{max} - 1i64, {max})? as isize)")
            }
            Type::Int { signed, bits } => {
                let (min, max) = if *signed {
                    (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1)
                } else {
                    (0, (1i64 << bits) - 1)
                };
                format!("({sql}.int_in({value}, {min}i64, {max}i64)? as {ty})")
            }
            Type::Float(32) => format!("{sql}.real32({value})?"),
            Type::Float(_) => format!("{sql}.real({value})?"),
            Type::Str => format!("{sql}.text({value})?"),
            Type::Slice(..) => format!("{sql}.bytes({value})?"),
            _ => unreachable!("field types are validated before expansion"),
        }
    }

    fn params(&mut self, structure: &Struct) {
        let db = self.db;
        writeln!(
            self.source,
            "pub fn params(&self) -> [{}]{db}.Value {{",
            structure.fields.len()
        )
        .unwrap();
        let mut values = Vec::new();
        for field in &structure.fields {
            let place = format!("self.{}", field.name);
            let value = if let Type::Option(inner) = &field.ty {
                let item = self.names.local("__param");
                let output = self.names.local("__param");
                let reference = if matches!(**inner, Type::Named(_)) {
                    item.clone()
                } else {
                    format!("(*{item})")
                };
                let encoded = self.encode(inner, &reference);
                writeln!(
                    self.source,
                    "{output} := match &{place} {{\nsome({item}) => {encoded},\nnone => {db}.Value.Null,\n}}"
                )
                .unwrap();
                output
            } else {
                self.encode(&field.ty, &place)
            };
            values.push(value);
        }
        writeln!(self.source, "return [{}]\n}}", values.join(", ")).unwrap();
    }

    /// A `db.Value` expression for the field value `place` of type `ty`.
    fn encode(&self, ty: &Type, place: &str) -> String {
        let db = self.db;
        match ty {
            Type::Bool => format!("{db}.Value.Int(if {place} {{ 1i64 }} else {{ 0i64 }})"),
            Type::Int { .. } => format!("{db}.Value.Int({place} as i64)"),
            Type::Float(_) => format!("{db}.Value.Real({place} as f64)"),
            Type::Str => format!("{db}.Value.Text({place})"),
            Type::Slice(..) => format!("{db}.Value.Bytes({place})"),
            Type::Named(_) => format!("{place}.encode_value()"),
            _ => unreachable!("field types are validated before expansion"),
        }
    }
}
