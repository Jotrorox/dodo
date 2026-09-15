//! Future-use scanning for conservative liveness and lexical diagnostic labels.
//! These two scans intentionally differ: see `binding_use_spans`.
use crate::ast::*;
use std::collections::HashMap;

pub(super) fn names_expr(expression: &Expr, names: &mut HashMap<String, Span>) {
    match &expression.kind {
        ExprKind::Name(name) => {
            names
                .entry(name.clone())
                .and_modify(|span| {
                    if expression.span.start > span.start {
                        *span = expression.span;
                    }
                })
                .or_insert(expression.span);
        }
        ExprKind::Unary(_, e)
        | ExprKind::Field(e, _)
        | ExprKind::Cast(e, _)
        | ExprKind::Try(e)
        | ExprKind::Unwrap(e)
        | ExprKind::Constant(e, _)
        | ExprKind::Repeat(e, _) => names_expr(e, names),
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) | ExprKind::Range(a, b) => {
            names_expr(a, names);
            names_expr(b, names);
        }
        ExprKind::Call { args, .. } | ExprKind::Array(_, args) => {
            for arg in args {
                names_expr(arg, names);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            names_expr(receiver, names);
            for arg in args {
                names_expr(arg, names);
            }
        }
        ExprKind::Struct(_, fields) => {
            for (_, e) in fields {
                names_expr(e, names);
            }
        }
        ExprKind::ValueBlock(body) => names_block(body, names),
        ExprKind::Slice {
            base, start, end, ..
        } => {
            names_expr(base, names);
            for e in start.iter().chain(end) {
                names_expr(e, names);
            }
        }
        _ => (),
    }
}
pub(super) fn names_stmt(statement: &Stmt, names: &mut HashMap<String, Span>) {
    match &statement.kind {
        StmtKind::Let { value: Some(e), .. }
        | StmtKind::Expr(e)
        | StmtKind::Yield(e)
        | StmtKind::Return(Some(e)) => names_expr(e, names),
        StmtKind::Assign { target, value, .. } => {
            names_expr(target, names);
            names_expr(value, names);
        }
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            names_expr(condition, names);
            names_block(then_block, names);
            names_block(else_block, names);
        }
        StmtKind::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            names_expr(value, names);
            names_block(then_block, names);
            names_block(else_block, names);
        }
        StmtKind::LetPattern {
            value, else_block, ..
        } => {
            names_expr(value, names);
            if let Some(block) = else_block {
                names_block(block, names);
            }
        }
        StmtKind::For {
            init,
            condition,
            step,
            body,
        } => {
            if let Some(s) = init {
                names_stmt(s, names);
            }
            if let Some(e) = condition {
                names_expr(e, names);
            }
            if let Some(s) = step {
                names_stmt(s, names);
            }
            names_block(body, names);
        }
        StmtKind::ForEach { iterable, body, .. } => {
            names_expr(iterable, names);
            names_block(body, names);
        }
        StmtKind::Match { value, arms } => {
            names_expr(value, names);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    names_expr(guard, names);
                }
                names_block(&arm.body, names);
            }
        }
        StmtKind::Block(b) | StmtKind::Unsafe(b) => names_block(b, names),
        _ => (),
    }
}
pub(super) fn names_block(block: &Block, names: &mut HashMap<String, Span>) {
    for s in block {
        names_stmt(s, names);
    }
}

/// Resolve diagnostic use sites lexically. The conservative name-based liveness
/// calculation above remains unchanged; a shadowed name must not be presented
/// as evidence that an earlier binding's borrow is used.
pub(super) fn binding_use_spans(function: &Function) -> HashMap<(usize, String), Span> {
    #[derive(Default)]
    struct Uses {
        scopes: Vec<HashMap<String, usize>>,
        spans: HashMap<(usize, String), Span>,
    }
    impl Uses {
        fn bind(&mut self, name: &str, span: Span) {
            self.scopes
                .last_mut()
                .unwrap()
                .insert(name.into(), span.start);
        }
        fn pattern(&mut self, pattern: &Pattern, span: Span) {
            for name in pattern.bindings() {
                self.bind(&name, span);
            }
        }
        fn expr(&mut self, expression: &Expr) {
            match &expression.kind {
                ExprKind::Name(name) => {
                    if let Some(declaration) =
                        self.scopes.iter().rev().find_map(|scope| scope.get(name))
                    {
                        self.spans
                            .entry((*declaration, name.clone()))
                            .and_modify(|span| {
                                if expression.span.start > span.start {
                                    *span = expression.span;
                                }
                            })
                            .or_insert(expression.span);
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
                ExprKind::Call { args, .. } | ExprKind::Array(_, args) => {
                    for arg in args {
                        self.expr(arg);
                    }
                }
                ExprKind::MethodCall { receiver, args, .. } => {
                    self.expr(receiver);
                    for arg in args {
                        self.expr(arg);
                    }
                }
                ExprKind::Struct(_, fields) => {
                    for (_, value) in fields {
                        self.expr(value);
                    }
                }
                ExprKind::ValueBlock(body) => self.block(body, true),
                ExprKind::Slice {
                    base, start, end, ..
                } => {
                    self.expr(base);
                    for bound in start.iter().chain(end) {
                        self.expr(bound);
                    }
                }
                _ => (),
            }
        }
        fn block(&mut self, block: &Block, scoped: bool) {
            if scoped {
                self.scopes.push(HashMap::new());
            }
            for statement in block {
                self.stmt(statement);
            }
            if scoped {
                self.scopes.pop();
            }
        }
        fn stmt(&mut self, statement: &Stmt) {
            match &statement.kind {
                StmtKind::Let { name, value, .. } => {
                    if let Some(value) = value {
                        self.expr(value);
                    }
                    self.bind(name, statement.span);
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
                    self.pattern(pattern, statement.span);
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
                    self.scopes.push(HashMap::new());
                    self.pattern(pattern, statement.span);
                    self.block(then_block, false);
                    self.scopes.pop();
                    self.block(else_block, true);
                }
                StmtKind::For {
                    init,
                    condition,
                    step,
                    body,
                } => {
                    self.scopes.push(HashMap::new());
                    if let Some(s) = init {
                        self.stmt(s);
                    }
                    if let Some(e) = condition {
                        self.expr(e);
                    }
                    if let Some(s) = step {
                        self.stmt(s);
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
                    self.scopes.push(HashMap::new());
                    if let Some(index) = index {
                        self.bind(index, statement.span);
                    }
                    self.bind(name, statement.span);
                    self.block(body, false);
                    self.scopes.pop();
                }
                StmtKind::Match { value, arms } => {
                    self.expr(value);
                    for arm in arms {
                        self.scopes.push(HashMap::new());
                        self.pattern(&arm.pattern, arm.span);
                        if let Some(guard) = &arm.guard {
                            self.expr(guard);
                        }
                        self.block(&arm.body, false);
                        self.scopes.pop();
                    }
                }
                StmtKind::Block(block) | StmtKind::Unsafe(block) => self.block(block, true),
                _ => (),
            }
        }
    }
    let mut uses = Uses {
        scopes: vec![HashMap::new()],
        ..Uses::default()
    };
    for parameter in &function.params {
        uses.bind(&parameter.name, parameter.span);
    }
    if let Some(body) = &function.body {
        uses.block(body, false);
    }
    uses.spans
}
