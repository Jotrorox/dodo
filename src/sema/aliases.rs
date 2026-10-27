//! Type aliases are transparent names. Before any other semantic pass, every
//! use is replaced by the aliased type, so later passes never see an alias.
use super::{Check, qualified_name};
use crate::ast::*;
use crate::diagnostic::{Diagnostic, DiagnosticKind};
use std::collections::{HashMap, HashSet};

// Aliases may refer to other aliases; a chain this deep is almost certainly a
// generated cycle through generic arguments rather than intentional code.
const MAX_ALIAS_DEPTH: usize = 64;

pub(super) fn resolve(program: &mut Program) -> Check<()> {
    if program.aliases.is_empty() {
        return Ok(());
    }
    let mut declared: HashSet<&str> = HashSet::new();
    for name in program
        .structs
        .iter()
        .map(|s| s.name.as_str())
        .chain(program.enums.iter().map(|e| e.name.as_str()))
        .chain(program.interfaces.iter().map(|i| i.name.as_str()))
        .chain(program.constants.iter().map(|c| c.name.as_str()))
        .chain(program.functions.iter().map(|f| f.name.as_str()))
    {
        declared.insert(name);
    }
    let mut aliases = HashMap::new();
    for alias in &program.aliases {
        if declared.contains(alias.name.as_str()) || aliases.contains_key(&alias.name) {
            return Err(Diagnostic::new(
                alias.span,
                format!("duplicate declaration `{}`", alias.name),
            ));
        }
        let mut unique = HashSet::new();
        if alias.generics.iter().any(|p| !unique.insert(p)) {
            return Err(Diagnostic::new(
                alias.span,
                "duplicate generic type parameter",
            ));
        }
        aliases.insert(alias.name.clone(), alias.clone());
    }
    let resolver = Resolver { aliases };
    let mut resolved = Vec::with_capacity(program.aliases.len());
    for alias in &program.aliases {
        let mut ty = alias.ty.clone();
        let scope = Scope {
            namespace: namespace(&alias.name),
            generics: alias.generics.iter().cloned().collect(),
        };
        resolver.ty(&mut ty, &scope, alias.span, &mut vec![alias.name.clone()])?;
        resolved.push(ty);
    }
    for (alias, ty) in program.aliases.iter_mut().zip(resolved) {
        alias.ty = ty;
    }
    for structure in &mut program.structs {
        let scope = Scope {
            namespace: namespace(&structure.name),
            generics: structure.generics.iter().cloned().collect(),
        };
        for field in &mut structure.fields {
            resolver.ty(&mut field.ty, &scope, field.span, &mut vec![])?;
        }
    }
    for enumeration in &mut program.enums {
        let scope = Scope {
            namespace: namespace(&enumeration.name),
            generics: enumeration.generics.iter().cloned().collect(),
        };
        for field in enumeration.variants.iter_mut().flat_map(|v| &mut v.fields) {
            resolver.ty(&mut field.ty, &scope, field.span, &mut vec![])?;
        }
    }
    for interface in &mut program.interfaces {
        let scope = Scope {
            namespace: namespace(&interface.name),
            generics: HashSet::from(["Self".to_owned()]),
        };
        for method in &mut interface.methods {
            for parameter in &mut method.params {
                resolver.ty(&mut parameter.ty, &scope, parameter.span, &mut vec![])?;
            }
            resolver.ty(&mut method.ret, &scope, method.ret_span, &mut vec![])?;
        }
    }
    for constant in &mut program.constants {
        let scope = Scope {
            namespace: namespace(&constant.name),
            generics: HashSet::new(),
        };
        resolver.ty(&mut constant.ty, &scope, constant.span, &mut vec![])?;
        resolver.expr(&mut constant.value, &scope, &HashSet::new())?;
    }
    let owners: HashSet<String> = program.structs.iter().map(|s| s.name.clone()).collect();
    for function in &mut program.functions {
        // Methods belong to their struct's package, like every other lookup.
        let owner = function
            .name
            .rsplit_once('.')
            .map(|(owner, _)| owner)
            .filter(|owner| owners.contains(*owner));
        let scope = Scope {
            namespace: owner.map_or_else(|| namespace(&function.name), namespace),
            generics: function.generics.iter().cloned().collect(),
        };
        for parameter in &mut function.params {
            resolver.ty(&mut parameter.ty, &scope, parameter.span, &mut vec![])?;
        }
        resolver.ty(&mut function.ret, &scope, function.ret_span, &mut vec![])?;
        for required in &mut function.requires_plain {
            resolver.ty(required, &scope, function.span, &mut vec![])?;
        }
        if let Some(body) = &mut function.body {
            let mut locals: HashSet<String> =
                function.params.iter().map(|p| p.name.clone()).collect();
            bound_names(body, &mut locals);
            resolver.block(body, &scope, &locals)?;
        }
    }
    Ok(())
}

