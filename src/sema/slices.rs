//! A deliberately narrow partition rule for the core slice pair. This does
//! not introduce independent borrowed-field lifetimes for ordinary structs.
use super::*;

impl Context {
    pub(super) fn split_mut_element(&self, ty: &Type) -> Option<Type> {
        let Type::Named(name) = ty else { return None };
        if !name.starts_with("slice.SplitMut$") {
            return None;
        }
        let declaration = self.structs.get(name)?;
        let [left, right] = declaration.fields.as_slice() else {
            return None;
        };
        if left.name != "left" || right.name != "right" || left.ty != right.ty {
            return None;
        }
        match &left.ty {
            Type::Slice(true, element) => Some(*element.clone()),
            _ => None,
        }
    }
}

impl Checker<'_> {
    pub(super) fn check_split_field_assignment(&self, target: &Expr) -> Check<()> {
        if let ExprKind::Field(base, _) = &target.kind
            && self
                .peek_type(base)
                .is_some_and(|ty| self.context.split_mut_element(dereferenced(&ty)).is_some())
        {
            return Err(Diagnostic::new(
                target.span,
                "SplitMut slice fields cannot be replaced; consume the pair or replace it as a whole",
            ));
        }
        Ok(())
    }

    pub(super) fn split_at_mut(
        &mut self,
        types: &[Type],
        args: &mut [Expr],
        span: Span,
    ) -> Check<Value> {
        if types.len() != 1 || args.len() != 2 {
            return Err(Diagnostic::new(
                span,
                "mem.split_at_mut expects a SplitMut type argument and two arguments",
            ));
        }
        let element = self
            .context
            .split_mut_element(&types[0])
            .ok_or_else(|| Diagnostic::new(span, "mem.split_at_mut requires slice.SplitMut<T>"))?;
        let source = Type::Slice(true, Box::new(element));
        let start = self.temporary.len();
        let value = self.expr(&mut args[0], Some(&source), false)?;
        self.expect(&source, &value.ty, args[0].span)?;
        self.reserve_argument_borrow(&value, true, start, args[0].span)?;
        let mid = self.expr(&mut args[1], Some(&Type::usize()), false)?;
        self.expect(&Type::usize(), &mid.ty, args[1].span)?;
        self.temporary.truncate(start);
        self.temporary.extend(value.deps.clone());
        Ok(Value {
            ty: types[0].clone(),
            deps: value.deps,
        })
    }

    // Only an owned destructure opens a pair. Merely borrowing its fields
    // retains the usual aggregate dependencies. Requiring exclusive access to
    // every source here also prevents reopening a pair with outstanding views.
    pub(super) fn bind_split_pattern(
        &mut self,
        pattern: &Pattern,
        bindings: &[(String, Type)],
        value: &Value,
        immutable: bool,
        span: Span,
    ) -> Check<bool> {
        let mut sides = HashMap::new();
        self.split_pattern_sides(pattern, &value.ty, &mut sides);
        if sides.is_empty() {
            return Ok(false);
        }
        for loan in &value.deps {
            self.conflict(loan, Access::Borrow(true), span)?;
        }
        let partition = self.next_partition;
        self.next_partition += 1;
        for (name, ty) in bindings {
            let mut deps = value.deps.clone();
            if let Some(side) = sides.get(name) {
                for loan in &mut deps {
                    // Allocators, policies, and stored references keep their
                    // full dependencies, including their original exclusivity.
                    if !loan.dependency {
                        loan.partitions.push((partition, *side));
                    }
                }
            }
            self.bind(name.clone(), ty.clone(), Some(deps), immutable, span)?;
        }
        Ok(true)
    }

    fn split_pattern_sides(&self, pattern: &Pattern, ty: &Type, sides: &mut HashMap<String, bool>) {
        match (pattern, ty) {
            (Pattern::Struct(_, fields, _), _) if self.context.split_mut_element(ty).is_some() => {
                for (field, pattern) in fields {
                    for name in pattern.bindings() {
                        sides.insert(name, field == "right");
                    }
                }
            }
            (Pattern::Variant(_, fields), Type::Option(element)) if fields.len() == 1 => {
                self.split_pattern_sides(&fields[0], element, sides);
            }
            _ => (),
        }
    }

    // A static partition identity must never prove disjointness between two
    // dynamic executions of the same split. Loops may use splits locally, but
    // cannot store a newly opened view in a binding from an earlier iteration.
    pub(super) fn check_split_loop_escape(
        &self,
        first: usize,
        depth: usize,
        span: Span,
    ) -> Check<()> {
        for variable in self
            .scopes
            .iter()
            .take(depth)
            .flatten()
            .filter(|v| v.initialized)
        {
            if let Some(loan) = variable
                .deps
                .iter()
                .find(|loan| loan.partitions.iter().any(|(id, _)| *id >= first))
            {
                return Err(
                    Diagnostic::new(span, "split view cannot escape its loop iteration")
                        .label(loan.origin, "source borrow begins here")
                        .label(
                            variable.span,
                            format!("view stored in outer binding `{}`", variable.name),
                        ),
                );
            }
        }
        Ok(())
    }
}
