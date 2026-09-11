//! Resolve constant dependencies and array lengths before generic specialization.
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::{HashMap, HashSet};

type Check<T> = Result<T, Diagnostic>;
type Locals = HashMap<String, Option<(Type, Expr)>>;

pub fn prepare(program: &mut Program, bits: u32) -> Check<()> {
    let mut resolver = Resolver {
        globals: HashMap::new(),
        ready: HashMap::new(),
        sizes: HashMap::new(),
        active: HashSet::new(),
        bits,
        nodes: 0,
    };
    for constant in &program.constants {
        if resolver
            .globals
            .insert(constant.name.clone(), constant.clone())
            .is_some()
        {
            return Err(Diagnostic::new(
                constant.span,
                format!("duplicate declaration `{}`", constant.name),
            ));
        }
    }
    for constant in &mut program.constants {
        *constant = resolver.global(&constant.name, constant.span)?;
    }
    for structure in &mut program.structs {
        for field in &mut structure.fields {
            resolver.ty(
                &mut field.ty,
                &Locals::new(),
                namespace(&structure.name),
                field.span,
            )?;
        }
    }
    for enumeration in &mut program.enums {
        for variant in &mut enumeration.variants {
            for field in &mut variant.fields {
                resolver.ty(
                    &mut field.ty,
                    &Locals::new(),
                    namespace(&enumeration.name),
                    field.span,
                )?;
            }
        }
    }
    for function in &mut program.functions {
        let owner = function.name.rsplit_once('.').map(|(owner, _)| owner);
        let ns = owner
            .and_then(|owner| program.structs.iter().find(|s| s.name == owner))
            .map_or_else(|| namespace(&function.name), |s| namespace(&s.name));
        let mut locals = Locals::new();
        for parameter in &mut function.params {
            resolver.ty(&mut parameter.ty, &locals, ns, parameter.span)?;
            locals.insert(parameter.name.clone(), None);
        }
        resolver.ty(&mut function.ret, &locals, ns, function.span)?;
        if let Some(body) = &mut function.body {
            resolver.block(body, &mut locals, ns)?;
        }
    }
    Ok(())
}
fn namespace(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(n, _)| n)
}
fn path(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Name(n) => Some(n.clone()),
        ExprKind::Field(e, n) => Some(format!("{}.{n}", path(e)?)),
        _ => None,
    }
}
struct Resolver {
    globals: HashMap<String, Constant>,
    ready: HashMap<String, Constant>,
    sizes: HashMap<String, usize>,
    active: HashSet<String>,
    bits: u32,
    nodes: usize,
}
impl Resolver {
    fn global(&mut self, name: &str, span: Span) -> Check<Constant> {
        if let Some(c) = self.ready.get(name) {
            self.nodes = self.nodes.saturating_add(self.sizes[name]);
            if self.nodes > 200_000 {
                return Err(Diagnostic::new(
                    span,
                    "constant expansion exceeds the supported size limit",
                ));
            }
            return Ok(c.clone());
        }
        let initial_nodes = self.nodes;
        if self.active.len() >= 128 || !self.active.insert(name.into()) {
            return Err(Diagnostic::new(
                span,
                format!("cyclic or excessively nested constant dependency involving `{name}`"),
            ));
        }
        let mut c = self.globals[name].clone();
        self.ty(&mut c.ty, &Locals::new(), namespace(name), c.span)?;
        self.expr(&mut c.value, &Locals::new(), namespace(name), true)?;
        self.active.remove(name);
        self.sizes
            .insert(name.into(), self.nodes - initial_nodes + 1);
        self.ready.insert(name.into(), c.clone());
        Ok(c)
    }
    fn ty(&mut self, ty: &mut Type, locals: &Locals, ns: &str, span: Span) -> Check<()> {
        match ty {
            Type::ArrayExpr(length, element) => {
                let mut value = length.expression(span);
                self.expr(&mut value, locals, ns, true)?;
                let length = crate::sema::array_length(&mut value, self.bits)?;
                self.ty(element, locals, ns, span)?;
                *ty = Type::Array(length, element.clone());
            }
            Type::Array(n, t) => {
                if *n as u128 > u32::MAX as u128 || *n as u128 >= (1u128 << self.bits) {
                    return Err(Diagnostic::new(
                        span,
                        "array length must fit the target and LLVM array limit",
                    ));
                }
                self.ty(t, locals, ns, span)?;
            }
            Type::Ref(_, t)
            | Type::Slice(_, t)
            | Type::Raw(_, t)
            | Type::Option(t)
            | Type::MaybeUninit(t) => self.ty(t, locals, ns, span)?,
            Type::Result(t, e) => {
                self.ty(t, locals, ns, span)?;
                self.ty(e, locals, ns, span)?;
            }
            Type::Generic(_, args) => {
                for arg in args {
                    self.ty(arg, locals, ns, span)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn expr(&mut self, e: &mut Expr, locals: &Locals, ns: &str, constant: bool) -> Check<()> {
        self.nodes += 1;
        if self.nodes > 200_000 {
            return Err(Diagnostic::new(
                e.span,
                "constant expansion exceeds the supported size limit",
            ));
        }
        if constant && let Some(name) = path(e) {
            if let Some(Some((ty, value))) = locals.get(&name) {
                e.kind = ExprKind::Constant(Box::new(value.clone()), ty.clone());
                return Ok(());
            }
            if !locals.contains_key(&name)
                && let Some(declaration) = self.globals.get(&name)
            {
                if declaration.mutable {
                    return Err(Diagnostic::new(
                        e.span,
                        "mutable static values cannot be used in constant expressions",
                    ));
                }
                if !declaration.public && namespace(&name) != ns {
                    return Err(Diagnostic::new(
                        e.span,
                        format!("constant `{name}` is private to its package"),
                    ));
                }
                let c = self.global(&name, e.span)?;
                e.kind = ExprKind::Constant(Box::new(c.value), c.ty);
                return Ok(());
            }
        }
        match &mut e.kind {
            ExprKind::Int(_, Some(t)) | ExprKind::Float(_, Some(t)) => {
                self.ty(t, locals, ns, e.span)?
            }
            ExprKind::Constant(v, t) | ExprKind::Repeat(v, t) | ExprKind::Cast(v, t) => {
                self.ty(t, locals, ns, e.span)?;
                self.expr(v, locals, ns, constant)?;
            }
            ExprKind::Unary(_, v) | ExprKind::Try(v) | ExprKind::Field(v, _) => {
                self.expr(v, locals, ns, constant)?
            }
            ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) | ExprKind::Range(a, b) => {
                self.expr(a, locals, ns, constant)?;
                self.expr(b, locals, ns, constant)?;
            }
            ExprKind::Array(t, values) => {
                self.ty(t, locals, ns, e.span)?;
                for v in values {
                    self.expr(v, locals, ns, constant)?;
                }
            }
            ExprKind::Struct(name, fields) => {
                if name.contains('<') {
                    let mut ty = crate::parser::parse(&format!(
                        "package generated\nfn instantiate(value: {name}) {{}}"
                    ))
                    .map_err(|_| Diagnostic::new(e.span, "invalid generic struct literal type"))?
                    .functions[0]
                        .params[0]
                        .ty
                        .clone();
                    self.ty(&mut ty, locals, ns, e.span)?;
                    *name = ty.to_string();
                }
                for (_, v) in fields {
                    self.expr(v, locals, ns, constant)?;
                }
            }
            ExprKind::Call {
                type_args, args, ..
            } => {
                for t in type_args {
                    self.ty(t, locals, ns, e.span)?;
                }
                for v in args {
                    self.expr(v, locals, ns, constant)?;
                }
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                self.expr(receiver, locals, ns, constant)?;
                for v in args {
                    self.expr(v, locals, ns, constant)?;
                }
            }
            ExprKind::ValueBlock(body) => self.block(body, &mut locals.clone(), ns)?,
            ExprKind::Slice {
                base, start, end, ..
            } => {
                self.expr(base, locals, ns, constant)?;
                for v in start.iter_mut().chain(end) {
                    self.expr(v, locals, ns, constant)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn block(&mut self, body: &mut Block, locals: &mut Locals, ns: &str) -> Check<()> {
        for s in body {
            self.stmt(s, locals, ns)?;
        }
        Ok(())
    }
    fn stmt(&mut self, s: &mut Stmt, locals: &mut Locals, ns: &str) -> Check<()> {
        match &mut s.kind {
            StmtKind::Let {
                name,
                ty,
                value,
                constant,
                ..
            } => {
                self.ty(ty, locals, ns, s.span)?;
                if let Some(v) = value {
                    self.expr(v, locals, ns, *constant)?;
                }
                locals.insert(
                    name.clone(),
                    if *constant {
                        value.as_ref().map(|v| (ty.clone(), v.clone()))
                    } else {
                        None
                    },
                );
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target, locals, ns, false)?;
                self.expr(value, locals, ns, false)?;
            }
            StmtKind::LetPattern {
                pattern,
                ty,
                value,
                else_block,
            } => {
                self.ty(ty, locals, ns, s.span)?;
                self.expr(value, locals, ns, false)?;
                if let Some(body) = else_block {
                    self.block(body, &mut locals.clone(), ns)?;
                }
                for name in pattern.bindings() {
                    locals.insert(name, None);
                }
            }
            StmtKind::IfLet {
                pattern,
                value,
                then_block,
                else_block,
            } => {
                self.expr(value, locals, ns, false)?;
                let mut inner = locals.clone();
                for name in pattern.bindings() {
                    inner.insert(name, None);
                }
                self.block(then_block, &mut inner, ns)?;
                self.block(else_block, &mut locals.clone(), ns)?;
            }
            StmtKind::Expr(e) | StmtKind::Yield(e) | StmtKind::Return(Some(e)) => {
                self.expr(e, locals, ns, false)?
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr(condition, locals, ns, false)?;
                self.block(then_block, &mut locals.clone(), ns)?;
                self.block(else_block, &mut locals.clone(), ns)?;
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                let mut inner = locals.clone();
                if let Some(s) = init {
                    self.stmt(s, &mut inner, ns)?;
                }
                if let Some(e) = condition {
                    self.expr(e, &inner, ns, false)?;
                }
                if let Some(s) = step {
                    self.stmt(s, &mut inner, ns)?;
                }
                self.block(body, &mut inner, ns)?;
            }
            StmtKind::ForEach {
                index,
                name,
                iterable,
                body,
                ..
            } => {
                self.expr(iterable, locals, ns, false)?;
                let mut inner = locals.clone();
                inner.insert(name.clone(), None);
                if let Some(n) = index {
                    inner.insert(n.clone(), None);
                }
                self.block(body, &mut inner, ns)?;
            }
            StmtKind::Match { value, arms } => {
                self.expr(value, locals, ns, false)?;
                for arm in arms {
                    let mut inner = locals.clone();
                    for name in arm.pattern.bindings() {
                        inner.insert(name, None);
                    }
                    if let Some(guard) = &mut arm.guard {
                        self.expr(guard, &inner, ns, false)?;
                    }
                    self.block(&mut arm.body, &mut inner, ns)?;
                }
            }
            StmtKind::Block(body) | StmtKind::Unsafe(body) => {
                self.block(body, &mut locals.clone(), ns)?
            }
            _ => (),
        }
        Ok(())
    }
}