fn namespace(name: &str) -> String {
    name.rsplit_once('.')
        .map_or(String::new(), |(n, _)| n.to_owned())
}

/// Every local name in a body. A value path is only an alias when no local of
/// that name exists anywhere in the function, which keeps shadowing simple.
fn bound_names(block: &[Stmt], names: &mut HashSet<String>) {
    for statement in block {
        match &statement.kind {
            StmtKind::Let { name, value, .. } => {
                names.insert(name.clone());
                if let Some(value) = value {
                    expression_names(value, names);
                }
            }
            StmtKind::LetPattern {
                pattern,
                value,
                else_block,
                ..
            } => {
                names.extend(pattern.bindings());
                expression_names(value, names);
                if let Some(body) = else_block {
                    bound_names(body, names);
                }
            }
            StmtKind::IfLet {
                pattern,
                value,
                then_block,
                else_block,
            } => {
                names.extend(pattern.bindings());
                expression_names(value, names);
                bound_names(then_block, names);
                bound_names(else_block, names);
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                expression_names(condition, names);
                bound_names(then_block, names);
                bound_names(else_block, names);
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                for statement in init.iter().chain(step) {
                    bound_names(std::slice::from_ref(statement.as_ref()), names);
                }
                if let Some(condition) = condition {
                    expression_names(condition, names);
                }
                bound_names(body, names);
            }
            StmtKind::ForEach {
                index,
                name,
                iterable,
                body,
                ..
            } => {
                names.insert(name.clone());
                names.extend(index.clone());
                expression_names(iterable, names);
                bound_names(body, names);
            }
            StmtKind::Match { value, arms } => {
                expression_names(value, names);
                for arm in arms {
                    names.extend(arm.pattern.bindings());
                    bound_names(&arm.body, names);
                }
            }
            StmtKind::Block(body) | StmtKind::Unsafe(body) => bound_names(body, names),
            StmtKind::Expr(value) | StmtKind::Yield(value) | StmtKind::Return(Some(value)) => {
                expression_names(value, names)
            }
            StmtKind::Assign { target, value, .. } => {
                expression_names(target, names);
                expression_names(value, names);
            }
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }
}

// Value blocks (`x := { ... }`, `if`/`match` expressions) and closures, which
// may appear anywhere in an expression, declare locals too.
fn expression_names(expression: &Expr, names: &mut HashSet<String>) {
    match &expression.kind {
        ExprKind::ValueBlock(body) => bound_names(body, names),
        ExprKind::Closure(closure) => {
            names.extend(closure.params.iter().map(|p| p.name.clone()));
            bound_names(&closure.body, names);
        }
        ExprKind::Array(_, values) => values.iter().for_each(|v| expression_names(v, names)),
        ExprKind::Struct(_, fields) => fields.iter().for_each(|(_, v)| expression_names(v, names)),
        ExprKind::Call { args, .. } => args.iter().for_each(|v| expression_names(v, names)),
        ExprKind::MethodCall { receiver, args, .. } => {
            expression_names(receiver, names);
            args.iter().for_each(|v| expression_names(v, names));
        }
        ExprKind::Invoke { callee, args } => {
            expression_names(callee, names);
            args.iter().for_each(|v| expression_names(v, names));
        }
        ExprKind::Asm(asm) => asm.inputs().for_each(|v| expression_names(v, names)),
        ExprKind::Repeat(value, _)
        | ExprKind::Constant(value, _)
        | ExprKind::Cast(value, _)
        | ExprKind::Unary(_, value)
        | ExprKind::Try(value)
        | ExprKind::Unwrap(value)
        | ExprKind::Field(value, _) => expression_names(value, names),
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) | ExprKind::Range(a, b) => {
            expression_names(a, names);
            expression_names(b, names);
        }
        ExprKind::Slice {
            base, start, end, ..
        } => {
            expression_names(base, names);
            for value in start.iter().chain(end) {
                expression_names(value, names);
            }
        }
        ExprKind::Int(..)
        | ExprKind::Float(..)
        | ExprKind::Bool(_)
        | ExprKind::String(..)
        | ExprKind::Name(_) => {}
    }
}

