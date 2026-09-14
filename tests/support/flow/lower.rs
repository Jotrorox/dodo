//! A deliberately small source adapter, independent of sema and AST annotations.
//! Failure discards the entire body; unsupported nodes never become no-ops.
use super::*;
use dodoc::ast::{self, Expr, ExprKind, Function, Program, StmtKind, UnaryOp};
use std::collections::HashMap;

#[derive(Debug)]
pub struct Limitation {
    pub span: Span,
    pub reason: String,
}
type Result<T> = std::result::Result<T, Limitation>;
fn unsupported<T>(span: Span, reason: &str) -> Result<T> {
    Err(Limitation {
        span,
        reason: reason.into(),
    })
}

/// Lower only the named body. Other bodies are not certified by its result.
/// Signatures are checked, but calls assume normal return and no hidden moves.
/// Neither this adapter nor initialization() validates borrowing or full Dodo.
pub fn lower(program: &Program, name: &str) -> Result<Body> {
    if !program.imports.is_empty() || !program.constants.is_empty() || !program.enums.is_empty() {
        return unsupported(
            Span::default(),
            "imports, globals, and enums are unsupported",
        );
    }
    let mut names = std::collections::HashSet::new();
    for structure in &program.structs {
        if !names.insert(&structure.name) || !structure.generics.is_empty() {
            return unsupported(
                structure.span,
                "duplicate or generic structs are unsupported",
            );
        }
        let mut fields = std::collections::HashSet::new();
        for field in &structure.fields {
            if !fields.insert(&field.name) || !scalar(&field.ty) {
                return unsupported(
                    field.span,
                    "only structs with distinct scalar fields are supported",
                );
            }
        }
    }
    for function in &program.functions {
        if !names.insert(&function.name)
            || !function.generics.is_empty()
            || function.generic_instance
            || function.unsafe_
            || function.extern_
            || function.imported
            || function.name.contains('.')
            || !function.from.is_empty()
        {
            return unsupported(
                function.span,
                "generics, methods, destructors, FFI, and borrow contracts are unsupported",
            );
        }
        supported_type(program, &function.ret, function.ret_span)?;
        if matches!(function.ret, Type::Ref(..)) {
            return unsupported(function.ret_span, "borrowed returns are unsupported");
        }
        for param in &function.params {
            supported_type(program, &param.ty, param.span)?;
            if param.ty == Type::Void {
                return unsupported(param.span, "void parameters are unsupported");
            }
        }
    }
    let function = program
        .functions
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| Limitation {
            span: Span::default(),
            reason: format!("missing function `{name}`"),
        })?;
    let body = function.body.as_ref().ok_or_else(|| Limitation {
        span: function.span,
        reason: "bodyless functions are unsupported".into(),
    })?;
    let mut lower = Lower {
        program,
        function,
        body: Body {
            locals: vec![],
            blocks: vec![],
        },
        scopes: vec![Scope::default()],
        loops: vec![],
        current: Some(0),
    };
    lower.block_id();
    for parameter in &function.params {
        let place = lower.bind(&parameter.name, parameter.ty.clone(), true, parameter.span)?;
        lower.body.locals[place.0].parameter = true;
    }
    lower.statements(body)?;
    if lower.current.is_some() {
        if function.ret != Type::Void {
            return unsupported(function.span, "non-void fallthrough is unsupported");
        }
        lower.cleanup(0, None, function.span);
        lower.end(Terminator::Return(None), function.span);
    }
    Ok(lower.body)
}
fn scalar(ty: &Type) -> bool {
    matches!(ty, Type::Bool | Type::Int { .. })
}
fn supported_type(program: &Program, ty: &Type, span: Span) -> Result<()> {
    let owned =
        |ty: &Type| matches!(ty, Type::Named(n) if program.structs.iter().any(|s| &s.name == n));
    if *ty == Type::Void
        || scalar(ty)
        || owned(ty)
        || matches!(ty, Type::Ref(_, inner) if scalar(inner) || owned(inner))
    {
        Ok(())
    } else {
        unsupported(
            span,
            "type is outside the scalar, scalar-field struct, and direct-reference subset",
        )
    }
}

