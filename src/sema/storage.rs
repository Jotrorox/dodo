//! Explicit region provenance for owner-bound allocated storage. A zero-length
//! array witnesses the stored type without owning an active payload. Raw pointer
//! validity remains unsafe; dependency deposition and return contracts are checked.
use super::*;

#[derive(Clone, Default)]
pub(super) struct StorageLoop {
    roots: HashSet<usize>,
    pub(super) accesses: Vec<(Loan, Span)>,
    deposits: Vec<(Loan, Vec<Loan>)>,
}

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
    pub(super) fn begin_storage_loop(&self) {
        self.storage_loops.borrow_mut().push(StorageLoop {
            roots: self.scopes.iter().flatten().map(|v| v.id).collect(),
            ..StorageLoop::default()
        });
    }
    pub(super) fn end_storage_loop(&mut self, span: Span) -> Check<()> {
        let state = self.storage_loops.borrow_mut().pop().unwrap();
        for (target, incoming) in state.deposits {
            // Retain effects on break/continue branches that the ordinary
            // fallthrough join excludes. Also check the next iteration's prefix.
            for loan in &incoming {
                if state
                    .accesses
                    .iter()
                    .any(|(access, _)| overlaps(access, loan))
                {
                    return Err(Diagnostic::new(
                        span,
                        "stored borrow conflicts with source mutation across a loop back edge",
                    )
                    .label(
                        loan.origin,
                        "this dependency can survive into the next iteration",
                    ));
                }
            }
            self.retain_storage_dependencies(&target, &incoming);
        }
        Ok(())
    }
    fn retain_storage_dependencies(&mut self, target: &Loan, incoming: &[Loan]) {
        for variable in self.scopes.iter_mut().flatten() {
            if variable.id == target.root
                || variable
                    .deps
                    .iter()
                    .any(|d| !d.dependency && d.root == target.root)
            {
                variable.deps.extend(incoming.iter().cloned().map(|mut d| {
                    d.dependency = true;
                    d.stored = true;
                    d
                }));
            }
        }
    }

    // Update the original owner, not a temporary reborrow. An external root has
    // no local binding: every incoming source must be covered by this body's
    // declared effect, including effects forwarded through helpers.
    pub(super) fn deposit(&mut self, owner: &Value, incoming: &[Loan], span: Span) -> Check<()> {
        for target in owner.deps.iter().filter(|d| !d.dependency) {
            if let Some(external) = &target.external {
                for loan in incoming {
                    let allowed = loan.external.as_ref().is_some_and(|source| {
                        source == "static"
                            || source == &format!("{external}.stored")
                            || (self.stores.first() == Some(external)
                                && self.stores[1..]
                                    .iter()
                                    .any(|s| source == s || source == &format!("{s}.stored")))
                    });
                    if !allowed {
                        return Err(Diagnostic::new(span, "stored borrow source is outside the declared stores(target, source...) effect")
                            .label(loan.origin, "this source would escape into caller storage")
                            .note("forward parameter dependencies with stores; local storage cannot escape through a mutation effect"));
                    }
                }
            } else if self.by_id(target.root).is_none() && !incoming.is_empty() {
                return Err(Diagnostic::new(
                    span,
                    "cannot identify the owner of this stored borrow",
                ));
            }
            let mut retained = vec![];
            for loan in incoming {
                if loan.root == target.root && loan.external.is_none() {
                    return Err(Diagnostic::new(
                        span,
                        "a stored borrow cannot depend on its own container storage",
                    ));
                }
                let mut loan = loan.clone();
                loan.dependency = true;
                loan.stored = true;
                retained.push(loan);
            }
            // Reborrows made before this call must observe the new region union.
            // Updating only the owner or only its temporary alias loses facts.
            self.retain_storage_dependencies(target, &retained);
            for state in self.storage_loops.borrow_mut().iter_mut() {
                if state.roots.contains(&target.root) || target.external.is_some() {
                    state.deposits.push((target.clone(), retained.clone()));
                }
            }
        }
        Ok(())
    }

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
        let temporary_start = self.temporary.len();
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
        let incoming = if operation == "store" {
            let value = self.expr(&mut args[1], Some(&element), true)?;
            self.expect(&element, &value.ty, args[1].span)?;
            value.deps
        } else {
            vec![]
        };
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
        if operation == "store" {
            self.deposit(&owner, &incoming, span)?;
        }
        let mut deps = if view {
            owner.deps
        } else if operation == "take" && self.context.carries_borrow(&element) {
            owner.deps.into_iter().filter(|d| d.stored).collect()
        } else {
            vec![]
        };
        if view {
            for loan in &mut deps {
                loan.mutable = false;
            }
        }
        self.temporary.truncate(temporary_start);
        self.temporary.extend(deps.clone());
        Ok(Value {
            ty: match operation {
                "view" => Type::Ref(false, element),
                "view_slice" => Type::Slice(false, element),
                "take" => *element,
                _ => Type::Void,
            },
            deps,
        })
    }
}