struct Scope {
    namespace: String,
    /// Generic parameters in scope shadow aliases of the same name.
    generics: HashSet<String>,
}

struct Resolver {
    aliases: HashMap<String, TypeAlias>,
}

impl Resolver {
    fn lookup(&self, name: &str, scope: &Scope, span: Span) -> Check<Option<&TypeAlias>> {
        if scope.generics.contains(name) {
            return Ok(None);
        }
        let Some(alias) = self.aliases.get(name) else {
            return Ok(None);
        };
        if !alias.public && namespace(&alias.name) != scope.namespace {
            return Err(Diagnostic::new(
                span,
                format!("type alias `{name}` is private to its package"),
            ));
        }
        Ok(Some(alias))
    }

    /// The aliased type with its parameters replaced by `arguments`.
    fn expand(
        &self,
        alias: &TypeAlias,
        arguments: Vec<Type>,
        span: Span,
        active: &mut Vec<String>,
    ) -> Check<Type> {
        if active.contains(&alias.name) {
            return Err(Diagnostic::new(
                span,
                format!("type alias `{}` refers to itself", alias.name),
            ));
        }
        if active.len() >= MAX_ALIAS_DEPTH {
            return Err(Diagnostic::new(
                span,
                format!(
                    "type alias expansion is nested too deeply at `{}`",
                    alias.name
                ),
            ));
        }
        if alias.generics.len() != arguments.len() {
            return Err(Diagnostic::new(
                span,
                if alias.generics.is_empty() {
                    format!("type alias `{}` has no type parameters", alias.name)
                } else {
                    format!(
                        "type alias `{}` expects {} type argument{}, found {}",
                        alias.name,
                        alias.generics.len(),
                        if alias.generics.len() == 1 { "" } else { "s" },
                        arguments.len()
                    )
                },
            ));
        }
        // Resolve the declaration in its own package and parameter scope, so
        // its body may use private names and other aliases of that package.
        let mut ty = alias.ty.clone();
        let scope = Scope {
            namespace: namespace(&alias.name),
            generics: alias.generics.iter().cloned().collect(),
        };
        active.push(alias.name.clone());
        self.ty(&mut ty, &scope, span, active)?;
        active.pop();
        let mapping: HashMap<_, _> = alias.generics.iter().cloned().zip(arguments).collect();
        Ok(substitute(&ty, &mapping))
    }

    fn ty(&self, ty: &mut Type, scope: &Scope, span: Span, active: &mut Vec<String>) -> Check<()> {
        match ty {
            Type::Named(name) => {
                if let Some(alias) = self.lookup(name, scope, span)? {
                    *ty = self.expand(alias, vec![], span, active)?;
                }
            }
            Type::Generic(name, arguments) => {
                for argument in arguments.iter_mut() {
                    self.ty(argument, scope, span, active)?;
                }
                if let Some(alias) = self.lookup(name, scope, span)? {
                    *ty = self.expand(alias, std::mem::take(arguments), span, active)?;
                }
            }
            Type::Array(_, inner)
            | Type::Slice(_, inner)
            | Type::Ref(_, inner)
            | Type::Raw(_, inner)
            | Type::Option(inner)
            | Type::MaybeUninit(inner) => self.ty(inner, scope, span, active)?,
            Type::ArrayExpr(length, inner) => {
                self.length(length, scope, span, active)?;
                self.ty(inner, scope, span, active)?;
            }
            Type::Result(ok, error) => {
                self.ty(ok, scope, span, active)?;
                self.ty(error, scope, span, active)?;
            }
            Type::Fn(params, ret) | Type::DynFn(params, ret) => {
                for param in params {
                    self.ty(param, scope, span, active)?;
                }
                self.ty(ret, scope, span, active)?;
            }
            Type::Unknown
            | Type::Void
            | Type::Bool
            | Type::Int { .. }
            | Type::Float(_)
            | Type::Str
            | Type::Dyn(_) => {}
        }
        Ok(())
    }

