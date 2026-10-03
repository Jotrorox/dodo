//! Pattern tests borrow the scrutinee; selected arms transfer all bindings in a
//! single operation. Guard previews never consume payloads or the scrutinee.
use super::*;

const PLAN_LIMIT: usize = 4096;
#[derive(Clone, Default)]
struct Plan {
    tests: Vec<(Vec<PatternProjection>, PatternTest)>,
    bindings: Vec<Binding>,
}
struct Selection<'a> {
    source: Place,
    targets: &'a HashMap<String, Place>,
    mode: PatternMode,
    guard: Option<&'a Expr>,
    yes: BlockId,
    no: BlockId,
    span: Span,
}
#[derive(Clone)]
struct Binding {
    name: String,
    ty: Type,
    projections: Vec<PatternProjection>,
}

impl Lower<'_> {
    fn plans(
        &self,
        pattern: &Pattern,
        source: Place,
        span: Span,
    ) -> Result<(Vec<Plan>, PatternMode)> {
        let ty = &self.body.locals[source.0].ty;
        let (ty, borrowed, path) = if let Type::Ref(mutable, inner) = ty {
            (
                inner.as_ref(),
                Some(*mutable),
                vec![PatternProjection::Deref],
            )
        } else {
            (ty, None, vec![])
        };
        let plans = self.pattern_plans(pattern, ty, borrowed, &path, span)?;
        let mut previous = None;
        for plan in &plans {
            let mut names: Vec<_> = plan.bindings.iter().map(|b| (&b.name, &b.ty)).collect();
            names.sort_by_key(|(name, _)| *name);
            if names.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                return unsupported(span, "duplicate pattern bindings");
            }
            if previous.as_ref().is_some_and(|old| old != &names) {
                return unsupported(
                    span,
                    "alternative patterns require identical binding names and types",
                );
            }
            previous = Some(names);
        }
        Ok((
            plans,
            borrowed.map_or(PatternMode::Owned, PatternMode::Borrowed),
        ))
    }

    fn pattern_plans(
        &self,
        pattern: &Pattern,
        ty: &Type,
        borrowed: Option<bool>,
        path: &[PatternProjection],
        span: Span,
    ) -> Result<Vec<Plan>> {
        let mut plan = Plan::default();
        match pattern {
            Pattern::Wildcard => (),
            Pattern::Binding(name) => {
                if *ty == Type::Void {
                    return unsupported(span, "void pattern binding");
                }
                plan.bindings.push(Binding {
                    name: name.clone(),
                    ty: borrowed.map_or_else(|| ty.clone(), |m| Type::Ref(m, Box::new(ty.clone()))),
                    projections: path.to_vec(),
                });
            }
            Pattern::Bool(value) if *ty == Type::Bool => {
                plan.tests.push((path.to_vec(), PatternTest::Bool(*value)))
            }
            Pattern::Int(value) if ty.is_integer() => {
                self.pattern_integer(*value, ty, span)?;
                plan.tests.push((path.to_vec(), PatternTest::Int(*value)));
            }
            Pattern::Range(start, end, inclusive) if ty.is_integer() => {
                if self.pattern_integer(*start, ty, span)?
                    > self.pattern_integer(*end, ty, span)? - i128::from(!inclusive)
                {
                    return unsupported(span, "empty or reversed range pattern");
                }
                plan.tests
                    .push((path.to_vec(), PatternTest::Range(*start, *end, *inclusive)));
            }
            Pattern::Or(alternatives) => {
                if alternatives.is_empty() {
                    return unsupported(span, "empty pattern alternatives");
                }
                let mut plans = vec![];
                for alternative in alternatives {
                    plans.extend(self.pattern_plans(alternative, ty, borrowed, path, span)?);
                    if plans.len() > PLAN_LIMIT {
                        return unsupported(span, "pattern expansion limit exceeded");
                    }
                }
                return Ok(plans);
            }
            Pattern::Variant(name, payloads) => {
                let fields = self.variant_fields(ty, name, span)?;
                if fields.len() != payloads.len() {
                    return unsupported(span, "variant pattern arity mismatch");
                }
                let variant = name.rsplit('.').next().unwrap();
                plan.tests
                    .push((path.to_vec(), PatternTest::Variant(variant.into())));
                let mut plans = vec![plan];
                for (index, (pattern, ty)) in payloads.iter().zip(fields).enumerate() {
                    let mut path = path.to_vec();
                    path.push(PatternProjection::Payload {
                        variant: variant.into(),
                        index,
                    });
                    plans = combine(
                        plans,
                        self.pattern_plans(pattern, &ty, borrowed, &path, span)?,
                        span,
                    )?;
                }
                return Ok(plans);
            }
            Pattern::Struct(name, fields, rest) => {
                self.expect(&Type::Named(name.clone()), ty, span)?;
                let Some(structure) = self.program.structs.iter().find(|s| &s.name == name) else {
                    return unsupported(span, "unknown struct pattern");
                };
                validate_struct(self.program, structure)?;
                let mut seen = HashSet::new();
                let mut plans = vec![plan];
                for (name, pattern) in fields {
                    let Some(field) = structure.fields.iter().find(|f| &f.name == name) else {
                        return unsupported(span, "unknown struct pattern field");
                    };
                    if !seen.insert(name) {
                        return unsupported(span, "duplicate struct pattern field");
                    }
                    let mut path = path.to_vec();
                    path.push(PatternProjection::Field(name.clone()));
                    plans = combine(
                        plans,
                        self.pattern_plans(pattern, &field.ty, borrowed, &path, span)?,
                        span,
                    )?;
                }
                if !rest && seen.len() != structure.fields.len() {
                    return unsupported(span, "missing struct pattern field");
                }
                return Ok(plans);
            }
            _ => return unsupported(span, "pattern type mismatch"),
        }
        Ok(vec![plan])
    }

    fn pattern_integer(&self, value: u64, ty: &Type, span: Span) -> Result<i128> {
        let Type::Int { signed, bits } = ty else {
            unreachable!()
        };
        let bits = if *bits == 0 { self.pointer_bits } else { *bits };
        if !(1..=64).contains(&bits) {
            return unsupported(span, "unsupported integer pattern width");
        }
        let value = if *signed {
            i128::from(value as i64)
        } else {
            i128::from(value)
        };
        let (min, max) = if *signed {
            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
        } else {
            (0, (1i128 << bits) - 1)
        };
        if value < min || value > max {
            return unsupported(span, "integer pattern outside target range");
        }
        Ok(value)
    }

    pub(super) fn test_branch(
        &mut self,
        source: Place,
        projections: &[PatternProjection],
        test: PatternTest,
        yes: BlockId,
        no: BlockId,
        span: Span,
    ) {
        let target = self.local("$pattern_test", Type::Bool, span);
        self.emit(
            Operation::PatternTest {
                target,
                source,
                projections: projections.to_vec(),
                test,
            },
            span,
        );
        self.emit(Operation::Cleanup(target), span);
        self.scopes.last_mut().unwrap().locals.pop();
        // The test has read the subject. As with Boolean conditions, retain
        // both successors; this analysis does not track tag or scalar values.
        self.end(
            Terminator::Branch {
                condition: Operand::Constant(Type::Bool),
                yes,
                no,
            },
            span,
        );
    }

    fn test_plan(&mut self, source: Place, plan: &Plan, yes: BlockId, no: BlockId, span: Span) {
        for (index, (path, test)) in plan.tests.iter().enumerate() {
            let next = if index + 1 == plan.tests.len() {
                yes
            } else {
                self.block_id()
            };
            self.test_branch(source, path, test.clone(), next, no, span);
            if next != yes {
                self.current = Some(next);
            }
        }
        if plan.tests.is_empty() {
            self.end(Terminator::Goto(yes), span);
        }
    }

    fn bind_plan(
        &mut self,
        source: Place,
        plan: &Plan,
        targets: &HashMap<String, Place>,
        mode: PatternMode,
        span: Span,
    ) {
        let source = if mode == PatternMode::Owned {
            self.operand(source)
        } else {
            Operand::Copy(source)
        };
        self.emit(
            Operation::PatternBind {
                source,
                bindings: plan
                    .bindings
                    .iter()
                    .map(|b| PayloadBinding {
                        target: targets[&b.name],
                        projections: b.projections.clone(),
                    })
                    .collect(),
                mode,
            },
            span,
        );
    }

    fn binding_targets(
        &mut self,
        plan: &Plan,
        mutable: bool,
        span: Span,
    ) -> Result<HashMap<String, Place>> {
        plan.bindings
            .iter()
            .map(|b| {
                Ok((
                    b.name.clone(),
                    self.bind(&b.name, b.ty.clone(), mutable, span)?,
                ))
            })
            .collect()
    }

    // Alternatives enter one shared body with the same initialized bindings.
    // A false guard cleans up preview slots and retries the next alternative.
    fn select_plans(&mut self, plans: &[Plan], selection: Selection<'_>) -> Result<()> {
        let Selection {
            source,
            targets,
            mode,
            guard,
            yes,
            no,
            span,
        } = selection;
        for (index, plan) in plans.iter().enumerate() {
            let matched = self.block_id();
            let next = if index + 1 == plans.len() {
                no
            } else {
                self.block_id()
            };
            self.test_plan(source, plan, matched, next, span);
            self.current = Some(matched);
            if let Some(guard) = guard {
                self.bind_plan(source, plan, targets, PatternMode::Preview, span);
                let previous_guard = self.guard;
                self.guard = true;
                let condition = self.condition(guard);
                self.guard = previous_guard;
                let condition = condition?;
                let commit = self.block_id();
                let failed = self.block_id();
                self.end(
                    Terminator::Branch {
                        condition,
                        yes: commit,
                        no: failed,
                    },
                    guard.span,
                );
                self.current = Some(failed);
                self.cleanup(self.scopes.len() - 1, None, guard.span);
                self.end(Terminator::Goto(next), guard.span);
                self.current = Some(commit);
            }
            self.bind_plan(source, plan, targets, mode, span);
            self.emit(Operation::Cleanup(source), span);
            self.end(Terminator::Goto(yes), span);
            self.current = Some(next);
        }
        Ok(())
    }

    pub(super) fn match_statement(
        &mut self,
        value: &Expr,
        arms: &[ast::MatchArm],
        span: Span,
    ) -> Result<()> {
        let start = self.body.locals.len();
        let source = self.expr(value, None)?;
        self.finish_subject_temporaries(start, source, value.span);
        let join = self.block_id();
        let mut continues = false;
        for arm in arms {
            let (plans, mode) = self.plans(&arm.pattern, source, arm.span)?;
            self.scopes.push(Scope::default());
            let targets = self.binding_targets(&plans[0], true, arm.span)?;
            let selected = self.block_id();
            let next = self.block_id();
            self.select_plans(
                &plans,
                Selection {
                    source,
                    targets: &targets,
                    mode,
                    guard: arm.guard.as_ref(),
                    yes: selected,
                    no: next,
                    span: arm.span,
                },
            )?;
            self.current = Some(selected);
            self.statements(&arm.body)?;
            if self.current.is_some() {
                self.cleanup(self.scopes.len() - 1, None, arm.span);
                self.end(Terminator::Goto(join), arm.span);
                continues = true;
            }
            self.scopes.pop();
            self.current = Some(next);
        }
        // Exhaustiveness and Result obligations are independently checked by
        // sema. A failed exhaustive match has no continuation.
        self.end(Terminator::Unreachable, span);
        self.current = continues.then_some(join);
        Ok(())
    }

    pub(super) fn if_let(
        &mut self,
        pattern: &Pattern,
        value: &Expr,
        then_block: &ast::Block,
        else_block: &ast::Block,
        span: Span,
    ) -> Result<()> {
        let start = self.body.locals.len();
        let source = self.expr(value, None)?;
        self.finish_subject_temporaries(start, source, value.span);
        let (plans, mode) = self.plans(pattern, source, span)?;
        let yes = self.block_id();
        let no = self.block_id();
        let join = self.block_id();
        self.scopes.push(Scope::default());
        let targets = self.binding_targets(&plans[0], false, span)?;
        self.select_plans(
            &plans,
            Selection {
                source,
                targets: &targets,
                mode,
                guard: None,
                yes,
                no,
                span,
            },
        )?;
        self.current = Some(yes);
        self.statements(then_block)?;
        let first = self.current.is_some();
        if first {
            self.cleanup(self.scopes.len() - 1, None, span);
            self.end(Terminator::Goto(join), span);
        }
        self.scopes.pop();
        self.current = Some(no);
        self.emit(Operation::Cleanup(source), span);
        self.scoped(else_block, span)?;
        let second = self.current.is_some();
        if second {
            self.end(Terminator::Goto(join), span);
        }
        self.current = (first || second).then_some(join);
        Ok(())
    }

    pub(super) fn let_pattern(
        &mut self,
        pattern: &Pattern,
        ty: &Type,
        value: &Expr,
        else_block: Option<&ast::Block>,
        span: Span,
    ) -> Result<()> {
        let start = self.body.locals.len();
        let source = self.expr(value, (*ty != Type::Unknown).then_some(ty))?;
        self.finish_subject_temporaries(start, source, value.span);
        let (plans, mode) = self.plans(pattern, source, span)?;
        let yes = self.block_id();
        let no = self.block_id();
        // Keep successful names out of the failure block's lexical scope.
        self.scopes.push(Scope::default());
        let targets = self.binding_targets(&plans[0], false, span)?;
        self.select_plans(
            &plans,
            Selection {
                source,
                targets: &targets,
                mode,
                guard: None,
                yes,
                no,
                span,
            },
        )?;
        let bindings = self.scopes.pop().unwrap();
        self.current = Some(no);
        self.emit(Operation::Cleanup(source), span);
        if let Some(block) = else_block {
            self.scoped(block, span)?;
        }
        if self.current.is_some() {
            self.end(Terminator::Unreachable, span);
        }
        let scope = self.scopes.last_mut().unwrap();
        for (name, binding) in bindings.names {
            if scope.names.insert(name, binding).is_some() {
                return unsupported(span, "duplicate pattern binding");
            }
        }
        scope.locals.extend(bindings.locals);
        self.current = Some(yes);
        Ok(())
    }

    fn finish_subject_temporaries(&mut self, start: usize, source: Place, span: Span) {
        // Preserve the matched temporary while disposing of the expressions
        // that produced it. Each selected or failed conditional path kills it.
        for scope in &mut self.scopes {
            scope.locals.retain(|p| *p != source);
        }
        self.finish_temporaries(start, span);
        self.scopes.last_mut().unwrap().locals.push(source);
    }
}

fn combine(left: Vec<Plan>, right: Vec<Plan>, span: Span) -> Result<Vec<Plan>> {
    if left.len().saturating_mul(right.len()) > PLAN_LIMIT {
        return unsupported(span, "pattern expansion limit exceeded");
    }
    Ok(left
        .into_iter()
        .flat_map(|left| {
            right.iter().map(move |right| {
                let mut plan = left.clone();
                plan.tests.extend(right.tests.clone());
                plan.bindings.extend(right.bindings.clone());
                plan
            })
        })
        .collect())
}
