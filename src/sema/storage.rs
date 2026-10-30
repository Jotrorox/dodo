//! Typed owner-bound allocated storage. A zero-length array witnesses the
//! stored type without owning an active payload. Raw pointer validity remains
//! unsafe; `flow` checks dependency deposition and return contracts.
use super::*;

impl Context {
    pub(super) fn storage_elements(&self, ty: &Type) -> Vec<Type> {
        match ty {
            Type::Array(0, element) => vec![*element.clone()],
            Type::Named(name) => self.structs.get(name).map_or_else(Vec::new, |s| {
                s.fields
                    .iter()
                    .flat_map(|f| self.storage_elements(&f.ty))
                    .collect()
            }),
            Type::Option(t) | Type::Array(_, t) => self.storage_elements(t),
            _ => vec![],
        }
    }
    pub(super) fn has_storage(&self, ty: &Type) -> bool {
        !self.storage_elements(ty).is_empty()
    }
    // Parameter dependencies summarize the entire argument, including owners
    // behind aggregate fields and checked references. Their stored sources
    // must survive ownership moves and projections before an eventual take.
    pub(super) fn contains_storage_witness(&self, ty: &Type) -> bool {
        fn contains(context: &Context, ty: &Type, seen: &mut HashSet<String>) -> bool {
            match ty {
                Type::Array(0, _) => true,
                Type::Ref(_, t) | Type::Slice(_, t) | Type::Array(_, t) | Type::Option(t) => {
                    contains(context, t, seen)
                }
                Type::Result(ok, err) => {
                    contains(context, ok, seen) || contains(context, err, seen)
                }
                Type::Named(name) if seen.insert(name.clone()) => {
                    context
                        .structs
                        .get(name)
                        .is_some_and(|s| s.fields.iter().any(|f| contains(context, &f.ty, seen)))
                        || context.enums.get(name).is_some_and(|e| {
                            e.variants
                                .iter()
                                .any(|v| v.fields.iter().any(|f| contains(context, &f.ty, seen)))
                        })
                }
                _ => false,
            }
        }
        contains(self, ty, &mut HashSet::new())
    }
    // Matching through a conservative region union cannot identify which
    // source Result was observed. Keep Result-bearing referents out of scope,
    // independently of contains_result (which tracks *owned* obligations).
    fn storage_result_reachable(&self, ty: &Type, seen: &mut HashSet<String>) -> bool {
        match ty {
            Type::Result(..) => true,
            Type::Ref(_, t) | Type::Slice(_, t) | Type::Array(_, t) | Type::Option(t) => {
                self.storage_result_reachable(t, seen)
            }
            Type::Named(name) if seen.insert(name.clone()) => {
                self.structs.get(name).is_some_and(|s| {
                    s.fields
                        .iter()
                        .any(|f| self.storage_result_reachable(&f.ty, seen))
                }) || self.enums.get(name).is_some_and(|e| {
                    e.variants.iter().any(|v| {
                        v.fields
                            .iter()
                            .any(|f| self.storage_result_reachable(&f.ty, seen))
                    })
                })
            }
            _ => false,
        }
    }
    pub(super) fn check_storage_element(&self, ty: &Type, span: Span) -> Check<()> {
        self.validate_type(ty, span, false)?;
        if self.storage_result_reachable(ty, &mut HashSet::new()) {
            return Err(Diagnostic::new(
                span,
                "Result-containing collection elements, including Result-bearing references, are unsupported: insertion failure, replacement, clear, drop and indexed matching cannot erase handling obligations",
            ));
        }
        if self.carries_mutable_borrow(ty) {
            return Err(Diagnostic::new(
                span,
                "collection elements carrying exclusive borrows are unsupported; use shared references or owned shared allocator handles",
            ));
        }
        Ok(())
    }
    pub(super) fn validate_stored_source(&self, function: &Function, source: &str) -> Check<()> {
        if !function
            .params
            .iter()
            .any(|p| p.name == source && matches!(&p.ty, Type::Ref(_, t) if self.has_storage(t)))
        {
            return Err(Diagnostic::new(
                function.ret_span,
                "from(owner.stored) requires a checked reference to a typed storage owner",
            ));
        }
        Ok(())
    }
    pub(super) fn validate_stores(&self, function: &Function) -> Check<()> {
        if function.stores.is_empty() {
            return Ok(());
        }
        if function.stores.len() < 2 || function.extern_ {
            return Err(Diagnostic::new(
                function.span,
                "stores(target, source...) requires a checked body and at least one source",
            ));
        }
        let target = &function.stores[0];
        if !function.params.iter().any(|p| {
            &p.name == target && matches!(&p.ty, Type::Ref(true, t) if self.has_storage(t))
        }) {
            return Err(Diagnostic::new(
                function.span,
                "stores target must be an exclusive reference to a typed storage owner",
            ));
        }
        for source in &function.stores[1..] {
            if source == target || !function.params.iter().any(|p| &p.name == source) {
                return Err(Diagnostic::new(
                    function.span,
                    format!("stores source `{source}` must name a different parameter"),
                ));
            }
        }
        Ok(())
    }
}

impl Checker<'_> {
    pub(super) fn storage_intrinsic(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &mut [Expr],
        span: Span,
    ) -> Check<Value> {
        self.unsafe_required(span, "accessing typed owner-bound storage")?;
        let operation = name.strip_prefix("core.ptr.").unwrap();
        let arity = match operation {
            "store" | "view_slice" => 3,
            "relocate" => 4,
            _ => 2,
        };
        if args.len() != arity || type_args.len() > 1 {
            return Err(Diagnostic::new(
                span,
                format!("ptr.{operation} expects {arity} arguments and at most one element type"),
            ));
        }
        let pointer = self.expr(&mut args[0], None, false)?;
        let Type::Raw(writable, element) = pointer.ty else {
            return Err(Diagnostic::new(
                span,
                "typed storage access requires a raw pointer",
            ));
        };
        self.context.check_storage_element(&element, span)?;
        if let Some(expected) = type_args.first() {
            self.expect(expected, &element, span)?;
        }
        let view = operation.starts_with("view");
        if !view && !writable {
            return Err(Diagnostic::new(
                span,
                "moving typed storage requires a mutable raw pointer",
            ));
        }
        if operation == "store" {
            let value = self.expr(&mut args[1], Some(&element), true)?;
            self.expect(&element, &value.ty, args[1].span)?;
        }
        if operation == "relocate" {
            let destination = self.expr(&mut args[1], None, false)?;
            self.expect(
                &Type::Raw(true, element.clone()),
                &destination.ty,
                args[1].span,
            )?;
        }
        if operation == "relocate" || operation == "view_slice" {
            let index = arity - 2;
            let count = self.expr(&mut args[index], Some(&Type::usize()), false)?;
            self.expect(&Type::usize(), &count.ty, args[index].span)?;
        }
        let owner = self.intrinsic_reference(&mut args[arity - 1])?;
        let Type::Ref(mutable, inner) = &owner.ty else {
            return Err(Diagnostic::new(
                span,
                "typed storage access requires a checked owner reference",
            ));
        };
        if !view && !mutable {
            return Err(Diagnostic::new(
                span,
                "moving typed storage requires an exclusive owner borrow",
            ));
        }
        if !self.context.storage_elements(inner).contains(&element) {
            return Err(Diagnostic::new(
                span,
                "typed storage owner lacks a zero-length witness for this element type",
            ));
        }
        Ok(Value {
            ty: match operation {
                "view" => Type::Ref(false, element),
                "view_slice" => Type::Slice(false, element),
                "take" => *element,
                _ => Type::Void,
            },
        })
    }
}