    fn length(
        &self,
        length: &mut LengthExpr,
        scope: &Scope,
        span: Span,
        active: &mut Vec<String>,
    ) -> Check<()> {
        match length {
            LengthExpr::Int(_, Some(ty)) => self.ty(ty, scope, span, active),
            LengthExpr::Cast(value, ty) => {
                self.length(value, scope, span, active)?;
                self.ty(ty, scope, span, active)
            }
            LengthExpr::Unary(_, value) => self.length(value, scope, span, active),
            LengthExpr::Binary(_, a, b) => {
                self.length(a, scope, span, active)?;
                self.length(b, scope, span, active)
            }
            LengthExpr::Int(_, None) | LengthExpr::Name(_) => Ok(()),
        }
    }

    /// Resolve the type spelled by a struct literal or pattern owner. Returns
    /// the replacement spelling when the name is an alias.
    fn type_path(&self, name: &str, scope: &Scope, span: Span) -> Check<Option<Type>> {
        let base = name.split('<').next().unwrap_or(name);
        if scope.generics.contains(base) || !self.aliases.contains_key(base) {
            return Ok(None);
        }
        let mut ty = if name.contains('<') {
            crate::parser::parse(&format!(
                "package generated\nfn instantiate(value: {name}) {{}}"
            ))
            .map_err(|_| Diagnostic::new(span, "invalid generic type alias arguments"))?
            .functions[0]
                .params[0]
                .ty
                .clone()
        } else {
            Type::Named(name.to_owned())
        };
        self.ty(&mut ty, scope, span, &mut vec![])?;
        Ok(Some(ty))
    }

    /// A struct literal or struct pattern written with an alias names its struct.
    fn struct_name(&self, name: &mut String, scope: &Scope, span: Span) -> Check<()> {
        if let Some(ty) = self.type_path(name, scope, span)? {
            match &ty {
                Type::Named(_) => *name = ty.to_string(),
                Type::Generic(base, _) if tuple_arity(base).is_none() => *name = ty.to_string(),
                _ => {
                    return Err(Diagnostic::new(
                        span,
                        format!("type alias `{name}` names `{ty}`, which is not a struct"),
                    )
                    .with_kind(DiagnosticKind::UnknownStruct(name.as_str().into())));
                }
            }
        }
        Ok(())
    }

    /// `Alias.member` for an enum variant or a static method: the owner path
    /// and any type arguments the alias supplies.
    fn member_owner(
        &self,
        path: &str,
        scope: &Scope,
        locals: &HashSet<String>,
        span: Span,
    ) -> Check<Option<(String, Vec<Type>)>> {
        let first = path.split('.').next().unwrap_or(path);
        if locals.contains(first) {
            return Ok(None);
        }
        Ok(match self.type_path(path, scope, span)? {
            Some(Type::Named(owner)) => Some((owner, vec![])),
            Some(Type::Generic(owner, arguments)) if tuple_arity(&owner).is_none() => {
                Some((owner, arguments))
            }
            Some(other) => {
                return Err(Diagnostic::new(
                    span,
                    format!("type alias `{path}` names `{other}`, which has no members"),
                ));
            }
            None => None,
        })
    }

    fn block(&self, block: &mut Block, scope: &Scope, locals: &HashSet<String>) -> Check<()> {
        for statement in block {
            self.statement(statement, scope, locals)?;
        }
        Ok(())
    }

