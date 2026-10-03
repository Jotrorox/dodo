//! A deliberately small source adapter, independent of sema and AST annotations.
//! Failure discards the entire body; unsupported nodes never become no-ops.
use super::*;
use crate::ast::{self, BinaryOp, Expr, ExprKind, Function, Pattern, Program, StmtKind, UnaryOp};
use std::collections::{HashMap, HashSet};
mod patterns;

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

/// Share program context; declaration restrictions apply only to bodies that use them.
pub(crate) struct Adapter<'a> {
    program: &'a Program,
    pointer_bits: u32,
}
impl<'a> Adapter<'a> {
    pub(crate) fn new(program: &'a Program, pointer_bits: u32) -> Result<Self> {
        if !matches!(pointer_bits, 32 | 64) {
            return limited(
                LimitationKind::Target,
                Span::default(),
                "unsupported target pointer width",
            );
        }
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

// Ambiguous declarations are a local limitation too: unrelated duplicates do
// not prevent lowering, but a referenced name must have exactly one meaning.
fn unique_declaration(program: &Program, name: &str) -> bool {
    program.structs.iter().filter(|s| s.name == name).count()
        + program.functions.iter().filter(|f| f.name == name).count()
        + program.enums.iter().filter(|e| e.name == name).count()
        + program.constants.iter().filter(|c| c.name == name).count()
        == 1
}

fn validate_struct(program: &Program, structure: &ast::Struct) -> Result<()> {
    plain_type(
        program,
        &Type::Named(structure.name.clone()),
        structure.span,
    )
}

// Check signatures at the body entry and at direct calls, without requiring
// the callee's body to lower (or recursively inspecting the call graph).
fn validate_function(program: &Program, function: &Function) -> Result<()> {
    // Package loading qualifies imported names as package.function (or
    // package.Struct.method). The package prefix is not a method boundary.
    let local_name = if function.imported {
        function
            .name
            .split_once('.')
            .map_or(function.name.as_str(), |(_, name)| name)
    } else {
        &function.name
    };
    if !unique_declaration(program, &function.name)
        || !function.generics.is_empty()
        || function.generic_instance
        || function.unsafe_
        || function.extern_
        || local_name.contains('.')
        || !function.from.is_empty()
        || !function.stores.is_empty()
        || !function.requires_plain.is_empty()
    {
        return limited(
            LimitationKind::FunctionDeclaration,
            function.span,
            "duplicate functions, generics, methods, destructors, FFI, and borrow contracts are unsupported",
        );
    }
    supported_type(program, &function.ret, function.ret_span)?;
    if matches!(function.ret, Type::Ref(..) | Type::Slice(..)) {
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
    Ok(())
}

fn lower_function(program: &Program, function: &Function, pointer_bits: u32) -> Result<Body> {
    validate_function(program, function)?;
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
        yields: vec![],
        current: Some(0),
        pointer_bits,
        guard: false,
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
    match ty {
        Type::Void => Ok(()),
        Type::Ref(_, inner) | Type::Slice(_, inner) => plain_type(program, inner, span),
        _ => plain_type(program, ty, span),
    }
}

// Owned aggregates deliberately exclude references and custom destructors.
// Their whole-slot moves are modeled; partial moves and native drops are not.
fn plain_type(program: &Program, ty: &Type, span: Span) -> Result<()> {
    validate_plain(program, ty, span, &mut HashSet::new())
}
fn validate_plain(
    program: &Program,
    ty: &Type,
    span: Span,
    visiting: &mut HashSet<String>,
) -> Result<()> {
    if scalar(ty) {
        return Ok(());
    }
    match ty {
        Type::Array(_, element) | Type::Option(element) => {
            validate_plain(program, element, span, visiting)
        }
        Type::Result(success, error) => {
            if **success != Type::Void {
                validate_plain(program, success, span, visiting)?;
            }
            validate_plain(program, error, span, visiting)
        }
        Type::Named(name) => {
            let (kind, declaration_span, generics) =
                if let Some(s) = program.structs.iter().find(|s| &s.name == name) {
                    (LimitationKind::StructDeclaration, s.span, &s.generics)
                } else if let Some(e) = program.enums.iter().find(|e| &e.name == name) {
                    (LimitationKind::Enums, e.span, &e.generics)
                } else {
                    return limited(LimitationKind::Type, span, "unknown aggregate type");
                };
            if !unique_declaration(program, name) || !generics.is_empty() {
                return limited(
                    kind,
                    declaration_span,
                    "duplicate or generic aggregates are unsupported",
                );
            }
            if !visiting.insert(name.clone()) {
                return limited(
                    kind,
                    declaration_span,
                    "recursive aggregates are unsupported",
                );
            }
            if let Some(drop) = program
                .functions
                .iter()
                .find(|f| f.name == format!("{name}.drop"))
            {
                return limited(
                    kind,
                    drop.span,
                    "aggregates with custom destructors are unsupported",
                );
            }
            let groups: Vec<_> = if let Some(s) = program.structs.iter().find(|s| &s.name == name) {
                vec![&s.fields]
            } else {
                let e = program.enums.iter().find(|e| &e.name == name).unwrap();
                let mut variants = HashSet::new();
                for variant in &e.variants {
                    if !variants.insert(&variant.name) {
                        return limited(
                            kind,
                            variant.span,
                            "duplicate enum variants are unsupported",
                        );
                    }
                }
                e.variants.iter().map(|v| &v.fields).collect()
            };
            for fields in groups {
                let mut seen = HashSet::new();
                for field in fields {
                    if !seen.insert(&field.name) {
                        return limited(
                            kind,
                            field.span,
                            "duplicate aggregate fields are unsupported",
                        );
                    }
                    if let Err(error) = validate_plain(program, &field.ty, field.span, visiting) {
                        return Err(if error.kind == LimitationKind::Type {
                            Limitation { kind, ..error }
                        } else {
                            error
                        });
                    }
                }
            }
            visiting.remove(name);
            Ok(())
        }
        _ => limited(
            LimitationKind::Type,
            span,
            "type is outside the scalar, plain aggregate, and direct-reference/slice subset",
        ),
    }
}

fn qualified_name(expression: &Expr) -> Option<String> {
    match &expression.kind {
        ExprKind::Name(name) => Some(name.clone()),
        ExprKind::Field(base, field) => Some(format!("{}.{field}", qualified_name(base)?)),
        _ => None,
    }
}

#[derive(Default)]
struct Scope {
    names: HashMap<String, (Place, bool)>,
    locals: Vec<Place>,
}
#[derive(Clone, Copy)]
struct Loop {
    continue_target: BlockId,
    exit: BlockId,
    depth: usize,
}
struct Yield {
    exit: BlockId,
    depth: usize,
    expected: Option<Type>,
    target: Option<Place>,
}
struct Lower<'a> {
    program: &'a Program,
    function: &'a Function,
    body: Body,
    scopes: Vec<Scope>,
    loops: Vec<Loop>,
    yields: Vec<Yield>,
    current: Option<BlockId>,
    pointer_bits: u32,
    guard: bool,
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

    fn tagged(&mut self, ty: Type, variant: &str, fields: Vec<Operand>, span: Span) -> Place {
        let target = self.local("$tagged", ty, span);
        self.emit(
            Operation::Tagged {
                target,
                variant: variant.into(),
                fields,
            },
            span,
        );
        target
    }

    fn variant_fields(&self, ty: &Type, name: &str, span: Span) -> Result<Vec<Type>> {
        let short = if let Some((owner, short)) = name.rsplit_once('.') {
            self.expect(&Type::Named(owner.into()), ty, span)?;
            short
        } else {
            name
        };
        let fields = match (ty, short) {
            (Type::Option(inner), "some") => Some(vec![*inner.clone()]),
            (Type::Option(_), "none") => Some(vec![]),
            (Type::Result(success, _), "ok") => Some(if **success == Type::Void {
                vec![]
            } else {
                vec![*success.clone()]
            }),
            (Type::Result(_, error), "err") => Some(vec![*error.clone()]),
            (Type::Named(owner), _) => self
                .program
                .enums
                .iter()
                .find(|e| &e.name == owner)
                .and_then(|e| e.variants.iter().find(|v| v.name == short))
                .map(|v| v.fields.iter().map(|f| f.ty.clone()).collect()),
            _ => None,
        };
        fields.ok_or_else(|| Limitation {
            kind: LimitationKind::UnsupportedConstruct,
            span,
            reason: format!("unknown variant `{name}` for `{ty}`"),
        })
    }

    fn constructor(
        &mut self,
        name: &str,
        args: &[Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Result<Place> {
        if name == "some" && expected.is_none() {
            let [argument] = args else {
                return unsupported(span, "some expects one argument");
            };
            let value = self.expr(argument, None)?;
            let ty = Type::Option(Box::new(self.body.locals[value.0].ty.clone()));
            plain_type(self.program, &ty, span)?;
            return Ok(self.tagged(ty, name, vec![self.operand(value)], span));
        }
        let ty = if matches!(name, "some" | "none" | "ok" | "err") {
            expected.cloned().ok_or_else(|| Limitation {
                kind: LimitationKind::Type,
                span,
                reason: "constructor requires a Result or Option context".into(),
            })?
        } else {
            Type::Named(name.rsplit_once('.').unwrap().0.into())
        };
        plain_type(self.program, &ty, span)?;
        let fields = self.variant_fields(&ty, name, span)?;
        if args.len() != fields.len() {
            return unsupported(span, "variant constructor arity mismatch");
        }
        let mut operands = vec![];
        for (argument, ty) in args.iter().zip(&fields) {
            let value = self.expr(argument, Some(ty))?;
            operands.push(self.operand(value));
        }
        Ok(self.tagged(ty, name.rsplit('.').next().unwrap(), operands, span))
    }

    fn integer_literal(
        &mut self,
        number: u64,
        suffix: Option<&Type>,
        expected: Option<&Type>,
        negative: bool,
        span: Span,
    ) -> Result<Place> {
        let ty = suffix.or(expected).cloned().unwrap_or(Type::isize());
        let Type::Int { signed, bits } = ty else {
            return unsupported(span, "integer literal requires integer type");
        };
        let bits = if bits == 0 { self.pointer_bits } else { bits };
        if !(1..=64).contains(&bits) {
            return unsupported(span, "integer literal outside target range");
        }
        let max = (1u128 << (bits - u32::from(signed))) - u128::from(!(signed && negative));
        if u128::from(number) > max {
            return unsupported(span, "integer literal outside target range");
        }
        Ok(self.constant(ty, span))
    }

    // Infer a range's common bound type without evaluating either bound or
    // relying on AST annotations. Actual lowering still validates every node.
    fn peek_type(&self, expression: &Expr) -> Option<Type> {
        match &expression.kind {
            ExprKind::Int(_, suffix) => suffix.clone(),
            ExprKind::Bool(_) => Some(Type::Bool),
            ExprKind::Name(_) => self
                .place(expression, false)
                .ok()
                .map(|p| self.body.locals[p.0].ty.clone()),
            ExprKind::Unary(UnaryOp::Deref, value) => match self.peek_type(value)? {
                Type::Ref(_, inner) => Some(*inner),
                _ => None,
            },
            ExprKind::Unary(op @ (UnaryOp::Borrow | UnaryOp::BorrowMut), value) => self
                .peek_type(value)
                .map(|ty| Type::Ref(*op == UnaryOp::BorrowMut, Box::new(ty))),
            ExprKind::Unary(_, value) => self.peek_type(value),
            ExprKind::Binary(op, left, right) => {
                if matches!(
                    op,
                    BinaryOp::Eq
                        | BinaryOp::Ne
                        | BinaryOp::Lt
                        | BinaryOp::Le
                        | BinaryOp::Gt
                        | BinaryOp::Ge
                        | BinaryOp::And
                        | BinaryOp::Or
                ) {
                    Some(Type::Bool)
                } else {
                    self.peek_type(left).or_else(|| self.peek_type(right))
                }
            }
            ExprKind::Field(base, field) => {
                let ty = self.peek_type(base)?;
                let ty = if let Type::Ref(_, inner) = &ty {
                    inner
                } else {
                    &ty
                };
                match ty {
                    Type::Array(..) | Type::Slice(..) if field == "len" => Some(Type::usize()),
                    Type::Named(name) => self
                        .program
                        .structs
                        .iter()
                        .find(|s| &s.name == name)?
                        .fields
                        .iter()
                        .find(|f| &f.name == field)
                        .map(|f| f.ty.clone()),
                    _ => None,
                }
            }
            ExprKind::Index(base, _) => {
                let ty = self.peek_type(base)?;
                let ty = if let Type::Ref(_, inner) = &ty {
                    inner
                } else {
                    &ty
                };
                match ty {
                    Type::Array(_, inner) | Type::Slice(_, inner) => Some(*inner.clone()),
                    _ => None,
                }
            }
            ExprKind::Call { name, .. } => self
                .program
                .functions
                .iter()
                .find(|f| &f.name == name)
                .map(|f| f.ret.clone()),
            ExprKind::Try(value) | ExprKind::Unwrap(value) => match self.peek_type(value)? {
                Type::Result(success, _) => Some(*success),
                _ => None,
            },
            _ => None,
        }
    }

    fn range_loop(
        &mut self,
        name: &str,
        start: &Expr,
        end: &Expr,
        body: &ast::Block,
        span: Span,
    ) -> Result<()> {
        let ty = self
            .peek_type(start)
            .or_else(|| self.peek_type(end))
            .unwrap_or(Type::isize());
        if !ty.is_integer() {
            return unsupported(span, "range bounds require integers of the same type");
        }
        self.scopes.push(Scope::default());
        // Allocate persistent slots before bound-expression temporaries. Bounds
        // execute once, left to right, outside every loop back edge.
        let counter = self.local("$range.index", ty.clone(), span);
        let upper = self.local("$range.end", ty.clone(), span);
        for (bound, target) in [(start, counter), (end, upper)] {
            let temporary_start = self.body.locals.len();
            let value = self.expr(bound, Some(&ty))?;
            self.emit(
                Operation::Assign {
                    target,
                    value: self.operand(value),
                },
                bound.span,
            );
            self.finish_temporaries(temporary_start, bound.span);
        }
        let header = self.block_id();
        let run = self.block_id();
        let step = self.block_id();
        let exit = self.block_id();
        self.end(Terminator::Goto(header), span);
        self.current = Some(header);
        let temporary_start = self.body.locals.len();
        // As for scalar binary expressions, record the reads and abstract away
        // the comparison value. Even literal empty ranges keep both edges.
        for source in [counter, upper] {
            let target = self.local("$value", ty.clone(), span);
            self.emit(
                Operation::Assign {
                    target,
                    value: Operand::Copy(source),
                },
                span,
            );
        }
        self.finish_temporaries(temporary_start, span);
        self.end(
            Terminator::Branch {
                condition: Operand::Constant(Type::Bool),
                yes: run,
                no: exit,
            },
            span,
        );
        self.loops.push(Loop {
            continue_target: step,
            exit,
            depth: self.scopes.len(),
        });
        self.current = Some(run);
        self.scopes.push(Scope::default());
        if name != "_" {
            let target = self.bind(name, ty.clone(), true, span)?;
            self.emit(
                Operation::Assign {
                    target,
                    value: Operand::Copy(counter),
                },
                span,
            );
        }
        self.statements(body)?;
        if self.current.is_some() {
            self.cleanup(self.scopes.len() - 1, None, span);
            self.end(Terminator::Goto(step), span);
        }
        self.scopes.pop();
        self.loops.pop();
        self.current = Some(step);
        // The increment is a pure scalar operation; only its read/write facts
        // matter. Continue reaches it after cleaning iteration-local bindings.
        self.emit(
            Operation::Assign {
                target: counter,
                value: Operand::Copy(counter),
            },
            span,
        );
        self.end(Terminator::Goto(header), span);
        self.current = Some(exit);
        self.cleanup(self.scopes.len() - 1, None, span);
        self.scopes.pop();
        Ok(())
    }

    // Projection roots are stable local identities; their address is captured
    // once, before any RHS evaluation. Aliasing/reservations remain in sema.
    fn destination(&mut self, expression: &Expr, write: bool) -> Result<Destination> {
        let (destination, mutable) = self.destination_access(expression)?;
        if write && !mutable {
            return unsupported(
                expression.span,
                "write through shared reference or immutable binding",
            );
        }
        Ok(destination)
    }

    // Resolve access once: recursively rechecking mutability would evaluate
    // index expressions twice. References grant access independently of the
    // mutability of the local holding them.
    fn destination_access(&mut self, expression: &Expr) -> Result<(Destination, bool)> {
        match &expression.kind {
            ExprKind::Name(_) => {
                let root = self.place(expression, false)?;
                let mutable = self.place(expression, true).is_ok();
                Ok((
                    Destination {
                        root,
                        projections: vec![],
                        ty: self.body.locals[root.0].ty.clone(),
                    },
                    mutable,
                ))
            }
            ExprKind::Unary(UnaryOp::Deref, base) => {
                let (mut destination, _) = self.destination_access(base)?;
                let Type::Ref(mutable, inner) = destination.ty else {
                    return unsupported(
                        expression.span,
                        "dereference requires a checked reference",
                    );
                };
                destination.ty = *inner;
                destination.projections.push(Projection::Deref);
                Ok((destination, mutable))
            }
            ExprKind::Field(base, name) => {
                let (mut destination, mutable) = self.autoderef_destination(base)?;
                if name == "len" && matches!(destination.ty, Type::Array(..) | Type::Slice(..)) {
                    destination.ty = Type::usize();
                    destination
                        .projections
                        .push(Projection::Field(name.clone()));
                    return Ok((destination, false));
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
                Ok((destination, mutable))
            }
            ExprKind::Index(base, index) => {
                let (destination, mutable) = self.autoderef_destination(base)?;
                let (element, mutable) = match &destination.ty {
                    Type::Array(length, element) => {
                        if let ExprKind::Int(index, _) = index.kind
                            && index >= *length as u64
                        {
                            return unsupported(
                                expression.span,
                                "constant index outside array length",
                            );
                        }
                        (*element.clone(), mutable)
                    }
                    Type::Slice(mutable, element) => (*element.clone(), *mutable),
                    _ => {
                        return unsupported(expression.span, "indexing requires an array or slice");
                    }
                };
                // Capture/read the base before the index. A side effect in the
                // index cannot retroactively initialize it. The captured address
                // survives until the eventual load/store; sema owns reservations.
                let address = self.capture(destination, base.span);
                let index = self.index_value(index)?;
                Ok((
                    Destination {
                        root: address,
                        projections: vec![Projection::Deref, Projection::Index(index)],
                        ty: element,
                    },
                    mutable,
                ))
            }
            _ => unsupported(expression.span, "assignment destination outside subset"),
        }
    }
    fn autoderef_destination(&mut self, expression: &Expr) -> Result<(Destination, bool)> {
        let (mut destination, mut mutable) = self.destination_access(expression)?;
        if let Type::Ref(access, inner) = destination.ty {
            mutable = access;
            destination.ty = *inner;
            destination.projections.push(Projection::Deref);
        }
        Ok((destination, mutable))
    }
    fn index_value(&mut self, expression: &Expr) -> Result<Place> {
        // A suffix or local's actual integer width need not be usize.
        let hint = matches!(expression.kind, ExprKind::Int(_, None)).then(Type::usize);
        let value = self.expr(expression, hint.as_ref())?;
        if !self.body.locals[value.0].ty.is_integer() {
            return unsupported(
                expression.span,
                "index or slice bound requires integer type",
            );
        }
        Ok(value)
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
                    if self.guard {
                        return unsupported(span, "assignments in pattern guards are unsupported");
                    }
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
                StmtKind::Yield(expression) => {
                    let Some(context) = self.yields.last() else {
                        return unsupported(span, "yield outside a value block");
                    };
                    let (depth, exit, target, expected) = (
                        context.depth,
                        context.exit,
                        context.target,
                        context.expected.clone(),
                    );
                    let value = self.expr(expression, expected.as_ref())?;
                    let ty = self.body.locals[value.0].ty.clone();
                    let target = if let Some(target) = target {
                        self.expect(&self.body.locals[target.0].ty, &ty, span)?;
                        target
                    } else {
                        let target = self.local("$yield", ty, span);
                        self.scopes.last_mut().unwrap().locals.pop();
                        self.scopes[depth - 1].locals.push(target);
                        self.yields.last_mut().unwrap().target = Some(target);
                        target
                    };
                    self.emit(
                        Operation::Assign {
                            target,
                            value: self.operand(value),
                        },
                        span,
                    );
                    self.cleanup(depth, None, span);
                    self.end(Terminator::Goto(exit), span);
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
                        continue_target: header,
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
                        loop_.continue_target
                    };
                    self.end(Terminator::Goto(target), span);
                }
                StmtKind::ForEach {
                    index,
                    name,
                    copy,
                    iterable,
                    body,
                } => {
                    let ExprKind::Range(start, end) = &iterable.kind else {
                        return limited(
                            LimitationKind::Statement,
                            span,
                            "statement outside subset (collection foreach iteration)",
                        );
                    };
                    if *copy || index.is_some() {
                        return limited(
                            LimitationKind::Statement,
                            span,
                            "range loops require one integer value binding",
                        );
                    }
                    self.range_loop(name, start, end, body, span)?;
                }
                StmtKind::Match { value, arms } => self.match_statement(value, arms, span)?,
                StmtKind::IfLet {
                    pattern,
                    value,
                    then_block,
                    else_block,
                } => self.if_let(pattern, value, then_block, else_block, span)?,
                StmtKind::LetPattern {
                    pattern,
                    ty,
                    value,
                    else_block,
                } => self.let_pattern(pattern, ty, value, else_block.as_ref(), span)?,
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
                self.integer_literal(*number, suffix.as_ref(), expected, false, span)?
            }
            ExprKind::Name(name) if name == "none" => {
                self.constructor(name, &[], expected, span)?
            }
            ExprKind::Name(_) => {
                let source = self.place(expression, false)?;
                if self.guard && !self.body.locals[source.0].ty.is_copy() {
                    return unsupported(span, "moves in pattern guards are unsupported");
                }
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
                if block.is_empty() {
                    return unsupported(span, "empty value block");
                }
                let depth = self.scopes.len();
                let exit = self.block_id();
                self.yields.push(Yield {
                    exit,
                    depth,
                    expected: expected.cloned(),
                    target: None,
                });
                self.scopes.push(Scope::default());
                self.statements(block)?;
                if self.current.is_some() {
                    return unsupported(span, "value block path without a yield");
                }
                self.scopes.pop();
                let Some(target) = self.yields.pop().unwrap().target else {
                    return unsupported(span, "diverging value block is unsupported");
                };
                self.current = Some(exit);
                target
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
            ExprKind::Unary(op @ (UnaryOp::Neg | UnaryOp::Not | UnaryOp::BitNot), value) => {
                let value = if *op == UnaryOp::Neg
                    && let ExprKind::Int(number, suffix) = &value.kind
                {
                    self.integer_literal(*number, suffix.as_ref(), expected, true, span)?
                } else {
                    self.expr(value, expected)?
                };
                let ty = self.body.locals[value.0].ty.clone();
                if !matches!(
                    (op, &ty),
                    (UnaryOp::Not, Type::Bool) | (UnaryOp::Neg, Type::Int { signed: true, .. })
                ) && !(*op == UnaryOp::BitNot && ty.is_integer())
                {
                    return unsupported(span, "unary operator type mismatch");
                }
                self.constant(ty, span)
            }
            ExprKind::Field(base, variant)
                if qualified_name(base)
                    .is_some_and(|name| self.program.enums.iter().any(|e| e.name == name)) =>
            {
                let name = format!("{}.{variant}", qualified_name(base).unwrap());
                self.constructor(&name, &[], expected, span)?
            }
            ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Unary(UnaryOp::Deref, _) => {
                let destination = self.destination(expression, false)?;
                if !destination.ty.is_copy() {
                    return unsupported(span, "moves out of projected storage are unsupported");
                }
                let address = self.capture(destination, span);
                self.load(address, span)
            }
            ExprKind::Array(ty, items) => {
                let Type::Array(length, declared) = ty else {
                    return unsupported(span, "unresolved array length");
                };
                if items.len() != *length {
                    return unsupported(span, "array literal length mismatch");
                }
                let mut element = if **declared != Type::Unknown {
                    Some(*declared.clone())
                } else if let Some(Type::Array(_, element)) = expected {
                    Some(*element.clone())
                } else {
                    items.iter().find_map(|item| match &item.kind {
                        ExprKind::Int(_, Some(ty)) => Some(ty.clone()),
                        ExprKind::Bool(_) => Some(Type::Bool),
                        ExprKind::Name(_) => self
                            .place(item, false)
                            .ok()
                            .map(|p| self.body.locals[p.0].ty.clone()),
                        ExprKind::Struct(name, _) => Some(Type::Named(name.clone())),
                        _ => None,
                    })
                };
                let mut elements = vec![];
                for item in items {
                    let value = self.expr(item, element.as_ref())?;
                    elements.push(self.operand(value));
                    element = Some(self.body.locals[value.0].ty.clone());
                }
                let Some(element) = element else {
                    return unsupported(span, "empty array needs an element type");
                };
                plain_type(self.program, &element, span)?;
                let target = self.local("$array", Type::Array(*length, Box::new(element)), span);
                self.emit(Operation::Aggregate { target, elements }, span);
                target
            }
            ExprKind::Slice {
                base,
                start,
                end,
                mutable,
            } => {
                let (destination, access) = self.autoderef_destination(base)?;
                let (element, access) = match &destination.ty {
                    Type::Array(_, element) => (*element.clone(), access),
                    Type::Slice(access, element) => (*element.clone(), *access),
                    _ => return unsupported(span, "slicing requires an array or slice"),
                };
                if *mutable && !access {
                    return unsupported(
                        span,
                        "mutable slice through shared reference or immutable binding",
                    );
                }
                self.capture(destination, base.span);
                for bound in start.iter().chain(end) {
                    self.index_value(bound)?;
                }
                // Only availability is represented. Slice provenance, bounds,
                // aliasing, and reservation conflicts remain with sema/codegen.
                self.constant(Type::Slice(*mutable, Box::new(element)), span)
            }
            ExprKind::Struct(name, fields) => {
                let Some(structure) = self.program.structs.iter().find(|s| &s.name == name) else {
                    return unsupported(span, "unknown struct");
                };
                validate_struct(self.program, structure)?;
                let mut seen = std::collections::HashSet::new();
                let mut elements = vec![];
                for (name, value) in fields {
                    let Some(field) = structure.fields.iter().find(|f| &f.name == name) else {
                        return unsupported(value.span, "unknown struct field");
                    };
                    if !seen.insert(name) {
                        return unsupported(value.span, "duplicate struct field");
                    }
                    let value = self.expr(value, Some(&field.ty))?;
                    elements.push(self.operand(value));
                }
                if seen.len() != structure.fields.len() {
                    return unsupported(span, "missing struct field");
                }
                let target = self.local("$struct", Type::Named(name.clone()), span);
                self.emit(Operation::Aggregate { target, elements }, span);
                target
            }
            ExprKind::Unary(op @ (UnaryOp::Borrow | UnaryOp::BorrowMut), source) => {
                let mutable = *op == UnaryOp::BorrowMut;
                if self.guard && mutable {
                    return unsupported(span, "mutable borrows in pattern guards are unsupported");
                }
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
            } if type_args.is_empty()
                && (matches!(name.as_str(), "ok" | "err" | "some" | "none")
                    || name.rsplit_once('.').is_some_and(|(owner, _)| {
                        self.program.enums.iter().any(|e| e.name == owner)
                    })) =>
            {
                self.constructor(name, args, expected, span)?
            }
            ExprKind::MethodCall {
                receiver,
                name,
                args,
            } if qualified_name(receiver)
                .is_some_and(|owner| self.program.enums.iter().any(|e| e.name == owner)) =>
            {
                let name = format!("{}.{name}", qualified_name(receiver).unwrap());
                self.constructor(&name, args, expected, span)?
            }
            ExprKind::Try(value) | ExprKind::Unwrap(value) => {
                if self.guard {
                    return unsupported(span, "Result exits in pattern guards are unsupported");
                }
                let source = self.expr(value, None)?;
                let Type::Result(success, error) = self.body.locals[source.0].ty.clone() else {
                    return unsupported(span, "Result propagation requires a Result operand");
                };
                let propagate = matches!(expression.kind, ExprKind::Try(_));
                if propagate {
                    let Type::Result(_, expected_error) = &self.function.ret else {
                        return unsupported(span, "propagation requires a Result return type");
                    };
                    self.expect(expected_error, &error, span)?;
                }
                let yes = self.block_id();
                let no = self.block_id();
                self.test_branch(
                    source,
                    &[],
                    PatternTest::Variant("ok".into()),
                    yes,
                    no,
                    span,
                );
                self.current = Some(no);
                if propagate {
                    let payload = self.local("$error", *error, span);
                    self.emit(
                        Operation::PatternBind {
                            source: self.operand(source),
                            bindings: vec![PayloadBinding {
                                target: payload,
                                projections: vec![PatternProjection::Payload {
                                    variant: "err".into(),
                                    index: 0,
                                }],
                            }],
                            mode: PatternMode::Owned,
                        },
                        span,
                    );
                    let returned = self.tagged(
                        self.function.ret.clone(),
                        "err",
                        vec![self.operand(payload)],
                        span,
                    );
                    self.cleanup(0, Some(returned), span);
                    self.end(Terminator::Return(Some(self.operand(returned))), span);
                } else {
                    // Unwrap panics without unwinding or scope cleanup.
                    self.end(Terminator::Unreachable, span);
                }
                self.current = Some(yes);
                let target = self.local("$success", *success.clone(), span);
                self.emit(
                    Operation::PatternBind {
                        source: self.operand(source),
                        bindings: if *success == Type::Void {
                            vec![]
                        } else {
                            vec![PayloadBinding {
                                target,
                                projections: vec![PatternProjection::Payload {
                                    variant: "ok".into(),
                                    index: 0,
                                }],
                            }]
                        },
                        mode: PatternMode::Owned,
                    },
                    span,
                );
                if *success == Type::Void {
                    self.emit(
                        Operation::Assign {
                            target,
                            value: Operand::Constant(Type::Void),
                        },
                        span,
                    );
                }
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
                validate_function(self.program, function)?;
                if args.len() != function.params.len() {
                    return unsupported(span, "call arity mismatch");
                }
                let mut operands = vec![];
                for (argument, parameter) in args.iter().zip(&function.params) {
                    if matches!(parameter.ty, Type::Ref(true, _) | Type::Slice(true, _))
                        && !matches!(
                            argument.kind,
                            ExprKind::Unary(UnaryOp::BorrowMut, _)
                                | ExprKind::Slice { mutable: true, .. }
                        )
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
