//! Generic specialization before ordinary checking, including provisional type shapes.
//! Every generated body still passes through the production checker.
use super::{Check, dereferenced, match_subject, qualified_name};
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::{HashMap, HashSet};

#[path = "printing.rs"]
mod printing;

/// Explicit type arguments specialize generic declarations before type checking.
/// Every emitted specialization goes through the same ordinary checker.
pub(super) fn instantiate(program: &mut Program) -> Check<()> {
    for function in &program.functions {
        if function.printing.is_some() && !function.imported {
            return Err(Diagnostic::new(
                function.span,
                "@compiler printing declarations are reserved for the bundled standard library",
            ));
        }
    }
    let mut declarations = HashSet::new();
    for (name, parameters, span) in program
        .structs
        .iter()
        .map(|s| (&s.name, &s.generics, s.span))
        .chain(program.enums.iter().map(|e| (&e.name, &e.generics, e.span)))
        .chain(
            program
                .functions
                .iter()
                .map(|f| (&f.name, &f.generics, f.span)),
        )
    {
        if !declarations.insert(name) {
            return Err(Diagnostic::new(
                span,
                format!("duplicate declaration `{name}`"),
            ));
        }
        let mut unique = HashSet::new();
        if parameters.iter().any(|p| !unique.insert(p)) {
            return Err(Diagnostic::new(span, "duplicate generic type parameter"));
        }
    }
    for constant in &program.constants {
        if !declarations.insert(&constant.name) {
            return Err(Diagnostic::new(
                constant.span,
                format!("duplicate declaration `{}`", constant.name),
            ));
        }
    }
    let mut expander = Expander {
        struct_templates: program
            .structs
            .iter()
            .filter(|s| !s.generics.is_empty())
            .map(|s| (s.name.clone(), s.clone()))
            .collect(),
        enum_templates: program
            .enums
            .iter()
            .filter(|e| !e.generics.is_empty())
            .map(|e| (e.name.clone(), e.clone()))
            .collect(),
        function_templates: program
            .functions
            .iter()
            .filter(|f| !f.generics.is_empty())
            .map(|f| (f.name.clone(), f.clone()))
            .collect(),
        known_structs: program
            .structs
            .iter()
            .map(|s| (s.name.clone(), s.clone()))
            .collect(),
        known_enums: program
            .enums
            .iter()
            .map(|e| (e.name.clone(), e.clone()))
            .collect(),
        signatures: program
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.clone()))
            .collect(),
        constants: program
            .constants
            .iter()
            .map(|c| (c.name.clone(), c.ty.clone()))
            .collect(),
        concrete_types: HashMap::new(),
        locals: HashMap::new(),
        return_type: Type::Void,
        yield_type: None,
        generated_types: HashSet::new(),
        generated_functions: HashSet::new(),
        structs: vec![],
        enums: vec![],
        functions: vec![],
        count: 0,
        printing_count: 0,
    };
    program.structs.retain(|s| s.generics.is_empty());
    program.enums.retain(|e| e.generics.is_empty());
    program
        .functions
        .retain(|f| f.generics.is_empty() && (f.printing.is_none() || f.generic_instance));
    let substitutions = HashMap::new();
    for structure in &mut program.structs {
        for field in &mut structure.fields {
            expander.ty(&mut field.ty, &substitutions, field.span)?;
        }
        expander
            .known_structs
            .insert(structure.name.clone(), structure.clone());
    }
    for enumeration in &mut program.enums {
        for variant in &mut enumeration.variants {
            for field in &mut variant.fields {
                expander.ty(&mut field.ty, &substitutions, field.span)?;
            }
        }
    }
    for function in &mut program.functions {
        expander.function(function, &substitutions)?;
    }
    for constant in &mut program.constants {
        expander.ty(&mut constant.ty, &substitutions, constant.span)?;
        expander.expr(&mut constant.value, &substitutions)?;
    }
    program.structs.append(&mut expander.structs);
    program.enums.append(&mut expander.enums);
    program.functions.append(&mut expander.functions);
    Ok(())
}
struct Expander {
    known_structs: HashMap<String, Struct>,
    known_enums: HashMap<String, Enum>,
    signatures: HashMap<String, Function>,
    constants: HashMap<String, Type>,
    concrete_types: HashMap<String, Type>,
    locals: HashMap<String, Type>,
    return_type: Type,
    yield_type: Option<Type>,
    struct_templates: HashMap<String, Struct>,
    enum_templates: HashMap<String, Enum>,
    function_templates: HashMap<String, Function>,
    generated_types: HashSet<String>,
    generated_functions: HashSet<String>,
    structs: Vec<Struct>,
    enums: Vec<Enum>,
    functions: Vec<Function>,
    count: usize,
    printing_count: usize,
}
impl Expander {
    fn budget(&mut self, span: Span) -> Check<()> {
        self.count += 1;
        if self.count > 256 {
            Err(Diagnostic::new(
                span,
                "generic instantiation limit exceeded (256 specializations)",
            ))
        } else {
            Ok(())
        }
    }
    fn substitutions(
        &self,
        parameters: &[String],
        arguments: &[Type],
        span: Span,
    ) -> Check<HashMap<String, Type>> {
        if parameters.len() != arguments.len() {
            return Err(Diagnostic::new(
                span,
                format!(
                    "expected {} type arguments, found {}",
                    parameters.len(),
                    arguments.len()
                ),
            ));
        }
        Ok(parameters
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect())
    }
    fn ty(
        &mut self,
        ty: &mut Type,
        substitutions: &HashMap<String, Type>,
        span: Span,
    ) -> Check<()> {
        match ty {
            Type::Named(name) => {
                if let Some(replacement) = substitutions.get(name) {
                    *ty = replacement.clone();
                    self.ty(ty, &HashMap::new(), span)?;
                }
            }
            Type::Array(_, t)
            | Type::Slice(_, t)
            | Type::Ref(_, t)
            | Type::Raw(_, t)
            | Type::Option(t)
            | Type::MaybeUninit(t) => self.ty(t, substitutions, span)?,
            Type::Result(t, e) => {
                self.ty(t, substitutions, span)?;
                self.ty(e, substitutions, span)?;
            }
            Type::Generic(name, arguments) => {
                for argument in arguments.iter_mut() {
                    self.ty(argument, substitutions, span)?;
                }
                let concrete = specialized(name, arguments);
                self.concrete_types.insert(
                    concrete.clone(),
                    Type::Generic(name.clone(), arguments.clone()),
                );
                if self.generated_types.insert(concrete.clone()) {
                    self.budget(span)?;
                    if let Some(mut structure) = self.struct_templates.get(name).cloned() {
                        let mapping = self.substitutions(&structure.generics, arguments, span)?;
                        let original_generics = structure.generics.clone();
                        structure.name = concrete.clone();
                        structure.generics.clear();
                        for field in &mut structure.fields {
                            self.ty(&mut field.ty, &mapping, field.span)?;
                        }
                        self.known_structs
                            .insert(concrete.clone(), structure.clone());
                        self.structs.push(structure);
                        let methods: Vec<_> = self
                            .function_templates
                            .values()
                            .filter(|f| {
                                f.name.starts_with(&format!("{name}."))
                                    && f.generics == original_generics
                            })
                            .cloned()
                            .collect();
                        for mut method in methods {
                            let suffix = method.name.strip_prefix(name.as_str()).unwrap();
                            method.name = format!("{concrete}{suffix}");
                            method.generics.clear();
                            method.generic_instance = true;
                            let mut mapping = mapping.clone();
                            mapping.insert(name.clone(), Type::Named(concrete.clone()));
                            if self.generated_functions.insert(method.name.clone()) {
                                self.function(&mut method, &mapping)?;
                                self.functions.push(method);
                            }
                        }
                    } else if let Some(mut enumeration) = self.enum_templates.get(name).cloned() {
                        let mapping = self.substitutions(&enumeration.generics, arguments, span)?;
                        enumeration.name = concrete.clone();
                        enumeration.generics.clear();
                        for variant in &mut enumeration.variants {
                            for field in &mut variant.fields {
                                self.ty(&mut field.ty, &mapping, field.span)?;
                            }
                        }
                        self.known_enums
                            .insert(concrete.clone(), enumeration.clone());
                        self.enums.push(enumeration);
                    } else {
                        return Err(Diagnostic::new(
                            span,
                            format!("unknown generic type `{name}`"),
                        ));
                    }
                }
                *ty = Type::Named(concrete);
            }
            _ => (),
        }
        Ok(())
    }
    fn function(
        &mut self,
        function: &mut Function,
        substitutions: &HashMap<String, Type>,
    ) -> Check<()> {
        for required in &mut function.requires_plain {
            self.ty(required, substitutions, function.span)?;
        }
        for parameter in &mut function.params {
            self.ty(&mut parameter.ty, substitutions, parameter.span)?;
        }
        self.ty(&mut function.ret, substitutions, function.span)?;
        self.signatures
            .insert(function.name.clone(), function.clone());
        let previous = std::mem::replace(
            &mut self.locals,
            function
                .params
                .iter()
                .map(|p| (p.name.clone(), p.ty.clone()))
                .collect(),
        );
        let ret = std::mem::replace(&mut self.return_type, function.ret.clone());
        let yielding = self.yield_type.take();
        if let Some(body) = &mut function.body {
            self.block(body, substitutions)?;
        }
        self.locals = previous;
        self.return_type = ret;
        self.yield_type = yielding;
        Ok(())
    }
    fn block(&mut self, body: &mut Block, substitutions: &HashMap<String, Type>) -> Check<()> {
        let locals = self.locals.clone();
        for statement in body {
            self.statement(statement, substitutions)?;
        }
        self.locals = locals;
        Ok(())
    }
    fn statement(
        &mut self,
        statement: &mut Stmt,
        substitutions: &HashMap<String, Type>,
    ) -> Check<()> {
        let span = statement.span;
        match &mut statement.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                self.ty(ty, substitutions, span)?;
                let actual = if let Some(e) = value {
                    self.expression(e, substitutions, (*ty != Type::Unknown).then_some(&*ty))?
                } else {
                    ty.clone()
                };
                self.locals.insert(
                    name.clone(),
                    if *ty == Type::Unknown {
                        actual
                    } else {
                        ty.clone()
                    },
                );
            }
            StmtKind::Assign { target, value, .. } => {
                let ty = self.expression(target, substitutions, None)?;
                self.expression(value, substitutions, Some(&ty))?;
            }
            StmtKind::Return(Some(e)) => {
                self.expression(e, substitutions, Some(&self.return_type.clone()))?;
            }
            StmtKind::Yield(e) => {
                let ty = self.expression(e, substitutions, self.yield_type.clone().as_ref())?;
                if ty != Type::Unknown {
                    self.yield_type = Some(ty);
                }
            }
            StmtKind::Expr(e) => {
                self.expression(e, substitutions, None)?;
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expression(condition, substitutions, Some(&Type::Bool))?;
                self.block(then_block, substitutions)?;
                self.block(else_block, substitutions)?;
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                let locals = self.locals.clone();
                if let Some(s) = init {
                    self.statement(s, substitutions)?;
                }
                if let Some(e) = condition {
                    self.expression(e, substitutions, Some(&Type::Bool))?;
                }
                if let Some(s) = step {
                    self.statement(s, substitutions)?;
                }
                self.block(body, substitutions)?;
                self.locals = locals;
            }
            StmtKind::ForEach {
                index,
                name,
                copy,
                iterable,
                body,
            } => {
                let ty = self.expression(iterable, substitutions, None)?;
                let locals = self.locals.clone();
                if let Some(n) = index {
                    self.locals.insert(n.clone(), Type::usize());
                }
                let element = if matches!(iterable.kind, ExprKind::Range(..)) {
                    ty
                } else {
                    match dereferenced(&ty) {
                        Type::Array(_, t) | Type::Slice(_, t) if *copy => *t.clone(),
                        Type::Array(_, t) | Type::Slice(_, t) => Type::Ref(
                            matches!(ty, Type::Ref(true, _) | Type::Slice(true, _)),
                            t.clone(),
                        ),
                        _ => Type::Unknown,
                    }
                };
                self.locals.insert(name.clone(), element);
                self.block(body, substitutions)?;
                self.locals = locals;
            }
            StmtKind::Match { value, arms } => {
                let ty = self.expression(value, substitutions, None)?;
                for arm in arms {
                    let locals = self.locals.clone();
                    let (matched, borrowed) = match_subject(&ty);
                    self.pattern_locals(&mut arm.pattern, matched, borrowed, substitutions, span)?;
                    if let Some(guard) = &mut arm.guard {
                        self.expression(guard, substitutions, Some(&Type::Bool))?;
                    }
                    self.block(&mut arm.body, substitutions)?;
                    self.locals = locals;
                }
            }
            StmtKind::IfLet {
                pattern,
                value,
                then_block,
                else_block,
            } => {
                let ty = self.expression(value, substitutions, None)?;
                let locals = self.locals.clone();
                let (matched, borrowed) = match_subject(&ty);
                self.pattern_locals(pattern, matched, borrowed, substitutions, span)?;
                self.block(then_block, substitutions)?;
                self.locals = locals;
                self.block(else_block, substitutions)?;
            }
            StmtKind::LetPattern {
                pattern,
                ty,
                value,
                else_block,
            } => {
                self.ty(ty, substitutions, span)?;
                let actual =
                    self.expression(value, substitutions, (*ty != Type::Unknown).then_some(&*ty))?;
                if let Some(block) = else_block {
                    self.block(block, substitutions)?;
                }
                let (matched, borrowed) = match_subject(&actual);
                self.pattern_locals(pattern, matched, borrowed, substitutions, span)?;
            }
            StmtKind::Block(body) | StmtKind::Unsafe(body) => self.block(body, substitutions)?,
            _ => (),
        }
        Ok(())
    }
    fn payloads(&self, ty: &Type, variant: &str) -> Vec<Type> {
        let variant = variant.rsplit('.').next().unwrap_or(variant);
        match (ty, variant) {
            (Type::Option(t), "some") | (Type::Result(t, _), "ok") if **t != Type::Void => {
                vec![*t.clone()]
            }
            (Type::Result(_, t), "err") => vec![*t.clone()],
            (Type::Named(n), v) => self
                .known_enums
                .get(n)
                .and_then(|e| e.variants.iter().find(|a| a.name == v))
                .map_or(vec![], |v| v.fields.iter().map(|f| f.ty.clone()).collect()),
            _ => vec![],
        }
    }
    fn pattern_locals(
        &mut self,
        pattern: &mut Pattern,
        ty: &Type,
        borrowed: Option<bool>,
        substitutions: &HashMap<String, Type>,
        span: Span,
    ) -> Check<()> {
        if let Pattern::Binding(name) = pattern
            && let Type::Named(owner) = dereferenced(ty)
            && self.known_enums.get(owner).is_some_and(|e| {
                e.variants
                    .iter()
                    .any(|v| v.name == *name && v.fields.is_empty())
            })
        {
            *pattern = Pattern::Variant(name.clone(), vec![]);
        }
        match pattern {
            Pattern::Binding(name) => {
                self.locals.insert(
                    name.clone(),
                    borrowed.map_or_else(|| ty.clone(), |m| Type::Ref(m, Box::new(ty.clone()))),
                );
                return Ok(());
            }
            Pattern::Or(alternatives) => {
                for alternative in alternatives {
                    self.pattern_locals(alternative, ty, borrowed, substitutions, span)?;
                }
                return Ok(());
            }
            _ => (),
        }
        let mut ty = ty;
        let mut borrowed = borrowed;
        while let Type::Ref(m, inner) = ty {
            borrowed = Some(borrowed.unwrap_or(true) && *m);
            ty = inner;
        }
        match pattern {
            Pattern::Variant(name, patterns) => {
                let mut depth = 0usize;
                let mut separator = None;
                for (index, ch) in name.char_indices() {
                    match ch {
                        '<' => depth += 1,
                        '>' => depth = depth.saturating_sub(1),
                        '.' if depth == 0 => separator = Some(index),
                        _ => (),
                    }
                }
                if let Some(index) = separator {
                    let mut owner = name[..index].to_string();
                    let mut variant = name[index + 1..].to_string();
                    if let Some(start) = variant.find('<') {
                        owner.push_str(&variant[start..]);
                        variant.truncate(start);
                    }
                    self.pattern_owner(&mut owner, ty, substitutions, span)?;
                    *name = format!("{owner}.{variant}");
                }
                let fields = self.payloads(ty, name);
                for (pattern, field) in patterns.iter_mut().zip(fields) {
                    self.pattern_locals(pattern, &field, borrowed, substitutions, span)?;
                }
            }
            Pattern::Struct(name, fields, _) => {
                self.pattern_owner(name, ty, substitutions, span)?;
                if let Some(declaration) = self.known_structs.get(name).cloned() {
                    for (name, pattern) in fields {
                        if let Some(field) = declaration.fields.iter().find(|f| &f.name == name) {
                            self.pattern_locals(pattern, &field.ty, borrowed, substitutions, span)?;
                        }
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn pattern_owner(
        &mut self,
        name: &mut String,
        matched: &Type,
        substitutions: &HashMap<String, Type>,
        span: Span,
    ) -> Check<()> {
        if name.contains('<') {
            let mut ty = crate::parser::parse(&format!(
                "package generated\nfn instantiate(value: {name}) {{}}"
            ))
            .map_err(|_| Diagnostic::new(span, "invalid generic pattern type"))?
            .functions[0]
                .params[0]
                .ty
                .clone();
            self.ty(&mut ty, substitutions, span)?;
            if let Type::Named(concrete) = ty {
                *name = concrete;
            }
        } else if let Type::Named(concrete) = matched
            && let Type::Generic(template, _) = self.shape(matched)
            && *name == template
        {
            *name = concrete.clone();
        } else if let Some(Type::Named(concrete)) = substitutions.get(name) {
            *name = concrete.clone();
        }
        Ok(())
    }
    fn shape(&self, ty: &Type) -> Type {
        if let Type::Named(n) = ty
            && let Some(t) = self.concrete_types.get(n)
        {
            return t.clone();
        }
        ty.clone()
    }
    fn substitute(ty: &Type, mapping: &HashMap<String, Type>) -> Type {
        match ty {
            Type::Named(n) => mapping.get(n).cloned().unwrap_or_else(|| ty.clone()),
            Type::Array(n, t) => Type::Array(*n, Box::new(Self::substitute(t, mapping))),
            Type::Ref(m, t) => Type::Ref(*m, Box::new(Self::substitute(t, mapping))),
            Type::Slice(m, t) => Type::Slice(*m, Box::new(Self::substitute(t, mapping))),
            Type::Raw(m, t) => Type::Raw(*m, Box::new(Self::substitute(t, mapping))),
            Type::Option(t) => Type::Option(Box::new(Self::substitute(t, mapping))),
            Type::MaybeUninit(t) => Type::MaybeUninit(Box::new(Self::substitute(t, mapping))),
            Type::Result(t, e) => Type::Result(
                Box::new(Self::substitute(t, mapping)),
                Box::new(Self::substitute(e, mapping)),
            ),
            Type::Generic(n, args) => Type::Generic(
                n.clone(),
                args.iter().map(|t| Self::substitute(t, mapping)).collect(),
            ),
            _ => ty.clone(),
        }
    }
    fn concrete(ty: &Type, parameters: &[String]) -> bool {
        match ty {
            Type::Unknown => false,
            Type::Named(n) => !parameters.contains(n),
            Type::Array(_, t)
            | Type::Ref(_, t)
            | Type::Slice(_, t)
            | Type::Raw(_, t)
            | Type::Option(t)
            | Type::MaybeUninit(t) => Self::concrete(t, parameters),
            Type::Result(t, e) => Self::concrete(t, parameters) && Self::concrete(e, parameters),
            Type::Generic(_, args) => args.iter().all(|t| Self::concrete(t, parameters)),
            _ => true,
        }
    }
    fn unify(
        &self,
        pattern: &Type,
        actual: &Type,
        parameters: &[String],
        mapping: &mut HashMap<String, Type>,
        span: Span,
    ) -> Check<()> {
        if *actual == Type::Unknown {
            return Ok(());
        }
        if let Type::Named(n) = pattern
            && parameters.contains(n)
        {
            if let Some(previous) = mapping.get(n) {
                if previous != actual {
                    return Err(Diagnostic::new(
                        span,
                        format!(
                            "conflicting types for generic parameter `{n}`: `{previous}` and `{actual}`"
                        ),
                    ));
                }
            } else {
                mapping.insert(n.clone(), actual.clone());
            }
            return Ok(());
        }
        let actual = self.shape(actual);
        match (pattern, &actual) {
            (Type::Array(_, p), Type::Array(_, a))
            | (Type::Ref(_, p), Type::Ref(_, a))
            | (Type::Slice(_, p), Type::Slice(_, a))
            | (Type::Raw(_, p), Type::Raw(_, a))
            | (Type::Option(p), Type::Option(a))
            | (Type::MaybeUninit(p), Type::MaybeUninit(a)) => {
                self.unify(p, a, parameters, mapping, span)?
            }
            (Type::Slice(_, p), Type::Ref(_, a)) => {
                if let Type::Array(_, a) = a.as_ref() {
                    self.unify(p, a, parameters, mapping, span)?;
                }
            }
            (Type::Result(p, e), Type::Result(a, b)) => {
                self.unify(p, a, parameters, mapping, span)?;
                self.unify(e, b, parameters, mapping, span)?;
            }
            (Type::Generic(p, ps), Type::Generic(a, args)) if p == a => {
                for (p, a) in ps.iter().zip(args) {
                    self.unify(p, a, parameters, mapping, span)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn guess(&self, e: &Expr) -> Option<Type> {
        match &e.kind {
            ExprKind::Int(_, t) | ExprKind::Float(_, t) => t.clone(),
            ExprKind::Bool(_) => Some(Type::Bool),
            ExprKind::Name(n) => self
                .locals
                .get(n)
                .or_else(|| self.constants.get(n))
                .cloned(),
            ExprKind::String(_, bytes) => Some(if *bytes {
                Type::Slice(false, Box::new(Type::u8()))
            } else {
                Type::Str
            }),
            ExprKind::Cast(_, t) | ExprKind::Constant(_, t) => Some(t.clone()),
            ExprKind::Unary(UnaryOp::Borrow, e) => {
                self.guess(e).map(|t| Type::Ref(false, Box::new(t)))
            }
            ExprKind::Unary(UnaryOp::BorrowMut, e) => {
                self.guess(e).map(|t| Type::Ref(true, Box::new(t)))
            }
            ExprKind::Unary(UnaryOp::Deref, e) => self.guess(e).and_then(|t| match t {
                Type::Ref(_, t) | Type::Raw(_, t) => Some(*t),
                _ => None,
            }),
            ExprKind::Unary(_, e) => self.guess(e),
            ExprKind::Binary(
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge
                | BinaryOp::And
                | BinaryOp::Or,
                _,
                _,
            ) => Some(Type::Bool),
            ExprKind::Binary(_, a, b) | ExprKind::Range(a, b) => {
                self.guess(a).or_else(|| self.guess(b))
            }
            ExprKind::Array(t, _) | ExprKind::Repeat(_, t) if Self::concrete(t, &[]) => {
                Some(t.clone())
            }
            ExprKind::Index(e, _) => self.guess(e).and_then(|t| match dereferenced(&t) {
                Type::Array(_, t) | Type::Slice(_, t) => Some(*t.clone()),
                _ => None,
            }),
            ExprKind::Field(base, n) => {
                if let Some(owner) = qualified_name(base)
                    && self.known_enums.get(&owner).is_some_and(|e| {
                        e.variants
                            .iter()
                            .any(|v| v.name == *n && v.fields.is_empty())
                    })
                {
                    return Some(Type::Named(owner));
                }

                if let Some(path) = qualified_name(e)
                    && let Some(t) = self.constants.get(&path)
                {
                    return Some(t.clone());
                }
                let ty = self.guess(base)?;
                match dereferenced(&ty) {
                    Type::Array(..) | Type::Slice(..) | Type::Str if n == "len" => {
                        Some(Type::usize())
                    }
                    Type::Named(owner) => self
                        .known_structs
                        .get(owner)?
                        .fields
                        .iter()
                        .find(|f| f.name == *n)
                        .map(|f| f.ty.clone()),
                    _ => None,
                }
            }
            ExprKind::MethodCall {
                receiver,
                name,
                args,
            } => qualified_name(receiver).and_then(|prefix| {
                intrinsic_result_type(
                    &format!("{prefix}.{name}"),
                    &[],
                    &args
                        .iter()
                        .map(|a| self.guess(a).unwrap_or(Type::Unknown))
                        .collect::<Vec<_>>(),
                )
            }),
            ExprKind::Call {
                name,
                type_args,
                args,
            } => self
                .signatures
                .get(name)
                .filter(|f| f.generics.is_empty())
                .map(|f| f.ret.clone())
                .or_else(|| {
                    intrinsic_result_type(
                        name,
                        type_args,
                        &args
                            .iter()
                            .map(|a| self.guess(a).unwrap_or(Type::Unknown))
                            .collect::<Vec<_>>(),
                    )
                }),
            ExprKind::Try(e) | ExprKind::Unwrap(e) => self.guess(e).and_then(|t| {
                if let Type::Result(t, _) = t {
                    Some(*t)
                } else {
                    None
                }
            }),
            _ => None,
        }
    }
    fn expr(&mut self, e: &mut Expr, substitutions: &HashMap<String, Type>) -> Check<()> {
        self.expression(e, substitutions, None).map(|_| ())
    }
    fn expression(
        &mut self,
        e: &mut Expr,
        substitutions: &HashMap<String, Type>,
        expected: Option<&Type>,
    ) -> Check<Type> {
        let span = e.span;
        if let ExprKind::MethodCall {
            receiver,
            name,
            args,
        } = &mut e.kind
            && let Some(prefix) = qualified_name(receiver)
            && !self.locals.contains_key(&prefix)
            && (self.signatures.contains_key(&format!("{prefix}.{name}"))
                || self.known_enums.contains_key(&prefix)
                || intrinsic_result_type(&format!("{prefix}.{name}"), &[], &[]).is_some())
        {
            e.kind = ExprKind::Call {
                name: format!("{prefix}.{name}"),
                type_args: vec![],
                args: std::mem::take(args),
            };
        }
        Ok(match &mut e.kind {
            ExprKind::Int(_, suffix) => {
                if let Some(t) = suffix {
                    self.ty(t, substitutions, span)?;
                    t.clone()
                } else {
                    expected
                        .filter(|t| t.is_integer())
                        .cloned()
                        .unwrap_or(Type::isize())
                }
            }
            ExprKind::Float(_, suffix) => suffix
                .clone()
                .or_else(|| expected.filter(|t| matches!(t, Type::Float(_))).cloned())
                .unwrap_or(Type::Float(64)),
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::String(_, bytes) => {
                if *bytes {
                    Type::Slice(false, Box::new(Type::u8()))
                } else {
                    Type::Str
                }
            }
            ExprKind::Name(n) => self
                .locals
                .get(n)
                .or_else(|| self.constants.get(n))
                .cloned()
                .or_else(|| expected.cloned())
                .unwrap_or(Type::Unknown),
            ExprKind::Constant(v, t) => {
                self.ty(t, substitutions, span)?;
                self.expression(v, substitutions, Some(t))?;
                t.clone()
            }
            ExprKind::Cast(v, t) => {
                self.ty(t, substitutions, span)?;
                self.expression(v, substitutions, None)?;
                t.clone()
            }
            ExprKind::Unary(op, value) => {
                let hint = match (*op, expected) {
                    (UnaryOp::Borrow | UnaryOp::BorrowMut, Some(Type::Ref(_, t))) => {
                        Some(t.as_ref())
                    }
                    _ => expected,
                };
                let actual = self.expression(value, substitutions, hint)?;
                match op {
                    UnaryOp::Borrow => Type::Ref(false, Box::new(actual)),
                    UnaryOp::BorrowMut => Type::Ref(true, Box::new(actual)),
                    UnaryOp::Deref => match actual {
                        Type::Ref(_, t) | Type::Raw(_, t) => *t,
                        _ => Type::Unknown,
                    },
                    _ => actual,
                }
            }
            ExprKind::Binary(op, a, b) => {
                let hint = expected
                    .filter(|t| t.is_numeric())
                    .cloned()
                    .or_else(|| self.guess(a))
                    .or_else(|| self.guess(b));
                let actual = self.expression(a, substitutions, hint.as_ref())?;
                self.expression(b, substitutions, Some(&actual))?;
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
                    Type::Bool
                } else {
                    actual
                }
            }
            ExprKind::Range(a, b) => {
                let hint = self.guess(a).or_else(|| self.guess(b));
                let actual = self.expression(a, substitutions, hint.as_ref())?;
                self.expression(b, substitutions, Some(&actual))?;
                actual
            }
            ExprKind::Array(t, items) => {
                self.ty(t, substitutions, span)?;
                let Type::Array(n, element) = t else {
                    return Err(Diagnostic::new(span, "unresolved array type"));
                };
                let mut hint = if **element != Type::Unknown {
                    Some(*element.clone())
                } else if let Some(Type::Array(_, t)) = expected {
                    Some(*t.clone())
                } else {
                    items.iter().find_map(|v| self.guess(v))
                };
                for v in items {
                    let actual = self.expression(v, substitutions, hint.as_ref())?;
                    if hint.is_none() && actual != Type::Unknown {
                        hint = Some(actual);
                    }
                }
                if let Some(t) = hint {
                    **element = t;
                }
                Type::Array(*n, element.clone())
            }
            ExprKind::Repeat(value, t) => {
                self.ty(t, substitutions, span)?;
                let hint = if let Some(Type::Array(_, t)) = expected {
                    Some(t.as_ref())
                } else {
                    None
                };
                let actual = self.expression(value, substitutions, hint)?;
                let Type::Array(n, _) = t else {
                    return Err(Diagnostic::new(span, "unresolved repeat type"));
                };
                Type::Array(*n, Box::new(actual))
            }
            ExprKind::Struct(name, fields) => {
                let mut ty = if name.contains('<') {
                    crate::parser::parse(&format!(
                        "package generated\nfn instantiate(value: {name}) {{}}"
                    ))
                    .map_err(|_| Diagnostic::new(span, "invalid generic struct literal type"))?
                    .functions[0]
                        .params[0]
                        .ty
                        .clone()
                } else {
                    substitutions
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| Type::Named(name.clone()))
                };
                if let Type::Named(owner) = &ty
                    && let Some(template) = self.struct_templates.get(owner).cloned()
                {
                    let parameters = &template.generics;
                    let mut mapping = HashMap::new();
                    if let Some(expected) = expected {
                        self.unify(
                            &Type::Generic(
                                owner.clone(),
                                parameters.iter().cloned().map(Type::Named).collect(),
                            ),
                            expected,
                            parameters,
                            &mut mapping,
                            span,
                        )?;
                    }
                    for (n, v) in fields.iter() {
                        if let Some(field) = template.fields.iter().find(|f| f.name == *n)
                            && let Some(actual) = self.guess(v)
                        {
                            self.unify(&field.ty, &actual, parameters, &mut mapping, span)?;
                        }
                    }
                    for (n, v) in fields.iter_mut() {
                        let field = template.fields.iter().find(|f| f.name == *n);
                        let hint = field
                            .map(|f| Self::substitute(&f.ty, &mapping))
                            .filter(|t| Self::concrete(t, parameters));
                        let actual = self.expression(v, substitutions, hint.as_ref())?;
                        if let Some(field) = field {
                            self.unify(&field.ty, &actual, parameters, &mut mapping, span)?;
                        }
                    }
                    ty = Type::Generic(
                        owner.clone(),
                        self.inferred_arguments(parameters, &mapping, span)?,
                    );
                }
                self.ty(&mut ty, substitutions, span)?;
                if let Type::Named(concrete) = &ty {
                    *name = concrete.clone();
                }
                let decl = self.known_structs.get(name).cloned();
                for (n, v) in fields {
                    let hint = decl
                        .as_ref()
                        .and_then(|s| s.fields.iter().find(|f| f.name == *n))
                        .map(|f| &f.ty);
                    self.expression(v, substitutions, hint)?;
                }
                ty
            }
            ExprKind::Call {
                name,
                type_args,
                args,
            } => {
                for t in type_args.iter_mut() {
                    self.ty(t, substitutions, span)?;
                }
                if matches!(name.as_str(), "mem.callback" | "core.mem.callback") {
                    if args.len() != 1 {
                        return Err(Diagnostic::new(
                            span,
                            "mem.callback expects one statically named function",
                        ));
                    }
                    let target = qualified_name(&args[0]).ok_or_else(|| {
                        Diagnostic::new(span, "mem.callback expects a function name")
                    })?;
                    let template = self.signatures.get(&target).cloned().ok_or_else(|| {
                        Diagnostic::new(span, format!("unknown callback function `{target}`"))
                    })?;
                    if template.printing.is_some() {
                        return Err(Diagnostic::new(
                            span,
                            "printing entry points cannot be used as callbacks; wrap a concrete call in an ordinary function",
                        ));
                    }
                    let mapping = self.substitutions(&template.generics, type_args, span)?;
                    let concrete = if template.generics.is_empty() {
                        target
                    } else {
                        let concrete = specialized(&target, type_args);
                        if self.generated_functions.insert(concrete.clone()) {
                            self.budget(span)?;
                            let mut function = template;
                            function.name = concrete.clone();
                            function.generics.clear();
                            function.generic_instance = true;
                            self.function(&mut function, &mapping)?;
                            self.functions.push(function);
                        }
                        concrete
                    };
                    args[0].kind = ExprKind::Name(concrete);
                    type_args.clear();
                    Type::Raw(false, Box::new(Type::u8()))
                } else if let Some(template) = self.signatures.get(name).cloned() {
                    if template.printing.is_some() && !template.generic_instance {
                        return self.printing_call(
                            name,
                            type_args,
                            args,
                            &template,
                            substitutions,
                            span,
                        );
                    }
                    let parameters = &template.generics;
                    let explicit = !type_args.is_empty();
                    let mut mapping = if explicit {
                        self.substitutions(parameters, type_args, span)?
                    } else {
                        HashMap::new()
                    };
                    if !explicit && !parameters.is_empty() {
                        if let Some(expected) = expected {
                            self.unify(&template.ret, expected, parameters, &mut mapping, span)?;
                        }
                        for (parameter, arg) in template.params.iter().zip(args.iter()) {
                            if let Some(actual) = self.guess(arg) {
                                self.unify(&parameter.ty, &actual, parameters, &mut mapping, span)?;
                            }
                        }
                    }
                    for (i, arg) in args.iter_mut().enumerate() {
                        let param = template.params.get(i);
                        let hint = param
                            .map(|p| Self::substitute(&p.ty, &mapping))
                            .filter(|t| Self::concrete(t, parameters));
                        let actual = self.expression(arg, substitutions, hint.as_ref())?;
                        if !explicit && let Some(param) = param {
                            self.unify(&param.ty, &actual, parameters, &mut mapping, span)?;
                        }
                    }
                    let mut ret = Self::substitute(&template.ret, &mapping);
                    if !parameters.is_empty() {
                        let arguments = self.inferred_arguments(parameters, &mapping, span)?;
                        let concrete = specialized(name, &arguments);
                        if self.generated_functions.insert(concrete.clone()) {
                            self.budget(span)?;
                            let mut function = template;
                            function.name = concrete.clone();
                            function.generics.clear();
                            function.generic_instance = true;
                            self.function(&mut function, &mapping)?;
                            self.functions.push(function);
                        }
                        *name = concrete;
                        type_args.clear();
                    }
                    self.ty(&mut ret, substitutions, span)?;
                    ret
                } else if matches!(
                    name.as_str(),
                    "core.wrapping_add" | "core.wrapping_sub" | "core.wrapping_mul"
                ) {
                    let mut inferred = type_args
                        .first()
                        .cloned()
                        .or_else(|| expected.filter(|t| t.is_integer()).cloned())
                        .or_else(|| {
                            args.iter()
                                .find_map(|arg| self.guess(arg).filter(|ty| *ty != Type::Unknown))
                        });
                    for arg in args.iter_mut() {
                        let actual = self.expression(arg, substitutions, inferred.as_ref())?;
                        inferred.get_or_insert(actual);
                    }
                    inferred.unwrap_or(Type::Unknown)
                } else if intrinsic_result_type(name, type_args, &[]).is_some() {
                    let mut actuals = Vec::new();
                    for arg in args.iter_mut() {
                        actuals.push(self.expression(arg, substitutions, None)?);
                    }
                    intrinsic_result_type(name, type_args, &actuals).unwrap_or(Type::Unknown)
                } else if matches!(name.as_str(), "some" | "ok" | "err" | "none") {
                    let payload = match (name.as_str(), expected) {
                        ("some", Some(Type::Option(t)))
                        | ("ok", Some(Type::Result(t, _)))
                        | ("err", Some(Type::Result(_, t))) => Some(t.as_ref()),
                        _ => None,
                    };
                    let mut actual = Type::Void;
                    for arg in args {
                        actual = self.expression(arg, substitutions, payload)?;
                    }
                    expected.cloned().unwrap_or_else(|| {
                        if name == "some" {
                            Type::Option(Box::new(actual))
                        } else {
                            Type::Unknown
                        }
                    })
                } else {
                    if let Some((owner, variant)) = name.rsplit_once('.')
                        && let Some(template) = self.enum_templates.get(owner).cloned()
                    {
                        let owner = owner.to_owned();
                        let variant = variant.to_owned();
                        let mut mapping = if type_args.is_empty() {
                            HashMap::new()
                        } else {
                            self.substitutions(&template.generics, type_args, span)?
                        };
                        if type_args.is_empty() {
                            if let Some(expected) = expected {
                                self.unify(
                                    &Type::Generic(
                                        owner.clone(),
                                        template
                                            .generics
                                            .iter()
                                            .cloned()
                                            .map(Type::Named)
                                            .collect(),
                                    ),
                                    expected,
                                    &template.generics,
                                    &mut mapping,
                                    span,
                                )?;
                            }
                            if let Some(payload) =
                                template.variants.iter().find(|v| v.name == variant)
                            {
                                for (field, arg) in payload.fields.iter().zip(args.iter()) {
                                    if let Some(actual) = self.guess(arg) {
                                        self.unify(
                                            &field.ty,
                                            &actual,
                                            &template.generics,
                                            &mut mapping,
                                            span,
                                        )?;
                                    }
                                }
                            }
                        }
                        let arguments =
                            self.inferred_arguments(&template.generics, &mapping, span)?;
                        let mut owner_type = Type::Generic(owner, arguments);
                        self.ty(&mut owner_type, substitutions, span)?;
                        let Type::Named(concrete) = owner_type else {
                            unreachable!()
                        };
                        *name = format!("{concrete}.{variant}");
                        type_args.clear();
                    }
                    let variant = name
                        .rsplit_once('.')
                        .map(|(owner, variant)| (Type::Named(owner.into()), variant.to_owned()));
                    let fields = variant
                        .as_ref()
                        .map_or(vec![], |(t, v)| self.payloads(t, v));
                    for (i, arg) in args.iter_mut().enumerate() {
                        self.expression(arg, substitutions, fields.get(i))?;
                    }
                    variant.map_or(Type::Unknown, |(t, _)| t)
                }
            }
            ExprKind::MethodCall {
                receiver,
                name,
                args,
            } => {
                let actual = self.expression(receiver, substitutions, None)?;
                let signature = if let Type::Named(owner) = dereferenced(&actual) {
                    self.signatures
                        .get(&format!("{owner}.{name}"))
                        .cloned()
                        .or_else(|| {
                            // Methods with their own type parameters remain generic
                            // after their containing struct has been specialized.
                            // Infer both sets from the concrete receiver and other
                            // arguments through the ordinary generic-call path.
                            let Type::Generic(template, _) = self.concrete_types.get(owner)? else {
                                return None;
                            };
                            self.signatures.get(&format!("{template}.{name}")).cloned()
                        })
                } else {
                    None
                };
                if let Some(signature) = &signature
                    && signature.printing.is_some()
                    && !signature.generic_instance
                {
                    let mut receiver = receiver.as_ref().clone();
                    // Existing places are exclusively reborrowed; a temporary
                    // stream wrapper is owned by the generated helper.
                    if printing::is_place(&receiver) {
                        if matches!(actual, Type::Ref(..)) {
                            receiver = Expr::new(
                                ExprKind::Unary(UnaryOp::Deref, Box::new(receiver)),
                                span,
                            );
                        }
                        receiver = Expr::new(
                            ExprKind::Unary(UnaryOp::BorrowMut, Box::new(receiver)),
                            span,
                        );
                    }
                    let mut arguments = vec![receiver];
                    arguments.append(args);
                    let mut target = signature.name.clone();
                    let ret = self.printing_call(
                        &mut target,
                        &[],
                        &mut arguments,
                        signature,
                        substitutions,
                        span,
                    )?;
                    e.kind = ExprKind::Call {
                        name: target,
                        type_args: vec![],
                        args: arguments,
                    };
                    return Ok(ret);
                }
                if let Some(signature) = &signature
                    && !signature.generics.is_empty()
                {
                    let mut receiver = receiver.as_ref().clone();
                    if let Some(Param {
                        ty: Type::Ref(mutable, _),
                        ..
                    }) = signature.params.first()
                    {
                        if matches!(actual, Type::Ref(..)) {
                            receiver = Expr::new(
                                ExprKind::Unary(UnaryOp::Deref, Box::new(receiver)),
                                span,
                            );
                        }
                        receiver = Expr::new(
                            ExprKind::Unary(
                                if *mutable {
                                    UnaryOp::BorrowMut
                                } else {
                                    UnaryOp::Borrow
                                },
                                Box::new(receiver),
                            ),
                            span,
                        );
                    }
                    let mut arguments = vec![receiver];
                    arguments.append(args);
                    e.kind = ExprKind::Call {
                        name: signature.name.clone(),
                        type_args: vec![],
                        args: arguments,
                    };
                    return self.expression(e, substitutions, expected);
                }
                for (i, arg) in args.iter_mut().enumerate() {
                    self.expression(
                        arg,
                        substitutions,
                        signature
                            .as_ref()
                            .and_then(|f| f.params.get(i + 1))
                            .map(|p| &p.ty),
                    )?;
                }
                signature.map_or(Type::Unknown, |f| f.ret)
            }
            ExprKind::Field(base, field) => {
                let actual = self.expression(base, substitutions, None)?;
                match dereferenced(&actual) {
                    Type::Array(..) | Type::Slice(..) | Type::Str if field == "len" => {
                        Type::usize()
                    }
                    Type::Named(owner) => self
                        .known_structs
                        .get(owner)
                        .and_then(|s| s.fields.iter().find(|f| f.name == *field))
                        .map_or(Type::Unknown, |f| f.ty.clone()),
                    _ => self.guess(e).unwrap_or(Type::Unknown),
                }
            }
            ExprKind::Index(base, index) => {
                let actual = self.expression(base, substitutions, None)?;
                self.expression(index, substitutions, Some(&Type::usize()))?;
                match dereferenced(&actual) {
                    Type::Array(_, t) | Type::Slice(_, t) => *t.clone(),
                    _ => Type::Unknown,
                }
            }
            ExprKind::Slice {
                base,
                start,
                end,
                mutable,
            } => {
                let actual = self.expression(base, substitutions, None)?;
                for v in start.iter_mut().chain(end) {
                    self.expression(v, substitutions, Some(&Type::usize()))?;
                }
                match dereferenced(&actual) {
                    Type::Array(_, t) | Type::Slice(_, t) => Type::Slice(*mutable, t.clone()),
                    _ => Type::Unknown,
                }
            }
            ExprKind::Try(value) | ExprKind::Unwrap(value) => {
                let actual = self.expression(value, substitutions, None)?;
                if let Type::Result(t, _) = actual {
                    *t
                } else {
                    Type::Unknown
                }
            }
            ExprKind::ValueBlock(body) => {
                let previous = std::mem::replace(&mut self.yield_type, expected.cloned());
                self.block(body, substitutions)?;
                let result = self.yield_type.take().unwrap_or(Type::Unknown);
                self.yield_type = previous;
                result
            }
        })
    }
    fn inferred_arguments(
        &self,
        parameters: &[String],
        mapping: &HashMap<String, Type>,
        span: Span,
    ) -> Check<Vec<Type>> {
        parameters.iter().map(|n| mapping.get(n).cloned().filter(|t| Self::concrete(t, parameters)).ok_or_else(|| Diagnostic::new(span, format!("cannot infer generic parameter `{n}`; supply explicit type arguments")))).collect()
    }
}

fn specialized(name: &str, arguments: &[Type]) -> String {
    let mut result = format!("{name}$");
    for (index, ty) in arguments.iter().enumerate() {
        if index > 0 {
            result.push('_');
        }
        for byte in ty.to_string().bytes() {
            use std::fmt::Write;
            write!(&mut result, "{byte:02x}").unwrap();
        }
    }
    result
}

// Intrinsic return shapes are needed before full checking so generic callers can
// infer their parameters from an intrinsic expression.
pub(super) fn intrinsic_result_type(name: &str, types: &[Type], args: &[Type]) -> Option<Type> {
    if matches!(
        name,
        "core.wrapping_add" | "core.wrapping_sub" | "core.wrapping_mul"
    ) {
        return Some(
            types
                .first()
                .or_else(|| args.iter().find(|t| **t != Type::Unknown))
                .cloned()
                .unwrap_or(Type::Unknown),
        );
    }
    let name = name.strip_prefix("core.").unwrap_or(name);
    let first = args.first().cloned().unwrap_or(Type::Unknown);
    let explicit = types.first().cloned().unwrap_or(Type::Unknown);
    Some(match name {
        "mem.split_at_mut" => explicit,
        "assert" | "assert_eq" | "assert_ne" => Type::Void,
        "mem.assert_send" | "mem.assert_sync" | "mem.atomic_store" => Type::Void,
        "mem.callback" => Type::Raw(false, Box::new(Type::u8())),
        "mem.atomic_load"
        | "mem.atomic_exchange"
        | "mem.atomic_fetch_add"
        | "mem.atomic_compare_exchange" => match first {
            Type::Raw(_, t) => *t,
            _ => explicit,
        },
        "mem.str_bytes" => Type::Slice(false, Box::new(Type::u8())),
        "mem.str_from_utf8" => Type::Str,
        "ptr.view"
        | "ptr.view_slice"
        | "ptr.borrow"
        | "ptr.borrow_mut"
        | "ptr.borrow_slice"
        | "ptr.borrow_slice_mut" => {
            let element = match first {
                Type::Raw(_, t) => t,
                _ => Box::new(explicit),
            };
            if name.contains("slice") {
                Type::Slice(name.ends_with("mut"), element)
            } else {
                Type::Ref(name.ends_with("mut"), element)
            }
        }
        "mem.size_of" | "mem.align_of" | "mem.offset_of" => Type::usize(),
        "mem.uninit" => Type::MaybeUninit(Box::new(explicit)),
        "mem.init" => Type::MaybeUninit(Box::new(if first == Type::Unknown {
            explicit
        } else {
            first
        })),
        "mem.assume_init" => match first {
            Type::MaybeUninit(t) => *t,
            _ => explicit,
        },
        "mem.uninit_as_ptr" | "mem.uninit_as_mut_ptr" => match first {
            Type::Ref(_, t) => match *t {
                Type::MaybeUninit(t) => Type::Raw(name.ends_with("mut_ptr"), t),
                _ => Type::Unknown,
            },
            _ => Type::Unknown,
        },
        "mem.replace" => match first {
            Type::Ref(_, t) => *t,
            _ => explicit,
        },
        "mem.storage_type"
        | "ptr.store"
        | "ptr.relocate"
        | "mem.swap"
        | "ptr.write"
        | "ptr.write_unaligned"
        | "ptr.write_volatile"
        | "ptr.copy"
        | "ptr.copy_nonoverlapping"
        | "ptr.write_bytes"
        | "ptr.drop_in_place" => Type::Void,
        "ptr.from_ref" | "ptr.from_mut" => match first {
            Type::Ref(_, t) => Type::Raw(name.ends_with("mut"), t),
            _ => Type::Unknown,
        },
        "ptr.as_ptr" | "ptr.as_mut_ptr" => match first {
            Type::Slice(_, t) => Type::Raw(name.ends_with("mut_ptr"), t),
            Type::Ref(_, t) => match *t {
                Type::Array(_, t) => Type::Raw(name.ends_with("mut_ptr"), t),
                _ => Type::Unknown,
            },
            _ => Type::Unknown,
        },
        "ptr.is_null" => Type::Bool,
        "ptr.offset" => first,
        "ptr.take" | "ptr.read" | "ptr.read_unaligned" | "ptr.read_volatile" => match first {
            Type::Raw(_, t) => *t,
            _ => explicit,
        },
        _ => return None,
    })
}