    fn statement(
        &self,
        statement: &mut Stmt,
        scope: &Scope,
        locals: &HashSet<String>,
    ) -> Check<()> {
        let span = statement.span;
        match &mut statement.kind {
            StmtKind::Let { ty, value, .. } => {
                self.ty(ty, scope, span, &mut vec![])?;
                if let Some(value) = value {
                    self.expr(value, scope, locals)?;
                }
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target, scope, locals)?;
                self.expr(value, scope, locals)?;
            }
            StmtKind::Expr(value) | StmtKind::Yield(value) | StmtKind::Return(Some(value)) => {
                self.expr(value, scope, locals)?
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr(condition, scope, locals)?;
                self.block(then_block, scope, locals)?;
                self.block(else_block, scope, locals)?;
            }
            StmtKind::IfLet {
                pattern,
                value,
                then_block,
                else_block,
            } => {
                self.pattern(pattern, scope, span)?;
                self.expr(value, scope, locals)?;
                self.block(then_block, scope, locals)?;
                self.block(else_block, scope, locals)?;
            }
            StmtKind::LetPattern {
                pattern,
                ty,
                value,
                else_block,
            } => {
                self.pattern(pattern, scope, span)?;
                self.ty(ty, scope, span, &mut vec![])?;
                self.expr(value, scope, locals)?;
                if let Some(body) = else_block {
                    self.block(body, scope, locals)?;
                }
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                if let Some(init) = init {
                    self.statement(init, scope, locals)?;
                }
                if let Some(condition) = condition {
                    self.expr(condition, scope, locals)?;
                }
                if let Some(step) = step {
                    self.statement(step, scope, locals)?;
                }
                self.block(body, scope, locals)?;
            }
            StmtKind::ForEach { iterable, body, .. } => {
                self.expr(iterable, scope, locals)?;
                self.block(body, scope, locals)?;
            }
            StmtKind::Match { value, arms } => {
                self.expr(value, scope, locals)?;
                for arm in arms {
                    self.pattern(&mut arm.pattern, scope, arm.span)?;
                    if let Some(guard) = &mut arm.guard {
                        self.expr(guard, scope, locals)?;
                    }
                    self.block(&mut arm.body, scope, locals)?;
                }
            }
            StmtKind::Block(body) | StmtKind::Unsafe(body) => self.block(body, scope, locals)?,
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
        Ok(())
    }

    fn pattern(&self, pattern: &mut Pattern, scope: &Scope, span: Span) -> Check<()> {
        match pattern {
            Pattern::Struct(name, fields, _) => {
                self.struct_name(name, scope, span)?;
                for (_, field) in fields {
                    self.pattern(field, scope, span)?;
                }
            }
            Pattern::Variant(name, fields) => {
                if let Some((owner, variant)) = split_member(name)
                    && let Some(ty) = self.type_path(owner, scope, span)?
                {
                    if matches!(&ty, Type::Generic(base, _) if tuple_arity(base).is_some())
                        || !matches!(ty, Type::Named(_) | Type::Generic(..))
                    {
                        return Err(Diagnostic::new(
                            span,
                            format!("type alias `{owner}` names `{ty}`, which has no variants"),
                        ));
                    }
                    *name = format!("{ty}.{variant}");
                }
                for field in fields {
                    self.pattern(field, scope, span)?;
                }
            }
            Pattern::Or(alternatives) => {
                for alternative in alternatives {
                    self.pattern(alternative, scope, span)?;
                }
            }
            Pattern::Wildcard
            | Pattern::Binding(_)
            | Pattern::Bool(_)
            | Pattern::Int(_)
            | Pattern::Range(..) => {}
        }
        Ok(())
    }

