//! A deliberately small source adapter, independent of sema and AST annotations.
//! Failure discards the entire body; unsupported nodes never become no-ops.
use super::*;
use crate::ast::{self, BinaryOp, Expr, ExprKind, Function, Program, StmtKind, UnaryOp};
use std::collections::HashMap;

/// Machine-readable fallback families; messages retain the specific restriction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LimitationKind {
    Target,
    Imports,
    Constants,
    Enums,
    StructDeclaration,
    FunctionDeclaration,
    Type,
    Statement,
    Expression,
    UnsupportedConstruct,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limitation {
    pub kind: LimitationKind,
    pub span: Span,
    pub reason: String,
}
type Result<T> = std::result::Result<T, Limitation>;
fn unsupported<T>(span: Span, reason: &str) -> Result<T> {
    limited(LimitationKind::UnsupportedConstruct, span, reason)
}
fn limited<T>(kind: LimitationKind, span: Span, reason: &str) -> Result<T> {
    Err(Limitation {
        kind,
        span,
        reason: reason.into(),
    })
}

/// Lower only the named body. Other bodies are not certified by its result.
/// Signatures are checked, but calls assume normal return and no hidden moves.
/// Neither this adapter nor initialization() validates borrowing or full Dodo.
pub fn lower(program: &Program, name: &str) -> Result<Body> {
    lower_for_target(program, name, 64)
}

pub fn lower_for_target(program: &Program, name: &str, pointer_bits: u32) -> Result<Body> {
    Adapter::new(program, pointer_bits)?.lower(name)
}

/// Validate the declaration subset once when checking a complete program.
pub(crate) struct Adapter<'a> {
    program: &'a Program,
    pointer_bits: u32,
}
impl<'a> Adapter<'a> {
    pub(crate) fn new(program: &'a Program, pointer_bits: u32) -> Result<Self> {
        validate(program, pointer_bits)?;
        Ok(Self {
            program,
            pointer_bits,
        })
    }
    fn lower(&self, name: &str) -> Result<Body> {
        let function = self
            .program
            .functions
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| Limitation {
                kind: LimitationKind::FunctionDeclaration,
                span: Span::default(),
                reason: format!("missing function `{name}`"),
            })?;
        self.lower_function(function)
    }
    pub(crate) fn lower_function(&self, function: &Function) -> Result<Body> {
        lower_function(self.program, function, self.pointer_bits)
    }
}

