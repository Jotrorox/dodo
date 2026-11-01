//! Lowering of a checked body. The AST checker has already resolved every
//! name and type, desugared method calls and range loops, inserted coercion
//! casts, and lifted closures, so every construct has a lowering here.
//!
//! Evaluation order follows the backend: operands left to right, assignment
//! destinations before their right-hand side, and both successors of every
//! branch kept possible.
use super::*;
use crate::ast::{BinaryOp, Block, Expr, ExprKind, Pattern, Stmt, StmtKind, UnaryOp};
use crate::sema::{Context, dereferenced, mutable_borrow, shared_access_route};
use std::collections::HashMap;

pub(in crate::sema) fn lower(context: &Context, function: &Function) -> Body {
    let mut lower = Lower {
        context,
        body: Body {
            name: function.name.clone(),
            locals: vec![],
            blocks: vec![],
            scopes: vec![Scope::default()],
            loops: vec![],
            from: function.from.clone(),
            stores: function.stores.clone(),
            return_contract: function.from_span.unwrap_or(function.ret_span),
            inferred_contract: function.from_span.is_none(),
        },
        current: None,
        scopes: vec![(0, HashMap::new())],
        loops: vec![],
        yields: vec![],
        guard: false,
        note: None,
        partitions: 0,
        reached: vec![],
        order: 0,
    };
    lower.current = Some(lower.new_block());
    let captures = function.environment.unwrap_or(0);
    for (index, parameter) in function.params.iter().enumerate() {
        lower.declare(
            &parameter.name,
            parameter.ty.clone(),
            LocalKind::Parameter {
                capture: index < captures,
            },
            parameter.span,
        );
    }
    if let Some(body) = &function.body {
        lower.statements(body);
    }
    if lower.current.is_some() {
        lower.leave_scopes(0, function.span);
        lower.end(Terminator::Return(None), function.span);
    }
    lower.body
}

struct LoopTarget {
    continue_target: BlockId,
    exit: BlockId,
    depth: usize,
}

struct YieldTarget {
    exit: BlockId,
    result: Place,
    depth: usize,
}

struct Lower<'a> {
    context: &'a Context,
    body: Body,
    /// None after a terminator: following code is unreachable.
    current: Option<BlockId>,
    /// Open scopes with their bindings by name.
    scopes: Vec<(usize, HashMap<String, Place>)>,
    loops: Vec<LoopTarget>,
    yields: Vec<YieldTarget>,
    guard: bool,
    note: Option<String>,
    partitions: usize,
    /// Blocks some reachable terminator jumps to.
    reached: Vec<bool>,
    order: usize,
}

/// How an intrinsic argument is evaluated.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Read,
    Move,
    /// Reborrowed for the duration of the call.
    Reference,
}