    fn expr(&self, expression: &mut Expr, scope: &Scope, locals: &HashSet<String>) -> Check<()> {
        let span = expression.span;
        match &mut expression.kind {
            // `Alias.Variant` names a unit variant of the aliased enum.
            ExprKind::Field(base, _) => {
                if let Some(path) = qualified_name(base)
                    && let Some((owner, _)) = self.member_owner(&path, scope, locals, span)?
                {
                    **base = Expr::new(ExprKind::Name(owner), base.span);
                } else {
                    self.expr(base, scope, locals)?;
                }
            }
            // `Alias.Variant(payload)` and `Alias.function(...)`.
            ExprKind::MethodCall {
                receiver,
                name,
                args,
            } => {
                for argument in args.iter_mut() {
                    self.expr(argument, scope, locals)?;
                }
                if let Some(path) = qualified_name(receiver)
                    && let Some((owner, arguments)) =
                        self.member_owner(&path, scope, locals, span)?
                {
                    if arguments.is_empty() {
                        **receiver = Expr::new(ExprKind::Name(owner), receiver.span);
                    } else {
                        expression.kind = ExprKind::Call {
                            name: format!("{owner}.{name}"),
                            type_args: arguments,
                            args: std::mem::take(args),
                        };
                    }
                } else {
                    self.expr(receiver, scope, locals)?;
                }
            }
            ExprKind::Call {
                name,
                type_args,
                args,
            } => {
                for ty in type_args.iter_mut() {
                    self.ty(ty, scope, span, &mut vec![])?;
                }
                for argument in args {
                    self.expr(argument, scope, locals)?;
                }
                if let Some((path, member)) = name.rsplit_once('.')
                    && let Some((owner, mut arguments)) =
                        self.member_owner(path, scope, locals, span)?
                {
                    *name = format!("{owner}.{member}");
                    arguments.append(type_args);
                    *type_args = arguments;
                }
            }
            ExprKind::Struct(name, fields) => {
                self.struct_name(name, scope, span)?;
                for (_, value) in fields {
                    self.expr(value, scope, locals)?;
                }
            }
            ExprKind::Array(ty, values) => {
                self.ty(ty, scope, span, &mut vec![])?;
                for value in values {
                    self.expr(value, scope, locals)?;
                }
            }
            ExprKind::Repeat(value, ty)
            | ExprKind::Constant(value, ty)
            | ExprKind::Cast(value, ty) => {
                self.ty(ty, scope, span, &mut vec![])?;
                self.expr(value, scope, locals)?;
            }
            ExprKind::Unary(_, value) | ExprKind::Try(value) | ExprKind::Unwrap(value) => {
                self.expr(value, scope, locals)?
            }
            ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) | ExprKind::Range(a, b) => {
                self.expr(a, scope, locals)?;
                self.expr(b, scope, locals)?;
            }
            ExprKind::Slice {
                base, start, end, ..
            } => {
                self.expr(base, scope, locals)?;
                for value in start.iter_mut().chain(end) {
                    self.expr(value, scope, locals)?;
                }
            }
            ExprKind::ValueBlock(body) => self.block(body, scope, locals)?,
            ExprKind::Closure(closure) => {
                for param in &mut closure.params {
                    self.ty(&mut param.ty, scope, param.span, &mut vec![])?;
                }
                self.ty(&mut closure.ret, scope, closure.ret_span, &mut vec![])?;
                self.block(&mut closure.body, scope, locals)?;
            }
            ExprKind::Invoke { callee, args } => {
                self.expr(callee, scope, locals)?;
                for argument in args {
                    self.expr(argument, scope, locals)?;
                }
            }
            ExprKind::Asm(asm) => {
                for (ty, span) in asm.output_types_mut() {
                    self.ty(ty, scope, span, &mut vec![])?;
                }
                for value in asm.inputs_mut() {
                    self.expr(value, scope, locals)?;
                }
            }
            ExprKind::Int(..)
            | ExprKind::Float(..)
            | ExprKind::Bool(_)
            | ExprKind::String(..)
            | ExprKind::Name(_) => {}
        }
        Ok(())
    }
}

/// Split `Owner.Variant`, where the owner may carry type arguments.
fn split_member(name: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut separator = None;
    for (index, ch) in name.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            '.' if depth == 0 => separator = Some(index),
            _ => {}
        }
    }
    separator.map(|index| (&name[..index], &name[index + 1..]))
}

fn substitute(ty: &Type, mapping: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Named(name) => mapping.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Generic(name, arguments) => Type::Generic(
            name.clone(),
            arguments.iter().map(|t| substitute(t, mapping)).collect(),
        ),
        Type::Array(n, t) => Type::Array(*n, Box::new(substitute(t, mapping))),
        Type::ArrayExpr(n, t) => Type::ArrayExpr(n.clone(), Box::new(substitute(t, mapping))),
        Type::Slice(m, t) => Type::Slice(*m, Box::new(substitute(t, mapping))),
        Type::Ref(m, t) => Type::Ref(*m, Box::new(substitute(t, mapping))),
        Type::Raw(m, t) => Type::Raw(*m, Box::new(substitute(t, mapping))),
        Type::Option(t) => Type::Option(Box::new(substitute(t, mapping))),
        Type::MaybeUninit(t) => Type::MaybeUninit(Box::new(substitute(t, mapping))),
        Type::Result(t, e) => Type::Result(
            Box::new(substitute(t, mapping)),
            Box::new(substitute(e, mapping)),
        ),
        Type::Fn(params, ret) => Type::Fn(
            params.iter().map(|t| substitute(t, mapping)).collect(),
            Box::new(substitute(ret, mapping)),
        ),
        Type::DynFn(params, ret) => Type::DynFn(
            params.iter().map(|t| substitute(t, mapping)).collect(),
            Box::new(substitute(ret, mapping)),
        ),
        Type::Unknown
        | Type::Void
        | Type::Bool
        | Type::Int { .. }
        | Type::Float(_)
        | Type::Str
        | Type::Dyn(_) => ty.clone(),
    }
}