#[derive(Default)]
struct Scope {
    names: HashMap<String, (Place, bool)>,
    locals: Vec<Place>,
}
#[derive(Clone, Copy)]
struct Loop {
    header: BlockId,
    exit: BlockId,
    depth: usize,
}
struct Lower<'a> {
    program: &'a Program,
    function: &'a Function,
    body: Body,
    scopes: Vec<Scope>,
    loops: Vec<Loop>,
    current: Option<BlockId>,
}
impl Lower<'_> {
    fn block_id(&mut self) -> BlockId {
        let id = self.body.blocks.len();
        self.body.blocks.push(BasicBlock {
            instructions: vec![],
            terminator: Terminator::Unreachable,
            span: Span::default(),
        });
        id
    }
    fn emit(&mut self, operation: Operation, span: Span) {
        self.body.blocks[self.current.unwrap()]
            .instructions
            .push(Instruction { operation, span });
    }
    fn end(&mut self, terminator: Terminator, span: Span) {
        let block = &mut self.body.blocks[self.current.take().unwrap()];
        block.terminator = terminator;
        block.span = span;
    }
    fn local(&mut self, name: &str, ty: Type, span: Span) -> Place {
        let place = Place(self.body.locals.len());
        self.body.locals.push(Local {
            name: name.into(),
            ty,
            span,
            parameter: false,
        });
        self.scopes.last_mut().unwrap().locals.push(place);
        place
    }
    fn bind(&mut self, name: &str, ty: Type, mutable: bool, span: Span) -> Result<Place> {
        if name == "_" || self.scopes.last().unwrap().names.contains_key(name) {
            return unsupported(span, "discard or duplicate binding is unsupported");
        }
        let place = self.local(name, ty, span);
        self.scopes
            .last_mut()
            .unwrap()
            .names
            .insert(name.into(), (place, mutable));
        Ok(place)
    }
    fn place(&self, expression: &Expr, write: bool) -> Result<Place> {
        if let ExprKind::Name(name) = &expression.kind {
            for scope in self.scopes.iter().rev() {
                if let Some((place, mutable)) = scope.names.get(name) {
                    if write && !mutable {
                        return unsupported(expression.span, "write through immutable binding");
                    }
                    return Ok(*place);
                }
            }
        }
        unsupported(
            expression.span,
            "place must be a resolved whole local; projections are unsupported",
        )
    }
    fn operand(&self, place: Place) -> Operand {
        if self.body.locals[place.0].ty.is_copy() {
            Operand::Copy(place)
        } else {
            Operand::Move(place)
        }
    }
    fn expect(&self, expected: &Type, actual: &Type, span: Span) -> Result<()> {
        if expected == actual {
            Ok(())
        } else {
            Err(Limitation {
                span,
                reason: format!("type mismatch: expected {expected}, found {actual}"),
            })
        }
    }
    fn cleanup(&mut self, depth: usize, except: Option<Place>, span: Span) {
        let departing: Vec<_> = self.scopes[depth..]
            .iter()
            .rev()
            .flat_map(|scope| scope.locals.iter().rev())
            .copied()
            .collect();
        for place in departing {
            if Some(place) != except {
                self.emit(Operation::Cleanup(place), span);
            }
        }
    }
    fn scoped(&mut self, block: &ast::Block, span: Span) -> Result<()> {
        self.scopes.push(Scope::default());
        self.statements(block)?;
        if self.current.is_some() {
            self.cleanup(self.scopes.len() - 1, None, span);
        }
        self.scopes.pop();
        Ok(())
    }
    fn statements(&mut self, block: &ast::Block) -> Result<()> {
        for statement in block {
            if self.current.is_none() {
                return unsupported(
                    statement.span,
                    "syntax after a terminating statement is unsupported",
                );
            }
            let span = statement.span;
            match &statement.kind {
                StmtKind::Let {
                    name,
                    ty,
                    value,
                    constant,
                    mutable,
                } => {
                    if *constant {
                        return unsupported(span, "constant bindings are unsupported");
                    }
                    let value = value
                        .as_ref()
                        .map(|e| self.expr(e, (*ty != Type::Unknown).then_some(ty)))
                        .transpose()?;
                    let ty = if *ty == Type::Unknown {
                        value
                            .map(|p| self.body.locals[p.0].ty.clone())
                            .unwrap_or(Type::Unknown)
                    } else {
                        ty.clone()
                    };
                    supported_type(self.program, &ty, span)?;
                    if ty == Type::Void {
                        return unsupported(span, "void binding is unsupported");
                    }
                    let target = self.bind(name, ty, *mutable, span)?;
                    if let Some(value) = value {
                        self.emit(
                            Operation::Assign {
                                target,
                                value: self.operand(value),
                            },
                            span,
                        );
                    }
                }
                StmtKind::Assign { target, op, value } => {
                    if op.is_some() {
                        return unsupported(span, "compound assignment is unsupported");
                    }
                    if matches!(&target.kind, ExprKind::Name(n) if n == "_") {
                        let value = self.expr(value, None)?;
                        self.emit(Operation::Cleanup(value), span);
                    } else {
                        let target = self.place(target, true)?;
                        let ty = self.body.locals[target.0].ty.clone();
                        let value = self.expr(value, Some(&ty))?;
                        self.emit(Operation::Cleanup(target), span);
                        self.emit(
                            Operation::Assign {
                                target,
                                value: self.operand(value),
                            },
                            span,
                        );
                    }
                }
                StmtKind::Expr(expression) => {
                    let value = self.expr(expression, None)?;
                    self.emit(Operation::Cleanup(value), span);
                }
                StmtKind::Return(expression) => {
                    let ret = self.function.ret.clone();
                    let value = expression
                        .as_ref()
                        .map(|e| self.expr(e, Some(&ret)))
                        .transpose()?;
                    if value.is_none() && ret != Type::Void {
                        return unsupported(span, "missing return value");
                    }
                    // The returned temporary survives cleanup until the return consumes it.
                    self.cleanup(0, value, span);
                    self.end(Terminator::Return(value.map(|p| self.operand(p))), span);
                }
                StmtKind::Block(block) => self.scoped(block, span)?,
                StmtKind::If {
                    condition,
                    then_block,
                    else_block,
                } => {
                    let condition = self.expr(condition, Some(&Type::Bool))?;
                    let yes = self.block_id();
                    let no = self.block_id();
                    let join = self.block_id();
                    self.end(
                        Terminator::Branch {
                            condition: self.operand(condition),
                            yes,
                            no,
                        },
                        span,
                    );
                    self.current = Some(yes);
                    self.scoped(then_block, span)?;
                    let then_continues = self.current.is_some();
                    if then_continues {
                        self.end(Terminator::Goto(join), span);
                    }
                    self.current = Some(no);
                    self.scoped(else_block, span)?;
                    let else_continues = self.current.is_some();
                    if else_continues {
                        self.end(Terminator::Goto(join), span);
                    }
                    self.current = (then_continues || else_continues).then_some(join);
                }
                StmtKind::For {
                    init: None,
                    condition,
                    step: None,
                    body,
                } => {
                    let header = self.block_id();
                    let run = self.block_id();
                    let exit = self.block_id();
                    self.end(Terminator::Goto(header), span);
                    self.current = Some(header);
                    if let Some(condition) = condition {
                        let condition = self.expr(condition, Some(&Type::Bool))?;
                        self.end(
                            Terminator::Branch {
                                condition: self.operand(condition),
                                yes: run,
                                no: exit,
                            },
                            span,
                        );
                    } else {
                        self.end(Terminator::Goto(run), span);
                    }
                    self.loops.push(Loop {
                        header,
                        exit,
                        depth: self.scopes.len(),
                    });
                    self.current = Some(run);
                    self.scoped(body, span)?;
                    if self.current.is_some() {
                        self.end(Terminator::Goto(header), span);
                    }
                    self.loops.pop();
                    self.current = Some(exit);
                }
                StmtKind::Break | StmtKind::Continue => {
                    let Some(loop_) = self.loops.last().copied() else {
                        return unsupported(span, "loop exit outside a loop");
                    };
                    self.cleanup(loop_.depth, None, span);
                    let target = if matches!(statement.kind, StmtKind::Break) {
                        loop_.exit
                    } else {
                        loop_.header
                    };
                    self.end(Terminator::Goto(target), span);
                }
                _ => {
                    return unsupported(
                        span,
                        "statement outside subset (patterns, foreach, for init/step, unsafe, or yield)",
                    );
                }
            }
        }
        Ok(())
    }

    /// Materialize every expression in order, including each call argument.
    /// This makes `take(s, s)` expose the second read after the first move.
    fn expr(&mut self, expression: &Expr, expected: Option<&Type>) -> Result<Place> {
        let span = expression.span;
        let result = match &expression.kind {
            ExprKind::Bool(_) => {
                let target = self.local("$bool", Type::Bool, span);
                self.emit(
                    Operation::Assign {
                        target,
                        value: Operand::Constant(Type::Bool),
                    },
                    span,
                );
                target
            }
            ExprKind::Int(number, suffix) => {
                let ty = suffix
                    .as_ref()
                    .or(expected)
                    .cloned()
                    .unwrap_or(Type::isize());
                let Type::Int { signed, bits } = ty else {
                    return unsupported(span, "integer literal requires integer type");
                };
                // Bound the adapter to 64-bit targets; production width handling is unchanged.
                let bits = if bits == 0 { 64 } else { bits };
                if !(1..=64).contains(&bits)
                    || u128::from(*number) >= (1u128 << (bits - u32::from(signed)))
                {
                    return unsupported(
                        span,
                        "integer literal outside 64-bit prototype target range",
                    );
                }
                let target = self.local("$integer", ty.clone(), span);
                self.emit(
                    Operation::Assign {
                        target,
                        value: Operand::Constant(ty),
                    },
                    span,
                );
                target
            }
            ExprKind::Name(_) => {
                let source = self.place(expression, false)?;
                let target = self.local("$value", self.body.locals[source.0].ty.clone(), span);
                self.emit(
                    Operation::Assign {
                        target,
                        value: self.operand(source),
                    },
                    span,
                );
                target
            }
            ExprKind::Unary(op @ (UnaryOp::Borrow | UnaryOp::BorrowMut), source) => {
                let mutable = *op == UnaryOp::BorrowMut;
                let source = self.place(source, mutable)?;
                let ty = Type::Ref(mutable, Box::new(self.body.locals[source.0].ty.clone()));
                supported_type(self.program, &ty, span)?;
                let target = self.local("$borrow", ty, span);
                self.emit(
                    Operation::Borrow {
                        target,
                        source,
                        mutable,
                    },
                    span,
                );
                target
            }
            ExprKind::Call {
                name,
                type_args,
                args,
            } if type_args.is_empty() => {
                let Some(function) = self.program.functions.iter().find(|f| &f.name == name) else {
                    return unsupported(span, "unresolved or intrinsic call is unsupported");
                };
                if args.len() != function.params.len() {
                    return unsupported(span, "call arity mismatch");
                }
                let mut operands = vec![];
                for (argument, parameter) in args.iter().zip(&function.params) {
                    if matches!(parameter.ty, Type::Ref(true, _))
                        && !matches!(argument.kind, ExprKind::Unary(UnaryOp::BorrowMut, _))
                    {
                        return unsupported(
                            argument.span,
                            "implicit mutable reborrow is unsupported",
                        );
                    }
                    let value = self.expr(argument, Some(&parameter.ty))?;
                    operands.push(self.operand(value));
                }
                let target = self.local("$call", function.ret.clone(), span);
                self.emit(
                    Operation::Call {
                        function: name.clone(),
                        args: operands,
                        target,
                    },
                    span,
                );
                target
            }
            _ => {
                return unsupported(
                    span,
                    "expression outside subset (operators, projections, aggregates, methods, casts, or propagation)",
                );
            }
        };
        if let Some(expected) = expected {
            self.expect(expected, &self.body.locals[result.0].ty, span)?;
        }
        Ok(result)
    }
}
