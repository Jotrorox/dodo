//! Closure literals: result tails, captured bindings, and environment access.
//!
//! A closure body is checked as its own function. Every captured binding
//! becomes a leading reference parameter of that function, and uses of the
//! binding inside the body read or write through it. The closure value keeps
//! a shared or mutable loan of each captured binding for as long as it lives.
use crate::ast::*;
use std::collections::HashSet;

/// Whether a final expression of the body still awaits its result type.
pub(super) fn has_tail(block: &Block) -> bool {
    let Some(last) = block.last() else {
        return false;
    };
    match &last.kind {
        StmtKind::Yield(_) => true,
        StmtKind::If {
            then_block,
            else_block,
            ..
        }
        | StmtKind::IfLet {
            then_block,
            else_block,
            ..
        } => has_tail(then_block) || has_tail(else_block),
        StmtKind::Match { arms, .. } => arms.iter().any(|arm| has_tail(&arm.body)),
        StmtKind::Block(body) | StmtKind::Unsafe(body) => has_tail(body),
        _ => false,
    }
}

/// Whether the body contains a `return` with a value outside nested closures.
pub(super) fn returns(block: &Block) -> bool {
    block.iter().any(|statement| match &statement.kind {
        StmtKind::Return(Some(_)) => true,
        StmtKind::If {
            then_block,
            else_block,
            ..
        }
        | StmtKind::IfLet {
            then_block,
            else_block,
            ..
        } => returns(then_block) || returns(else_block),
        StmtKind::LetPattern {
            else_block: Some(body),
            ..
        }
        | StmtKind::For { body, .. }
        | StmtKind::ForEach { body, .. }
        | StmtKind::Block(body)
        | StmtKind::Unsafe(body) => returns(body),
        StmtKind::Match { arms, .. } => arms.iter().any(|arm| returns(&arm.body)),
        _ => false,
    })
}

/// Final expressions become returns of a value-returning closure, or plain
/// statements of a void one. Only tail positions are rewritten.
pub(super) fn settle_tails(block: &mut Block, value: bool) {
    let Some(last) = block.last_mut() else {
        return;
    };
    match &mut last.kind {
        StmtKind::Yield(e) | StmtKind::Return(Some(e)) => {
            let e = std::mem::replace(e, Expr::new(ExprKind::Bool(false), last.span));
            last.kind = if value {
                StmtKind::Return(Some(e))
            } else {
                StmtKind::Expr(e)
            };
        }
        StmtKind::If {
            then_block,
            else_block,
            ..
        }
        | StmtKind::IfLet {
            then_block,
            else_block,
            ..
        } => {
            settle_tails(then_block, value);
            settle_tails(else_block, value);
        }
        StmtKind::Match { arms, .. } => {
            for arm in arms {
                settle_tails(&mut arm.body, value);
            }
        }
        StmtKind::Block(body) | StmtKind::Unsafe(body) => settle_tails(body, value),
        _ => (),
    }
}

/// Names a closure body uses without binding them itself, in order of first
/// use. Callers keep only those that resolve to enclosing local bindings.
pub(super) fn free_names(closure: &Closure) -> Vec<String> {
    let mut scan = Scan {
        scopes: vec![closure.params.iter().map(|p| p.name.clone()).collect()],
        rewrite: None,
        found: Vec::new(),
    };
    let mut body = closure.body.clone();
    scan.block(&mut body, false);
    scan.found
}

/// Route every use of a captured binding through its environment reference:
/// `x` becomes `*x`, and a call of a captured function value `f(..)` becomes
/// a call of `*f`. Shadowing bindings inside the body are left alone.
pub(super) fn route_captures(closure: &mut Closure, captured: &HashSet<String>) {
    let mut scan = Scan {
        scopes: vec![closure.params.iter().map(|p| p.name.clone()).collect()],
        rewrite: Some(captured),
        found: Vec::new(),
    };
    scan.block(&mut closure.body, false);
}

struct Scan<'a> {
    scopes: Vec<HashSet<String>>,
    rewrite: Option<&'a HashSet<String>>,
    found: Vec<String>,
}