fn validate(program: &Program, pointer_bits: u32) -> Result<()> {
    if !matches!(pointer_bits, 32 | 64) {
        return limited(
            LimitationKind::Target,
            Span::default(),
            "unsupported target pointer width",
        );
    }
    if !program.imports.is_empty() {
        // Program currently stores import names, but not their source spans.
        return limited(
            LimitationKind::Imports,
            Span::default(),
            "imports are unsupported",
        );
    }
    if let Some(constant) = program.constants.first() {
        return limited(
            LimitationKind::Constants,
            constant.span,
            "globals are unsupported",
        );
    }
    if let Some(enumeration) = program.enums.first() {
        return limited(
            LimitationKind::Enums,
            enumeration.span,
            "enums are unsupported",
        );
    }
    let mut names = std::collections::HashSet::new();
    for structure in &program.structs {
        if !names.insert(&structure.name) || !structure.generics.is_empty() {
            return limited(
                LimitationKind::StructDeclaration,
                structure.span,
                "duplicate or generic structs are unsupported",
            );
        }
        let mut fields = std::collections::HashSet::new();
        for field in &structure.fields {
            if !fields.insert(&field.name) || !scalar(&field.ty) {
                return limited(
                    LimitationKind::StructDeclaration,
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
            return limited(
                LimitationKind::FunctionDeclaration,
                function.span,
                "generics, methods, destructors, FFI, and borrow contracts are unsupported",
            );
        }
        supported_type(program, &function.ret, function.ret_span)?;
        if matches!(function.ret, Type::Ref(..)) {
            return limited(
                LimitationKind::FunctionDeclaration,
                function.ret_span,
                "borrowed returns are unsupported",
            );
        }
        for param in &function.params {
            supported_type(program, &param.ty, param.span)?;
            if param.ty == Type::Void {
                return limited(
                    LimitationKind::FunctionDeclaration,
                    param.span,
                    "void parameters are unsupported",
                );
            }
        }
    }
    Ok(())
}

fn lower_function(program: &Program, function: &Function, pointer_bits: u32) -> Result<Body> {
    let body = function.body.as_ref().ok_or_else(|| Limitation {
        kind: LimitationKind::FunctionDeclaration,
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
        pointer_bits,
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
        limited(
            LimitationKind::Type,
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
    pointer_bits: u32,
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
    fn constant(&mut self, ty: Type, span: Span) -> Place {
        let target = self.local("$computed", ty.clone(), span);
        self.emit(
            Operation::Assign {
                target,
                value: Operand::Constant(ty),
            },
            span,
        );
        target
    }

    // Projection roots are stable local identities; their address is captured
    // once, before any RHS evaluation. Aliasing/reservations remain in sema.
    fn destination(&self, expression: &Expr, write: bool) -> Result<Destination> {
        match &expression.kind {
            ExprKind::Name(_) => {
                let root = self.place(expression, write)?;
                Ok(Destination {
                    root,
                    projections: vec![],
                    ty: self.body.locals[root.0].ty.clone(),
                })
            }
            ExprKind::Unary(UnaryOp::Deref, base) => {
                let mut destination = self.destination(base, false)?;
                let Type::Ref(mutable, inner) = destination.ty else {
                    return unsupported(
                        expression.span,
                        "dereference requires a checked reference",
                    );
                };
                if write && !mutable {
                    return unsupported(expression.span, "write through shared reference");
                }
                destination.ty = *inner;
                destination.projections.push(Projection::Deref);
                Ok(destination)
            }
            ExprKind::Field(base, name) => {
                let mut destination = self.destination(base, false)?;
                if matches!(destination.ty, Type::Ref(..)) {
                    let Type::Ref(mutable, inner) = destination.ty else {
                        unreachable!()
                    };
                    if write && !mutable {
                        return unsupported(expression.span, "write through shared reference");
                    }
                    destination.ty = *inner;
                    destination.projections.push(Projection::Deref);
                } else if write {
                    // Enforce binding mutability when no reference grants access.
                    self.destination(base, true)?;
                }
                let Type::Named(structure) = &destination.ty else {
                    return unsupported(expression.span, "field requires a struct");
                };
                let Some(field) = self
                    .program
                    .structs
                    .iter()
                    .find(|s| &s.name == structure)
                    .and_then(|s| s.fields.iter().find(|f| &f.name == name))
                else {
                    return unsupported(expression.span, "unknown struct field");
                };
                destination.ty = field.ty.clone();
                destination
                    .projections
                    .push(Projection::Field(name.clone()));
                Ok(destination)
            }
            _ => unsupported(expression.span, "assignment destination outside subset"),
        }
    }
    fn capture(&mut self, destination: Destination, span: Span) -> Place {
        let target = self.local(
            "$address",
            Type::Ref(true, Box::new(destination.ty.clone())),
            span,
        );
        self.emit(
            Operation::Capture {
                target,
                destination,
            },
            span,
        );
        target
    }
    fn load(&mut self, address: Place, span: Span) -> Place {
        let Type::Ref(_, ty) = &self.body.locals[address.0].ty else {
            unreachable!()
        };
        let target = self.local("$loaded", *ty.clone(), span);
        self.emit(Operation::Load { target, address }, span);
        target
    }
    fn binary_type(&self, op: BinaryOp, ty: &Type, span: Span) -> Result<Type> {
        use BinaryOp::*;
        match op {
            And | Or if *ty == Type::Bool => Ok(Type::Bool),
            Eq | Ne if scalar(ty) => Ok(Type::Bool),
            Lt | Le | Gt | Ge if ty.is_integer() => Ok(Type::Bool),
            Add | Sub | Mul | Div | Rem | BitAnd | BitOr | BitXor | Shl | Shr
                if ty.is_integer() =>
            {
                Ok(ty.clone())
            }
            _ => unsupported(span, "operator type mismatch"),
        }
    }
    fn expect(&self, expected: &Type, actual: &Type, span: Span) -> Result<()> {
        if expected == actual {
            Ok(())
        } else {
            Err(Limitation {
                kind: LimitationKind::Type,
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
    fn finish_temporaries(&mut self, start: usize, span: Span) {
        let mut departing = Vec::new();
        for scope in &mut self.scopes {
            scope.locals.retain(|place| {
                let temporary = place.0 >= start && self.body.locals[place.0].name.starts_with('$');
                if temporary {
                    departing.push(*place);
                }
                !temporary
            });
        }
        departing.sort_by_key(|place| std::cmp::Reverse(place.0));
        for place in departing {
            self.emit(Operation::Cleanup(place), span);
        }
    }
    fn condition(&mut self, expression: &Expr) -> Result<Operand> {
        let start = self.body.locals.len();
        self.expr(expression, Some(&Type::Bool))?;
        self.finish_temporaries(start, expression.span);
        // Values are abstract: the expression's reads/moves have happened, and
        // both Boolean successors remain possible even for literal conditions.
        Ok(Operand::Constant(Type::Bool))
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
    fn statements(&mut self, block: &[ast::Stmt]) -> Result<()> {
        for statement in block {
            if self.current.is_none() {
                return unsupported(
                    statement.span,
                    "syntax after a terminating statement is unsupported",
                );
            }
            let span = statement.span;
            let temporary_start = self.body.locals.len();
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
                    if matches!(&target.kind, ExprKind::Name(n) if n == "_") {
                        if op.is_some() {
                            return unsupported(span, "compound discard is unsupported");
                        }
                        let value = self.expr(value, None)?;
                        self.emit(Operation::Cleanup(value), span);
                    } else {
                        let destination = self.destination(target, true)?;
                        let ty = destination.ty.clone();
                        let root = destination.root;
                        let address = if destination.projections.is_empty() {
                            None
                        } else {
                            Some(self.capture(destination, target.span))
                        };
                        if let Some(op) = op {
                            self.binary_type(*op, &ty, span)?;
                            // Read the previous value before evaluating the RHS.
                            if let Some(address) = address {
                                self.load(address, target.span);
                            } else {
                                self.expr(target, Some(&ty))?;
                            }
                        }
                        let value = self.expr(value, Some(&ty))?;
                        if let Some(address) = address {
                            self.emit(
                                Operation::Store {
                                    address,
                                    value: self.operand(value),
                                },
                                span,
                            );
                        } else {
                            self.emit(Operation::Cleanup(root), span);
                            self.emit(
                                Operation::Assign {
                                    target: root,
                                    value: self.operand(value),
                                },
                                span,
                            );
                        }
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
                    let condition = self.condition(condition)?;
                    let yes = self.block_id();
                    let no = self.block_id();
                    let join = self.block_id();
                    self.end(Terminator::Branch { condition, yes, no }, span);
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
                        let condition = self.condition(condition)?;
                        self.end(
                            Terminator::Branch {
                                condition,
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
                    return limited(
                        LimitationKind::Statement,
                        span,
                        "statement outside subset (patterns, foreach, for init/step, unsafe, or yield)",
                    );
                }
            }
            if self.current.is_some() {
                self.finish_temporaries(temporary_start, span);
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
                let bits = if bits == 0 { self.pointer_bits } else { bits };
                if !(1..=64).contains(&bits)
                    || u128::from(*number) >= (1u128 << (bits - u32::from(signed)))
                {
                    return unsupported(span, "integer literal outside target range");
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
            ExprKind::ValueBlock(block) => {
                let Some((last, statements)) = block.split_last() else {
                    return unsupported(span, "empty value block");
                };
                let StmtKind::Yield(value) = &last.kind else {
                    return unsupported(span, "value block requires a final yield");
                };
                self.scopes.push(Scope::default());
                self.statements(statements)?;
                if self.current.is_none() {
                    return unsupported(span, "diverging value block is unsupported");
                }
                let value = self.expr(value, expected)?;
                // The yielded temporary leaves this scope with its value intact.
                self.cleanup(self.scopes.len() - 1, Some(value), span);
                self.scopes.pop();
                self.scopes.last_mut().unwrap().locals.push(value);
                value
            }
            ExprKind::Binary(op @ (BinaryOp::And | BinaryOp::Or), left, right) => {
                let left = self.expr(left, Some(&Type::Bool))?;
                let target = self.local("$logical", Type::Bool, span);
                let evaluate = self.block_id();
                let skipped = self.block_id();
                let join = self.block_id();
                let (yes, no) = if *op == BinaryOp::And {
                    (evaluate, skipped)
                } else {
                    (skipped, evaluate)
                };
                self.end(
                    Terminator::Branch {
                        condition: self.operand(left),
                        yes,
                        no,
                    },
                    span,
                );
                self.current = Some(evaluate);
                let right = self.expr(right, Some(&Type::Bool))?;
                self.emit(
                    Operation::Assign {
                        target,
                        value: self.operand(right),
                    },
                    span,
                );
                self.end(Terminator::Goto(join), span);
                self.current = Some(skipped);
                self.emit(
                    Operation::Assign {
                        target,
                        value: Operand::Constant(Type::Bool),
                    },
                    span,
                );
                self.end(Terminator::Goto(join), span);
                self.current = Some(join);
                target
            }
            ExprKind::Binary(op, left, right) => {
                let left = self.expr(left, expected.filter(|ty| ty.is_integer()))?;
                let ty = self.body.locals[left.0].ty.clone();
                self.expr(right, Some(&ty))?;
                let result_ty = self.binary_type(*op, &ty, span)?;
                self.constant(result_ty, span)
            }
            ExprKind::Unary(op @ (UnaryOp::Not | UnaryOp::BitNot), value) => {
                let value = self.expr(value, expected)?;
                let ty = self.body.locals[value.0].ty.clone();
                if !matches!((op, &ty), (UnaryOp::Not, Type::Bool))
                    && !(*op == UnaryOp::BitNot && ty.is_integer())
                {
                    return unsupported(span, "unary operator type mismatch");
                }
                self.constant(ty, span)
            }
            ExprKind::Field(..) | ExprKind::Unary(UnaryOp::Deref, _) => {
                let destination = self.destination(expression, false)?;
                if !destination.ty.is_copy() {
                    return unsupported(span, "moves out of projected storage are unsupported");
                }
                let address = self.capture(destination, span);
                self.load(address, span)
            }
            ExprKind::Struct(name, fields) => {
                let Some(structure) = self.program.structs.iter().find(|s| &s.name == name) else {
                    return unsupported(span, "unknown struct");
                };
                let mut seen = std::collections::HashSet::new();
                for (name, value) in fields {
                    let Some(field) = structure.fields.iter().find(|f| &f.name == name) else {
                        return unsupported(value.span, "unknown struct field");
                    };
                    if !seen.insert(name) {
                        return unsupported(value.span, "duplicate struct field");
                    }
                    self.expr(value, Some(&field.ty))?;
                }
                if seen.len() != structure.fields.len() {
                    return unsupported(span, "missing struct field");
                }
                self.constant(Type::Named(name.clone()), span)
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
                return limited(
                    LimitationKind::Expression,
                    span,
                    "expression outside subset (unsupported operator, projection, aggregate, method, cast, or propagation)",
                );
            }
        };
        if let Some(expected) = expected {
            self.expect(expected, &self.body.locals[result.0].ty, span)?;
        }
        Ok(result)
    }
}
