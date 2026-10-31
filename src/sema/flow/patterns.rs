//! Pattern bindings of a checked pattern: their types, and the field paths
//! that project a borrowed subject's own loans.
use crate::ast::{Pattern, Type};
use crate::sema::Context;
use std::collections::HashMap;

/// Bindings in the order the checker declares them, with their types.
pub(super) fn bindings(
    context: &Context,
    pattern: &Pattern,
    ty: &Type,
    borrowed: Option<bool>,
) -> Vec<(String, Type)> {
    let mut result = vec![];
    collect(context, pattern, ty, borrowed, &mut result);
    result
}

fn collect(
    context: &Context,
    pattern: &Pattern,
    ty: &Type,
    borrowed: Option<bool>,
    result: &mut Vec<(String, Type)>,
) {
    match pattern {
        Pattern::Or(alternatives) => {
            if let Some(first) = alternatives.first() {
                collect(context, first, ty, borrowed, result);
            }
            return;
        }
        Pattern::Binding(name) => {
            let ty = borrowed.map_or_else(|| ty.clone(), |m| Type::Ref(m, Box::new(ty.clone())));
            result.push((name.clone(), ty));
            return;
        }
        Pattern::Wildcard => return,
        _ => (),
    }
    let mut ty = ty;
    let mut borrowed = borrowed;
    while let Type::Ref(mutable, inner) = ty {
        borrowed = Some(borrowed.unwrap_or(true) && *mutable);
        ty = inner;
    }
    match pattern {
        Pattern::Variant(name, payloads) => {
            let short = name.rsplit_once('.').map_or(name.as_str(), |(_, n)| n);
            let fields = variants(context, ty).remove(short).unwrap_or_default();
            for (pattern, field) in payloads.iter().zip(&fields) {
                collect(context, pattern, field, borrowed, result);
            }
        }
        Pattern::Struct(name, fields, _) => {
            let Some(declaration) = context.structs.get(name) else {
                return;
            };
            let mut found = vec![];
            for field in &declaration.fields {
                if let Some((_, pattern)) = fields.iter().find(|(n, _)| n == &field.name) {
                    collect(context, pattern, &field.ty, borrowed, &mut found);
                }
            }
            let order = pattern.bindings();
            found.sort_by_key(|(name, _)| {
                order.iter().position(|n| n == name).unwrap_or(usize::MAX)
            });
            result.extend(found);
        }
        _ => (),
    }
}

fn variants(context: &Context, ty: &Type) -> HashMap<String, Vec<Type>> {
    match ty {
        Type::Result(t, e) => [
            (
                "ok".into(),
                if **t == Type::Void {
                    vec![]
                } else {
                    vec![t.as_ref().clone()]
                },
            ),
            ("err".into(), vec![e.as_ref().clone()]),
        ]
        .into(),
        Type::Option(t) => [
            ("some".into(), vec![t.as_ref().clone()]),
            ("none".into(), vec![]),
        ]
        .into(),
        Type::Named(name) => context.enums.get(name).map_or_else(HashMap::new, |e| {
            e.variants
                .iter()
                .map(|v| {
                    (
                        v.name.clone(),
                        v.fields.iter().map(|f| f.ty.clone()).collect(),
                    )
                })
                .collect()
        }),
        _ => HashMap::new(),
    }
}

/// Field paths from the subject to each binding (alternatives add paths).
pub(super) fn binding_paths(
    pattern: &Pattern,
    path: &[String],
    paths: &mut HashMap<String, Vec<Vec<String>>>,
) {
    match pattern {
        Pattern::Binding(name) => {
            paths.entry(name.clone()).or_default().push(path.to_vec());
        }
        Pattern::Struct(_, fields, _) => {
            for (field, pattern) in fields {
                let mut projected = path.to_vec();
                projected.push(field.clone());
                binding_paths(pattern, &projected, paths);
            }
        }
        Pattern::Variant(_, fields) | Pattern::Or(fields) => {
            for pattern in fields {
                binding_paths(pattern, path, paths);
            }
        }
        _ => (),
    }
}

/// Bindings of an owned SplitMut destructure, by side (`true` is `right`).
pub(super) fn split_sides(
    context: &Context,
    pattern: &Pattern,
    ty: &Type,
    sides: &mut HashMap<String, bool>,
) {
    match (pattern, ty) {
        (Pattern::Struct(_, fields, _), _) if context.split_mut_element(ty).is_some() => {
            for (field, pattern) in fields {
                for name in pattern.bindings() {
                    sides.insert(name, field == "right");
                }
            }
        }
        (Pattern::Variant(_, fields), Type::Option(element)) if fields.len() == 1 => {
            split_sides(context, &fields[0], element, sides);
        }
        _ => (),
    }
}
