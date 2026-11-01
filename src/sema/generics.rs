//! Generic specialization before ordinary checking, including provisional type shapes.
//! Every generated body still passes through the production checker.
use super::{Check, Obligation, dereferenced, match_subject, qualified_name};
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::{HashMap, HashSet};

#[path = "pio.rs"]
mod pio;
#[path = "printing.rs"]
mod printing;

// Composed standard-library apps need many independent specializations. Bound
// total work separately from recursive expansion so that raising the breadth
// limit does not allow runaway generic recursion to exhaust the compiler stack.
const MAX_SPECIALIZATIONS: usize = 4096;
const MAX_SPECIALIZATION_DEPTH: usize = 64;

/// Explicit type arguments specialize generic declarations before type checking.
/// Every emitted specialization goes through the same ordinary checker.
/// Returns the interface bounds each specialization must satisfy.
pub(super) fn instantiate(program: &mut Program) -> Check<Vec<Obligation>> {
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
        depth: 0,
        printing_count: 0,
        pio_version: program.pio_version,
        obligations: vec![],
        required: HashSet::new(),
        pending: vec![],
    };
    for interface in &mut program.interfaces {
        for method in &mut interface.methods {
            for param in &mut method.params {
                expander.ty(&mut param.ty, &HashMap::new(), param.span)?;
            }
            expander.ty(&mut method.ret, &HashMap::new(), method.span)?;
        }
    }
    program.structs.retain(|s| s.generics.is_empty());
    program.enums.retain(|e| e.generics.is_empty());
    program.functions.retain(|f| {
        f.generics.is_empty() && (f.printing.is_none() || f.generic_instance) && !f.pio
    });
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
        let ty = constant.ty.clone();
        expander.expression(&mut constant.value, &substitutions, Some(&ty))?;
    }
    program.structs.append(&mut expander.structs);
    program.enums.append(&mut expander.enums);
    program.functions.append(&mut expander.functions);
    Ok(expander.obligations)
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
    depth: usize,
    printing_count: usize,
    /// The build chip's PIO version, for `rp_pio.assemble`.
    pio_version: Option<u8>,
    obligations: Vec<Obligation>,
    required: HashSet<(String, String, String)>,
    // Generic parameters of the call whose arguments are being inferred.
    pending: Vec<String>,
}
impl Expander {
    /// A generic struct declaration; every tuple arity has an implicit one with
    /// public fields `0`, `1`, ... of types `T0`, `T1`, ...
    fn struct_template(&mut self, name: &str) -> Option<Struct> {
        if let Some(template) = self.struct_templates.get(name) {
            return Some(template.clone());
        }
        let arity = tuple_arity(name).filter(|_| !name.contains('$'))?;
        let generics: Vec<String> = (0..arity).map(|index| format!("T{index}")).collect();
        let template = Struct {
            name: name.to_owned(),
            public: true,
            generics: generics.clone(),
            bounds: vec![],
            implements: vec![],
            fields: generics
                .into_iter()
                .enumerate()
                .map(|(index, parameter)| Field {
                    name: index.to_string(),
                    ty: Type::Named(parameter),
                    public: true,
                    span: Span::default(),
                })
                .collect(),
            span: Span::default(),
            repr_c: false,
            unsafe_send: false,
            unsafe_sync: false,
            interior_mutable: false,
            async_future: false,
        };
        self.struct_templates
            .insert(name.to_owned(), template.clone());
        Some(template)
    }
    /// The function pointer type of a named, non-generic function.
    fn function_value_type(&self, name: &str) -> Option<Type> {
        let function = self.signatures.get(name)?;
        (function.generics.is_empty() && function.printing.is_none() && !function.pio).then(|| {
            let params = function.params.iter().map(|p| p.ty.clone()).collect();
            let ret = Box::new(function.ret.clone());
            if function.extern_ {
                Type::ExternFn(params, ret)
            } else {
                Type::Fn(params, ret)
            }
        })
    }
    /// A named function used as a value. Generic functions are specialized
    /// from the expected function type, and the name becomes the specialization.
    fn function_value(
        &mut self,
        name: &mut String,
        expected: Option<&Type>,
        span: Span,
    ) -> Check<Option<Type>> {
        let Some(template) = self.signatures.get(name.as_str()).cloned() else {
            return Ok(None);
        };
        if template.printing.is_some() {
            return Err(Diagnostic::new(
                span,
                "printing entry points cannot be used as function values; wrap a concrete call in an ordinary function",
            ));
        }
        if template.pio {
            return Err(Diagnostic::new(
                span,
                "rp_pio.assemble runs at compile time and cannot be used as a function value",
            ));
        }
        if template.generics.is_empty() {
            return Ok(self.function_value_type(name));
        }
        let parameters = &template.generics;
        let mut mapping = HashMap::new();
        let pattern = Type::Fn(
            template.params.iter().map(|p| p.ty.clone()).collect(),
            Box::new(template.ret.clone()),
        );
        match expected.map(|t| t.callable_signature()) {
            Some(Some((params, ret))) => {
                let actual = Type::Fn(params.to_vec(), Box::new(ret.clone()));
                self.unify(&pattern, &actual, parameters, &mut mapping, span)?;
            }
            _ => {
                return Err(Diagnostic::new(
                    span,
                    format!(
                        "generic function `{name}` needs a function type from its context to be used as a value"
                    ),
                ));
            }
        }
        let arguments = self.inferred_arguments(parameters, &mapping, span)?;
        self.require_bounds(name, false, parameters, &arguments, &template.bounds, span);
        let concrete = specialized(name, &arguments);
        if self.generated_functions.insert(concrete.clone()) {
            self.begin_specialization(span)?;
            let mut function = template.clone();
            function.name = concrete.clone();
            function.generics.clear();
            function.bounds.clear();
            function.generic_instance = true;
            self.function(&mut function, &mapping)?;
            self.functions.push(function);
            self.depth -= 1;
        }
        *name = concrete;
        let mut ty = Self::substitute(&pattern, &mapping);
        self.ty(&mut ty, &HashMap::new(), span)?;
        Ok(Some(ty))
    }
    /// Annotate a closure literal from its expected type and guess its type.
    fn closure(
        &mut self,
        closure: &mut Closure,
        substitutions: &HashMap<String, Type>,
        expected: Option<&Type>,
    ) -> Check<Type> {
        let hint = expected
            .and_then(|t| t.callable_signature())
            .filter(|(params, _)| params.len() == closure.params.len())
            .map(|(p, r)| (p.to_vec(), r.clone()));
        let pending = self.pending.clone();
        let known = |ty: &Type| ty != &Type::Unknown && Self::concrete(ty, &pending);
        for (i, param) in closure.params.iter_mut().enumerate() {
            self.ty(&mut param.ty, substitutions, param.span)?;
            if param.ty == Type::Unknown
                && let Some((params, _)) = &hint
                && known(&params[i])
            {
                param.ty = params[i].clone();
            }
        }
        self.ty(&mut closure.ret, substitutions, closure.ret_span)?;
        let declared = if closure.ret != Type::Unknown {
            closure.ret.clone()
        } else {
            hint.as_ref()
                .map(|(_, r)| r.clone())
                .filter(|r| known(r))
                .unwrap_or(Type::Unknown)
        };
        let locals = self.locals.clone();
        for param in &closure.params {
            self.locals.insert(param.name.clone(), param.ty.clone());
        }
        let ret = std::mem::replace(&mut self.return_type, declared.clone());
        let yielding = std::mem::replace(
            &mut self.yield_type,
            (declared != Type::Unknown).then(|| declared.clone()),
        );
        let pending = std::mem::take(&mut self.pending);
        let result = self.block(&mut closure.body, substitutions);
        self.pending = pending;
        let returned = std::mem::replace(&mut self.return_type, ret);
        let yielded = std::mem::replace(&mut self.yield_type, yielding);
        self.locals = locals;
        result?;
        let ty = if declared != Type::Unknown {
            declared
        } else if let Some(ty) = yielded.filter(|t| *t != Type::Unknown) {
            ty
        } else if returned != Type::Unknown {
            returned
        } else if super::closures::has_tail(&closure.body)
            || super::closures::returns(&closure.body)
        {
            Type::Unknown
        } else {
            Type::Void
        };
        Ok(Type::Fn(
            closure.params.iter().map(|p| p.ty.clone()).collect(),
            Box::new(ty),
        ))
    }
    // Record each specialization's bounds once per type, interface, and owner.
    fn require_bounds(
        &mut self,
        owner: &str,
        owner_is_type: bool,
        parameters: &[String],
        arguments: &[Type],
        bounds: &[Bound],
        span: Span,
    ) {
        for bound in bounds {
            let Some(index) = parameters.iter().position(|p| *p == bound.param) else {
                continue;
            };
            let Some(ty) = arguments.get(index) else {
                continue;
            };
            for interface in &bound.interfaces {
                if self
                    .required
                    .insert((ty.to_string(), interface.clone(), owner.to_owned()))
                {
                    self.obligations.push(Obligation {
                        ty: ty.clone(),
                        interface: interface.clone(),
                        span,
                        bound: bound.span,
                        param: bound.param.clone(),
                        owner: owner.to_owned(),
                        owner_is_type,
                    });
                }
            }
        }
    }
    fn begin_specialization(&mut self, span: Span) -> Check<()> {
        self.count += 1;
        if self.count > MAX_SPECIALIZATIONS {
            return Err(Diagnostic::new(
                span,
                format!(
                    "generic instantiation limit exceeded ({MAX_SPECIALIZATIONS} specializations)"
                ),
            ));
        }
        if self.depth == MAX_SPECIALIZATION_DEPTH {
            return Err(Diagnostic::new(
                span,
                format!(
                    "generic instantiation depth exceeded ({MAX_SPECIALIZATION_DEPTH} nested specializations)"
                ),
            ));
        }
        self.depth += 1;
        Ok(())
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
            Type::Fn(params, ret) | Type::DynFn(params, ret) | Type::ExternFn(params, ret) => {
                for param in params.iter_mut() {
                    self.ty(param, substitutions, span)?;
                }
                self.ty(ret, substitutions, span)?;
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
                    self.begin_specialization(span)?;
                    if let Some(mut structure) = self.struct_template(name) {
                        let mapping = self.substitutions(&structure.generics, arguments, span)?;
                        self.require_bounds(
                            name,
                            true,
                            &structure.generics,
                            arguments,
                            &structure.bounds,
                            span,
                        );
                        let original_generics = structure.generics.clone();
                        structure.name = concrete.clone();
                        structure.generics.clear();
                        structure.bounds.clear();
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
                            method.bounds.clear();
                            method.generic_instance = true;
                            let mut mapping = mapping.clone();
                            mapping.insert(name.clone(), Type::Named(concrete.clone()));
                            if self.generated_functions.insert(method.name.clone()) {
                                self.begin_specialization(method.span)?;
                                self.function(&mut method, &mapping)?;
                                self.functions.push(method);
                                self.depth -= 1;
                            }
                        }
                    } else if let Some(mut enumeration) = self.enum_templates.get(name).cloned() {
                        let mapping = self.substitutions(&enumeration.generics, arguments, span)?;
                        self.require_bounds(
                            name,
                            true,
                            &enumeration.generics,
                            arguments,
                            &enumeration.bounds,
                            span,
                        );
                        enumeration.name = concrete.clone();
                        enumeration.generics.clear();
                        enumeration.bounds.clear();
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
                    self.depth -= 1;
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
                let expected = self.return_type.clone();
                let actual = self.expression(
                    e,
                    substitutions,
                    (expected != Type::Unknown).then_some(&expected),
                )?;
                // A closure without a declared result infers it from its returns.
                if expected == Type::Unknown {
                    self.return_type = actual;
                }
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
            Type::Fn(params, ret) => Type::Fn(
                params
                    .iter()
                    .map(|t| Self::substitute(t, mapping))
                    .collect(),
                Box::new(Self::substitute(ret, mapping)),
            ),
            Type::DynFn(params, ret) => Type::DynFn(
                params
                    .iter()
                    .map(|t| Self::substitute(t, mapping))
                    .collect(),
                Box::new(Self::substitute(ret, mapping)),
            ),
            Type::ExternFn(params, ret) => Type::ExternFn(
                params
                    .iter()
                    .map(|t| Self::substitute(t, mapping))
                    .collect(),
                Box::new(Self::substitute(ret, mapping)),
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
            Type::Fn(params, ret) | Type::DynFn(params, ret) | Type::ExternFn(params, ret) => {
                params.iter().all(|t| Self::concrete(t, parameters))
                    && Self::concrete(ret, parameters)
            }
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
            (Type::Fn(ps, p) | Type::DynFn(ps, p), Type::Fn(args, a) | Type::DynFn(args, a))
                if ps.len() == args.len() =>
            {
                for (p, a) in ps.iter().zip(args) {
                    self.unify(p, a, parameters, mapping, span)?;
                }
                self.unify(p, a, parameters, mapping, span)?;
            }
            (Type::ExternFn(ps, p), Type::ExternFn(args, a)) if ps.len() == args.len() => {
                for (p, a) in ps.iter().zip(args) {
                    self.unify(p, a, parameters, mapping, span)?;
                }
                self.unify(p, a, parameters, mapping, span)?;
            }
            // A function pointer converts to a closure reference.
            (Type::Ref(_, p), Type::Fn(..)) if matches!(p.as_ref(), Type::DynFn(..)) => {
                self.unify(p, &actual, parameters, mapping, span)?
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
                .cloned()
                .or_else(|| self.function_value_type(n)),
            ExprKind::Field(..)
                if let Some(path) = qualified_name(e)
                    && !self
                        .locals
                        .contains_key(path.split('.').next().unwrap_or(""))
                    && let Some(ty) = self.function_value_type(&path) =>
            {
                Some(ty)
            }
            ExprKind::Closure(closure)
                if closure.ret != Type::Unknown
                    && closure.params.iter().all(|p| p.ty != Type::Unknown) =>
            {
                Some(Type::Fn(
                    closure.params.iter().map(|p| p.ty.clone()).collect(),
                    Box::new(closure.ret.clone()),
                ))
            }
            ExprKind::Asm(asm) => asm
                .operands
                .iter()
                .filter_map(|operand| match &operand.kind {
                    AsmOperandKind::Out(ty) => Some(Some(ty.clone())),
                    AsmOperandKind::InOut(value) => Some(self.guess(value)),
                    AsmOperandKind::In(_) | AsmOperandKind::Sym(_) | AsmOperandKind::Const(_) => {
                        None
                    }
                })
                .collect::<Option<Vec<_>>>()
                .map(asm_result),
            ExprKind::Invoke { callee, .. } => self
                .guess(callee)
                .and_then(|t| t.call_signature().map(|(_, r)| r.clone())),
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
            ExprKind::Await { value, .. } => self.guess(value).map(|t| self.awaited(&t)),
            _ => None,
        }
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
                || substitutions.contains_key(&prefix)
                || self.known_enums.contains_key(&prefix)
                || intrinsic_result_type(&format!("{prefix}.{name}"), &[], &[]).is_some())
        {
            e.kind = ExprKind::Call {
                name: format!("{prefix}.{name}"),
                type_args: vec![],
                args: std::mem::take(args),
            };
        }
        if let ExprKind::Call {
            name,
            type_args,
            args,
        } = &e.kind
            && let Some(template) = self.signatures.get(name)
            && template.pio
        {
            e.kind = self.pio_program(template, type_args, args, span)?;
        }
        // A qualified function used as a value, such as `math.square`.
        if let ExprKind::Field(..) = &e.kind
            && let Some(path) = qualified_name(e)
            && !self
                .locals
                .contains_key(path.split('.').next().unwrap_or(""))
            && self.signatures.contains_key(&path)
        {
            e.kind = ExprKind::Name(path);
        }
        // A local function value shadows any function of the same name, and a
        // constant or static holding a function value is called the same way.
        if let ExprKind::MethodCall {
            receiver,
            name,
            args,
        } = &mut e.kind
            && let Some(prefix) = qualified_name(receiver)
            && !self.locals.contains_key(&prefix)
            && self
                .constants
                .get(&format!("{prefix}.{name}"))
                .is_some_and(|ty| ty.call_signature().is_some())
        {
            e.kind = ExprKind::Call {
                name: format!("{prefix}.{name}"),
                type_args: vec![],
                args: std::mem::take(args),
            };
        }
        if let ExprKind::Call {
            name,
            type_args,
            args,
        } = &mut e.kind
            && type_args.is_empty()
            && (self.locals.contains_key(name.as_str())
                || self
                    .constants
                    .get(name.as_str())
                    .is_some_and(|ty| ty.call_signature().is_some()))
        {
            e.kind = ExprKind::Invoke {
                callee: Box::new(Expr::new(ExprKind::Name(std::mem::take(name)), span)),
                args: std::mem::take(args),
            };
        }
        // `value.field(args)` calls a function-valued field when no method exists.
        if let ExprKind::MethodCall {
            receiver,
            name,
            args,
        } = &mut e.kind
            && let Some(actual) = self.guess(receiver)
            && let Type::Named(owner) = dereferenced(&actual)
            && !self.signatures.contains_key(&format!("{owner}.{name}"))
            && self.known_structs.get(owner).is_some_and(|s| {
                s.fields
                    .iter()
                    .any(|f| f.name == *name && f.ty.callable_signature().is_some())
            })
        {
            let field = Expr::new(
                ExprKind::Field(
                    Box::new(std::mem::replace(
                        receiver.as_mut(),
                        Expr::new(ExprKind::Bool(false), span),
                    )),
                    std::mem::take(name),
                ),
                span,
            );
            e.kind = ExprKind::Invoke {
                callee: Box::new(field),
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
            ExprKind::Name(n) => {
                if let Some(ty) = self.locals.get(n).or_else(|| self.constants.get(n)) {
                    ty.clone()
                } else if let Some(ty) = self.function_value(n, expected, span)? {
                    ty
                } else {
                    expected.cloned().unwrap_or(Type::Unknown)
                }
            }
            ExprKind::Closure(closure) => self.closure(closure, substitutions, expected)?,
            ExprKind::Asm(asm) => {
                let mut outputs = vec![];
                for operand in &mut asm.operands {
                    match &mut operand.kind {
                        AsmOperandKind::In(value) => {
                            self.expression(value, substitutions, None)?;
                        }
                        AsmOperandKind::InOut(value) => {
                            outputs.push(self.expression(value, substitutions, None)?);
                        }
                        AsmOperandKind::Out(ty) => {
                            self.ty(ty, substitutions, operand.span)?;
                            outputs.push(ty.clone());
                        }
                        AsmOperandKind::Const(value) => {
                            self.expression(value, substitutions, None)?;
                        }
                        AsmOperandKind::Sym(_) => {}
                    }
                }
                let several = outputs.len() > 1;
                let mut ty = asm_result(outputs);
                if several {
                    self.ty(&mut ty, substitutions, span)?;
                    asm.tuple = Some(ty.clone());
                }
                ty
            }
            ExprKind::Invoke { callee, args } => {
                let callee = self.expression(callee, substitutions, None)?;
                let signature = callee
                    .call_signature()
                    .map(|(p, r)| (p.to_vec(), r.clone()));
                for (i, arg) in args.iter_mut().enumerate() {
                    let hint = signature.as_ref().and_then(|(p, _)| p.get(i));
                    self.expression(arg, substitutions, hint)?;
                }
                signature.map_or(Type::Unknown, |(_, r)| r)
            }
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
                    if hint.is_none() && Self::concrete(&actual, &[]) {
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
                    && let Some(template) = self.struct_template(owner)
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
                    // An expected tuple type fixes its element types; the checker
                    // then reports a mismatched element where it is written.
                    let fixed: HashSet<String> = if tuple_arity(owner).is_some() {
                        mapping.keys().cloned().collect()
                    } else {
                        HashSet::new()
                    };
                    let inferred =
                        |field: &Field| !matches!(&field.ty, Type::Named(p) if fixed.contains(p));
                    for (n, v) in fields.iter() {
                        if let Some(field) = template.fields.iter().find(|f| f.name == *n)
                            && inferred(field)
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
                        if let Some(field) = field
                            && inferred(field)
                        {
                            self.unify(&field.ty, &actual, parameters, &mut mapping, span)?;
                        }
                    }
                    let arguments = if tuple_arity(owner).is_some() {
                        // Tuple parameters are implicit, so name the element instead.
                        parameters
                            .iter()
                            .enumerate()
                            .map(|(index, n)| {
                                mapping.get(n).cloned().filter(|t| Self::concrete(t, parameters)).ok_or_else(|| {
                                    Diagnostic::new(
                                        span,
                                        format!("cannot infer the type of tuple element {index}; annotate the binding or the element"),
                                    )
                                })
                            })
                            .collect::<Check<Vec<_>>>()?
                    } else {
                        self.inferred_arguments(parameters, &mapping, span)?
                    };
                    ty = Type::Generic(owner.clone(), arguments);
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
                // Static methods on a type parameter must follow the same
                // substitution as parameter and return types. This enables
                // ordinary checked protocols such as T.decode_json(value).
                if let Some((owner, method)) = name.rsplit_once('.')
                    && !self.signatures.contains_key(name)
                    && let Some(replacement) = substitutions.get(owner).cloned()
                {
                    let method = method.to_owned();
                    let mut owner_type = replacement;
                    self.ty(&mut owner_type, substitutions, span)?;
                    let Type::Named(owner) = owner_type else {
                        return Err(Diagnostic::new(
                            span,
                            format!("type `{owner_type}` has no static method `{method}`"),
                        ));
                    };
                    *name = format!("{owner}.{method}");
                }
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
                    if template.pio {
                        return Err(Diagnostic::new(
                            span,
                            "rp_pio.assemble runs at compile time and cannot be used as a callback",
                        ));
                    }
                    let mapping = self.substitutions(&template.generics, type_args, span)?;
                    self.require_bounds(
                        &target,
                        false,
                        &template.generics,
                        type_args,
                        &template.bounds,
                        span,
                    );
                    let concrete = if template.generics.is_empty() {
                        target
                    } else {
                        let concrete = specialized(&target, type_args);
                        if self.generated_functions.insert(concrete.clone()) {
                            self.begin_specialization(span)?;
                            let mut function = template;
                            function.name = concrete.clone();
                            function.generics.clear();
                            function.bounds.clear();
                            function.generic_instance = true;
                            self.function(&mut function, &mapping)?;
                            self.functions.push(function);
                            self.depth -= 1;
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
                    // Closure literals take their parameter types from the
                    // arguments before them, so check them after the others.
                    let closure_last: Vec<_> = (0..args.len())
                        .filter(|&i| !matches!(args[i].kind, ExprKind::Closure(_)))
                        .chain(
                            (0..args.len())
                                .filter(|&i| matches!(args[i].kind, ExprKind::Closure(_))),
                        )
                        .collect();
                    for i in closure_last {
                        let arg = &mut args[i];
                        let param = template.params.get(i);
                        let hint = param
                            .map(|p| Self::substitute(&p.ty, &mapping))
                            .filter(|t| {
                                Self::concrete(t, parameters)
                                    || matches!(arg.kind, ExprKind::Closure(_))
                                        && t.callable_signature().is_some()
                            });
                        let pending = std::mem::replace(&mut self.pending, parameters.clone());
                        let actual = self.expression(arg, substitutions, hint.as_ref());
                        self.pending = pending;
                        let actual = actual?;
                        if !explicit && let Some(param) = param {
                            self.unify(&param.ty, &actual, parameters, &mut mapping, span)?;
                        }
                    }
                    let mut ret = Self::substitute(&template.ret, &mapping);
                    if !parameters.is_empty() {
                        let arguments = self.inferred_arguments(parameters, &mapping, span)?;
                        self.require_bounds(
                            name,
                            false,
                            parameters,
                            &arguments,
                            &template.bounds,
                            span,
                        );
                        let concrete = specialized(name, &arguments);
                        if self.generated_functions.insert(concrete.clone()) {
                            self.begin_specialization(span)?;
                            let mut function = template;
                            function.name = concrete.clone();
                            function.generics.clear();
                            function.bounds.clear();
                            function.generic_instance = true;
                            self.function(&mut function, &mapping)?;
                            self.functions.push(function);
                            self.depth -= 1;
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
            ExprKind::Await { value, .. } => {
                let future = self.expression(value, substitutions, None)?;
                self.awaited(&future)
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
    /// What `await` on a value of type `future` yields: its `output`
    /// method's result, or void.
    fn awaited(&self, future: &Type) -> Type {
        match future {
            Type::Named(name) => self
                .signatures
                .get(&format!("{name}.output"))
                .map_or(Type::Void, |output| output.ret.clone()),
            _ => Type::Unknown,
        }
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

/// The source spelling of a type that may name specializations: `W$7538`
/// becomes `W<u8>`, including specializations nested in arguments.
pub(super) fn source_spelling(ty: &str) -> String {
    let mut result = String::new();
    let mut word = String::new();
    for c in ty.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.') {
            word.push(c);
            continue;
        }
        match word.split_once('$') {
            Some((base, arguments)) => {
                let decoded: Option<Vec<_>> = arguments
                    .split('_')
                    .map(|hex| {
                        let bytes = (0..hex.len())
                            .step_by(2)
                            .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
                            .collect::<Option<Vec<_>>>()?;
                        String::from_utf8(bytes).ok().map(|t| source_spelling(&t))
                    })
                    .collect();
                match decoded {
                    Some(arguments) => {
                        result.push_str(&format!("{base}<{}>", arguments.join(", ")))
                    }
                    None => result.push_str(&word),
                }
            }
            None => result.push_str(&word),
        }
        word.clear();
        result.push(c);
    }
    result.pop();
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