impl Scan<'_> {
    fn bound(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains(name))
    }
    fn bind(&mut self, name: &str) {
        self.scopes.last_mut().unwrap().insert(name.to_owned());
    }
    /// Record a free use; true when it must be routed through the environment.
    fn free(&mut self, name: &str) -> bool {
        if self.bound(name) || name.contains('.') {
            return false;
        }
        if !self.found.iter().any(|found| found == name) {
            self.found.push(name.to_owned());
        }
        self.rewrite.is_some_and(|captured| captured.contains(name))
    }
    fn block(&mut self, block: &mut Block, scoped: bool) {
        if scoped {
            self.scopes.push(HashSet::new());
        }
        for statement in block {
            self.statement(statement);
        }
        if scoped {
            self.scopes.pop();
        }
    }
    fn statement(&mut self, statement: &mut Stmt) {
        match &mut statement.kind {
            StmtKind::Let { name, value, .. } => {
                if let Some(value) = value {
                    self.expr(value);
                }
                self.bind(name);
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::Expr(e) | StmtKind::Yield(e) | StmtKind::Return(Some(e)) => self.expr(e),
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr(condition);
                self.block(then_block, true);
                self.block(else_block, true);
            }
            StmtKind::IfLet {
                pattern,
                value,
                then_block,
                else_block,
            } => {
                self.expr(value);
                self.scopes.push(HashSet::new());
                for name in pattern.bindings() {
                    self.bind(&name);
                }
                self.block(then_block, false);
                self.scopes.pop();
                self.block(else_block, true);
            }
            StmtKind::LetPattern {
                pattern,
                value,
                else_block,
                ..
            } => {
                self.expr(value);
                if let Some(block) = else_block {
                    self.block(block, true);
                }
                for name in pattern.bindings() {
                    self.bind(&name);
                }
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                self.scopes.push(HashSet::new());
                if let Some(init) = init {
                    self.statement(init);
                }
                if let Some(condition) = condition {
                    self.expr(condition);
                }
                if let Some(step) = step {
                    self.statement(step);
                }
                self.block(body, true);
                self.scopes.pop();
            }
            StmtKind::ForEach {
                index,
                name,
                iterable,
                body,
                ..
            } => {
                self.expr(iterable);
                self.scopes.push(HashSet::new());
                if let Some(index) = index {
                    self.bind(index);
                }
                self.bind(name);
                self.block(body, false);
                self.scopes.pop();
            }
            StmtKind::Match { value, arms } => {
                self.expr(value);
                for arm in arms {
                    self.scopes.push(HashSet::new());
                    for name in arm.pattern.bindings() {
                        self.bind(&name);
                    }
                    if let Some(guard) = &mut arm.guard {
                        self.expr(guard);
                    }
                    self.block(&mut arm.body, false);
                    self.scopes.pop();
                }
            }
            StmtKind::Block(block) | StmtKind::Unsafe(block) => self.block(block, true),
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => (),
        }
    }
    fn expr(&mut self, expression: &mut Expr) {
        let span = expression.span;
        match &mut expression.kind {
            ExprKind::Name(name) => {
                if self.free(name) {
                    let name = Expr::new(ExprKind::Name(std::mem::take(name)), span);
                    expression.kind = ExprKind::Unary(UnaryOp::Deref, Box::new(name));
                }
            }
            ExprKind::Call {
                name,
                type_args,
                args,
            } => {
                for arg in args.iter_mut() {
                    self.expr(arg);
                }
                if type_args.is_empty() && self.free(name) {
                    let callee = Expr::new(ExprKind::Name(std::mem::take(name)), span);
                    expression.kind = ExprKind::Invoke {
                        callee: Box::new(Expr::new(
                            ExprKind::Unary(UnaryOp::Deref, Box::new(callee)),
                            span,
                        )),
                        args: std::mem::take(args),
                    };
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                self.expr(receiver);
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::Invoke { callee, args } => {
                self.expr(callee);
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::Asm(asm) => {
                for value in asm.inputs_mut() {
                    self.expr(value);
                }
            }
            ExprKind::Array(_, values) => {
                for value in values {
                    self.expr(value);
                }
            }
            ExprKind::Struct(_, fields) => {
                for (_, value) in fields {
                    self.expr(value);
                }
            }
            ExprKind::Unary(_, e)
            | ExprKind::Field(e, _)
            | ExprKind::Cast(e, _)
            | ExprKind::Try(e)
            | ExprKind::Unwrap(e)
            | ExprKind::Constant(e, _)
            | ExprKind::Repeat(e, _) => self.expr(e),
            ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) | ExprKind::Range(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::Slice {
                base, start, end, ..
            } => {
                self.expr(base);
                for bound in start.iter_mut().chain(end) {
                    self.expr(bound);
                }
            }
            ExprKind::ValueBlock(body) => self.block(body, true),
            ExprKind::Closure(closure) => {
                self.scopes
                    .push(closure.params.iter().map(|p| p.name.clone()).collect());
                self.block(&mut closure.body, false);
                self.scopes.pop();
            }
            ExprKind::Int(..) | ExprKind::Float(..) | ExprKind::Bool(_) | ExprKind::String(..) => {}
        }
    }
}

use super::ownership::Value;
use super::uses::{binding_use_spans, names_block};
use super::{Check, Checker, static_loan};
use crate::diagnostic::Diagnostic;
use std::collections::HashMap;

impl Checker<'_> {
    /// A named function used as a value: a function pointer with no environment.
    pub(super) fn function_value(&self, name: &str, span: Span) -> Check<Type> {
        let signature = &self.context.functions[name];
        if let Some((owner, _)) = name.rsplit_once('.')
            && self.context.interfaces.contains_key(owner)
        {
            return Err(Diagnostic::new(
                span,
                format!("interface method `{name}` dispatches through a `dyn` reference and is not a function value"),
            )
            .note("wrap the call in a closure, as in `fn(value: &dyn Shape) -> f64 { value.area() }`"));
        }
        if !signature.public && self.context.function_namespace(name) != self.namespace {
            return Err(Diagnostic::new(
                span,
                format!("function `{name}` is private to its package"),
            ));
        }
        if let Some(vector) = &signature.interrupt {
            return Err(super::interrupt_handler_use(name, vector, span));
        }
        if signature.unsafe_ || signature.extern_ {
            return Err(Diagnostic::new(
                span,
                format!("unsafe or foreign function `{name}` cannot be used as a function value"),
            )
            .note("wrap the call in a safe function; pass C callbacks with `mem.callback`"));
        }
        if signature.unavailable
            || name.ends_with(".drop")
            || !signature.stores.is_empty()
            || signature
                .from
                .iter()
                .any(|source| source.ends_with(".stored"))
        {
            return Err(Diagnostic::new(
                span,
                format!(
                    "`{name}` has a storage or destruction contract and cannot be used as a function value"
                ),
            ));
        }
        Ok(Type::Fn(
            signature.params.iter().map(|p| p.ty.clone()).collect(),
            Box::new(signature.ret.clone()),
        ))
    }

    /// Check a closure literal. Its body is lifted into a function whose
    /// leading parameters reference the captured bindings.
    pub(super) fn closure(
        &mut self,
        closure: &mut Closure,
        expected: Option<&Type>,
        span: Span,
    ) -> Check<Value> {
        let hint = expected
            .and_then(Type::callable_signature)
            .filter(|(params, _)| params.len() == closure.params.len())
            .map(|(params, ret)| (params.to_vec(), ret.clone()));
        let mut names = HashSet::new();
        for (index, param) in closure.params.iter_mut().enumerate() {
            if !names.insert(param.name.clone()) {
                return Err(Diagnostic::new(
                    param.span,
                    format!("duplicate parameter `{}`", param.name),
                ));
            }
            if param.ty == Type::Unknown {
                param.ty = hint
                    .as_ref()
                    .map(|(params, _)| params[index].clone())
                    .ok_or_else(|| {
                        Diagnostic::new(
                            param.span,
                            format!("cannot infer the type of closure parameter `{}`", param.name),
                        )
                        .note("annotate it, as in `fn(value: i32)`, or pass the closure where a function type is expected")
                    })?;
            }
            self.context.validate_type(&param.ty, param.span, false)?;
        }
        if closure.ret == Type::Unknown {
            if let Some((_, ret)) = &hint {
                closure.ret = ret.clone();
            }
        } else {
            self.context
                .validate_type(&closure.ret, closure.ret_span, true)?;
        }
        let captured: Vec<(String, Type)> = free_names(closure)
            .into_iter()
            .filter_map(|name| self.lookup(&name).map(|v| (name, v.ty.clone())))
            .collect();
        let routed: HashSet<String> = captured.iter().map(|(name, _)| name.clone()).collect();
        route_captures(closure, &routed);
        settle_tails(&mut closure.body, closure.ret != Type::Void);

        let name = format!("{}#closure{}", self.function_name, span.start);
        let from: Vec<String> = closure
            .params
            .iter()
            .filter(|p| self.context.carries_borrow(&p.ty))
            .map(|p| p.name.clone())
            .collect();
        let mut function = Function {
            name: name.clone(),
            imported: self.imported,
            test: false,
            ignore: None,
            public: false,
            unsafe_: false,
            extern_: false,
            printing: None,
            pio: false,
            generics: vec![],
            bounds: vec![],
            generic_instance: false,
            params: captured
                .iter()
                .map(|(name, ty)| Param {
                    name: name.clone(),
                    ty: Type::Ref(true, Box::new(ty.clone())),
                    span,
                })
                .chain(closure.params.iter().cloned())
                .collect(),
            ret: closure.ret.clone(),
            ret_span: closure.ret_span,
            from: from.clone(),
            stores: vec![],
            requires_plain: vec![],
            from_span: None,
            body: Some(std::mem::take(&mut closure.body)),
            span,
            environment: Some(captured.len()),
            interrupt: None,
            placement: Placement::default(),
        };
        let mut uses = HashMap::new();
        names_block(function.body.as_ref().unwrap(), &mut uses);
        let mut checker = Checker::new(self.context, closure.ret.clone(), from, uses);
        checker.recover = self.recover;
        checker.namespace = self.namespace.clone();
        checker.function_name = name.clone();
        checker.imported = self.imported;
        checker.unsafe_depth = self.unsafe_depth;
        checker.binding_uses = binding_use_spans(&function);
        checker.return_contract = Some(closure.ret_span);
        checker.inferred_contract = true;
        for parameter in &function.params[..captured.len()] {
            checker
                .capture_roots
                .insert(usize::MAX / 2 + checker.next_id, parameter.name.clone());
            checker.bind_parameter(&parameter.name, &parameter.ty, parameter.span)?;
        }
        for parameter in &closure.params {
            checker.bind_parameter(&parameter.name, &parameter.ty, parameter.span)?;
        }
        let body = function.body.as_mut().unwrap();
        let result = checker.block(body, false);
        self.closures.append(&mut checker.closures);
        let had_errors = !checker.diagnostics.is_empty();
        self.diagnostics.append(&mut checker.diagnostics);
        let terminates = result?;
        if checker.return_ty == Type::Unknown {
            checker.return_ty = Type::Void;
        }
        let ret = checker.return_ty.clone();
        if ret != Type::Void && !terminates && !had_errors {
            return Err(Diagnostic::new(
                span,
                format!("closure can reach its end without returning {ret}"),
            ));
        }
        checker.finish_scope(span)?;
        if ret == Type::Void {
            settle_tails(body, false);
        }
        let mutated = checker.mutated_captures.take();
        closure.ret = ret.clone();
        closure.body = body.clone();
        function.ret = ret.clone();

        let signature = Type::Fn(
            closure.params.iter().map(|p| p.ty.clone()).collect(),
            Box::new(ret),
        );
        if let Some(expected) = expected {
            let first_mutated = || {
                captured
                    .iter()
                    .find(|(name, _)| mutated.contains(name))
                    .map(|(name, _)| name.clone())
            };
            match expected {
                Type::Fn(..) if !captured.is_empty() => {
                    return Err(Diagnostic::new(
                        span,
                        format!(
                            "a function pointer `{expected}` cannot capture local bindings, but this closure uses `{}`",
                            captured[0].0
                        ),
                    )
                    .note(format!(
                        "accept a closure reference `&dyn {expected}` instead; it borrows the captured bindings"
                    )));
                }
                Type::Ref(false, inner) if matches!(inner.as_ref(), Type::DynFn(..)) => {
                    if let Some(name) = first_mutated() {
                        return Err(Diagnostic::new(
                            span,
                            format!("closure modifies captured `{name}`, so it is `&mut dyn {signature}`, not `{expected}`"),
                        )
                        .note("a shared closure reference may only read what it captures; accept `&mut dyn fn(..)` to allow modification"));
                    }
                }
                _ => {}
            }
        }

        let mut deps = Vec::new();
        let mut captures = Vec::new();
        for (index, (name, ty)) in captured.iter().enumerate() {
            let mutable = mutated.contains(name);
            function.params[index].ty = Type::Ref(mutable, Box::new(ty.clone()));
            if mutable && self.lookup(name).is_some_and(|v| v.immutable) {
                return Err(Diagnostic::new(
                    span,
                    format!("closure modifies captured `{name}`, an immutable binding"),
                )
                .note(format!("declare `{name}` with `:=` to allow modification")));
            }
            let mut borrow = Expr::new(
                ExprKind::Unary(
                    if mutable {
                        UnaryOp::BorrowMut
                    } else {
                        UnaryOp::Borrow
                    },
                    Box::new(Expr::new(ExprKind::Name(name.clone()), span)),
                ),
                span,
            );
            let value = self.expr(&mut borrow, None, false).map_err(|error| {
                error.note(format!(
                    "the closure {} `{name}` for as long as it is used",
                    if mutable {
                        "mutably borrows"
                    } else {
                        "borrows"
                    }
                ))
            })?;
            deps.extend(value.deps);
            captures.push(Capture {
                name: name.clone(),
                mutable,
            });
        }
        closure.function = name;
        closure.captures = captures;
        self.closures.push(function);
        if captured.is_empty() {
            Ok(Value {
                ty: signature,
                deps: vec![],
            })
        } else {
            let Type::Fn(params, ret) = signature else {
                unreachable!()
            };
            Ok(Value {
                ty: Type::Ref(!mutated.is_empty(), Box::new(Type::DynFn(params, ret))),
                deps,
            })
        }
    }

    /// A call through a function pointer or closure reference.
    pub(super) fn invoke(
        &mut self,
        callee: &mut Expr,
        args: &mut [Expr],
        span: Span,
    ) -> Check<Value> {
        // Call closure references stored behind further references directly.
        while let Some(Type::Ref(_, inner)) = self.peek_type(callee)
            && !matches!(*inner, Type::DynFn(..))
            && (inner.callable_signature().is_some() || matches!(*inner, Type::Ref(..)))
        {
            let inner = std::mem::replace(callee, Expr::new(ExprKind::Bool(false), span));
            *callee = Expr::new(ExprKind::Unary(UnaryOp::Deref, Box::new(inner)), span);
        }
        let start = self.temporary.len();
        let peeked = self.peek_type(callee);
        let value = self.expr(callee, peeked.as_ref(), false)?;
        let Some((params, ret)) = value
            .ty
            .callable_signature()
            .map(|(p, r)| (p.to_vec(), r.clone()))
        else {
            return Err(Diagnostic::new(
                callee.span,
                format!(
                    "`{}` is not a function value and cannot be called",
                    value.ty
                ),
            ));
        };
        if let Some((mutable, ..)) = value.ty.closure_signature() {
            self.reserve_argument_borrow(&value, mutable, start, callee.span)?;
        }
        if args.len() != params.len() {
            return Err(Diagnostic::new(
                span,
                format!(
                    "function value expects {} arguments, found {}",
                    params.len(),
                    args.len()
                ),
            ));
        }
        let mut dependencies = vec![];
        for (arg, param) in args.iter_mut().zip(&params) {
            let reference = matches!(param, Type::Ref(..) | Type::Slice(..));
            let argument_start = self.temporary.len();
            let value = self.expr(arg, Some(param), !reference)?;
            self.expect(param, &value.ty, arg.span)?;
            if reference {
                self.reserve_argument_borrow(
                    &value,
                    super::mutable_borrow(param),
                    argument_start,
                    arg.span,
                )?;
            }
            if self.context.carries_borrow(param) {
                dependencies.extend(value.deps);
            }
        }
        self.temporary.truncate(start);
        if self.context.carries_borrow(&ret) {
            if dependencies.is_empty() {
                dependencies.push(static_loan(span));
            }
            self.temporary.extend(dependencies.clone());
        } else {
            dependencies.clear();
        }
        Ok(Value {
            ty: ret,
            deps: dependencies,
        })
    }
}