impl Lower<'_> {
    // ---- graph construction ---------------------------------------------

    fn new_block(&mut self) -> BlockId {
        self.reached.push(false);
        self.body.blocks.push(BasicBlock {
            instructions: vec![],
            terminator: Terminator::Unreachable,
            span: Span::default(),
        });
        self.body.blocks.len() - 1
    }

    fn emit(&mut self, operation: Operation, span: Span) {
        if let Some(current) = self.current {
            self.order += 1;
            self.body.blocks[current].instructions.push(Instruction {
                operation,
                span,
                order: self.order,
                guard: self.guard,
                note: self.note.clone(),
            });
        }
    }

    fn end(&mut self, terminator: Terminator, span: Span) {
        if let Some(current) = self.current.take() {
            match &terminator {
                Terminator::Goto(target) => self.reached[*target] = true,
                Terminator::Branch { targets, .. } => {
                    for target in targets {
                        self.reached[*target] = true;
                    }
                }
                _ => (),
            }
            let block = &mut self.body.blocks[current];
            block.terminator = terminator;
            block.span = span;
        }
    }

    fn goto(&mut self, target: BlockId, span: Span) {
        self.end(Terminator::Goto(target), span);
    }

    fn branch(&mut self, condition: Option<Place>, targets: Vec<BlockId>, span: Span) {
        self.end(Terminator::Branch { condition, targets }, span);
    }

    fn scope(&self) -> usize {
        self.scopes.last().unwrap().0
    }

    fn local(&mut self, name: &str, ty: Type, kind: LocalKind, span: Span) -> Place {
        let place = Place(self.body.locals.len());
        self.body.locals.push(Local {
            name: name.into(),
            ty,
            span,
            kind,
            scope: self.scope(),
        });
        place
    }

    fn temporary(&mut self, ty: Type, span: Span) -> Place {
        self.local("$temporary", ty, LocalKind::Temporary, span)
    }

    fn declare(&mut self, name: &str, ty: Type, kind: LocalKind, span: Span) -> Place {
        let place = self.local(name, ty, kind, span);
        let scope = self.scope();
        self.body.scopes[scope].locals.push(place);
        self.scopes.last_mut().unwrap().1.insert(name.into(), place);
        place
    }

    fn binding(&mut self, name: &str, ty: Type, immutable: bool, span: Span) -> Place {
        self.declare(name, ty, LocalKind::Binding { immutable }, span)
    }

    fn lookup(&self, name: &str) -> Option<Place> {
        self.scopes
            .iter()
            .rev()
            .find_map(|(_, names)| names.get(name).copied())
    }

    fn push_scope(&mut self) {
        let parent = self.scope();
        self.body.scopes.push(Scope {
            parent: Some(parent),
            locals: vec![],
        });
        self.scopes
            .push((self.body.scopes.len() - 1, HashMap::new()));
    }

    /// Check and clean up the innermost scope, then close it.
    fn pop_scope(&mut self, span: Span) {
        self.leave_scopes(self.scopes.len() - 1, span);
        self.scopes.pop();
    }

    /// Check and clean up the open scopes from stack index `from` inward
    /// (innermost first) without closing them: break, continue, and yields.
    fn leave_scopes(&mut self, from: usize, span: Span) {
        for index in (from..self.scopes.len()).rev() {
            let scope = self.scopes[index].0;
            self.emit(Operation::ScopeEnd { scope }, span);
            self.cleanup(scope, span);
        }
    }

    fn cleanup(&mut self, scope: usize, span: Span) {
        let locals = self.body.scopes[scope].locals.clone();
        for place in locals.into_iter().rev() {
            self.emit(Operation::Cleanup(place), span);
        }
    }

    /// Return or propagate an error: check, then destroy every scope.
    fn exit(&mut self, returned: Option<Place>, checked: Option<Place>, span: Span) {
        let scope = self.scope();
        self.emit(
            Operation::Exit {
                scope,
                returned: checked,
            },
            span,
        );
        for index in (0..self.scopes.len()).rev() {
            let scope = self.scopes[index].0;
            self.cleanup(scope, span);
        }
        self.end(Terminator::Return(returned), span);
    }

    fn carries(&self, ty: &Type) -> bool {
        self.context.carries_borrow(ty)
    }

    // ---- statements -----------------------------------------------------

    fn statements(&mut self, block: &Block) {
        for statement in block {
            if self.current.is_none() {
                return;
            }
            self.statement(statement);
        }
    }

    fn scoped(&mut self, block: &Block, span: Span) {
        self.push_scope();
        self.statements(block);
        let end = block.last().map_or(span, |statement| Span {
            start: statement.span.start,
            end: statement.span.start,
        });
        self.pop_scope(end);
    }

    fn statement(&mut self, statement: &Stmt) {
        let span = statement.span;
        match &statement.kind {
            StmtKind::Let {
                name,
                ty,
                value,
                mutable,
                ..
            } => {
                let value = value.as_ref().map(|value| self.expr(value, true));
                if self.current.is_none() {
                    return;
                }
                let binding = self.binding(name, ty.clone(), !*mutable, span);
                if let Some(value) = value {
                    self.emit(
                        Operation::Assign {
                            target: binding,
                            value,
                        },
                        span,
                    );
                }
            }
            StmtKind::Assign { target, op, value } => {
                self.assign(target, op.is_some(), value, span)
            }
            StmtKind::Expr(expression) => {
                let value = self.expr(expression, true);
                self.emit(Operation::Cleanup(value), span);
            }
            StmtKind::Yield(expression) => {
                let value = self.expr(expression, true);
                let Some(target) = self.yields.last() else {
                    return;
                };
                let (exit, result, depth) = (target.exit, target.result, target.depth);
                let to = self.scopes[depth - 1].0;
                let from = self.scope();
                self.emit(Operation::Yield { value, from, to }, expression.span);
                self.leave_scopes(depth, span);
                self.emit(
                    Operation::Combine {
                        target: result,
                        sources: vec![value],
                    },
                    span,
                );
                self.goto(exit, span);
            }
            StmtKind::Return(expression) => {
                let value = expression.as_ref().map(|e| (self.expr(e, true), e.span));
                if self.current.is_none() {
                    return;
                }
                let (returned, at) = match value {
                    Some((place, at)) => (Some(place), at),
                    None => (None, span),
                };
                let checked = returned.filter(|place| self.carries(&self.body.locals[place.0].ty));
                self.exit(returned, checked, at);
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let condition = self.expr(condition, false);
                let then_entry = self.new_block();
                let else_entry = self.new_block();
                let join = self.new_block();
                self.branch(Some(condition), vec![then_entry, else_entry], span);
                self.current = Some(then_entry);
                self.scoped(then_block, span);
                self.goto(join, span);
                self.current = Some(else_entry);
                self.scoped(else_block, span);
                self.goto(join, span);
                self.current = Some(join);
                self.unreachable_unless_reached(join);
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => self.for_loop(
                init.as_deref(),
                condition.as_ref(),
                step.as_deref(),
                body,
                span,
            ),
            StmtKind::ForEach {
                index,
                name,
                copy,
                iterable,
                body,
            } => self.foreach(index.as_deref(), name, *copy, iterable, body, span),
            StmtKind::IfLet {
                pattern,
                value,
                then_block,
                else_block,
            } => {
                let subject = self.subject(value);
                let then_entry = self.new_block();
                let else_entry = self.new_block();
                let join = self.new_block();
                self.branch(None, vec![then_entry, else_entry], span);
                self.current = Some(then_entry);
                self.push_scope();
                self.bind_pattern(pattern, subject, &value.ty, true, span);
                self.statements(then_block);
                self.pop_scope(span);
                self.goto(join, span);
                self.current = Some(else_entry);
                self.scoped(else_block, span);
                self.goto(join, span);
                self.current = Some(join);
                self.unreachable_unless_reached(join);
            }
            StmtKind::LetPattern {
                pattern,
                value,
                else_block,
                ..
            } => {
                let subject = self.subject(value);
                if let Some(else_block) = else_block {
                    let else_entry = self.new_block();
                    let next = self.new_block();
                    self.branch(None, vec![else_entry, next], span);
                    self.current = Some(else_entry);
                    self.scoped(else_block, span);
                    self.end(Terminator::Unreachable, span);
                    self.current = Some(next);
                }
                self.bind_pattern(pattern, subject, &value.ty, true, span);
            }
            StmtKind::Match { value, arms } => {
                let subject = self.subject(value);
                let join = self.new_block();
                for arm in arms {
                    let arm_entry = self.new_block();
                    let next = self.new_block();
                    self.branch(None, vec![arm_entry, next], arm.span);
                    self.current = Some(arm_entry);
                    self.push_scope();
                    self.bind_pattern(&arm.pattern, subject, &value.ty, false, arm.span);
                    if let Some(guard) = &arm.guard {
                        let previous = std::mem::replace(&mut self.guard, true);
                        let condition = self.expr(guard, false);
                        self.guard = previous;
                        let body = self.new_block();
                        let rejected = self.new_block();
                        self.branch(Some(condition), vec![body, rejected], guard.span);
                        // A false guard releases the arm's previews and tries
                        // the next arm.
                        self.current = Some(rejected);
                        let scope = self.scope();
                        self.cleanup(scope, arm.span);
                        self.goto(next, arm.span);
                        self.current = Some(body);
                    }
                    self.statements(&arm.body);
                    self.pop_scope(arm.span);
                    self.goto(join, arm.span);
                    self.current = Some(next);
                }
                self.end(Terminator::Unreachable, span);
                self.current = Some(join);
                self.unreachable_unless_reached(join);
            }
            StmtKind::Break | StmtKind::Continue => {
                let Some(target) = self.loops.last() else {
                    return;
                };
                let (depth, destination) = (
                    target.depth,
                    if matches!(statement.kind, StmtKind::Break) {
                        target.exit
                    } else {
                        target.continue_target
                    },
                );
                self.leave_scopes(depth, span);
                self.goto(destination, span);
            }
            StmtKind::Block(block) | StmtKind::Unsafe(block) => self.scoped(block, span),
        }
    }

    /// A join block nobody jumps to stays unreachable.
    fn unreachable_unless_reached(&mut self, join: BlockId) {
        if !self.reached[join] {
            self.current = None;
        }
    }

    fn assign(&mut self, target: &Expr, compound: bool, value: &Expr, span: Span) {
        if matches!(&target.kind, ExprKind::Name(name) if name == "_") {
            let value = self.expr(value, true);
            self.emit(Operation::Cleanup(value), span);
            return;
        }
        if let ExprKind::Name(name) = &target.kind
            && let Some(binding) = self.lookup(name)
        {
            self.emit(
                Operation::Write {
                    path: Path::local(binding, target.span),
                    read: compound,
                },
                target.span,
            );
            let value = self.expr(value, true);
            if self.current.is_none() {
                return;
            }
            let value = if compound {
                let scalar = self.temporary(self.body.locals[binding.0].ty.clone(), span);
                self.emit(Operation::Constant { target: scalar }, span);
                scalar
            } else {
                value
            };
            self.emit(
                Operation::Assign {
                    target: binding,
                    value,
                },
                span,
            );
            return;
        }
        let path = self.path(target);
        self.emit(
            Operation::Write {
                path: path.clone(),
                read: true,
            },
            target.span,
        );
        let reservation =
            self.temporary(Type::Ref(false, Box::new(target.ty.clone())), target.span);
        self.emit(
            Operation::Reservation {
                target: reservation,
                path: path.clone(),
            },
            target.span,
        );
        let value = self.expr(value, true);
        self.emit(
            Operation::Store {
                path,
                value,
                reservation,
            },
            span,
        );
    }

    fn for_loop(
        &mut self,
        init: Option<&Stmt>,
        condition: Option<&Expr>,
        step: Option<&Stmt>,
        body: &Block,
        span: Span,
    ) {
        self.body.loops.push(span);
        self.push_scope();
        let loop_scope = self.scope();
        if let Some(init) = init {
            self.statement(init);
        }
        let header = self.new_block();
        let entry = self.new_block();
        let step_block = self.new_block();
        let exit = self.new_block();
        self.goto(header, span);
        self.current = Some(header);
        match condition {
            Some(condition) => {
                let value = self.expr(condition, false);
                self.branch(Some(value), vec![entry, exit], condition.span);
            }
            None => self.goto(entry, span),
        }
        let first_partition = self.partitions;
        self.loops.push(LoopTarget {
            continue_target: step_block,
            exit,
            depth: self.scopes.len(),
        });
        self.current = Some(entry);
        self.scoped(body, span);
        self.goto(step_block, span);
        // As in the backend, `continue` inside the step repeats the step.
        self.current = Some(step_block);
        if let Some(step) = step {
            self.statement(step);
        }
        self.goto(header, span);
        self.loops.pop();
        self.current = Some(exit);
        self.unreachable_unless_reached(exit);
        let partitions: Vec<usize> = (first_partition..self.partitions).collect();
        if !partitions.is_empty() {
            self.emit(
                Operation::SplitEscape {
                    partitions,
                    scope: loop_scope,
                },
                span,
            );
        }
        self.pop_scope(span);
    }

    fn foreach(
        &mut self,
        index: Option<&str>,
        name: &str,
        copy: bool,
        iterable: &Expr,
        body: &Block,
        span: Span,
    ) {
        self.body.loops.push(span);
        // The collection is evaluated and borrowed once, before the header.
        let collection = match &iterable.kind {
            ExprKind::Unary(UnaryOp::Borrow | UnaryOp::BorrowMut, _) => self.expr(iterable, false),
            _ => {
                let path = self.path(iterable);
                let mutable = matches!(self.place_type(iterable), Type::Slice(true, _));
                let element = match dereferenced(&iterable.ty) {
                    Type::Array(_, t) | Type::Slice(_, t) => t.as_ref().clone(),
                    other => other.clone(),
                };
                let target = self.temporary(Type::Slice(mutable, Box::new(element)), iterable.span);
                self.emit(
                    Operation::Borrow {
                        target,
                        path,
                        mutable,
                        carried: false,
                    },
                    iterable.span,
                );
                target
            }
        };
        if self.current.is_none() {
            return;
        }
        let (mutable, element) = match &self.body.locals[collection.0].ty {
            Type::Slice(m, t) => (*m, t.as_ref().clone()),
            Type::Ref(m, t) => match t.as_ref() {
                Type::Array(_, e) => (*m, e.as_ref().clone()),
                other => (*m, other.clone()),
            },
            other => (false, other.clone()),
        };
        let outer = self.scope();
        self.push_scope();
        let collection_scope = self.scope();
        let held = self.binding(
            &format!("$foreach.collection.{}", span.start),
            self.body.locals[collection.0].ty.clone(),
            false,
            span,
        );
        self.emit(
            Operation::Assign {
                target: held,
                value: collection,
            },
            iterable.span,
        );
        if copy {
            self.emit(Operation::Demote { place: held }, iterable.span);
        }
        let header = self.new_block();
        let entry = self.new_block();
        let exit = self.new_block();
        self.goto(header, span);
        self.current = Some(header);
        self.branch(None, vec![entry, exit], iterable.span);
        let first_partition = self.partitions;
        self.loops.push(LoopTarget {
            continue_target: header,
            exit,
            depth: self.scopes.len(),
        });
        self.current = Some(entry);
        self.push_scope();
        if let Some(index) = index {
            let binding = self.binding(index, Type::usize(), false, span);
            let value = self.temporary(Type::usize(), span);
            self.emit(Operation::Constant { target: value }, iterable.span);
            self.emit(
                Operation::Assign {
                    target: binding,
                    value,
                },
                iterable.span,
            );
        }
        // The held collection stays borrowed across every iteration.
        let cursor = self.temporary(self.body.locals[held.0].ty.clone(), iterable.span);
        self.emit(
            Operation::Alias {
                target: cursor,
                source: held,
            },
            iterable.span,
        );
        let mut iteration = None;
        if copy {
            if name != "_" {
                // Copied elements carry the references stored in the source.
                let value = self.temporary(element.clone(), iterable.span);
                let source = match &iterable.kind {
                    ExprKind::Unary(UnaryOp::Borrow, source)
                        if matches!(source.ty, Type::Array(..)) =>
                    {
                        source.as_ref()
                    }
                    _ => iterable,
                };
                if self.carries(&element) {
                    // A collection place's elements carry its provenance;
                    // other collection values carry their own loans.
                    let place = matches!(
                        source.kind,
                        ExprKind::Name(_)
                            | ExprKind::Field(..)
                            | ExprKind::Index(..)
                            | ExprKind::Unary(UnaryOp::Deref, _)
                    );
                    let carrier = if place {
                        let path = self.path(source);
                        let carrier = self.temporary(source.ty.clone(), iterable.span);
                        self.emit(
                            Operation::Read {
                                target: carrier,
                                path,
                                consume: false,
                            },
                            iterable.span,
                        );
                        carrier
                    } else {
                        let carrier = self.temporary(element.clone(), iterable.span);
                        self.emit(
                            Operation::Alias {
                                target: carrier,
                                source: cursor,
                            },
                            iterable.span,
                        );
                        carrier
                    };
                    self.emit(Operation::Transitive { place: carrier }, iterable.span);
                    self.emit(
                        Operation::Combine {
                            target: value,
                            sources: vec![carrier],
                        },
                        iterable.span,
                    );
                } else {
                    self.emit(Operation::Constant { target: value }, iterable.span);
                }
                let binding = self.binding(name, element.clone(), false, span);
                self.emit(
                    Operation::Assign {
                        target: binding,
                        value,
                    },
                    iterable.span,
                );
            }
        } else {
            let ty = Type::Ref(mutable, Box::new(element.clone()));
            let value = self.temporary(ty.clone(), iterable.span);
            self.emit(
                Operation::Combine {
                    target: value,
                    sources: vec![cursor],
                },
                iterable.span,
            );
            let binding = self.binding(name, ty, false, span);
            self.emit(
                Operation::Assign {
                    target: binding,
                    value,
                },
                iterable.span,
            );
            iteration = Some(binding);
        }
        self.statements(body);
        self.pop_scope(span);
        self.goto(header, span);
        self.loops.pop();
        self.current = Some(exit);
        if let Some(binding) = iteration.filter(|_| mutable) {
            self.emit(
                Operation::IterationEscape {
                    binding,
                    scope: outer,
                },
                span,
            );
        }
        let partitions: Vec<usize> = (first_partition..self.partitions).collect();
        if !partitions.is_empty() {
            self.emit(
                Operation::SplitEscape {
                    partitions,
                    scope: outer,
                },
                span,
            );
        }
        debug_assert_eq!(self.scope(), collection_scope);
        self.pop_scope(span);
    }

    // ---- patterns -------------------------------------------------------

    /// Evaluate a matched value; matching a Result (or a whole borrow of a
    /// Result binding) handles its obligation.
    fn subject(&mut self, value: &Expr) -> Place {
        let subject = self.expr(value, true);
        if self.context.contains_result(dereferenced(&value.ty)) {
            let owner = match &value.kind {
                ExprKind::Unary(UnaryOp::Borrow | UnaryOp::BorrowMut, owner) => owner.as_ref(),
                _ => value,
            };
            if let ExprKind::Name(name) = &owner.kind
                && let Some(binding) = self.lookup(name)
            {
                self.emit(Operation::Handled(binding), value.span);
            }
        }
        subject
    }

    fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        subject: Place,
        ty: &Type,
        immutable: bool,
        span: Span,
    ) {
        if self.current.is_none() {
            return;
        }
        let (matched, borrowed) = match ty {
            Type::Ref(m, inner) => (inner.as_ref(), Some(*m)),
            _ => (ty, None),
        };
        let names = patterns::bindings(self.context, pattern, matched, borrowed);
        let mut sides = HashMap::new();
        patterns::split_sides(self.context, pattern, ty, &mut sides);
        if !sides.is_empty() {
            let partition = self.partitions;
            self.partitions += 1;
            let mut bindings = vec![];
            for (name, ty) in names {
                let binding = self.binding(&name, ty, immutable, span);
                bindings.push((binding, sides.get(&name).copied()));
            }
            self.emit(
                Operation::Split {
                    source: subject,
                    bindings,
                    partition,
                },
                span,
            );
            return;
        }
        let mut paths = HashMap::new();
        // Borrow-carrying aggregates keep their complete dependencies: their
        // stored references' sources do not share the aggregate's paths.
        if matches!(ty, Type::Ref(..)) && !self.carries(dereferenced(ty)) {
            patterns::binding_paths(pattern, &[], &mut paths);
        }
        for (name, ty) in names {
            let binding = self.binding(&name, ty, immutable, span);
            self.emit(
                Operation::Project {
                    target: binding,
                    source: subject,
                    paths: paths.remove(&name).unwrap_or_default(),
                },
                span,
            );
        }
    }

    // ---- places ---------------------------------------------------------

    /// The type the checker gives a place before any coercion of its value.
    fn place_type(&self, expression: &Expr) -> Type {
        let ty = match &expression.kind {
            ExprKind::Name(name) => match self.lookup(name) {
                Some(place) => return self.body.locals[place.0].ty.clone(),
                None => self
                    .context
                    .constants
                    .get(name)
                    .map_or_else(|| expression.ty.clone(), |c| c.ty.clone()),
            },
            ExprKind::Field(base, field) => match dereferenced(&base.ty) {
                Type::Named(name) => self
                    .context
                    .structs
                    .get(name)
                    .and_then(|s| s.fields.iter().find(|f| f.name == *field))
                    .map_or_else(|| expression.ty.clone(), |f| f.ty.clone()),
                Type::Array(..) | Type::Slice(..) | Type::Str if field == "len" => Type::usize(),
                _ => expression.ty.clone(),
            },
            ExprKind::Index(base, _) => match dereferenced(&base.ty) {
                Type::Array(_, t) | Type::Slice(_, t) => t.as_ref().clone(),
                _ => expression.ty.clone(),
            },
            ExprKind::Unary(UnaryOp::Deref, base) => match &base.ty {
                Type::Ref(_, t) | Type::Raw(_, t) => t.as_ref().clone(),
                _ => expression.ty.clone(),
            },
            _ => return expression.ty.clone(),
        };
        if shared_access_route(expression) {
            match ty {
                Type::Ref(true, t) => Type::Ref(false, t),
                Type::Slice(true, t) => Type::Slice(false, t),
                other => other,
            }
        } else {
            ty
        }
    }

    /// Lower a place expression. Sub-expressions (dereferenced values and
    /// indices) are evaluated left to right; the access itself is not.
    fn path(&mut self, expression: &Expr) -> Path {
        let span = expression.span;
        match &expression.kind {
            ExprKind::Name(name) => match self.lookup(name) {
                Some(place) => Path::local(place, span),
                None => Path {
                    root: Root::Global(name.clone()),
                    root_span: span,
                    projections: vec![],
                },
            },
            ExprKind::Unary(UnaryOp::Deref, operand) => {
                let reference = self.expr(operand, false);
                Path {
                    root: Root::Local(reference),
                    root_span: operand.span,
                    projections: vec![Projection::Deref],
                }
            }
            ExprKind::Field(base, field) => {
                let mut path = self.path(base);
                path.projections.push(Projection::Field {
                    name: field.clone(),
                    through_ref: matches!(base.ty, Type::Ref(..)),
                });
                path
            }
            ExprKind::Index(base, index) => {
                let mut path = self.path(base);
                if path.projections.is_empty()
                    && let Root::Local(root) = path.root
                    && self.body.locals[root.0].kind != LocalKind::Temporary
                {
                    self.emit(Operation::Require(root), path.root_span);
                }
                let view = matches!(base.ty, Type::Ref(..) | Type::Slice(..));
                let index = self.expr(index, false);
                path.projections.push(Projection::Index { index, view });
                path
            }
            _ => {
                let value = self.expr(expression, false);
                Path::local(value, span)
            }
        }
    }

    // ---- expressions ----------------------------------------------------

    /// Lower an expression to a temporary holding its value. `consume`
    /// moves a non-copy place read as a whole value.
    fn expr(&mut self, expression: &Expr, consume: bool) -> Place {
        let (place, natural) = self.value(expression, consume);
        // Reborrows may weaken exclusive access to shared access.
        let weakened = matches!(
            (&natural, &expression.ty),
            (Type::Ref(true, _), Type::Ref(false, _))
                | (Type::Slice(true, _), Type::Slice(false, _))
        ) && !matches!(&natural, Type::Ref(_, t) if matches!(t.as_ref(), Type::DynFn(..)));
        if weakened {
            self.emit(Operation::Demote { place }, expression.span);
        }
        place
    }

    fn constant(&mut self, ty: &Type, span: Span) -> Place {
        let target = self.temporary(ty.clone(), span);
        self.emit(Operation::Constant { target }, span);
        target
    }

    fn combine(&mut self, ty: &Type, sources: Vec<Place>, span: Span) -> Place {
        let target = self.temporary(ty.clone(), span);
        self.emit(Operation::Combine { target, sources }, span);
        target
    }

    fn read(&mut self, expression: &Expr, consume: bool) -> (Place, Type) {
        let natural = self.place_type(expression);
        let path = self.path(expression);
        let target = self.temporary(natural.clone(), expression.span);
        self.emit(
            Operation::Read {
                target,
                path,
                consume,
            },
            expression.span,
        );
        (target, natural)
    }

    fn value(&mut self, expression: &Expr, consume: bool) -> (Place, Type) {
        let span = expression.span;
        let ty = &expression.ty;
        let place = match &expression.kind {
            ExprKind::Int(..) | ExprKind::Float(..) | ExprKind::Bool(_) => self.constant(ty, span),
            ExprKind::String(..) => {
                let target = self.temporary(ty.clone(), span);
                self.emit(Operation::Static { target }, span);
                target
            }
            ExprKind::Name(name) => {
                if self.lookup(name).is_some() || self.context.constants.contains_key(name) {
                    return self.read(expression, consume);
                }
                // `none`, or a function used as a value.
                self.constant(ty, span)
            }
            ExprKind::Field(base, _)
                if matches!(&base.kind, ExprKind::Name(n)
                    if self.lookup(n).is_none() && self.context.enums.contains_key(n)) =>
            {
                self.constant(ty, span)
            }
            ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnaryOp::Deref, _) => {
                return self.read(expression, consume);
            }
            ExprKind::Constant(value, _) => return self.value(value, consume),
            ExprKind::Repeat(value, _) => {
                let value = self.expr(value, true);
                self.combine(ty, vec![value], span)
            }
            ExprKind::Array(_, items) => {
                let sources = items.iter().map(|item| self.expr(item, true)).collect();
                self.combine(ty, sources, span)
            }
            ExprKind::Struct(_, fields) => {
                let sources = fields.iter().map(|(_, e)| self.expr(e, true)).collect();
                self.combine(ty, sources, span)
            }
            ExprKind::ValueBlock(block) => {
                let result = self.temporary(ty.clone(), span);
                let exit = self.new_block();
                self.yields.push(YieldTarget {
                    exit,
                    result,
                    depth: self.scopes.len(),
                });
                self.push_scope();
                self.statements(block);
                // The checker guarantees every continuing path yields.
                self.end(Terminator::Unreachable, span);
                self.scopes.pop();
                self.yields.pop();
                self.current = Some(exit);
                self.unreachable_unless_reached(exit);
                result
            }
            ExprKind::Slice {
                base,
                start,
                end,
                mutable,
            } => {
                let mut path = self.path(base);
                if matches!(base.ty, Type::Ref(..) | Type::Slice(..)) {
                    path.projections.push(Projection::Deref);
                }
                // Reserve the source while the bounds run: reads such as
                // `data.len` are allowed, mutating it is not.
                let reservation = self.temporary(Type::Ref(false, Box::new(base.ty.clone())), span);
                self.emit(
                    Operation::Borrow {
                        target: reservation,
                        path: path.clone(),
                        mutable: false,
                        carried: false,
                    },
                    span,
                );
                for bound in start.iter().chain(end) {
                    self.expr(bound, false);
                }
                self.emit(Operation::Release(reservation), span);
                let natural = match ty {
                    Type::Slice(_, element) => Type::Slice(*mutable, element.clone()),
                    other => other.clone(),
                };
                let target = self.temporary(natural.clone(), span);
                self.emit(
                    Operation::Borrow {
                        target,
                        path,
                        mutable: *mutable,
                        carried: false,
                    },
                    span,
                );
                return (target, natural);
            }
            ExprKind::Unary(op @ (UnaryOp::Borrow | UnaryOp::BorrowMut), operand) => {
                let mutable = *op == UnaryOp::BorrowMut;
                let carried = self.carries(&self.place_type(operand));
                let path = self.path(operand);
                let natural = match ty {
                    Type::Slice(_, element) => Type::Slice(mutable, element.clone()),
                    Type::Ref(_, inner) => Type::Ref(mutable, inner.clone()),
                    other => other.clone(),
                };
                let target = self.temporary(natural.clone(), span);
                self.emit(
                    Operation::Borrow {
                        target,
                        path,
                        mutable,
                        carried,
                    },
                    span,
                );
                return (target, natural);
            }
            ExprKind::Unary(op, operand) => {
                if !(*op == UnaryOp::Neg && matches!(operand.kind, ExprKind::Int(..))) {
                    self.expr(operand, false);
                }
                self.constant(ty, span)
            }
            ExprKind::Binary(op @ (BinaryOp::And | BinaryOp::Or), left, right) => {
                let left = self.expr(left, false);
                let target = self.temporary(Type::Bool, span);
                let evaluate = self.new_block();
                let skip = self.new_block();
                let join = self.new_block();
                let targets = if *op == BinaryOp::And {
                    vec![evaluate, skip]
                } else {
                    vec![skip, evaluate]
                };
                self.branch(Some(left), targets, span);
                self.current = Some(evaluate);
                self.expr(right, false);
                self.emit(Operation::Constant { target }, span);
                self.goto(join, span);
                self.current = Some(skip);
                self.emit(Operation::Constant { target }, span);
                self.goto(join, span);
                self.current = Some(join);
                target
            }
            ExprKind::Binary(_, left, right) => {
                self.expr(left, false);
                self.expr(right, false);
                self.constant(ty, span)
            }
            ExprKind::Call {
                name,
                type_args,
                args,
            } => return self.call(name, type_args, args, ty, span),
            ExprKind::MethodCall { .. } | ExprKind::Range(..) => self.constant(ty, span),
            ExprKind::Cast(operand, target) => {
                let value = self.expr(operand, false);
                if target.closure_signature().is_some() {
                    if matches!(operand.ty, Type::Fn(..))
                        || matches!(&operand.kind, ExprKind::Closure(c) if c.captures.is_empty())
                    {
                        self.constant(ty, span)
                    } else {
                        self.combine(ty, vec![value], span)
                    }
                } else if let Type::Ref(wanted, object) = target
                    && matches!(object.as_ref(), Type::Dyn(_))
                    && matches!(operand.ty, Type::Ref(..))
                {
                    let place = self.combine(ty, vec![value], span);
                    if !*wanted {
                        self.emit(Operation::Demote { place }, span);
                    }
                    place
                } else {
                    self.constant(ty, span)
                }
            }
            ExprKind::Try(operand) | ExprKind::Unwrap(operand) => {
                let propagate = matches!(expression.kind, ExprKind::Try(_));
                let value = self.expr(operand, true);
                let Type::Result(success, error) = &operand.ty else {
                    return (self.constant(ty, span), ty.clone());
                };
                let failure = self.new_block();
                let ok = self.new_block();
                self.branch(None, vec![failure, ok], span);
                self.current = Some(failure);
                if propagate {
                    let checked = self.carries(error).then_some(value);
                    self.exit(Some(value), checked, span);
                } else {
                    self.end(Terminator::Unreachable, span);
                }
                self.current = Some(ok);
                if self.carries(success) {
                    self.combine(ty, vec![value], span)
                } else {
                    self.constant(ty, span)
                }
            }
            ExprKind::Closure(closure) => {
                if closure.captures.is_empty() {
                    self.constant(ty, span)
                } else {
                    let mut sources = vec![];
                    for capture in &closure.captures {
                        let Some(binding) = self.lookup(&capture.name) else {
                            continue;
                        };
                        let captured = self.body.locals[binding.0].ty.clone();
                        let target = self.temporary(
                            Type::Ref(capture.mutable, Box::new(captured.clone())),
                            span,
                        );
                        self.note = Some(format!(
                            "the closure {} `{}` for as long as it is used",
                            if capture.mutable {
                                "mutably borrows"
                            } else {
                                "borrows"
                            },
                            capture.name
                        ));
                        self.emit(
                            Operation::Borrow {
                                target,
                                path: Path::local(binding, span),
                                mutable: capture.mutable,
                                carried: self.carries(&captured),
                            },
                            span,
                        );
                        self.note = None;
                        sources.push(target);
                    }
                    self.combine(ty, sources, span)
                }
            }
            ExprKind::Invoke { callee, args } => {
                let value = self.expr(callee, false);
                let mut reservations = vec![];
                if let Some((mutable, ..)) = callee.ty.closure_signature() {
                    reservations.push(self.reserve(value, mutable, callee.span));
                }
                let params: Vec<Type> = callee
                    .ty
                    .call_signature()
                    .map(|(p, _)| p.to_vec())
                    .unwrap_or_default();
                let mut values = vec![value];
                let mut from = vec![];
                for (index, arg) in args.iter().enumerate() {
                    let param = params.get(index).cloned().unwrap_or(Type::Unknown);
                    let reference = matches!(param, Type::Ref(..) | Type::Slice(..));
                    let value = self.expr(arg, !reference);
                    if reference {
                        reservations.push(self.reserve(value, mutable_borrow(&param), arg.span));
                    }
                    if self.carries(&param) {
                        from.push(index + 1);
                    }
                    values.push(value);
                }
                let borrowed = self.carries(ty);
                let target = self.temporary(ty.clone(), span);
                self.emit(
                    Operation::Call {
                        target,
                        args: values,
                        reservations,
                        contract: Contract {
                            from,
                            fallback_static: true,
                            ..Contract::default()
                        },
                        borrowed,
                    },
                    span,
                );
                target
            }
            // The future moves into the suspended body and is destroyed once
            // ready. Its output may keep the borrows the future carried.
            ExprKind::Await { value, .. } => {
                let future = self.expr(value, true);
                let borrowed = self.carries(ty);
                let target = self.temporary(ty.clone(), span);
                self.emit(
                    Operation::Call {
                        target,
                        args: vec![future],
                        reservations: vec![],
                        contract: Contract {
                            from: if borrowed { vec![0] } else { vec![] },
                            ..Contract::default()
                        },
                        borrowed,
                    },
                    span,
                );
                target
            }
            ExprKind::Asm(asm) => {
                for value in asm.inputs() {
                    self.expr(value, true);
                }
                self.constant(ty, span)
            }
        };
        (place, ty.clone())
    }

    fn reserve(&mut self, source: Place, mutable: bool, span: Span) -> Place {
        let ty = self.body.locals[source.0].ty.clone();
        let target = self.temporary(ty, span);
        self.emit(
            Operation::Reserve {
                target,
                source,
                mutable,
            },
            span,
        );
        target
    }

    fn call(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &[Expr],
        ty: &Type,
        span: Span,
    ) -> (Place, Type) {
        let constructor = matches!(name, "ok" | "err" | "some" | "none")
            || name
                .rsplit_once('.')
                .is_some_and(|(owner, _)| self.context.enums.contains_key(owner));
        if constructor {
            let sources = args.iter().map(|arg| self.expr(arg, true)).collect();
            return (self.combine(ty, sources, span), ty.clone());
        }
        if name.starts_with("core.") {
            return (self.intrinsic(name, type_args, args, ty, span), ty.clone());
        }
        let Some(signature) = self.context.functions.get(name) else {
            for arg in args {
                self.expr(arg, true);
            }
            return (self.constant(ty, span), ty.clone());
        };
        let params = signature.params.clone();
        let from = signature.from.clone();
        let stores = signature.stores.clone();
        let ret = signature.ret.clone();
        let mut values = vec![];
        let mut reservations = vec![];
        for (arg, parameter) in args.iter().zip(&params) {
            let reference = matches!(parameter.ty, Type::Ref(..) | Type::Slice(..));
            let value = self.expr(arg, !reference);
            if reference {
                reservations.push(self.reserve(value, mutable_borrow(&parameter.ty), arg.span));
            }
            values.push(value);
        }
        let position = |name: &str| params.iter().position(|p| p.name == name);
        let contract = Contract {
            from: params
                .iter()
                .enumerate()
                .filter(|(_, p)| from.contains(&p.name))
                .map(|(i, _)| i)
                .collect(),
            stored: params
                .iter()
                .enumerate()
                .filter(|(_, p)| from.contains(&format!("{}.stored", p.name)))
                .map(|(i, _)| i)
                .collect(),
            static_: from.iter().any(|source| source == "static"),
            fallback_static: false,
            stores: stores.first().and_then(|target| {
                Some((
                    position(target)?,
                    stores[1..].iter().filter_map(|s| position(s)).collect(),
                ))
            }),
        };
        let borrowed = self.carries(&ret);
        let target = self.temporary(ret.clone(), span);
        self.emit(
            Operation::Call {
                target,
                args: values,
                reservations,
                contract,
                borrowed,
            },
            span,
        );
        (target, ret)
    }

    fn arguments(&mut self, args: &[Expr], modes: &[Mode]) -> (Vec<Place>, Vec<Place>) {
        let mut values = vec![];
        let mut reservations = vec![];
        for (index, arg) in args.iter().enumerate() {
            let mode = modes.get(index).copied().unwrap_or(Mode::Read);
            let value = self.expr(arg, mode == Mode::Move);
            if mode == Mode::Reference {
                let mutable = mutable_borrow(&self.body.locals[value.0].ty);
                reservations.push(self.reserve(value, mutable, arg.span));
            }
            values.push(value);
        }
        (values, reservations)
    }

    fn release(&mut self, reservations: Vec<Place>, span: Span) {
        for reservation in reservations {
            self.emit(Operation::Release(reservation), span);
        }
    }

    fn intrinsic(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &[Expr],
        ty: &Type,
        span: Span,
    ) -> Place {
        let _ = type_args;
        let operation = name.strip_prefix("core.").unwrap_or(name);
        use Mode::{Move, Read, Reference};
        match operation {
            "mem.storage_type" | "mem.size_of" | "mem.align_of" | "mem.assert_send"
            | "mem.assert_sync" | "mem.offset_of" | "mem.uninit" | "mem.callback" => {
                self.constant(ty, span)
            }
            "ptr.store" => {
                let (values, reservations) = self.arguments(args, &[Read, Move, Reference]);
                self.emit(
                    Operation::Deposit {
                        owner: values[2],
                        sources: vec![values[1]],
                    },
                    span,
                );
                self.release(reservations, span);
                self.constant(ty, span)
            }
            "ptr.take" | "ptr.view" | "ptr.view_slice" | "ptr.relocate" => {
                let modes: &[Mode] = match operation {
                    "ptr.relocate" => &[Read, Read, Read, Reference],
                    "ptr.view_slice" => &[Read, Read, Reference],
                    _ => &[Read, Reference],
                };
                let (values, reservations) = self.arguments(args, modes);
                let owner = *values.last().unwrap();
                self.release(reservations, span);
                let view = operation.starts_with("ptr.view");
                if view || (operation == "ptr.take" && self.carries(ty)) {
                    let target = self.temporary(ty.clone(), span);
                    self.emit(
                        Operation::Alias {
                            target,
                            source: owner,
                        },
                        span,
                    );
                    self.emit(
                        Operation::Filter {
                            place: target,
                            shared: view,
                            stored: !view,
                        },
                        span,
                    );
                    target
                } else {
                    self.constant(ty, span)
                }
            }
            "mem.split_at_mut" => {
                let value = self.expr(&args[0], false);
                let reservation = self.reserve(value, true, args[0].span);
                if let Some(mid) = args.get(1) {
                    self.expr(mid, false);
                }
                self.release(vec![reservation], span);
                self.combine(ty, vec![value], span)
            }
            "mem.str_bytes" | "mem.str_from_utf8" => {
                let value = self.expr(&args[0], false);
                let reservation = self.reserve(value, false, args[0].span);
                self.release(vec![reservation], span);
                self.combine(ty, vec![value], span)
            }
            "mem.init" | "mem.assume_init" | "drop" => {
                let (values, _) = self.arguments(args, &[Move]);
                if operation == "drop"
                    && let Some(value) = values.first()
                {
                    self.emit(Operation::Cleanup(*value), span);
                }
                self.constant(ty, span)
            }
            "mem.replace" => {
                let (_, reservations) = self.arguments(args, &[Reference, Move]);
                self.release(reservations, span);
                self.constant(ty, span)
            }
            "mem.swap" => {
                let (_, reservations) = self.arguments(args, &[Reference, Reference]);
                self.release(reservations, span);
                self.constant(ty, span)
            }
            "ptr.borrow" | "ptr.borrow_mut" | "ptr.borrow_slice" | "ptr.borrow_slice_mut" => {
                let modes: &[Mode] = if operation.contains("slice") {
                    &[Read, Read, Reference]
                } else {
                    &[Read, Reference]
                };
                let (values, reservations) = self.arguments(args, modes);
                let owner = *values.last().unwrap();
                self.release(reservations, span);
                let target = self.temporary(ty.clone(), span);
                self.emit(
                    Operation::Alias {
                        target,
                        source: owner,
                    },
                    span,
                );
                if !operation.ends_with("mut") {
                    self.emit(
                        Operation::Filter {
                            place: target,
                            shared: true,
                            stored: false,
                        },
                        span,
                    );
                }
                target
            }
            "ptr.write" | "ptr.write_unaligned" | "ptr.write_volatile" => {
                self.arguments(args, &[Read, Move]);
                self.constant(ty, span)
            }
            "ptr.write_bytes" => {
                self.arguments(args, &[Read, Move, Move]);
                self.constant(ty, span)
            }
            _ if operation.starts_with("mmio.") || operation.starts_with("cpu.") => {
                self.arguments(args, &[Move, Move]);
                self.constant(ty, span)
            }
            _ => {
                self.arguments(args, &[]);
                self.constant(ty, span)
            }
        }
    }
}
