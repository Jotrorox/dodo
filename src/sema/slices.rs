//! The core slice pair. Opening it partitions its loans (see `flow`); this
//! does not introduce independent borrowed-field lifetimes for ordinary structs.
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
        let value = self.expr(&mut args[0], Some(&source), false)?;
        self.expect(&source, &value.ty, args[0].span)?;
        let mid = self.expr(&mut args[1], Some(&Type::usize()), false)?;
        self.expect(&Type::usize(), &mid.ty, args[1].span)?;
        Ok(Value {
            ty: types[0].clone(),
        })
    }
}
