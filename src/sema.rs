//! Name resolution, type checking, ownership, and conservative borrow checking.
//!
//! Loans carry their source places through references and aggregates. Liveness is
//! based on future uses, extended across loop back edges and custom destruction.
//! This is an intentionally conservative checker, not a formal safety proof.
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::{HashMap, HashSet};

type Check<T> = Result<T, Diagnostic>;

pub fn check(program: &mut Program) -> Check<()> {
    check_for_target(program, 64)
}

pub fn check_for_target(program: &mut Program, pointer_bits: u32) -> Check<()> {
    crate::prepare::prepare(program, pointer_bits)?;
    instantiate(program)?;
    let context = Context::new(program, pointer_bits)?;
    // Publish inferred contracts before checking bodies, so editor queries can
    // inspect signatures even when a body has a diagnostic.
    for function in &mut program.functions {
        function.from = context.functions[&function.name].from.clone();
    }
    for constant in &mut program.constants {
        let mut checker = Checker::new(&context, Type::Void, vec![], HashMap::new());
        let value = checker.expr(&mut constant.value, Some(&constant.ty), true)?;
        checker.expect(&constant.ty, &value.ty, constant.span)?;
        if !constant_expression(&constant.value) {
            return Err(Diagnostic::new(
                constant.span,
                "global initializers must be compile-time constants",
            ));
        }
        validate_constant(&constant.value, pointer_bits)?;
    }
    for function in &mut program.functions {
        if function.body.is_none() {
            continue;
        }
        let mut uses = HashMap::new();
        names_block(function.body.as_ref().unwrap(), &mut uses);
        let mut checker = Checker::new(
            &context,
            function.ret.clone(),
            context.functions[&function.name].from.clone(),
            uses,
        );
        checker.namespace = context.function_namespace(&function.name);
        checker.binding_uses = binding_use_spans(function);
        checker.return_contract = Some(function.from_span.unwrap_or(function.ret_span));
        checker.inferred_contract = function.from_span.is_none();
        for parameter in &function.params {
            let id = checker.next_id;
            let deps = if context.carries_borrow(&parameter.ty) {
                vec![Loan {
                    root: usize::MAX / 2 + id,
                    fields: vec![],
                    mutable: mutable_borrow(&parameter.ty),
                    origin: parameter.span,
                    via: vec![],
                    external: Some(parameter.name.clone()),
                }]
            } else {
                vec![]
            };
            checker.bind(
                parameter.name.clone(),
                parameter.ty.clone(),
                Some(deps),
                false,
                parameter.span,
            )?;
        }
        let terminates = checker.block(function.body.as_mut().unwrap(), false)?;
        if function.ret != Type::Void && !terminates {
            return Err(Diagnostic::new(
                function.span,
                format!(
                    "function `{}` can reach its end without returning {}",
                    function.name, function.ret
                ),
            ));
        }
        checker.finish_scope(function.span)?;
    }
    Ok(())
}

pub(crate) fn array_length(expression: &mut Expr, bits: u32) -> Check<usize> {
    let context = Context::new(&Program::default(), bits)?;
    let mut checker = Checker::new(&context, Type::Void, vec![], HashMap::new());
    let value = checker.expr(expression, None, false)?;
    if !value.ty.is_integer() || !constant_expression(expression) {
        return Err(Diagnostic::new(
            expression.span,
            "array length must be an integer constant expression",
        ));
    }
    let crate::consteval::Scalar::Int(n) = crate::consteval::eval(expression, bits)
        .map_err(|e| Diagnostic::new(expression.span, e))?
    else {
        unreachable!()
    };
    if n < 0 || n > u32::MAX as i128 || n >= (1i128 << bits) {
        return Err(Diagnostic::new(
            expression.span,
            "array length must be nonnegative and fit the target and LLVM array limit",
        ));
    }
    Ok(n as usize)
}

#[derive(Clone)]
struct Signature {
    params: Vec<Param>,
    ret: Type,
    from: Vec<String>,
    unsafe_: bool,
    extern_: bool,
    public: bool,
}
struct Context {
    structs: HashMap<String, Struct>,
    enums: HashMap<String, Enum>,
    functions: HashMap<String, Signature>,
    constants: HashMap<String, Constant>,
    pointer_bits: u32,
    imports: HashSet<String>,
}
impl Context {
    fn new(program: &Program, pointer_bits: u32) -> Check<Self> {
        let mut context = Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
            functions: HashMap::new(),
            constants: HashMap::new(),
            pointer_bits,
            imports: program.imports.iter().cloned().collect(),
        };
        let mut names = HashSet::new();
        for declaration in &program.structs {
            if !names.insert(declaration.name.clone()) {
                return Err(Diagnostic::new(
                    declaration.span,
                    format!("duplicate declaration `{}`", declaration.name),
                ));
            }
            context
                .structs
                .insert(declaration.name.clone(), declaration.clone());
        }
        for declaration in &program.enums {
            if !names.insert(declaration.name.clone()) {
                return Err(Diagnostic::new(
                    declaration.span,
                    format!("duplicate declaration `{}`", declaration.name),
                ));
            }
            context
                .enums
                .insert(declaration.name.clone(), declaration.clone());
        }
        for constant in &program.constants {
            if !names.insert(constant.name.clone()) {
                return Err(Diagnostic::new(
                    constant.span,
                    format!("duplicate declaration `{}`", constant.name),
                ));
            }
            context
                .constants
                .insert(constant.name.clone(), constant.clone());
        }
        for declaration in &program.structs {
            let mut fields = HashSet::new();
            for field in &declaration.fields {
                if !fields.insert(&field.name) {
                    return Err(Diagnostic::new(
                        field.span,
                        format!("duplicate field `{}`", field.name),
                    ));
                }
                context.validate_type(&field.ty, field.span, false)?;
                if declaration.public && field.public {
                    context.public_type(&field.ty, field.span)?;
                }
            }
            context.no_value_cycle(
                &Type::Named(declaration.name.clone()),
                &mut HashSet::new(),
                declaration.span,
            )?;
        }
        for declaration in &program.enums {
            if declaration.variants.is_empty() {
                return Err(Diagnostic::new(
                    declaration.span,
                    "an enum must have at least one variant",
                ));
            }
            let mut variants = HashSet::new();
            for variant in &declaration.variants {
                if !variants.insert(&variant.name) {
                    return Err(Diagnostic::new(
                        variant.span,
                        format!("duplicate variant `{}`", variant.name),
                    ));
                }
                for field in &variant.fields {
                    context.validate_type(&field.ty, field.span, false)?;
                    if declaration.public {
                        context.public_type(&field.ty, field.span)?;
                    }
                }
            }
            context.no_value_cycle(
                &Type::Named(declaration.name.clone()),
                &mut HashSet::new(),
                declaration.span,
            )?;
        }
        for constant in &program.constants {
            context.validate_type(&constant.ty, constant.span, false)?;
            if constant.public {
                context.public_type(&constant.ty, constant.span)?;
            }
        }
        for function in &program.functions {
            if !names.insert(function.name.clone()) {
                return Err(Diagnostic::new(
                    function.span,
                    format!("duplicate declaration `{}`", function.name),
                ));
            }
            let mut parameters = HashSet::new();
            for param in &function.params {
                if !parameters.insert(&param.name) {
                    return Err(Diagnostic::new(
                        param.span,
                        format!("duplicate parameter `{}`", param.name),
                    ));
                }
                context.validate_type(&param.ty, param.span, false)?;
                if function.public {
                    context.public_type(&param.ty, param.span)?;
                }
            }
            context.validate_type(&function.ret, function.span, true)?;
            if function.public {
                context.public_type(&function.ret, function.span)?;
            }
            let mut from = function.from.clone();
            if context.carries_borrow(&function.ret) {
                if from.is_empty() {
                    if let Some(receiver) = function
                        .params
                        .first()
                        .filter(|p| p.name == "self" && context.carries_borrow(&p.ty))
                    {
                        from.push(receiver.name.clone());
                    } else {
                        let sources: Vec<_> = function
                            .params
                            .iter()
                            .filter(|p| context.carries_borrow(&p.ty))
                            .collect();
                        if sources.len() != 1 {
                            let mut diagnostic = Diagnostic::new(
                                function.ret_span,
                                "borrowed return needs an explicit `from(...)` contract",
                            )
                            .primary_label("specify the sources of this borrowed return");
                            for source in sources {
                                diagnostic = diagnostic.label(
                                    source.span,
                                    format!("possible borrowed source `{}`", source.name),
                                );
                            }
                            return Err(diagnostic.note("name its borrowed input sources, or use `from(static)` for program-lifetime storage"));
                        }
                        from.push(sources[0].name.clone());
                    }
                }
                for source in &from {
                    if source != "static"
                        && !function
                            .params
                            .iter()
                            .any(|p| p.name == *source && context.carries_borrow(&p.ty))
                    {
                        let mut diagnostic = Diagnostic::new(
                            function.from_span.unwrap_or(function.ret_span),
                            format!("borrow source `{source}` is not a borrow-carrying parameter"),
                        )
                        .primary_label(format!(
                            "`{source}` cannot be used as a borrowed-return source"
                        ))
                        .label(function.ret_span, "borrowed return type is declared here");
                        if let Some(parameter) = function
                            .params
                            .iter()
                            .find(|parameter| parameter.name == *source)
                        {
                            diagnostic = diagnostic.label(
                                parameter.span,
                                format!("`{source}` does not carry a borrow"),
                            );
                        }
                        return Err(diagnostic);
                    }
                }
            } else if !from.is_empty() {
                return Err(Diagnostic::new(
                    function.from_span.unwrap_or(function.ret_span),
                    "`from(...)` requires a borrow-carrying return type",
                )
                .primary_label("borrow sources do not apply to this return type")
                .label(
                    function.ret_span,
                    "this return type does not carry a borrow",
                ));
            }
            if function.name.ends_with(".drop") {
                let owner = function.name.trim_end_matches(".drop");
                if function.params.len() != 1
                    || function.params[0].name != "self"
                    || function.params[0].ty != Type::Ref(true, Box::new(Type::Named(owner.into())))
                    || function.ret != Type::Void
                    || function.unsafe_
                    || function.extern_
                {
                    return Err(Diagnostic::new(
                        function.span,
                        "drop must have signature `fn drop(self: &mut Self) -> void`",
                    ));
                }
            }
            if function.extern_ {
                for param in &function.params {
                    context.ffi_type(&param.ty, param.span)?;
                }
                context.ffi_type(&function.ret, function.span)?;
            }
            context.functions.insert(
                function.name.clone(),
                Signature {
                    params: function.params.clone(),
                    ret: function.ret.clone(),
                    from,
                    unsafe_: function.unsafe_,
                    extern_: function.extern_,
                    public: function.public,
                },
            );
        }
        Ok(context)
    }
    fn validate_type(&self, ty: &Type, span: Span, void: bool) -> Check<()> {
        match ty {
            Type::Unknown => Err(Diagnostic::new(span, "a concrete type is required")),
            Type::Void if !void => Err(Diagnostic::new(
                span,
                "void is only valid as a return or Result success type",
            )),
            Type::Named(name)
                if !self.structs.contains_key(name) && !self.enums.contains_key(name) =>
            {
                Err(Diagnostic::new(span, format!("unknown type `{name}`")))
            }
            Type::Generic(..) => Err(Diagnostic::new(span, "unresolved generic type")),
            Type::Array(_, t)
            | Type::Slice(_, t)
            | Type::Ref(_, t)
            | Type::Raw(_, t)
            | Type::Option(t) => self.validate_type(t, span, false),
            Type::Result(t, e) => {
                self.validate_type(t, span, true)?;
                self.validate_type(e, span, false)
            }
            _ => Ok(()),
        }
    }
    fn public_type(&self, ty: &Type, span: Span) -> Check<()> {
        match ty {
            Type::Named(name)
                if self.structs.get(name).is_some_and(|s| !s.public)
                    || self.enums.get(name).is_some_and(|e| !e.public) =>
            {
                return Err(Diagnostic::new(
                    span,
                    format!("public API exposes private type `{name}`"),
                ));
            }
            Type::Array(_, t)
            | Type::Slice(_, t)
            | Type::Ref(_, t)
            | Type::Raw(_, t)
            | Type::Option(t) => self.public_type(t, span)?,
            Type::Result(t, e) => {
                self.public_type(t, span)?;
                self.public_type(e, span)?;
            }
            _ => (),
        }
        Ok(())
    }
    fn ffi_type(&self, ty: &Type, span: Span) -> Check<()> {
        match ty {
            Type::Void | Type::Bool | Type::Int { .. } | Type::Float(_) | Type::Raw(..) => Ok(()),
            // Aggregate ABI classification is target-dependent and deliberately explicit.
            _ => Err(Diagnostic::new(
                span,
                "C ABI parameters and results currently require primitive or raw-pointer types",
            )
            .note("pass a pointer to a @repr(C) struct instead of passing it by value")),
        }
    }
    fn no_value_cycle(&self, ty: &Type, seen: &mut HashSet<String>, span: Span) -> Check<()> {
        match ty {
            Type::Named(name) => {
                if !seen.insert(name.clone()) {
                    return Err(Diagnostic::new(
                        span,
                        format!("type `{name}` has an infinitely sized by-value cycle"),
                    ));
                }
                if let Some(s) = self.structs.get(name) {
                    for f in &s.fields {
                        self.no_value_cycle(&f.ty, seen, span)?;
                    }
                }
                if let Some(e) = self.enums.get(name) {
                    for v in &e.variants {
                        for f in &v.fields {
                            self.no_value_cycle(&f.ty, seen, span)?;
                        }
                    }
                }
                seen.remove(name);
            }
            Type::Array(_, t) | Type::Option(t) => self.no_value_cycle(t, seen, span)?,
            Type::Result(t, e) => {
                self.no_value_cycle(t, seen, span)?;
                self.no_value_cycle(e, seen, span)?;
            }
            _ => (),
        }
        Ok(())
    }
    fn function_namespace(&self, name: &str) -> String {
        if let Some((owner, _)) = name.rsplit_once('.') {
            if self.structs.contains_key(owner) {
                type_namespace(owner).into()
            } else {
                owner.into()
            }
        } else {
            String::new()
        }
    }
    fn carries_borrow(&self, ty: &Type) -> bool {
        self.borrow_inner(ty, &mut HashSet::new())
    }
    fn borrow_inner(&self, ty: &Type, seen: &mut HashSet<String>) -> bool {
        match ty {
            Type::Named(name) => {
                if !seen.insert(name.clone()) {
                    return false;
                }
                let result = self
                    .structs
                    .get(name)
                    .map(|s| s.fields.iter().any(|f| self.borrow_inner(&f.ty, seen)))
                    .unwrap_or(false)
                    || self
                        .enums
                        .get(name)
                        .map(|e| {
                            e.variants
                                .iter()
                                .any(|v| v.fields.iter().any(|f| self.borrow_inner(&f.ty, seen)))
                        })
                        .unwrap_or(false);
                seen.remove(name);
                result
            }
            Type::Array(_, t) | Type::Option(t) => self.borrow_inner(t, seen),
            Type::Result(t, e) => self.borrow_inner(t, seen) || self.borrow_inner(e, seen),
            _ => ty.carries_borrow(),
        }
    }
    // References do not own their referent's handling obligation.
    fn contains_result(&self, ty: &Type) -> bool {
        match ty {
            Type::Result(..) => true,
            Type::Array(_, t) | Type::Option(t) => self.contains_result(t),
            Type::Named(name) => {
                self.structs
                    .get(name)
                    .is_some_and(|s| s.fields.iter().any(|f| self.contains_result(&f.ty)))
                    || self.enums.get(name).is_some_and(|e| {
                        e.variants
                            .iter()
                            .any(|v| v.fields.iter().any(|f| self.contains_result(&f.ty)))
                    })
            }
            _ => false,
        }
    }
    fn has_drop(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name) => {
                self.functions.contains_key(&format!("{name}.drop"))
                    || self
                        .structs
                        .get(name)
                        .map(|s| s.fields.iter().any(|f| self.has_drop(&f.ty)))
                        .unwrap_or(false)
                    || self
                        .enums
                        .get(name)
                        .map(|e| {
                            e.variants
                                .iter()
                                .any(|v| v.fields.iter().any(|f| self.has_drop(&f.ty)))
                        })
                        .unwrap_or(false)
            }
            Type::Array(_, t) | Type::Option(t) => self.has_drop(t),
            Type::Result(t, e) => self.has_drop(t) || self.has_drop(e),
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
struct Loan {
    root: usize,
    fields: Vec<String>,
    mutable: bool,
    origin: Span,
    via: Vec<usize>,
    external: Option<String>,
}
#[derive(Clone)]
struct Value {
    ty: Type,
    deps: Vec<Loan>,
}
#[derive(Clone)]
struct Variable {
    id: usize,
    name: String,
    ty: Type,
    initialized: bool,
    moved_at: Option<Span>,
    immutable: bool,
    deps: Vec<Loan>,
    span: Span,
    last_use: usize,
    last_use_span: Option<Span>,
    pending_result: bool,
}
#[derive(Clone)]
struct Place {
    ty: Type,
    loans: Vec<Loan>,
    mutable: bool,
    direct: Option<usize>,
}
#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
    Borrow(bool),
    Move,
}
struct LoopUse {
    span: Span,
    implicit: bool,
}
struct YieldContext {
    expected: Option<Type>,
    depth: usize,
    values: Vec<Value>,
    states: Vec<Vec<Vec<Variable>>>,
}
struct Checker<'a> {
    context: &'a Context,
    scopes: Vec<Vec<Variable>>,
    next_id: usize,
    return_ty: Type,
    from: Vec<String>,
    return_contract: Option<Span>,
    inferred_contract: bool,
    uses: HashMap<String, Span>,
    binding_uses: HashMap<(usize, String), Span>,
    position: usize,
    unsafe_depth: usize,
    loop_depth: usize,
    temporary: Vec<Loan>,
    protected: Vec<Loan>,
    yields: Vec<YieldContext>,
    loop_uses: Vec<HashMap<String, LoopUse>>,
    namespace: String,
    guard_depth: usize,
    expression_deps: HashMap<(usize, usize), Vec<Loan>>,
}
impl<'a> Checker<'a> {
    fn new(
        context: &'a Context,
        return_ty: Type,
        from: Vec<String>,
        uses: HashMap<String, Span>,
    ) -> Self {
        Self {
            context,
            scopes: vec![vec![]],
            next_id: 1,
            return_ty,
            from,
            return_contract: None,
            inferred_contract: false,
            uses,
            binding_uses: HashMap::new(),
            position: 0,
            unsafe_depth: 0,
            loop_depth: 0,
            temporary: vec![],
            protected: vec![],
            yields: vec![],
            loop_uses: vec![],
            namespace: String::new(),
            guard_depth: 0,
            expression_deps: HashMap::new(),
        }
    }
    fn lookup(&self, name: &str) -> Option<&Variable> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|v| v.name == name)
    }
    fn by_id(&self, id: usize) -> Option<&Variable> {
        self.scopes.iter().flatten().find(|v| v.id == id)
    }
    fn by_id_mut(&mut self, id: usize) -> Option<&mut Variable> {
        self.scopes.iter_mut().flatten().find(|v| v.id == id)
    }
    fn bind(
        &mut self,
        name: String,
        ty: Type,
        deps: Option<Vec<Loan>>,
        immutable: bool,
        span: Span,
    ) -> Check<()> {
        if name == "_" {
            if matches!(ty, Type::Result(..)) {
                return Err(Diagnostic::new(
                    span,
                    "a Result must be handled, propagated, or returned",
                ));
            }
            return Err(Diagnostic::new(
                span,
                "`_` is a discard target, not a binding name",
            ));
        }
        if self.scopes.last().unwrap().iter().any(|v| v.name == name) {
            return Err(Diagnostic::new(
                span,
                format!("duplicate binding `{name}` in this scope"),
            ));
        }
        let initialized = deps.is_some();
        let pending_result = initialized && self.context.contains_result(&ty);
        let variable = Variable {
            id: self.next_id,
            last_use: self.uses.get(&name).map_or(span.end, |span| span.start),
            last_use_span: self.binding_uses.get(&(span.start, name.clone())).copied(),
            name,
            ty,
            initialized,
            moved_at: None,
            immutable,
            deps: deps.unwrap_or_default(),
            span,
            pending_result,
        };
        self.next_id += 1;
        self.scopes.last_mut().unwrap().push(variable);
        Ok(())
    }
    fn live(&self, v: &Variable) -> bool {
        v.initialized
            && (v.last_use >= self.position
                || self.context.has_drop(&v.ty)
                || self
                    .loop_uses
                    .iter()
                    .any(|names| names.contains_key(&v.name)))
    }
    fn expect(&self, expected: &Type, actual: &Type, span: Span) -> Check<()> {
        if expected != actual {
            Err(Diagnostic::new(
                span,
                format!("expected `{expected}`, found `{actual}`"),
            ))
        } else {
            Ok(())
        }
    }
    fn unsafe_required(&self, span: Span, operation: &str) -> Check<()> {
        if self.unsafe_depth == 0 {
            Err(Diagnostic::new(
                span,
                format!("{operation} requires an explicit unsafe block"),
            ))
        } else {
            Ok(())
        }
    }
    fn conflict(&self, place: &Loan, access: Access, span: Span) -> Check<()> {
        if self.guard_depth > 0
            && matches!(access, Access::Write | Access::Move | Access::Borrow(true))
        {
            return Err(Diagnostic::new(
                span,
                "match guards cannot move values, mutate storage, or borrow mutably",
            ));
        }
        for variable in self.scopes.iter().flatten().filter(|v| self.live(v)) {
            if place.via.contains(&variable.id) {
                continue;
            }
            for loan in &variable.deps {
                if overlaps(place, loan) && incompatible(access, loan.mutable) {
                    let kind = if loan.mutable { "mutable" } else { "shared" };
                    let diagnostic = Diagnostic::new(
                        span,
                        format!(
                            "access conflicts with a live {kind} borrow held by `{}`",
                            variable.name
                        ),
                    )
                    .primary_label(self.access_label(place, access))
                    .label(loan.origin, format!("{kind} borrow begins here"));
                    return Err(self.label_live_borrow(diagnostic, variable, span)
                        .note("end the borrow's uses before this access, or borrow disjoint struct fields"));
                }
            }
        }
        for loan in self.temporary.iter().chain(&self.protected) {
            if overlaps(place, loan) && incompatible(access, loan.mutable) {
                return Err(Diagnostic::new(
                    span,
                    "overlapping borrows within the same expression",
                )
                .primary_label(self.access_label(place, access))
                .label(
                    loan.origin,
                    format!(
                        "{} borrow begins here and remains live for this expression",
                        if loan.mutable { "mutable" } else { "shared" }
                    ),
                ));
            }
        }
        Ok(())
    }
    fn access_label(&self, place: &Loan, access: Access) -> String {
        let name = self.by_id(place.root).map_or_else(
            || {
                place
                    .external
                    .clone()
                    .unwrap_or_else(|| "this value".into())
            },
            |variable| format!("`{}`", variable.name),
        );
        let operation = match access {
            Access::Read => "read",
            Access::Write => "modify",
            Access::Move => "move",
            Access::Borrow(false) => "borrow",
            Access::Borrow(true) => "mutably borrow",
        };
        format!("cannot {operation} {name} while this borrow is live")
    }
    fn label_live_borrow(
        &self,
        diagnostic: Diagnostic,
        variable: &Variable,
        access: Span,
    ) -> Diagnostic {
        if let Some(span) = variable
            .last_use_span
            .filter(|span| span.start >= access.end)
        {
            diagnostic.label(span, "borrow is used here")
        } else if let Some(usage) = self
            .loop_uses
            .iter()
            .rev()
            .find_map(|uses| uses.get(&variable.name))
        {
            if usage.implicit {
                diagnostic.label(usage.span, "borrow remains live for this iteration")
            } else if variable.last_use_span == Some(usage.span) {
                diagnostic.label(usage.span, "borrow is used here on a later loop iteration")
            } else {
                diagnostic.label(
                    variable.span,
                    "borrow remains live under conservative loop checking",
                )
            }
        } else if self.context.has_drop(&variable.ty) {
            diagnostic.label(
                variable.span,
                format!(
                    "borrow may be used when `{}` is destroyed at scope exit",
                    variable.name
                ),
            )
        } else if let Some(span) = variable
            .last_use_span
            .filter(|span| span.start >= self.position)
        {
            diagnostic.label(span, "borrow is used here in the same statement")
        } else {
            diagnostic.label(
                variable.span,
                format!("borrow is held by `{}`", variable.name),
            )
        }
    }
    fn label_return_contract(&self, diagnostic: Diagnostic) -> Diagnostic {
        if let Some(span) = self.return_contract {
            diagnostic.label(
                span,
                format!(
                    "{}return contract allows borrows from({})",
                    if self.inferred_contract {
                        "inferred "
                    } else {
                        ""
                    },
                    self.from.join(", ")
                ),
            )
        } else {
            diagnostic
        }
    }
    fn finish_scope(&mut self, span: Span) -> Check<()> {
        let departing: HashSet<_> = self.scopes.last().unwrap().iter().map(|v| v.id).collect();
        self.check_drop_order(span)?;
        for variable in self.scopes.last().unwrap() {
            if variable.initialized && variable.pending_result {
                return Err(Diagnostic::new(
                    variable.span,
                    format!("Result `{}` is never handled", variable.name),
                )
                .note("match both variants, return the Result, or propagate it with `?`"));
            }
        }
        for variable in self
            .scopes
            .iter()
            .take(self.scopes.len().saturating_sub(1))
            .flatten()
            .filter(|v| self.live(v))
        {
            if let Some(loan) = variable
                .deps
                .iter()
                .find(|d| departing.contains(&d.root) && d.external.is_none())
            {
                let diagnostic = Diagnostic::new(
                    span,
                    format!(
                        "borrow in `{}` outlives its source in this scope",
                        variable.name
                    ),
                )
                .primary_label("borrowed source cannot outlive this scope")
                .label(loan.origin, "borrow begins here");
                return Err(self.label_live_borrow(diagnostic, variable, span));
            }
        }
        Ok(())
    }
    fn block(&mut self, block: &mut Block, scoped: bool) -> Check<bool> {
        if scoped {
            self.scopes.push(vec![]);
        }
        let mut terminated = false;
        for statement in block {
            if terminated {
                return Err(Diagnostic::new(statement.span, "unreachable statement"));
            }
            self.position = statement.span.start;
            self.expression_deps.clear();
            self.temporary.clear();
            terminated = self.statement(statement)?;
            self.temporary.clear();
        }
        if scoped {
            self.finish_scope(Span {
                start: self.position,
                end: self.position,
            })?;
            self.scopes.pop();
        }
        Ok(terminated)
    }
    fn statement(&mut self, statement: &mut Stmt) -> Check<bool> {
        let span = statement.span;
        if let StmtKind::ForEach {
            index,
            name,
            copy,
            iterable,
            body,
        } = &statement.kind
            && let ExprKind::Range(start, end) = &iterable.kind
        {
            if *copy {
                return Err(Diagnostic::new(
                    span,
                    "range loops yield integer values, not references; use `for value in start..end`",
                ));
            }
            if index.is_some() {
                return Err(Diagnostic::new(
                    span,
                    "range loops bind one integer; use `for i in start..end`",
                ));
            }
            let ty = self
                .peek_type(start)
                .or_else(|| self.peek_type(end))
                .or_else(|| self.literal_type(start))
                .unwrap_or(Type::isize());
            if !ty.is_integer() {
                return Err(Diagnostic::new(
                    span,
                    "range bounds must be integers of the same type",
                ));
            }
            let first = format!("$range.start.{}", span.start);
            let last = format!("$range.end.{}", span.start);
            let counter = format!("$range.index.{}", span.start);
            let expr = |name: &String| Expr::new(ExprKind::Name(name.clone()), span);
            let stmt = |kind| Stmt { kind, span };
            let binding = |name: String, value: Expr| {
                stmt(StmtKind::Let {
                    name,
                    ty: ty.clone(),
                    value: Some(value),
                    constant: false,
                    mutable: true,
                })
            };
            let mut inner = body.clone();
            if name != "_" {
                inner.insert(0, binding(name.clone(), expr(&counter)));
            }
            statement.kind = StmtKind::Block(vec![
                binding(first.clone(), *start.clone()),
                binding(last.clone(), *end.clone()),
                stmt(StmtKind::For {
                    init: Some(Box::new(binding(counter.clone(), expr(&first)))),
                    condition: Some(Expr::new(
                        ExprKind::Binary(
                            BinaryOp::Lt,
                            Box::new(expr(&counter)),
                            Box::new(expr(&last)),
                        ),
                        span,
                    )),
                    step: Some(Box::new(stmt(StmtKind::Assign {
                        target: expr(&counter),
                        op: Some(BinaryOp::Add),
                        value: Expr::new(ExprKind::Int(1, None), span),
                    }))),
                    body: inner,
                }),
            ]);
        }
        match &mut statement.kind {
            StmtKind::Let {
                name,
                ty,
                value,
                constant,
                mutable,
            } => {
                if *ty != Type::Unknown {
                    self.context.validate_type(ty, span, false)?;
                }
                let deps = if let Some(expression) = value {
                    let expected = (*ty != Type::Unknown).then(|| ty.clone());
                    let value = self.expr(expression, expected.as_ref(), true)?;
                    if *ty == Type::Unknown {
                        *ty = value.ty.clone();
                    } else {
                        self.expect(ty, &value.ty, expression.span)?;
                    }
                    if *ty == Type::Void {
                        return Err(Diagnostic::new(span, "cannot bind a void value"));
                    }
                    if *constant {
                        if !constant_expression(expression) {
                            return Err(Diagnostic::new(
                                span,
                                "const initializer must be a compile-time expression",
                            ));
                        }
                        validate_constant(expression, self.context.pointer_bits)?;
                    }
                    Some(value.deps)
                } else {
                    if *ty == Type::Unknown {
                        return Err(Diagnostic::new(
                            span,
                            "uninitialized binding requires an explicit type",
                        ));
                    }
                    if *constant {
                        return Err(Diagnostic::new(
                            span,
                            "const binding requires an initializer",
                        ));
                    }
                    None
                };
                self.bind(name.clone(), ty.clone(), deps, !*mutable, span)?;
            }
            StmtKind::Assign { target, op, value } => {
                if self.guard_depth > 0 {
                    return Err(Diagnostic::new(span, "match guards cannot mutate storage"));
                }
                if matches!(&target.kind, ExprKind::Name(name) if name == "_") {
                    let val = self.expr(value, None, true)?;
                    target.ty = val.ty.clone();
                    if self.context.contains_result(&val.ty) {
                        return Err(Diagnostic::new(
                            span,
                            "a Result cannot be discarded through `_ =`",
                        ));
                    }
                    return Ok(false);
                }
                let place = self.place(target, op.is_some())?;
                if !place.mutable {
                    return Err(Diagnostic::new(
                        target.span,
                        "cannot assign through an immutable binding or shared reference",
                    ));
                }
                for loan in &place.loans {
                    self.conflict(loan, Access::Write, target.span)?;
                }
                let val = self.expr(value, Some(&place.ty), true)?;
                self.expect(&place.ty, &val.ty, value.span)?;
                if let Some(binary) = op {
                    self.binary_type(*binary, &place.ty, span)?;
                }
                let pending_result = self.context.contains_result(&place.ty);
                if let Some(id) = place.direct {
                    let variable = self.by_id_mut(id).unwrap();
                    if variable.initialized && variable.pending_result {
                        return Err(Diagnostic::new(span, "overwriting an unhandled Result"));
                    }
                    variable.initialized = true;
                    variable.moved_at = None;
                    variable.deps = val.deps;
                    variable.pending_result = pending_result;
                } else if self.context.carries_borrow(&place.ty) {
                    // Field replacement may narrow an aggregate's lifetime, never erase
                    // its existing conservative dependency set.
                    for loan in &place.loans {
                        if let Some(owner) = self.by_id_mut(loan.root) {
                            if !loan.via.is_empty() {
                                return Err(Diagnostic::new(span, "replacing borrow-carrying fields through a reference is not supported in 0.1").note("replace the owned aggregate so its complete borrow dependencies can be checked"));
                            }
                            owner.deps.extend(val.deps.clone());
                        }
                    }
                }
            }
            StmtKind::Expr(expression) => {
                let value = self.expr(expression, None, true)?;
                if self.context.contains_result(&value.ty) {
                    return Err(Diagnostic::new(
                        span,
                        "Result must be handled, propagated, or returned",
                    ));
                }
            }
            StmtKind::Yield(expression) => {
                let context = self
                    .yields
                    .last()
                    .ok_or_else(|| Diagnostic::new(span, "value exit outside a value block"))?;
                let expected = context.expected.clone();
                let depth = context.depth;
                let value = self.expr(expression, expected.as_ref(), true)?;
                if let Some(expected) = &expected {
                    self.expect(expected, &value.ty, span)?;
                }
                let departing: HashSet<_> = self
                    .scopes
                    .iter()
                    .skip(depth)
                    .flatten()
                    .map(|v| v.id)
                    .collect();
                if let Some(loan) = value
                    .deps
                    .iter()
                    .find(|loan| departing.contains(&loan.root) && loan.external.is_none())
                {
                    let mut diagnostic = Diagnostic::new(
                        expression.span,
                        "value block cannot yield a borrow of its local storage",
                    )
                    .primary_label("local storage does not live beyond this value block")
                    .label(loan.origin, "local borrow begins here");
                    if let Some(source) = self.by_id(loan.root) {
                        diagnostic = diagnostic.label(
                            source.span,
                            format!("local value `{}` is declared here", source.name),
                        );
                    }
                    return Err(diagnostic);
                }
                if self
                    .scopes
                    .iter()
                    .skip(depth)
                    .flatten()
                    .any(|v| v.initialized && v.pending_result)
                {
                    return Err(Diagnostic::new(
                        span,
                        "a Result must be handled before leaving its value block",
                    ));
                }
                let context = self.yields.last_mut().unwrap();
                context.expected = Some(value.ty.clone());
                context.values.push(value);
                context.states.push(self.scopes[..depth].to_vec());
                return Ok(true);
            }
            StmtKind::Return(expression) => {
                let expected = self.return_ty.clone();
                let return_span = expression
                    .as_ref()
                    .map_or(span, |expression| expression.span);
                let value = match expression {
                    Some(e) => self.expr(e, Some(&expected), true)?,
                    None => Value {
                        ty: Type::Void,
                        deps: vec![],
                    },
                };
                self.expect(&expected, &value.ty, span)?;
                if self.context.carries_borrow(&value.ty) {
                    for loan in &value.deps {
                        match &loan.external {
                            Some(source) if self.from.contains(source) || source == "static" => (),
                            Some(source) => {
                                let source_span = self.scopes[0]
                                    .iter()
                                    .find(|variable| &variable.name == source)
                                    .map_or(loan.origin, |variable| variable.span);
                                return Err(self.label_return_contract(Diagnostic::new(
                                    return_span,
                                    format!(
                                        "returned borrow depends on `{source}`, outside the return contract"
                                    ),
                                )
                                .primary_label(format!("returned borrow comes from `{source}`"))
                                .label(source_span, format!("borrowed source `{source}` is declared here"))));
                            }
                            None => {
                                let mut diagnostic = Diagnostic::new(
                                    return_span,
                                    "cannot return a borrow of a local value",
                                )
                                .primary_label(
                                    "local value does not live long enough to be returned",
                                )
                                .label(loan.origin, "local borrow begins here");
                                if let Some(source) = self.by_id(loan.root) {
                                    diagnostic = diagnostic.label(
                                        source.span,
                                        format!("local value `{}` is declared here", source.name),
                                    );
                                }
                                return Err(self.label_return_contract(diagnostic));
                            }
                        }
                    }
                }
                self.check_pending_all(span)?;
                return Ok(true);
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let value = self.expr(condition, Some(&Type::Bool), false)?;
                self.expect(&Type::Bool, &value.ty, condition.span)?;
                self.temporary.clear();
                let before = self.scopes.clone();
                let first = self.block(then_block, true)?;
                let then_state = self.scopes.clone();
                self.scopes = before;
                let second = self.block(else_block, true)?;
                let else_state = self.scopes.clone();
                self.scopes = if first && !second {
                    else_state
                } else if second && !first {
                    then_state
                } else {
                    merge_states(then_state, else_state)
                };
                return Ok(first && second);
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                self.scopes.push(vec![]);
                if let Some(initializer) = init {
                    if let StmtKind::Let {
                        name,
                        ty,
                        value: Some(value),
                        ..
                    } = &mut initializer.kind
                        && *ty == Type::Unknown
                        && matches!(value.kind, ExprKind::Int(_, None))
                        && let Some(inferred) = condition
                            .as_ref()
                            .and_then(|e| self.infer_loop_type(e, name))
                    {
                        *ty = inferred;
                    }
                    self.statement(initializer)?;
                    self.temporary.clear();
                }
                let before = self.scopes.clone();
                let mut names = HashMap::new();
                names_block(body, &mut names);
                if let Some(e) = condition.as_ref() {
                    names_expr(e, &mut names);
                }
                if let Some(s) = step.as_ref() {
                    names_stmt(s, &mut names);
                }
                self.loop_uses.push(
                    names
                        .into_iter()
                        .map(|(name, span)| {
                            (
                                name,
                                LoopUse {
                                    span,
                                    implicit: false,
                                },
                            )
                        })
                        .collect(),
                );
                self.loop_depth += 1;
                if let Some(e) = condition {
                    let val = self.expr(e, Some(&Type::Bool), false)?;
                    self.expect(&Type::Bool, &val.ty, e.span)?;
                    self.temporary.clear();
                }
                self.block(body, true)?;
                if let Some(step) = step {
                    self.statement(step)?;
                    self.temporary.clear();
                }
                self.loop_depth -= 1;
                self.loop_uses.pop();
                self.check_loop_moves(&before, span)?;
                self.scopes = merge_states(before, self.scopes.clone());
                self.finish_scope(span)?;
                self.scopes.pop();
                // A forever loop is a diverging statement unless it can break.
                return Ok(condition.is_none() && !contains_break(body));
            }
            StmtKind::ForEach {
                index,
                name,
                copy,
                iterable,
                body,
            } => {
                let place = match &iterable.kind {
                    ExprKind::Unary(UnaryOp::Borrow | UnaryOp::BorrowMut, _) => None,
                    _ => Some(self.place(iterable, true)?),
                };
                let value = if let Some(place) = place {
                    let mutable = matches!(place.ty, Type::Slice(true, _));
                    let mut deps = place.loans.clone();
                    for loan in &mut deps {
                        loan.mutable = mutable;
                        self.conflict(loan, Access::Borrow(mutable), iterable.span)?;
                    }
                    let ty = match dereferenced(&place.ty) {
                        Type::Array(_, t) | Type::Slice(_, t) => Type::Slice(mutable, t.clone()),
                        _ => {
                            return Err(Diagnostic::new(
                                iterable.span,
                                "foreach requires an array or slice",
                            ));
                        }
                    };
                    // Preserve the original expression type for backend collection layout.
                    Value { ty, deps }
                } else {
                    self.expr(iterable, None, false)?
                };
                let (mutable, element) = match &value.ty {
                    Type::Slice(m, t) => (*m, t.as_ref().clone()),
                    Type::Ref(m, t) => match t.as_ref() {
                        Type::Array(_, e) => (*m, e.as_ref().clone()),
                        _ => {
                            return Err(Diagnostic::new(
                                span,
                                "foreach requires an array or slice",
                            ));
                        }
                    },
                    _ => return Err(Diagnostic::new(span, "foreach requires an array or slice")),
                };
                if *copy && (mutable || matches!(iterable.ty, Type::Ref(true, _))) {
                    return Err(Diagnostic::new(
                        span,
                        "`&value` requires shared iteration; use a shared array borrow or slice",
                    )
                    .note("mutable iteration remains `for value in &mut values`"));
                }
                if *copy && !element.is_copy() {
                    return Err(Diagnostic::new(
                        span,
                        format!("`&value` loop pattern requires a copyable element; `{element}` is not copyable"),
                    )
                    .note("use `for value in values` to borrow elements without copying"));
                }
                let copied_deps = if *copy && self.context.carries_borrow(&element) {
                    // Reading an owned array's element copies the references stored
                    // in it, without borrowing the array storage itself.
                    let source = match &iterable.kind {
                        ExprKind::Unary(UnaryOp::Borrow, source)
                            if matches!(source.ty, Type::Array(..)) =>
                        {
                            source.as_ref()
                        }
                        _ => iterable,
                    };
                    let deps = if matches!(source.ty, Type::Array(..)) {
                        self.provenance(source)
                    } else {
                        // References and slices carry the source collection's
                        // provenance, rather than borrowing their local binding.
                        self.provenance(iterable)
                    };
                    self.transitive_dependencies(deps)
                } else {
                    vec![]
                };
                self.temporary.clear();
                let before = self.scopes.clone();
                self.scopes.push(vec![]);
                if let Some(index) = index {
                    self.bind(index.clone(), Type::usize(), Some(vec![]), false, span)?;
                }
                let collection = format!("$foreach.collection.{}", span.start);
                if *copy {
                    // The captured collection stays borrowed across every loop
                    // back edge, independently of reassignment of the copy binding.
                    let mut deps = value.deps;
                    deps.extend(self.provenance(iterable));
                    let mut deps = self.transitive_dependencies(deps);
                    for loan in &mut deps {
                        self.conflict(loan, Access::Borrow(false), iterable.span)?;
                        loan.mutable = false;
                    }
                    self.bind(collection.clone(), value.ty, Some(deps), false, span)?;
                    if name != "_" {
                        self.bind(name.clone(), element, Some(copied_deps), false, span)?;
                    }
                } else {
                    self.bind(
                        name.clone(),
                        Type::Ref(mutable, Box::new(element)),
                        Some(value.deps),
                        false,
                        span,
                    )?;
                }
                let iteration_id = self.lookup(name).map(|variable| variable.id);
                let mut names = HashMap::new();
                names_block(body, &mut names);
                let mut loop_uses: HashMap<_, _> = names
                    .into_iter()
                    .map(|(name, span)| {
                        (
                            name,
                            LoopUse {
                                span,
                                implicit: false,
                            },
                        )
                    })
                    .collect();
                loop_uses.entry(name.clone()).or_insert(LoopUse {
                    span: iterable.span,
                    implicit: true,
                });
                if *copy {
                    loop_uses.insert(
                        collection,
                        LoopUse {
                            span: iterable.span,
                            implicit: true,
                        },
                    );
                }
                self.loop_uses.push(loop_uses);
                self.loop_depth += 1;
                self.block(body, false)?;
                self.loop_depth -= 1;
                self.loop_uses.pop();
                if mutable {
                    for variable in self.scopes.iter().take(self.scopes.len() - 1).flatten() {
                        if let Some(loan) = variable
                            .deps
                            .iter()
                            .find(|d| iteration_id.is_some_and(|id| d.via.contains(&id)))
                        {
                            let diagnostic = Diagnostic::new(
                                span,
                                "mutable foreach element borrow cannot escape its iteration",
                            )
                            .primary_label("element borrow escapes this iteration")
                            .label(loan.origin, "mutable element borrow begins here")
                            .label(
                                variable.span,
                                format!("borrow is stored in outer binding `{}`", variable.name),
                            );
                            return Err(self.label_live_borrow(diagnostic, variable, span));
                        }
                    }
                }
                self.finish_scope(span)?;
                self.scopes.pop();
                self.check_loop_moves(&before, span)?;
                self.scopes = merge_states(before, self.scopes.clone());
            }
            StmtKind::IfLet {
                pattern,
                value,
                then_block,
                else_block,
            } => {
                let val = self.expr(value, None, true)?;
                self.temporary.clear();
                let (matched, borrowed) = match_subject(&val.ty);
                let (checked, bindings) = self.check_pattern(pattern, matched, borrowed, span)?;
                self.check_conditional_pattern(&checked, matched, span)?;
                if self.context.contains_result(dereferenced(&val.ty)) {
                    self.mark_matched_result(value);
                }
                let before = self.scopes.clone();
                self.scopes.push(vec![]);
                self.bind_pattern(pattern, bindings, &val, true, span)?;
                let first = self.block(then_block, false)?;
                self.finish_scope(span)?;
                self.scopes.pop();
                let then_state = self.scopes.clone();
                self.scopes = before;
                let second = self.block(else_block, true)?;
                let else_state = self.scopes.clone();
                self.scopes = if first && !second {
                    else_state
                } else if second && !first {
                    then_state
                } else {
                    merge_states(then_state, else_state)
                };
                return Ok(first && second);
            }
            StmtKind::LetPattern {
                pattern,
                ty,
                value,
                else_block,
            } => {
                if *ty != Type::Unknown {
                    self.context.validate_type(ty, span, false)?;
                }
                let val = self.expr(value, (*ty != Type::Unknown).then_some(&*ty), true)?;
                if *ty == Type::Unknown {
                    *ty = val.ty.clone();
                } else {
                    self.expect(ty, &val.ty, span)?;
                }
                self.temporary.clear();
                let (matched, borrowed) = match_subject(&val.ty);
                let (checked, bindings) = self.check_pattern(pattern, matched, borrowed, span)?;
                self.check_conditional_pattern(&checked, matched, span)?;
                if self.context.contains_result(dereferenced(&val.ty)) {
                    self.mark_matched_result(value);
                }
                let irrefutable =
                    self.patterns_exhaustive(&[vec![checked]], std::slice::from_ref(matched))?;
                if let Some(block) = else_block {
                    let before = self.scopes.clone();
                    if !self.block(block, true)? {
                        return Err(Diagnostic::new(
                            span,
                            "the `else` block of a let pattern must diverge (return, break, or continue)",
                        ));
                    }
                    self.scopes = before;
                } else if !irrefutable {
                    return Err(Diagnostic::new(
                        span,
                        "refutable let pattern requires an `else` block",
                    ));
                }
                self.bind_pattern(pattern, bindings, &val, true, span)?;
            }
            StmtKind::Match { value, arms } => {
                let val = self.expr(value, None, true)?;
                if self.context.contains_result(dereferenced(&val.ty)) {
                    self.mark_matched_result(value);
                }
                self.temporary.clear();
                let before = self.scopes.clone();
                let (match_ty, borrowed) = match_subject(&val.ty);
                let mut rows = vec![];
                let mut surviving = vec![];
                let mut all_terminate = true;
                for arm in arms {
                    if self.patterns_exhaustive(&rows, std::slice::from_ref(match_ty))? {
                        return Err(Diagnostic::new(
                            arm.span,
                            "unreachable match arm after exhaustive patterns",
                        ));
                    }
                    self.scopes = before.clone();
                    self.scopes.push(vec![]);
                    let (checked, bindings) =
                        self.check_pattern(&arm.pattern, match_ty, borrowed, arm.span)?;
                    self.bind_pattern(&arm.pattern, bindings, &val, false, arm.span)?;
                    if let Some(guard) = &mut arm.guard {
                        self.guard_depth += 1;
                        let result = self.expr(guard, Some(&Type::Bool), false);
                        self.guard_depth -= 1;
                        let guard_value = result?;
                        self.expect(&Type::Bool, &guard_value.ty, guard.span)?;
                        self.temporary.clear();
                    } else {
                        if rows
                            .iter()
                            .any(|r: &Vec<CheckedPattern>| r == &vec![checked.clone()])
                        {
                            return Err(Diagnostic::new(arm.span, "duplicate match pattern"));
                        }
                        rows.push(vec![checked]);
                    }
                    let terminates = self.block(&mut arm.body, false)?;
                    self.finish_scope(arm.span)?;
                    self.scopes.pop();
                    all_terminate &= terminates;
                    if !terminates {
                        surviving.push(self.scopes.clone());
                    }
                }
                if !self.patterns_exhaustive(&rows, std::slice::from_ref(match_ty))? {
                    return Err(Diagnostic::new(span, "non-exhaustive match")
                        .note("cover every possible payload; guarded arms do not establish exhaustiveness"));
                }
                self.scopes = surviving.into_iter().reduce(merge_states).unwrap_or(before);
                return Ok(all_terminate);
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.loop_depth == 0 {
                    return Err(Diagnostic::new(
                        span,
                        "break and continue require an enclosing for loop",
                    ));
                }
                // The containing loop joins all exits conservatively.
                return Ok(true);
            }
            StmtKind::Block(block) => return self.block(block, true),
            StmtKind::Unsafe(block) => {
                self.unsafe_depth += 1;
                let result = self.block(block, true);
                self.unsafe_depth -= 1;
                return result;
            }
        }
        Ok(false)
    }
    fn check_drop_order(&self, span: Span) -> Check<()> {
        for scope in &self.scopes {
            for variable in scope
                .iter()
                .filter(|v| v.initialized && self.context.has_drop(&v.ty))
            {
                for loan in &variable.deps {
                    if loan.external.is_none()
                        && scope
                            .iter()
                            .any(|source| source.id == loan.root && source.id > variable.id)
                    {
                        let mut diagnostic = Diagnostic::new(span, format!("destructor of `{}` would run after its borrowed source is destroyed", variable.name))
                            .primary_label("borrowed source is destroyed before its borrower")
                            .label(loan.origin, "borrow begins here")
                            .label(variable.span, format!("`{}` is declared first and will be destroyed last", variable.name));
                        if let Some(source) = self.by_id(loan.root) {
                            diagnostic = diagnostic.label(source.span, format!("borrowed source `{}` is declared later and will be destroyed first", source.name));
                        }
                        return Err(diagnostic.note("declare the borrowed source before the value whose destructor accesses it"));
                    }
                }
            }
        }
        Ok(())
    }
    fn check_pending_all(&self, span: Span) -> Check<()> {
        self.check_drop_order(span)?;
        for variable in self.scopes.iter().flatten() {
            if variable.initialized && variable.pending_result {
                return Err(Diagnostic::new(
                    span,
                    format!("Result `{}` is left unhandled on this exit", variable.name),
                ));
            }
        }
        Ok(())
    }
    fn check_loop_moves(&self, before: &[Vec<Variable>], span: Span) -> Check<()> {
        for old in before.iter().flatten() {
            if old.initialized && self.by_id(old.id).is_some_and(|v| !v.initialized) {
                let moved_at = self
                    .by_id(old.id)
                    .and_then(|variable| variable.moved_at)
                    .unwrap_or(span);
                return Err(Diagnostic::new(
                    moved_at,
                    format!(
                        "`{}` is moved in a loop and may be used again on the next iteration",
                        old.name
                    ),
                )
                .primary_label(format!("value `{}` is moved here", old.name))
                .label(span, "this loop may repeat the move on a later iteration")
                .label(
                    old.span,
                    format!("binding `{}` is declared outside the loop", old.name),
                )
                .note("reinitialize the binding before continuing the loop"));
            }
        }
        Ok(())
    }
    fn expr(
        &mut self,
        expression: &mut Expr,
        expected: Option<&Type>,
        consume: bool,
    ) -> Check<Value> {
        let span = expression.span;
        if let Some(name) = qualified_name(expression)
            && self.context.constants.contains_key(&name)
        {
            if !self.context.constants[&name].public && type_namespace(&name) != self.namespace {
                return Err(Diagnostic::new(
                    span,
                    format!("constant `{name}` is private to its package"),
                ));
            }
            expression.kind = ExprKind::Name(name);
        }
        // Module-qualified and associated calls have no receiver argument.
        if let ExprKind::MethodCall {
            receiver,
            name,
            args,
        } = &mut expression.kind
            && let Some(prefix) = qualified_name(receiver)
        {
            let qualified = format!("{prefix}.{name}");
            if self.lookup(&prefix).is_none()
                && (self.context.functions.contains_key(&qualified)
                    || self.context.enums.contains_key(&prefix)
                    || prefix == "core"
                    || prefix.starts_with("core.")
                    || matches!(prefix.as_str(), "mmio" | "mem" | "ptr"))
            {
                expression.kind = ExprKind::Call {
                    name: qualified,
                    type_args: vec![],
                    args: std::mem::take(args),
                };
            }
        }
        // Method syntax is resolved statically and lowered to an ordinary call.
        if let ExprKind::MethodCall {
            receiver,
            name,
            args,
        } = &mut expression.kind
        {
            let receiver_ty = self.peek_type(receiver).ok_or_else(|| {
                Diagnostic::new(receiver.span, "cannot resolve the receiver type")
            })?;
            let owner = match dereferenced(&receiver_ty) {
                Type::Named(n) => n.clone(),
                _ => {
                    return Err(Diagnostic::new(
                        span,
                        format!("type `{receiver_ty}` has no methods"),
                    ));
                }
            };
            let qualified = format!("{owner}.{name}");
            let signature =
                self.context.functions.get(&qualified).ok_or_else(|| {
                    Diagnostic::new(span, format!("unknown method `{qualified}`"))
                })?;
            let first = signature
                .params
                .first()
                .filter(|p| p.name == "self")
                .ok_or_else(|| {
                    Diagnostic::new(
                        span,
                        format!(
                            "`{qualified}` is an associated function; call it through its type"
                        ),
                    )
                })?;
            let mut receiver = receiver.as_ref().clone();
            if let Type::Ref(mutable, _) = &first.ty {
                if matches!(receiver_ty, Type::Ref(..)) {
                    receiver = Expr::new(ExprKind::Unary(UnaryOp::Deref, Box::new(receiver)), span);
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
            let mut lowered = vec![receiver];
            lowered.append(args);
            expression.kind = ExprKind::Call {
                name: qualified,
                type_args: vec![],
                args: lowered,
            };
        }
        let mut result = match &mut expression.kind {
            ExprKind::Int(number, suffix) => {
                let ty = suffix
                    .clone()
                    .or_else(|| expected.filter(|t| t.is_integer()).cloned())
                    .unwrap_or_else(Type::isize);
                self.integer_range(*number, &ty, false, span)?;
                Value { ty, deps: vec![] }
            }
            ExprKind::Float(value, suffix) => {
                let ty = suffix
                    .clone()
                    .or_else(|| expected.filter(|t| matches!(t, Type::Float(_))).cloned())
                    .unwrap_or(Type::Float(64));
                if !value.is_finite() || ty == Type::Float(32) && !(*value as f32).is_finite() {
                    return Err(Diagnostic::new(
                        span,
                        "floating-point literal is out of range",
                    ));
                }
                Value { ty, deps: vec![] }
            }
            ExprKind::Bool(_) => Value {
                ty: Type::Bool,
                deps: vec![],
            },
            ExprKind::String(_, bytes) => Value {
                ty: if *bytes {
                    Type::Slice(false, Box::new(Type::u8()))
                } else {
                    Type::Str
                },
                deps: vec![static_loan(span)],
            },
            ExprKind::Name(name) if name == "none" => {
                let ty = expected
                    .filter(|t| matches!(t, Type::Option(_)))
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::new(span, "`none` needs an Option type from its context")
                    })?;
                Value { ty, deps: vec![] }
            }
            ExprKind::Name(_)
            | ExprKind::Field(..)
            | ExprKind::Index(..)
            | ExprKind::Unary(UnaryOp::Deref, _) => {
                if let Some(ty) = self.enum_value_type(expression) {
                    if let ExprKind::Field(base, _) = &mut expression.kind
                        && let Some(name) = qualified_name(base)
                    {
                        base.kind = ExprKind::Name(name);
                    }
                    let result = Value { ty, deps: vec![] };
                    expression.ty = result.ty.clone();
                    return Ok(result);
                }
                let place = self.place(expression, true)?;
                for loan in &place.loans {
                    self.conflict(
                        loan,
                        if consume && !place.ty.is_copy() {
                            Access::Move
                        } else {
                            Access::Read
                        },
                        span,
                    )?;
                }
                let deps = if self.context.carries_borrow(&place.ty) {
                    self.provenance(expression)
                } else {
                    vec![]
                };
                if consume && !place.ty.is_copy() {
                    if let Some(id) = place.direct {
                        let variable = self.by_id_mut(id).unwrap();
                        variable.initialized = false;
                        variable.moved_at = Some(span);
                        variable.pending_result = false;
                    } else {
                        return Err(Diagnostic::new(span, "moving a non-copy field or indexed element is not supported").note("move the complete owned value, borrow the subobject, or call an explicit clone function"));
                    }
                }
                Value { ty: place.ty, deps }
            }
            ExprKind::Constant(value, ty) => {
                let value = self.expr(value, Some(ty), true)?;
                self.expect(ty, &value.ty, span)?;
                value
            }
            ExprKind::Repeat(value, ty) => {
                let Type::Array(_, element) = ty else {
                    return Err(Diagnostic::new(span, "unresolved repetition length"));
                };
                let hint = if **element != Type::Unknown {
                    Some(*element.clone())
                } else if let Some(Type::Array(_, t)) = expected {
                    Some(*t.clone())
                } else {
                    None
                };
                let value = self.expr(value, hint.as_ref(), true)?;
                if let Some(hint) = &hint {
                    self.expect(hint, &value.ty, span)?;
                }
                if !value.ty.is_copy() || value.ty == Type::Void {
                    return Err(Diagnostic::new(
                        span,
                        "array repetition requires a copyable element",
                    ));
                }
                **element = value.ty;
                Value {
                    ty: ty.clone(),
                    deps: value.deps,
                }
            }
            ExprKind::ValueBlock(body) => {
                let depth = self.scopes.len();
                let protected_len = self.protected.len();
                let temporaries = std::mem::take(&mut self.temporary);
                self.protected.extend(temporaries.iter().cloned());
                let expression_deps = std::mem::take(&mut self.expression_deps);
                self.yields.push(YieldContext {
                    expected: expected.cloned(),
                    depth,
                    values: vec![],
                    states: vec![],
                });
                let terminates = self.block(body, true)?;
                let context = self.yields.pop().unwrap();
                self.protected.truncate(protected_len);
                self.temporary = temporaries;
                self.expression_deps = expression_deps;
                if !terminates || context.values.is_empty() {
                    return Err(Diagnostic::new(
                        span,
                        "every continuing path of a value block must end with a value",
                    ));
                }
                self.scopes = context.states.into_iter().reduce(merge_states).unwrap();
                let deps: Vec<_> = context.values.into_iter().flat_map(|v| v.deps).collect();
                self.temporary.extend(deps.clone());
                Value {
                    ty: context.expected.unwrap(),
                    deps,
                }
            }
            ExprKind::Slice {
                base,
                start,
                end,
                mutable,
            } => {
                let inherited = self.temporary.len();
                let mut place = self.place(base, true)?;
                while let Type::Ref(m, inner) = place.ty.clone() {
                    place.ty = *inner;
                    place.mutable = m;
                    place.loans = self.provenance(base);
                }
                let element = match &place.ty {
                    Type::Array(_, t) => *t.clone(),
                    Type::Slice(m, t) => {
                        place.mutable = *m;
                        place.loans = self.provenance(base);
                        *t.clone()
                    }
                    _ => return Err(Diagnostic::new(span, "slicing requires an array or slice")),
                };
                if *mutable && !place.mutable {
                    return Err(Diagnostic::new(
                        span,
                        "cannot mutably slice a shared reference or immutable binding",
                    ));
                }
                if place.loans.is_empty() {
                    return Err(Diagnostic::new(
                        span,
                        "slice requires checked source storage",
                    ));
                }
                // Reserve the captured source while bounds run: reads such as
                // data.len are allowed, but moving or mutating it is not.
                self.temporary.truncate(inherited);
                let mut deps = place.loans;
                for loan in &mut deps {
                    self.conflict(loan, Access::Borrow(false), span)?;
                    loan.mutable = false;
                    loan.origin = span;
                }
                self.temporary.extend(deps.clone());
                let reserved = self.temporary.len();
                for bound in start.iter_mut().chain(end) {
                    let value = self.expr(bound, Some(&Type::usize()), false)?;
                    if !value.ty.is_integer() {
                        return Err(Diagnostic::new(bound.span, "slice bounds must be integers"));
                    }
                    self.temporary.truncate(reserved);
                }
                self.temporary.truncate(inherited);
                for loan in &mut deps {
                    self.conflict(loan, Access::Borrow(*mutable), span)?;
                    loan.mutable = *mutable;
                }
                self.temporary.extend(deps.clone());
                Value {
                    ty: Type::Slice(*mutable, Box::new(element)),
                    deps,
                }
            }
            ExprKind::Range(..) => {
                return Err(Diagnostic::new(
                    span,
                    "ranges are only supported in for loops",
                ));
            }
            ExprKind::Array(ty, items) => {
                if let Type::Array(_, element) = ty
                    && **element == Type::Unknown
                {
                    **element = match expected {
                        Some(Type::Array(_, t)) => *t.clone(),
                        _ => items
                            .iter()
                            .find_map(|e| self.peek_type(e))
                            .or_else(|| items.first().and_then(|e| self.literal_type(e)))
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    span,
                                    "empty array literal needs an element type from its context",
                                )
                            })?,
                    };
                }
                self.context.validate_type(ty, span, false)?;
                let (length, element) = match ty {
                    Type::Array(n, t) => (*n, t.as_ref().clone()),
                    _ => {
                        return Err(Diagnostic::new(
                            span,
                            "array literal requires a fixed-size array type",
                        ));
                    }
                };
                if items.len() != length {
                    return Err(Diagnostic::new(
                        span,
                        format!(
                            "array literal needs {length} elements, found {}",
                            items.len()
                        ),
                    ));
                }
                let mut deps = vec![];
                for item in items {
                    let value = self.expr(item, Some(&element), true)?;
                    self.expect(&element, &value.ty, item.span)?;
                    deps.extend(value.deps);
                }
                Value {
                    ty: ty.clone(),
                    deps,
                }
            }
            ExprKind::Struct(name, fields) => {
                let declaration = self
                    .context
                    .structs
                    .get(name)
                    .cloned()
                    .ok_or_else(|| Diagnostic::new(span, format!("unknown struct `{name}`")))?;
                let mut initialized = HashSet::new();
                let mut deps = vec![];
                for (name, expression) in fields {
                    if !initialized.insert(name.clone()) {
                        return Err(Diagnostic::new(
                            expression.span,
                            format!("field `{name}` initialized more than once"),
                        ));
                    }
                    let field = declaration
                        .fields
                        .iter()
                        .find(|f| f.name == *name)
                        .ok_or_else(|| {
                            Diagnostic::new(expression.span, format!("unknown field `{name}`"))
                        })?;
                    if !field.public && type_namespace(&declaration.name) != self.namespace {
                        return Err(Diagnostic::new(
                            expression.span,
                            format!("field `{name}` is private to its package"),
                        ));
                    }
                    let value = self.expr(expression, Some(&field.ty), true)?;
                    self.expect(&field.ty, &value.ty, expression.span)?;
                    deps.extend(value.deps);
                }
                if let Some(missing) = declaration
                    .fields
                    .iter()
                    .find(|f| !initialized.contains(&f.name))
                {
                    return Err(Diagnostic::new(
                        span,
                        format!("missing initializer for field `{}`", missing.name),
                    ));
                }
                Value {
                    ty: Type::Named(name.clone()),
                    deps,
                }
            }
            ExprKind::Unary(op @ (UnaryOp::Borrow | UnaryOp::BorrowMut), operand) => {
                let mutable = *op == UnaryOp::BorrowMut;
                if !matches!(
                    operand.kind,
                    ExprKind::Name(_)
                        | ExprKind::Field(..)
                        | ExprKind::Index(..)
                        | ExprKind::Unary(UnaryOp::Deref, _)
                ) {
                    return Err(Diagnostic::new(
                        span,
                        "cannot borrow a temporary value; bind it to a local first",
                    ));
                }
                if matches!(&operand.kind, ExprKind::Field(base, field) if field == "len" && self.peek_type(base).is_some_and(|t| matches!(dereferenced(&t), Type::Array(..) | Type::Slice(..) | Type::Str)))
                {
                    return Err(Diagnostic::new(
                        span,
                        "collection length is a value, not an addressable field",
                    ));
                }
                let place = self.place(operand, true)?;
                if mutable && !place.mutable {
                    return Err(Diagnostic::new(
                        span,
                        "cannot mutably borrow an immutable binding or shared reference",
                    ));
                }
                if place.loans.is_empty() {
                    return Err(Diagnostic::new(
                        span,
                        "creating a checked borrow from a raw pointer requires a lifetime primitive, which 0.1 does not yet provide",
                    ));
                }
                let mut deps = place.loans;
                if self.context.carries_borrow(&place.ty) {
                    deps.extend(self.provenance(operand));
                }
                for loan in &mut deps {
                    self.conflict(loan, Access::Borrow(mutable), span)?;
                    loan.mutable = mutable;
                    loan.origin = span;
                }
                self.temporary.extend(deps.clone());
                let ty = match (expected, &place.ty) {
                    (Some(Type::Slice(m, element)), Type::Array(_, actual))
                        if (!*m || mutable) && element == actual =>
                    {
                        Type::Slice(*m, element.clone())
                    }
                    _ => Type::Ref(mutable, Box::new(place.ty)),
                };
                Value { ty, deps }
            }
            ExprKind::Unary(op, operand) => {
                let value = if *op == UnaryOp::Neg {
                    if let ExprKind::Int(number, suffix) = &operand.kind {
                        let ty = suffix
                            .clone()
                            .or_else(|| expected.filter(|t| t.is_integer()).cloned())
                            .unwrap_or_else(Type::isize);
                        self.integer_range(*number, &ty, true, span)?;
                        operand.ty = ty.clone();
                        Value { ty, deps: vec![] }
                    } else {
                        self.expr(operand, expected, false)?
                    }
                } else {
                    self.expr(operand, expected, false)?
                };
                let valid = match op {
                    UnaryOp::Neg => {
                        matches!(value.ty, Type::Int { signed: true, .. } | Type::Float(_))
                    }
                    UnaryOp::Not => value.ty == Type::Bool,
                    UnaryOp::BitNot => value.ty.is_integer(),
                    _ => false,
                };
                if !valid {
                    return Err(Diagnostic::new(
                        span,
                        format!("unary operator is not defined for `{}`", value.ty),
                    ));
                }
                Value {
                    ty: value.ty,
                    deps: vec![],
                }
            }
            ExprKind::Binary(op, left, right) => {
                let inferred = expected
                    .filter(|t| t.is_numeric())
                    .cloned()
                    .or_else(|| self.peek_type(left))
                    .or_else(|| self.peek_type(right));
                let lhs = self.expr(left, inferred.as_ref(), false)?;
                let rhs = self.expr(right, Some(&lhs.ty), false)?;
                self.expect(&lhs.ty, &rhs.ty, right.span)?;
                let ty = self.binary_type(*op, &lhs.ty, span)?;
                Value { ty, deps: vec![] }
            }
            ExprKind::Call {
                name,
                type_args,
                args,
            } => self.call(name, type_args, args, expected, span)?,
            ExprKind::Cast(operand, target) => {
                self.context.validate_type(target, span, false)?;
                let value = self.expr(operand, None, false)?;
                if value.ty.is_numeric() && target.is_numeric() {
                    Value {
                        ty: target.clone(),
                        deps: vec![],
                    }
                } else if (matches!(value.ty, Type::Raw(..)) || value.ty.is_integer())
                    && matches!(target, Type::Raw(..))
                    || matches!(value.ty, Type::Raw(..)) && target.is_integer()
                {
                    self.unsafe_required(span, "raw pointer conversion")?;
                    Value {
                        ty: target.clone(),
                        deps: vec![],
                    }
                } else if matches!(&value.ty, Type::Ref(..)) && matches!(target, Type::Raw(..)) {
                    if matches!(
                        (&value.ty, &*target),
                        (Type::Ref(false, _), Type::Raw(true, _))
                    ) {
                        return Err(Diagnostic::new(
                            span,
                            "shared reference cannot become a mutable pointer",
                        ));
                    }
                    let (source, destination) = match (&value.ty, &*target) {
                        (Type::Ref(_, s), Type::Raw(_, d)) => (s, d),
                        _ => unreachable!(),
                    };
                    self.expect(source, destination, span)?;
                    Value {
                        ty: target.clone(),
                        deps: vec![],
                    }
                } else if matches!(&value.ty, Type::Raw(..)) && matches!(target, Type::Ref(..)) {
                    self.unsafe_required(span, "raw pointer to checked-reference conversion")?;
                    return Err(Diagnostic::new(
                        span,
                        "raw-pointer-to-reference casts require an explicit lifetime primitive, which 0.1 does not yet provide",
                    ));
                } else {
                    return Err(Diagnostic::new(
                        span,
                        format!("cannot cast `{}` to `{target}`", value.ty),
                    ));
                }
            }
            ExprKind::Try(operand) => {
                let value = self.expr(operand, None, true)?;
                let (success, error) = match &value.ty {
                    Type::Result(t, e) => (t.as_ref().clone(), e.as_ref().clone()),
                    _ => return Err(Diagnostic::new(span, "`?` requires a Result value")),
                };
                match &self.return_ty {
                    Type::Result(_, expected_error) if **expected_error == error => (),
                    Type::Result(..) => {
                        return Err(Diagnostic::new(
                            span,
                            "`?` error type differs from the enclosing function's error type",
                        ));
                    }
                    _ => {
                        return Err(Diagnostic::new(
                            span,
                            "`?` requires an enclosing function returning Result",
                        ));
                    }
                }
                self.check_pending_all(span)?;
                Value {
                    ty: success.clone(),
                    deps: if self.context.carries_borrow(&success) {
                        value.deps
                    } else {
                        vec![]
                    },
                }
            }
            ExprKind::MethodCall { .. } => unreachable!(),
        };
        // Reborrows may weaken exclusive access to shared access; numeric
        // conversions remain explicit, and references never gain mutability.
        if let Some(expected) = expected {
            match (&result.ty, expected) {
                (Type::Ref(true, t), Type::Ref(false, u)) if t == u => {
                    result.ty = expected.clone();
                    for loan in &mut result.deps {
                        loan.mutable = false;
                    }
                }
                (Type::Slice(true, t), Type::Slice(false, u)) if t == u => {
                    result.ty = expected.clone();
                    for loan in &mut result.deps {
                        loan.mutable = false;
                    }
                }
                _ => (),
            }
        }
        expression.ty = result.ty.clone();
        self.expression_deps
            .insert((span.start, span.end), result.deps.clone());
        Ok(result)
    }
    fn infer_loop_type(&self, expression: &Expr, name: &str) -> Option<Type> {
        if let ExprKind::Binary(_, left, right) = &expression.kind {
            if matches!(&left.kind, ExprKind::Name(n) if n == name) {
                return self.peek_type(right).filter(Type::is_integer);
            }
            if matches!(&right.kind, ExprKind::Name(n) if n == name) {
                return self.peek_type(left).filter(Type::is_integer);
            }
            return self
                .infer_loop_type(left, name)
                .or_else(|| self.infer_loop_type(right, name));
        }
        None
    }
    fn integer_range(&self, number: u64, ty: &Type, negative: bool, span: Span) -> Check<()> {
        let Type::Int { signed, bits } = ty else {
            return Err(Diagnostic::new(
                span,
                "integer literal requires an integer type",
            ));
        };
        let bits = if *bits == 0 {
            self.context.pointer_bits
        } else {
            *bits
        };
        let max = if *signed {
            (1u128 << (bits - 1)) - u128::from(!negative)
        } else {
            (1u128 << bits) - 1
        };
        if u128::from(number) > max {
            return Err(Diagnostic::new(
                span,
                format!("integer literal is out of range for `{ty}`"),
            ));
        }
        Ok(())
    }
    fn binary_type(&self, op: BinaryOp, ty: &Type, span: Span) -> Check<Type> {
        use BinaryOp::*;
        match op {
            And | Or if *ty == Type::Bool => Ok(Type::Bool),
            Eq | Ne
                if ty.is_numeric()
                    || *ty == Type::Bool
                    || matches!(ty, Type::Raw(..))
                    || matches!(ty, Type::Named(n) if self.context.enums.get(n).is_some_and(|e| e.variants.iter().all(|v| v.fields.is_empty()))) =>
            {
                Ok(Type::Bool)
            }
            Lt | Le | Gt | Ge if ty.is_numeric() => Ok(Type::Bool),
            Add | Sub | Mul | Div | Rem if ty.is_numeric() => Ok(ty.clone()),
            BitAnd | BitOr | BitXor | Shl | Shr if ty.is_integer() => Ok(ty.clone()),
            _ => Err(Diagnostic::new(
                span,
                format!("operator is not defined for `{ty}`"),
            )),
        }
    }
    fn literal_type(&self, expression: &Expr) -> Option<Type> {
        match &expression.kind {
            ExprKind::Int(..) => Some(Type::isize()),
            ExprKind::Float(..) => Some(Type::Float(64)),
            _ => self.peek_type(expression),
        }
    }
    fn peek_type(&self, expression: &Expr) -> Option<Type> {
        if expression.ty != Type::Unknown {
            return Some(expression.ty.clone());
        }
        match &expression.kind {
            ExprKind::Int(_, suffix) | ExprKind::Float(_, suffix) => suffix.clone(),
            ExprKind::Bool(_) => Some(Type::Bool),
            ExprKind::String(_, bytes) => Some(if *bytes {
                Type::Slice(false, Box::new(Type::u8()))
            } else {
                Type::Str
            }),
            ExprKind::Name(n) => self
                .lookup(n)
                .map(|v| v.ty.clone())
                .or_else(|| self.context.constants.get(n).map(|c| c.ty.clone())),
            ExprKind::Array(t, _)
            | ExprKind::Cast(_, t)
            | ExprKind::Constant(_, t)
            | ExprKind::Repeat(_, t) => Some(t.clone()),
            ExprKind::ValueBlock(_) => None,
            ExprKind::Range(a, b) => self.peek_type(a).or_else(|| self.peek_type(b)),
            ExprKind::Slice { base, mutable, .. } => {
                self.peek_type(base).and_then(|t| match dereferenced(&t) {
                    Type::Array(_, t) | Type::Slice(_, t) => Some(Type::Slice(*mutable, t.clone())),
                    _ => None,
                })
            }
            ExprKind::Struct(n, _) => Some(Type::Named(n.clone())),
            ExprKind::Unary(UnaryOp::Borrow, e) => {
                self.peek_type(e).map(|t| Type::Ref(false, Box::new(t)))
            }
            ExprKind::Unary(UnaryOp::BorrowMut, e) => {
                self.peek_type(e).map(|t| Type::Ref(true, Box::new(t)))
            }
            ExprKind::Unary(UnaryOp::Deref, e) => self.peek_type(e).and_then(|t| match t {
                Type::Ref(_, t) | Type::Raw(_, t) => Some(*t),
                _ => None,
            }),
            ExprKind::Unary(_, e) => self.peek_type(e),
            ExprKind::Binary(op, a, b) => {
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
                    self.peek_type(a).or_else(|| self.peek_type(b))
                }
            }
            ExprKind::Field(base, field) => {
                if let Some(ty) = self.enum_value_type(expression) {
                    return Some(ty);
                }
                let ty = self.peek_type(base)?;
                match dereferenced(&ty) {
                    Type::Named(n) => self
                        .context
                        .structs
                        .get(n)?
                        .fields
                        .iter()
                        .find(|f| f.name == *field)
                        .map(|f| f.ty.clone()),
                    Type::Array(..) | Type::Slice(..) | Type::Str if field == "len" => {
                        Some(Type::usize())
                    }
                    _ => None,
                }
            }
            ExprKind::Index(base, _) => {
                let ty = self.peek_type(base)?;
                match dereferenced(&ty) {
                    Type::Array(_, t) | Type::Slice(_, t) => Some(t.as_ref().clone()),
                    _ => None,
                }
            }
            ExprKind::Call { name, .. } => self
                .context
                .functions
                .get(name)
                .map(|f| f.ret.clone())
                .or_else(|| {
                    name.rsplit_once('.')
                        .filter(|(n, _)| self.context.enums.contains_key(*n))
                        .map(|(n, _)| Type::Named(n.to_owned()))
                }),
            ExprKind::MethodCall { receiver, name, .. } => {
                let ty = self.peek_type(receiver)?;
                if let Type::Named(owner) = dereferenced(&ty) {
                    self.context
                        .functions
                        .get(&format!("{owner}.{name}"))
                        .map(|f| f.ret.clone())
                } else {
                    None
                }
            }
            ExprKind::Try(e) => self.peek_type(e).and_then(|t| match t {
                Type::Result(t, _) => Some(*t),
                _ => None,
            }),
        }
    }
    fn enum_value_type(&self, expression: &Expr) -> Option<Type> {
        if let ExprKind::Field(base, variant) = &expression.kind
            && let Some(name) = qualified_name(base)
            && self.context.enums.get(&name).is_some_and(|e| {
                e.variants
                    .iter()
                    .any(|v| v.name == *variant && v.fields.is_empty())
            })
        {
            return Some(Type::Named(name));
        }
        None
    }
    fn provenance(&self, expression: &Expr) -> Vec<Loan> {
        if let Some(deps) = self
            .expression_deps
            .get(&(expression.span.start, expression.span.end))
            && !matches!(expression.kind, ExprKind::Name(_))
        {
            return deps.clone();
        }
        match &expression.kind {
            ExprKind::Name(name) => self
                .lookup(name)
                .map(|v| {
                    let mut deps = v.deps.clone();
                    for loan in &mut deps {
                        if !loan.via.contains(&v.id) {
                            loan.via.push(v.id);
                        }
                    }
                    deps
                })
                .unwrap_or_else(|| {
                    if self.context.constants.contains_key(name) {
                        vec![static_loan(expression.span)]
                    } else {
                        vec![]
                    }
                }),
            ExprKind::Field(base, _)
            | ExprKind::Index(base, _)
            | ExprKind::Unary(UnaryOp::Deref, base)
            | ExprKind::Cast(base, _) => self.provenance(base),
            ExprKind::String(..) => vec![static_loan(expression.span)],
            _ => vec![],
        }
    }
    fn transitive_dependencies(&self, mut deps: Vec<Loan>) -> Vec<Loan> {
        let mut visited = HashSet::new();
        let mut cursor = 0;
        while cursor < deps.len() {
            let root = deps[cursor].root;
            cursor += 1;
            if visited.insert(root)
                && let Some(variable) = self.by_id(root)
            {
                deps.extend(variable.deps.clone());
            }
        }
        deps
    }
    fn place(&mut self, expression: &mut Expr, initialized: bool) -> Check<Place> {
        let span = expression.span;
        if let Some(name) = qualified_name(expression)
            && let Some(constant) = self.context.constants.get(&name)
        {
            if !constant.public && type_namespace(&name) != self.namespace {
                return Err(Diagnostic::new(
                    span,
                    format!("constant `{name}` is private to its package"),
                ));
            }
            expression.kind = ExprKind::Name(name);
        }

        let place = match &mut expression.kind {
            ExprKind::Name(name) => {
                if let Some(variable) = self.lookup(name).cloned() {
                    if initialized && !variable.initialized {
                        let mut diagnostic = Diagnostic::new(
                            span,
                            format!("`{name}` is uninitialized or has been moved"),
                        )
                        .label(variable.span, format!("binding `{name}` is declared here"));
                        diagnostic = if let Some(moved_at) = variable.moved_at {
                            diagnostic
                                .primary_label(format!("cannot use `{name}` after it was moved"))
                                .label(moved_at, format!("value `{name}` is moved here"))
                        } else {
                            diagnostic
                                .primary_label(format!("`{name}` is not initialized on every path"))
                        };
                        return Err(diagnostic);
                    }
                    Place {
                        ty: variable.ty,
                        loans: vec![Loan {
                            root: variable.id,
                            fields: vec![],
                            mutable: !variable.immutable,
                            origin: span,
                            via: vec![],
                            external: None,
                        }],
                        mutable: !variable.immutable,
                        direct: Some(variable.id),
                    }
                } else if let Some(constant) = self.context.constants.get(name) {
                    if constant.mutable {
                        self.unsafe_required(span, "mutable static access")?;
                    }
                    Place {
                        ty: constant.ty.clone(),
                        loans: vec![static_loan(span)],
                        mutable: constant.mutable,
                        direct: None,
                    }
                } else {
                    return Err(Diagnostic::new(span, format!("unknown binding `{name}`")));
                }
            }
            ExprKind::Unary(UnaryOp::Deref, operand) => {
                let value = self.expr(operand, None, false)?;
                match value.ty {
                    Type::Ref(mutable, ty) => Place {
                        ty: *ty,
                        loans: value.deps,
                        mutable,
                        direct: None,
                    },
                    Type::Raw(mutable, ty) => {
                        self.unsafe_required(span, "raw pointer dereference")?;
                        if self.context.carries_borrow(&ty) {
                            return Err(Diagnostic::new(
                                span,
                                "accessing checked-borrow values through raw pointers is unsupported",
                            ));
                        }
                        Place {
                            ty: *ty,
                            loans: vec![],
                            mutable,
                            direct: None,
                        }
                    }
                    _ => {
                        return Err(Diagnostic::new(
                            span,
                            "dereference requires a reference or raw pointer",
                        ));
                    }
                }
            }
            ExprKind::Field(base, field) => {
                let mut place = self.place(base, true)?;
                while let Type::Ref(mutable, inner) = place.ty.clone() {
                    place.ty = *inner;
                    place.mutable = mutable;
                    place.loans = self.provenance(base);
                    place.direct = None;
                }
                let field_ty = match &place.ty {
                    Type::Named(name) => {
                        let declared = self
                            .context
                            .structs
                            .get(name)
                            .and_then(|s| s.fields.iter().find(|f| f.name == *field))
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    span,
                                    format!("type `{name}` has no field `{field}`"),
                                )
                            })?;
                        if !declared.public && type_namespace(name) != self.namespace {
                            return Err(Diagnostic::new(
                                span,
                                format!("field `{field}` is private to its package"),
                            ));
                        }
                        declared.ty.clone()
                    }
                    Type::Array(..) | Type::Slice(..) | Type::Str if field == "len" => {
                        place.mutable = false;
                        Type::usize()
                    }
                    _ => {
                        return Err(Diagnostic::new(
                            span,
                            format!("type `{}` has no field `{field}`", place.ty),
                        ));
                    }
                };
                for loan in &mut place.loans {
                    loan.fields.push(field.clone());
                }
                place.ty = field_ty;
                place.direct = None;
                place
            }
            ExprKind::Index(base, index) => {
                let mut place = self.place(base, true)?;
                while let Type::Ref(mutable, inner) = place.ty.clone() {
                    place.ty = *inner;
                    place.mutable = mutable;
                    place.loans = self.provenance(base);
                    place.direct = None;
                }
                let (element, length) = match place.ty.clone() {
                    Type::Array(n, t) => (*t, Some(n)),
                    Type::Slice(m, t) => {
                        place.mutable = m;
                        place.loans = self.provenance(base);
                        (*t, None)
                    }
                    Type::Str => {
                        return Err(Diagnostic::new(
                            span,
                            "UTF-8 strings cannot be indexed; use a byte slice",
                        ));
                    }
                    _ => return Err(Diagnostic::new(span, "indexing requires an array or slice")),
                };
                let value = self.expr(index, Some(&Type::usize()), false)?;
                if !value.ty.is_integer() {
                    return Err(Diagnostic::new(
                        index.span,
                        "array index must be an integer",
                    ));
                }
                if let (Some(length), ExprKind::Int(index, _)) = (length, &index.kind)
                    && *index >= length as u64
                {
                    return Err(Diagnostic::new(
                        span,
                        format!("constant index {index} is outside array length {length}"),
                    ));
                }
                for loan in &mut place.loans {
                    loan.fields.push("[]".into());
                }
                place.ty = element;
                place.direct = None;
                place
            }
            ExprKind::Call { .. }
            | ExprKind::String(..)
            | ExprKind::MethodCall { .. }
            | ExprKind::Slice { .. }
            | ExprKind::ValueBlock(..) => {
                let value = self.expr(expression, None, false)?;
                if !matches!(value.ty, Type::Ref(..) | Type::Slice(..) | Type::Str) {
                    return Err(Diagnostic::new(
                        span,
                        "expected an addressable place; bind the temporary to a local first",
                    ));
                }
                Place {
                    mutable: mutable_borrow(&value.ty),
                    ty: value.ty,
                    loans: value.deps,
                    direct: None,
                }
            }
            _ => {
                return Err(Diagnostic::new(
                    span,
                    "expected an addressable place (binding, field, index, or dereference)",
                ));
            }
        };
        expression.ty = place.ty.clone();
        Ok(place)
    }
    fn call(
        &mut self,
        name: &mut String,
        type_args: &mut [Type],
        args: &mut [Expr],
        expected: Option<&Type>,
        span: Span,
    ) -> Check<Value> {
        if name.ends_with(".drop") && name != "core.drop" {
            return Err(Diagnostic::new(
                span,
                "a custom drop method cannot be called directly; use `core.drop(value)`",
            ));
        }
        if name == "some" && expected.is_none() {
            if args.len() != 1 {
                return Err(Diagnostic::new(span, "`some` expects one argument"));
            }
            let value = self.expr(&mut args[0], None, true)?;
            if value.ty == Type::Void {
                return Err(Diagnostic::new(span, "Option requires a non-void payload"));
            }
            return Ok(Value {
                ty: Type::Option(Box::new(value.ty)),
                deps: value.deps,
            });
        }
        if matches!(name.as_str(), "ok" | "err" | "some" | "none") {
            let ty = expected.cloned().ok_or_else(|| {
                Diagnostic::new(
                    span,
                    format!("`{name}` needs a Result or Option type from its context"),
                )
            })?;
            let payload = match (name.as_str(), &ty) {
                ("ok", Type::Result(t, _)) => t.as_ref().clone(),
                ("err", Type::Result(_, e)) => e.as_ref().clone(),
                ("some", Type::Option(t)) => t.as_ref().clone(),
                ("none", Type::Option(_)) => Type::Void,
                _ => {
                    return Err(Diagnostic::new(
                        span,
                        format!("`{name}` cannot construct `{ty}`"),
                    ));
                }
            };
            let count = usize::from(payload != Type::Void);
            if args.len() != count {
                return Err(Diagnostic::new(
                    span,
                    format!("`{name}` expects {count} arguments"),
                ));
            }
            let deps = if count == 1 {
                let val = self.expr(&mut args[0], Some(&payload), true)?;
                self.expect(&payload, &val.ty, args[0].span)?;
                val.deps
            } else {
                vec![]
            };
            return Ok(Value { ty, deps });
        }
        if let Some((owner, variant)) = name.rsplit_once('.')
            && let Some(declaration) = self.context.enums.get(owner)
        {
            let variant = declaration
                .variants
                .iter()
                .find(|v| v.name == variant)
                .cloned()
                .ok_or_else(|| Diagnostic::new(span, format!("unknown variant `{name}`")))?;
            if args.len() != variant.fields.len() {
                return Err(Diagnostic::new(
                    span,
                    format!(
                        "variant `{name}` expects {} arguments",
                        variant.fields.len()
                    ),
                ));
            }
            let mut deps = vec![];
            for (arg, field) in args.iter_mut().zip(&variant.fields) {
                let val = self.expr(arg, Some(&field.ty), true)?;
                self.expect(&field.ty, &val.ty, arg.span)?;
                deps.extend(val.deps);
            }
            return Ok(Value {
                ty: Type::Named(owner.into()),
                deps,
            });
        }
        if name == "core.drop" {
            if args.len() != 1 {
                return Err(Diagnostic::new(span, "core.drop expects one argument"));
            }
            let value = self.expr(&mut args[0], None, true)?;
            if self.context.contains_result(&value.ty) {
                return Err(Diagnostic::new(
                    span,
                    "a Result cannot be discarded with core.drop",
                ));
            }
            return Ok(Value {
                ty: Type::Void,
                deps: vec![],
            });
        }
        let normalized = if let Some((prefix, suffix)) = name.split_once('.') {
            if matches!(prefix, "mem" | "ptr" | "mmio") {
                if !self.context.imports.contains(&format!("core/{prefix}")) {
                    return Err(Diagnostic::new(
                        span,
                        format!("`{prefix}` requires `import \"core/{prefix}\"`"),
                    ));
                }
                format!("core.{prefix}.{suffix}")
            } else {
                name.clone()
            }
        } else {
            name.clone()
        };
        if normalized.starts_with("core.") {
            *name = normalized;
            return self.intrinsic(name, type_args, args, span);
        }
        let signature = self
            .context
            .functions
            .get(name)
            .cloned()
            .ok_or_else(|| Diagnostic::new(span, format!("unknown function `{name}`")))?;
        if !signature.public && self.context.function_namespace(name) != self.namespace {
            return Err(Diagnostic::new(
                span,
                format!("function `{name}` is private to its package"),
            ));
        }
        if signature.unsafe_ || signature.extern_ {
            self.unsafe_required(span, "calling an unsafe or foreign function")?;
        }
        if !type_args.is_empty() {
            return Err(Diagnostic::new(
                span,
                "type arguments supplied to a non-generic function",
            ));
        }
        if args.len() != signature.params.len() {
            return Err(Diagnostic::new(
                span,
                format!(
                    "`{name}` expects {} arguments, found {}",
                    signature.params.len(),
                    args.len()
                ),
            ));
        }
        let temporary_start = self.temporary.len();
        let mut dependencies = vec![];
        for (arg, parameter) in args.iter_mut().zip(&signature.params) {
            let reference = matches!(parameter.ty, Type::Ref(..) | Type::Slice(..));
            let value = self.expr(arg, Some(&parameter.ty), !reference)?;
            self.expect(&parameter.ty, &value.ty, arg.span)?;
            if reference
                && !matches!(
                    arg.kind,
                    ExprKind::Unary(UnaryOp::Borrow | UnaryOp::BorrowMut, _)
                )
            {
                for mut loan in value.deps.clone() {
                    loan.mutable = mutable_borrow(&parameter.ty);
                    self.conflict(&loan, Access::Borrow(loan.mutable), arg.span)?;
                    // Passing a reference reserves a temporary reborrow here.
                    // Returned dependencies retain the original source span.
                    loan.origin = arg.span;
                    self.temporary.push(loan);
                }
            }
            if signature.from.contains(&parameter.name) {
                dependencies.extend(value.deps);
            }
        }
        if signature.from.iter().any(|n| n == "static") {
            dependencies.push(static_loan(span));
        }
        self.temporary.truncate(temporary_start);
        if self.context.carries_borrow(&signature.ret) {
            self.temporary.extend(dependencies.clone());
        } else {
            dependencies.clear();
        }
        Ok(Value {
            ty: signature.ret,
            deps: dependencies,
        })
    }
    fn intrinsic(
        &mut self,
        name: &str,
        type_args: &[Type],
        args: &mut [Expr],
        span: Span,
    ) -> Check<Value> {
        if matches!(name, "core.mem.size_of" | "core.mem.align_of") {
            if type_args.len() != 1 || !args.is_empty() {
                return Err(Diagnostic::new(
                    span,
                    format!("{name} expects one type argument and no value arguments"),
                ));
            }
            self.context.validate_type(&type_args[0], span, true)?;
            return Ok(Value {
                ty: Type::usize(),
                deps: vec![],
            });
        }
        if let Some(operation) = name.strip_prefix("core.mmio.") {
            self.unsafe_required(span, "volatile MMIO access")?;
            let (write, width) = if let Some(w) = operation.strip_prefix("read") {
                (false, w)
            } else if let Some(w) = operation.strip_prefix("write") {
                (true, w)
            } else {
                return Err(Diagnostic::new(
                    span,
                    format!("unknown MMIO operation `{operation}`"),
                ));
            };
            let width: u32 = width
                .parse()
                .ok()
                .filter(|w| matches!(w, 8 | 16 | 32 | 64))
                .ok_or_else(|| Diagnostic::new(span, "MMIO width must be 8, 16, 32, or 64 bits"))?;
            if width > self.context.pointer_bits {
                return Err(Diagnostic::new(
                    span,
                    "MMIO access width is unsupported on this target",
                ));
            }
            let ty = Type::Int {
                signed: false,
                bits: width,
            };
            let expected = if write {
                vec![Type::usize(), ty.clone()]
            } else {
                vec![Type::usize()]
            };
            self.arguments(args, &expected, span)?;
            return Ok(Value {
                ty: if write { Type::Void } else { ty },
                deps: vec![],
            });
        }
        if let Some(operation) = name.strip_prefix("core.ptr.") {
            self.unsafe_required(span, "raw pointer operation")?;
            if args.is_empty() {
                return Err(Diagnostic::new(
                    span,
                    "pointer operation requires a pointer argument",
                ));
            }
            let pointer = self.expr(&mut args[0], None, false)?;
            let (mutable, element) = match pointer.ty.clone() {
                Type::Raw(m, t) => (m, *t),
                _ => {
                    return Err(Diagnostic::new(
                        args[0].span,
                        "pointer operation requires a raw pointer",
                    ));
                }
            };
            if type_args.len() > 1 || type_args.first().is_some_and(|t| t != &element) {
                return Err(Diagnostic::new(
                    span,
                    "pointer type argument does not match the pointer element type",
                ));
            }
            match operation {
                "read" | "read_unaligned" | "read_volatile" => {
                    if self.context.carries_borrow(&element) {
                        return Err(Diagnostic::new(
                            span,
                            "reading checked-borrow values through raw pointers is unsupported",
                        ));
                    }
                    if args.len() != 1 {
                        return Err(Diagnostic::new(span, "pointer read expects one argument"));
                    }
                    Ok(Value {
                        ty: element,
                        deps: vec![],
                    })
                }
                "write" | "write_unaligned" | "write_volatile" => {
                    if args.len() != 2 {
                        return Err(Diagnostic::new(span, "pointer write expects two arguments"));
                    }
                    if !mutable {
                        return Err(Diagnostic::new(
                            span,
                            "cannot write through a const raw pointer",
                        ));
                    }
                    if self.context.carries_borrow(&element) {
                        return Err(Diagnostic::new(
                            span,
                            "writing checked borrows through raw pointers is unsupported",
                        ));
                    }
                    let value = self.expr(&mut args[1], Some(&element), true)?;
                    self.expect(&element, &value.ty, args[1].span)?;
                    Ok(Value {
                        ty: Type::Void,
                        deps: vec![],
                    })
                }
                "offset" => {
                    if args.len() != 2 {
                        return Err(Diagnostic::new(
                            span,
                            "pointer offset expects two arguments",
                        ));
                    }
                    let value = self.expr(&mut args[1], Some(&Type::isize()), false)?;
                    self.expect(&Type::isize(), &value.ty, args[1].span)?;
                    Ok(Value {
                        ty: pointer.ty,
                        deps: vec![],
                    })
                }
                _ => Err(Diagnostic::new(
                    span,
                    format!("unsupported pointer intrinsic `{operation}`"),
                )),
            }
        } else {
            Err(Diagnostic::new(
                span,
                format!("unknown core intrinsic `{name}`"),
            ))
        }
    }
    fn arguments(&mut self, args: &mut [Expr], expected: &[Type], span: Span) -> Check<()> {
        if args.len() != expected.len() {
            return Err(Diagnostic::new(
                span,
                format!(
                    "expected {} arguments, found {}",
                    expected.len(),
                    args.len()
                ),
            ));
        }
        for (arg, ty) in args.iter_mut().zip(expected) {
            let value = self.expr(arg, Some(ty), true)?;
            self.expect(ty, &value.ty, arg.span)?;
        }
        Ok(())
    }
    fn pattern_variants(&self, ty: &Type, span: Span) -> Check<HashMap<String, Vec<Type>>> {
        Ok(match ty {
            Type::Bool => [("true".into(), vec![]), ("false".into(), vec![])].into(),
            Type::Int { .. } => HashMap::new(),
            Type::Result(t, e) => [
                (
                    "ok".into(),
                    if **t == Type::Void {
                        vec![]
                    } else {
                        vec![t.as_ref().clone()]
                    },
                ),
                ("err".into(), vec![e.as_ref().clone()]),
            ]
            .into(),
            Type::Option(t) => [
                ("some".into(), vec![t.as_ref().clone()]),
                ("none".into(), vec![]),
            ]
            .into(),
            Type::Named(name) => self
                .context
                .enums
                .get(name)
                .ok_or_else(|| {
                    Diagnostic::new(
                        span,
                        "match requires a bool, integer, enum, Option, or Result",
                    )
                })?
                .variants
                .iter()
                .map(|v| {
                    (
                        v.name.clone(),
                        v.fields.iter().map(|f| f.ty.clone()).collect(),
                    )
                })
                .collect(),
            _ => {
                return Err(Diagnostic::new(
                    span,
                    "match requires a bool, integer, enum, Option, or Result",
                ));
            }
        })
    }
    fn mark_matched_result(&mut self, expression: &Expr) {
        match &expression.kind {
            ExprKind::Unary(UnaryOp::Borrow | UnaryOp::BorrowMut, owner) => {
                self.mark_matched_result(owner)
            }
            ExprKind::Name(name) => {
                if let Some(variable) = self.lookup(name).cloned() {
                    self.by_id_mut(variable.id).unwrap().pending_result = false;
                    for loan in variable.deps {
                        if loan.fields.is_empty()
                            && let Some(owner) = self.by_id_mut(loan.root)
                        {
                            owner.pending_result = false;
                        }
                    }
                }
            }
            _ => (),
        }
    }
    fn bind_pattern(
        &mut self,
        pattern: &Pattern,
        bindings: Vec<(String, Type)>,
        value: &Value,
        immutable: bool,
        span: Span,
    ) -> Check<()> {
        let mut paths = HashMap::new();
        if matches!(value.ty, Type::Ref(..)) {
            pattern_binding_paths(pattern, &[], &mut paths);
        }
        for (name, ty) in bindings {
            let deps = if self.context.carries_borrow(&ty) {
                if let Some(projections) = paths.get(&name) {
                    projections
                        .iter()
                        .flat_map(|path| {
                            value.deps.iter().map(move |loan| {
                                let mut loan = loan.clone();
                                loan.fields.extend(path.iter().cloned());
                                loan
                            })
                        })
                        .collect()
                } else {
                    value.deps.clone()
                }
            } else {
                vec![]
            };
            let pending = self.context.contains_result(dereferenced(&ty));
            self.bind(name, ty, Some(deps), immutable, span)?;
            if pending {
                self.scopes
                    .last_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .pending_result = true;
            }
        }
        Ok(())
    }
    fn check_conditional_pattern(
        &self,
        pattern: &CheckedPattern,
        ty: &Type,
        span: Span,
    ) -> Check<()> {
        if self.context.contains_result(dereferenced(ty))
            && !self.patterns_exhaustive(
                &[
                    vec![pattern.clone()],
                    vec![self.result_free_pattern(dereferenced(ty))],
                ],
                std::slice::from_ref(ty),
            )?
        {
            return Err(Diagnostic::new(
                span,
                "conditional patterns cannot discard a Result on the unmatched path",
            )
            .note("use an exhaustive match that explicitly handles both `ok` and `err`"));
        }
        Ok(())
    }
    // An unmatched value may be discarded only when its active payload contains
    // no Result. For example, `none` is harmless for Option<Result<T, E>>, while
    // either Result constructor still imposes an explicit handling obligation.
    fn result_free_pattern(&self, ty: &Type) -> CheckedPattern {
        if !self.context.contains_result(ty) {
            return CheckedPattern::Any;
        }
        match ty {
            Type::Option(inner) => CheckedPattern::Or(vec![
                CheckedPattern::Constructor("none".into(), vec![]),
                CheckedPattern::Constructor("some".into(), vec![self.result_free_pattern(inner)]),
            ]),
            Type::Named(name) => {
                if let Some(structure) = self.context.structs.get(name) {
                    CheckedPattern::Constructor(
                        "$struct".into(),
                        structure
                            .fields
                            .iter()
                            .map(|f| self.result_free_pattern(&f.ty))
                            .collect(),
                    )
                } else if let Some(enumeration) = self.context.enums.get(name) {
                    CheckedPattern::Or(
                        enumeration
                            .variants
                            .iter()
                            .map(|v| {
                                CheckedPattern::Constructor(
                                    v.name.clone(),
                                    v.fields
                                        .iter()
                                        .map(|f| self.result_free_pattern(&f.ty))
                                        .collect(),
                                )
                            })
                            .collect(),
                    )
                } else {
                    CheckedPattern::Or(vec![])
                }
            }
            // Result itself is never freely discardable. Array patterns do not
            // inspect elements, so only an irrefutable binding can handle one.
            _ => CheckedPattern::Or(vec![]),
        }
    }
    fn check_pattern(
        &self,
        pattern: &Pattern,
        ty: &Type,
        borrowed: Option<bool>,
        span: Span,
    ) -> Check<(CheckedPattern, Vec<(String, Type)>)> {
        let (checked, bindings) = self.pattern_inner(pattern, ty, borrowed, span)?;
        if pattern_expansion_size(&checked) > 4096 {
            return Err(Diagnostic::new(
                span,
                "pattern expands to more than 4096 alternatives; split it into smaller patterns",
            ));
        }
        let mut names = HashSet::new();
        for (name, _) in &bindings {
            if !names.insert(name) {
                return Err(Diagnostic::new(
                    span,
                    format!("duplicate pattern binding `{name}`"),
                ));
            }
        }
        Ok((checked, bindings))
    }
    fn pattern_inner(
        &self,
        pattern: &Pattern,
        ty: &Type,
        borrowed: Option<bool>,
        span: Span,
    ) -> Check<(CheckedPattern, Vec<(String, Type)>)> {
        if let Pattern::Or(alternatives) = pattern {
            let mut checked = vec![];
            let mut bindings = None;
            let mut comparison = None;
            for alternative in alternatives {
                let (p, names) = self.check_pattern(alternative, ty, borrowed, span)?;
                let mut sorted = names.clone();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                if comparison
                    .as_ref()
                    .is_some_and(|previous| previous != &sorted)
                {
                    return Err(Diagnostic::new(
                        span,
                        "alternative patterns must bind the same names with the same types and borrowing modes",
                    ));
                }
                comparison = Some(sorted);
                if bindings.is_none() {
                    bindings = Some(names);
                }
                checked.push(p);
            }
            return Ok((CheckedPattern::Or(checked), bindings.unwrap_or_default()));
        }
        if let Pattern::Binding(name) = pattern {
            let ty = borrowed.map_or_else(|| ty.clone(), |m| Type::Ref(m, Box::new(ty.clone())));
            if ty == Type::Void {
                return Err(Diagnostic::new(span, "cannot bind a void value"));
            }
            return Ok((CheckedPattern::Any, vec![(name.clone(), ty)]));
        }
        if matches!(pattern, Pattern::Wildcard) {
            if self.context.contains_result(dereferenced(ty)) {
                return Err(Diagnostic::new(
                    span,
                    "a Result pattern must explicitly handle both `ok` and `err`; a nested Result cannot be discarded",
                ));
            }
            return Ok((CheckedPattern::Any, vec![]));
        }
        let mut ty = ty;
        let mut borrowed = borrowed;
        while let Type::Ref(mutable, inner) = ty {
            borrowed = Some(borrowed.unwrap_or(true) && *mutable);
            ty = inner;
        }
        match pattern {
            Pattern::Bool(value) if *ty == Type::Bool => Ok((
                CheckedPattern::Constructor(value.to_string(), vec![]),
                vec![],
            )),
            Pattern::Int(value) if ty.is_integer() => {
                let value = self.pattern_integer(*value, ty, span)?;
                Ok((CheckedPattern::Range(value, value), vec![]))
            }
            Pattern::Range(start, end, inclusive) if ty.is_integer() => {
                let start = self.pattern_integer(*start, ty, span)?;
                let end = self.pattern_integer(*end, ty, span)? - i128::from(!*inclusive);
                if start > end {
                    return Err(Diagnostic::new(span, "range pattern is empty or reversed"));
                }
                Ok((CheckedPattern::Range(start, end), vec![]))
            }
            Pattern::Variant(name, payloads) => {
                let short = if let Some((owner, name)) = name.rsplit_once('.') {
                    if *ty != Type::Named(owner.into()) {
                        return Err(Diagnostic::new(
                            span,
                            format!("pattern `{owner}.{name}` does not match `{ty}`"),
                        ));
                    }
                    name
                } else {
                    name
                };
                let variants = self.pattern_variants(ty, span)?;
                let fields = variants.get(short).ok_or_else(|| {
                    Diagnostic::new(span, format!("unknown variant pattern `{name}` for `{ty}`"))
                })?;
                if payloads.len() != fields.len() {
                    return Err(Diagnostic::new(
                        span,
                        format!("pattern `{name}` expects {} payload patterns", fields.len()),
                    ));
                }
                let mut checked = vec![];
                let mut bindings = vec![];
                for (pattern, field) in payloads.iter().zip(fields) {
                    let (p, names) = self.pattern_inner(pattern, field, borrowed, span)?;
                    checked.push(p);
                    bindings.extend(names);
                }
                Ok((CheckedPattern::Constructor(short.into(), checked), bindings))
            }
            Pattern::Struct(name, fields, rest) => {
                if *ty != Type::Named(name.clone()) {
                    return Err(Diagnostic::new(
                        span,
                        format!("struct pattern `{name}` does not match `{ty}`"),
                    ));
                }
                let declaration = self.context.structs.get(name).ok_or_else(|| {
                    Diagnostic::new(span, format!("unknown struct pattern `{name}`"))
                })?;
                if borrowed.is_none()
                    && self.context.functions.contains_key(&format!("{name}.drop"))
                {
                    return Err(Diagnostic::new(
                        span,
                        "cannot destructure an owned struct with a custom drop method; borrow it instead",
                    ));
                }
                let mut seen = HashSet::new();
                for (field, _) in fields {
                    if !seen.insert(field) {
                        return Err(Diagnostic::new(
                            span,
                            format!("duplicate field `{field}` in pattern"),
                        ));
                    }
                    let declared = declaration
                        .fields
                        .iter()
                        .find(|f| &f.name == field)
                        .ok_or_else(|| {
                            Diagnostic::new(
                                span,
                                format!("unknown field `{field}` in struct pattern"),
                            )
                        })?;
                    if !declared.public && type_namespace(name) != self.namespace {
                        return Err(Diagnostic::new(
                            span,
                            format!("field `{field}` is private to its package"),
                        ));
                    }
                }
                let mut checked = vec![];
                let mut bindings = vec![];
                for field in &declaration.fields {
                    let pattern = fields
                        .iter()
                        .find(|(n, _)| n == &field.name)
                        .map(|(_, p)| p);
                    if pattern.is_none() && !*rest {
                        return Err(Diagnostic::new(
                            span,
                            format!(
                                "missing field `{}` in struct pattern; use `..` to omit fields",
                                field.name
                            ),
                        ));
                    }
                    let (p, names) = self.pattern_inner(
                        pattern.unwrap_or(&Pattern::Wildcard),
                        &field.ty,
                        borrowed,
                        span,
                    )?;
                    checked.push(p);
                    bindings.extend(names);
                }
                let order = pattern.bindings();
                bindings.sort_by_key(|(name, _)| {
                    order.iter().position(|n| n == name).unwrap_or(usize::MAX)
                });
                Ok((
                    CheckedPattern::Constructor("$struct".into(), checked),
                    bindings,
                ))
            }
            _ => Err(Diagnostic::new(
                span,
                format!("pattern does not match `{ty}`"),
            )),
        }
    }
    fn pattern_integer(&self, value: u64, ty: &Type, span: Span) -> Check<i128> {
        if matches!(ty, Type::Int { signed: true, .. }) && value > i64::MAX as u64 {
            self.integer_range(value.wrapping_neg(), ty, true, span)?;
            Ok(value as i64 as i128)
        } else {
            self.integer_range(value, ty, false, span)?;
            Ok(i128::from(value))
        }
    }
    // Specialize one column at a time. Integer interval boundaries partition a
    // finite domain, avoiding enumeration of even 64-bit ranges. Constructor
    // payload columns retain correlations between fields and nested patterns.
    fn patterns_exhaustive(&self, rows: &[Vec<CheckedPattern>], types: &[Type]) -> Check<bool> {
        self.coverage(rows, types, &mut 131_072).ok_or_else(|| Diagnostic::new(
            Span { start: self.position, end: self.position },
            "pattern exhaustiveness analysis is too complex; simplify the patterns or add a fallback arm",
        ))
    }
    fn coverage(
        &self,
        rows: &[Vec<CheckedPattern>],
        types: &[Type],
        budget: &mut usize,
    ) -> Option<bool> {
        *budget = budget.checked_sub(rows.len() + types.len() + 1)?;
        if types.is_empty() {
            return Some(!rows.is_empty());
        }
        if rows.is_empty() {
            return Some(false);
        }
        if rows
            .iter()
            .any(|r| r.iter().all(|p| matches!(p, CheckedPattern::Any)))
        {
            return Some(true);
        }
        if rows.iter().all(|r| matches!(r[0], CheckedPattern::Any)) {
            let tails: Vec<_> = rows.iter().map(|r| r[1..].to_vec()).collect();
            return self.coverage(&tails, &types[1..], budget);
        }
        let mut expanded = vec![];
        for row in rows {
            expand_pattern_row(row, &mut expanded);
        }
        *budget = budget.checked_sub(expanded.len())?;
        let ty = dereferenced(&types[0]);
        if let Type::Int { signed, bits } = ty {
            let bits = if *bits == 0 {
                self.context.pointer_bits
            } else {
                *bits
            };
            let (min, max) = if *signed {
                (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
            } else {
                (0, (1i128 << bits) - 1)
            };
            let mut boundaries = vec![min, max + 1];
            for row in &expanded {
                if let CheckedPattern::Range(a, b) = row[0] {
                    boundaries.extend([a, b + 1]);
                }
            }
            boundaries.sort_unstable();
            boundaries.dedup();
            for interval in boundaries.windows(2) {
                let point = interval[0];
                let specialized: Vec<_> = expanded
                    .iter()
                    .filter(|row| match row[0] {
                        CheckedPattern::Any => true,
                        CheckedPattern::Range(a, b) => a <= point && point <= b,
                        _ => false,
                    })
                    .map(|row| row[1..].to_vec())
                    .collect();
                if !self.coverage(&specialized, &types[1..], budget)? {
                    return Some(false);
                }
            }
            return Some(true);
        }
        let constructors: Vec<(String, Vec<Type>)> = if let Type::Named(name) = ty
            && let Some(s) = self.context.structs.get(name)
        {
            vec![(
                "$struct".into(),
                s.fields.iter().map(|f| f.ty.clone()).collect(),
            )]
        } else if let Ok(variants) = self.pattern_variants(ty, Span::default()) {
            variants.into_iter().collect()
        } else {
            let defaults: Vec<_> = expanded
                .iter()
                .filter(|r| matches!(r[0], CheckedPattern::Any))
                .map(|r| r[1..].to_vec())
                .collect();
            return self.coverage(&defaults, &types[1..], budget);
        };
        for (name, fields) in constructors {
            let specialized: Vec<_> = expanded
                .iter()
                .filter_map(|row| {
                    let mut payload = match &row[0] {
                        CheckedPattern::Any => vec![CheckedPattern::Any; fields.len()],
                        CheckedPattern::Constructor(key, payload) if key == &name => {
                            payload.clone()
                        }
                        _ => return None,
                    };
                    payload.extend_from_slice(&row[1..]);
                    Some(payload)
                })
                .collect();
            let mut specialized_types = fields;
            specialized_types.extend_from_slice(&types[1..]);
            if !self.coverage(&specialized, &specialized_types, budget)? {
                return Some(false);
            }
        }
        Some(true)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CheckedPattern {
    Any,
    Constructor(String, Vec<CheckedPattern>),
    Range(i128, i128),
    Or(Vec<CheckedPattern>),
}
fn pattern_binding_paths(
    pattern: &Pattern,
    path: &[String],
    paths: &mut HashMap<String, Vec<Vec<String>>>,
) {
    match pattern {
        Pattern::Binding(name) => {
            paths.entry(name.clone()).or_default().push(path.to_vec());
        }
        Pattern::Struct(_, fields, _) => {
            for (field, pattern) in fields {
                let mut projected = path.to_vec();
                projected.push(field.clone());
                pattern_binding_paths(pattern, &projected, paths);
            }
        }
        Pattern::Variant(_, fields) | Pattern::Or(fields) => {
            for pattern in fields {
                pattern_binding_paths(pattern, path, paths);
            }
        }
        _ => (),
    }
}
fn pattern_expansion_size(pattern: &CheckedPattern) -> usize {
    match pattern {
        CheckedPattern::Or(patterns) => patterns
            .iter()
            .fold(0usize, |n, p| n.saturating_add(pattern_expansion_size(p))),
        CheckedPattern::Constructor(_, fields) => fields
            .iter()
            .fold(1usize, |n, p| n.saturating_mul(pattern_expansion_size(p))),
        _ => 1,
    }
}
fn match_subject(ty: &Type) -> (&Type, Option<bool>) {
    if let Type::Ref(m, inner) = ty {
        (inner, Some(*m))
    } else {
        (ty, None)
    }
}
fn expand_pattern_row(row: &[CheckedPattern], result: &mut Vec<Vec<CheckedPattern>>) {
    if let Some(CheckedPattern::Or(alternatives)) = row.first() {
        for alternative in alternatives {
            let mut expanded = vec![alternative.clone()];
            expanded.extend_from_slice(&row[1..]);
            expand_pattern_row(&expanded, result);
        }
    } else {
        result.push(row.to_vec());
    }
}

fn qualified_name(expression: &Expr) -> Option<String> {
    match &expression.kind {
        ExprKind::Name(n) => Some(n.clone()),
        ExprKind::Field(e, n) => qualified_name(e).map(|prefix| format!("{prefix}.{n}")),
        _ => None,
    }
}
fn type_namespace(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(owner, _)| owner)
}
fn static_loan(span: Span) -> Loan {
    Loan {
        root: 0,
        fields: vec![],
        mutable: false,
        origin: span,
        via: vec![],
        external: Some("static".into()),
    }
}
fn dereferenced(mut ty: &Type) -> &Type {
    while let Type::Ref(_, inner) = ty {
        ty = inner;
    }
    ty
}
fn mutable_borrow(ty: &Type) -> bool {
    matches!(ty, Type::Ref(true, _) | Type::Slice(true, _))
}
fn overlaps(a: &Loan, b: &Loan) -> bool {
    a.root == b.root
        && a.fields
            .iter()
            .zip(&b.fields)
            .all(|(a, b)| a == b || a == "[]" || b == "[]")
}
fn incompatible(access: Access, mutable: bool) -> bool {
    match access {
        Access::Read | Access::Borrow(false) => mutable,
        Access::Write | Access::Move | Access::Borrow(true) => true,
    }
}
fn merge_states(mut a: Vec<Vec<Variable>>, b: Vec<Vec<Variable>>) -> Vec<Vec<Variable>> {
    for variable in a.iter_mut().flatten() {
        if let Some(other) = b.iter().flatten().find(|v| v.id == variable.id) {
            variable.initialized &= other.initialized;
            variable.moved_at = variable.moved_at.or(other.moved_at);
            variable.pending_result |= other.pending_result;
            variable.deps.extend(other.deps.clone());
            variable
                .deps
                .sort_by_key(|d| (d.root, d.fields.clone(), d.mutable));
            variable.deps.dedup_by(|a, b| {
                a.root == b.root
                    && a.fields == b.fields
                    && a.mutable == b.mutable
                    && a.external == b.external
            });
        }
    }
    a
}
fn names_expr(expression: &Expr, names: &mut HashMap<String, Span>) {
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
fn names_stmt(statement: &Stmt, names: &mut HashMap<String, Span>) {
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
fn names_block(block: &Block, names: &mut HashMap<String, Span>) {
    for s in block {
        names_stmt(s, names);
    }
}

/// Resolve diagnostic use sites lexically. The conservative name-based liveness
/// calculation above remains unchanged; a shadowed name must not be presented
/// as evidence that an earlier binding's borrow is used.
fn binding_use_spans(function: &Function) -> HashMap<(usize, String), Span> {
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

fn contains_break(block: &Block) -> bool {
    block.iter().any(|s| match &s.kind {
        StmtKind::Break => true,
        StmtKind::If {
            then_block,
            else_block,
            ..
        }
        | StmtKind::IfLet {
            then_block,
            else_block,
            ..
        } => contains_break(then_block) || contains_break(else_block),
        StmtKind::LetPattern {
            else_block: Some(block),
            ..
        } => contains_break(block),
        StmtKind::Block(b) | StmtKind::Unsafe(b) => contains_break(b),
        StmtKind::Match { arms, .. } => arms.iter().any(|a| contains_break(&a.body)),
        _ => false,
    })
}
fn validate_constant(expression: &Expr, bits: u32) -> Check<()> {
    match &expression.kind {
        ExprKind::Array(_, items) => {
            for item in items {
                validate_constant(item, bits)?;
            }
        }
        ExprKind::Struct(_, fields) => {
            for (_, field) in fields {
                validate_constant(field, bits)?;
            }
        }
        ExprKind::String(..) => (),
        ExprKind::Constant(e, _) | ExprKind::Repeat(e, _) => validate_constant(e, bits)?,
        _ => {
            crate::consteval::eval(expression, bits)
                .map_err(|e| Diagnostic::new(expression.span, e))?;
        }
    }
    Ok(())
}
fn constant_expression(expression: &Expr) -> bool {
    match &expression.kind {
        ExprKind::Int(..) | ExprKind::Float(..) | ExprKind::Bool(_) | ExprKind::String(..) => true,
        ExprKind::Unary(UnaryOp::Neg | UnaryOp::Not | UnaryOp::BitNot, e)
        | ExprKind::Cast(e, _)
        | ExprKind::Constant(e, _)
        | ExprKind::Repeat(e, _) => constant_expression(e),
        ExprKind::Binary(_, a, b) => constant_expression(a) && constant_expression(b),
        ExprKind::Array(_, elements) => elements.iter().all(constant_expression),
        ExprKind::Struct(_, fields) => fields.iter().all(|(_, e)| constant_expression(e)),
        _ => false,
    }
}

/// Explicit type arguments specialize generic declarations before type checking.
/// Every emitted specialization goes through the same ordinary checker.
fn instantiate(program: &mut Program) -> Check<()> {
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
    };
    program.structs.retain(|s| s.generics.is_empty());
    program.enums.retain(|e| e.generics.is_empty());
    program.functions.retain(|f| f.generics.is_empty());
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
            | Type::Option(t) => self.ty(t, substitutions, span)?,
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
            | Type::Option(t) => Self::concrete(t, parameters),
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
            | (Type::Option(p), Type::Option(a)) => self.unify(p, a, parameters, mapping, span)?,
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
            ExprKind::Call { name, .. } => self
                .signatures
                .get(name)
                .filter(|f| f.generics.is_empty())
                .map(|f| f.ret.clone()),
            ExprKind::Try(e) => self.guess(e).and_then(|t| {
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
                || self.known_enums.contains_key(&prefix))
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
                if let Some(template) = self.signatures.get(name).cloned() {
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
                            self.function(&mut function, &mapping)?;
                            self.functions.push(function);
                        }
                        *name = concrete;
                        type_args.clear();
                    }
                    self.ty(&mut ret, substitutions, span)?;
                    ret
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
                    self.signatures.get(&format!("{owner}.{name}")).cloned()
                } else {
                    None
                };
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
            ExprKind::Try(value) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    fn checked(body: &str) -> Check<Program> {
        let mut program = crate::parser::parse(&format!("package test\n{body}\n"))?;
        check(&mut program)?;
        Ok(program)
    }
    fn accepts(body: &str) {
        if let Err(error) = checked(body) {
            panic!(
                "{}\nsource:\n{body}",
                error.render("test.dodo", &format!("package test\n{body}\n"))
            );
        }
    }
    fn rejects(body: &str, message: &str) {
        let error = checked(body).expect_err("source should fail semantic checking");
        assert!(
            error.message.contains(message),
            "expected {message:?}, got {:?}\n{body}",
            error.message
        );
    }
    #[test]
    fn primitive_types_and_literals() {
        accepts(
            "fn add(a: u32, b: u32) -> u32 { return a + b }\nfn main() -> i32 {\n x := 1\n u8 y = 255\n i8 z = -128\n return (add(2, 3) + y as u32 + z as u32 + x as u32) as i32\n}",
        );
    }
    #[test]
    fn wrong_argument_type() {
        rejects(
            "fn f(x: u8) -> void {}\nfn main() -> void { f(true) }",
            "expected `u8`",
        );
    }
    #[test]
    fn integer_literal_overflow() {
        rejects("fn main() -> void { u8 n = 256 }", "out of range");
    }
    #[test]
    fn wrong_condition_type() {
        rejects("fn main() -> void { if 1 {} }", "expected `bool`");
    }
    #[test]
    fn return_required_on_every_path() {
        rejects(
            "fn f(x: bool) -> u8 { if x { return 1 } }",
            "without returning",
        );
    }
    #[test]
    fn definite_initialization_branches() {
        accepts("fn f(b: bool) -> u8 {\n u8 x\n if b { x = 1 } else { x = 2 }\n return x\n}");
        rejects(
            "fn f(b: bool) -> u8 {\n u8 x\n if b { x = 1 }\n return x\n}",
            "uninitialized",
        );
    }
    #[test]
    fn early_return_initialization() {
        accepts("fn f(b: bool) -> u8 {\n u8 x\n if b { return 0 } else { x = 1 }\n return x\n}");
    }
    #[test]
    fn move_invalidation_and_reinitialization() {
        rejects(
            "struct S { u8 n }\nfn take(s: S) -> void {}\nfn f() -> u8 {\n s := S{n: 1}\n take(s)\n return s.n\n}",
            "moved",
        );
        accepts(
            "struct S { u8 n }\nfn take(s: S) -> void {}\nfn f() -> u8 {\n s := S{n: 1}\n take(s)\n s = S{n: 2}\n return s.n\n}",
        );
    }
    fn diagnostic_label_source<'a>(
        body: &str,
        diagnostic: &'a Diagnostic,
        message: &str,
    ) -> (String, &'a str) {
        let source = format!("package test\n{body}\n");
        let label = diagnostic
            .labels
            .iter()
            .find(|label| label.message.contains(message))
            .unwrap_or_else(|| {
                panic!(
                    "missing label {message:?}: {}",
                    diagnostic.render("test.dodo", &source)
                )
            });
        (
            source[label.span.start..label.span.end].to_owned(),
            &label.message,
        )
    }
    #[test]
    fn borrow_diagnostic_labels_origin_conflict_and_actual_use() {
        let body = "fn consume(view: &u8) -> void {}\nfn f() -> void {\n u8 value = 1\n view := &value\n value = 2\n consume(view)\n}";
        let error = checked(body).unwrap_err();
        assert_eq!(
            diagnostic_label_source(body, &error, "shared borrow begins here").0,
            "&value"
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "cannot modify").0,
            "value"
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "borrow is used here").0,
            "view"
        );
        assert!(
            !error
                .render("test.dodo", &format!("package test\n{body}\n"))
                .contains("source byte")
        );
    }
    #[test]
    fn borrow_diagnostic_distinguishes_loop_back_edge_use() {
        let body = "fn f() -> void {\n u8 x = 1\n r := &x\n for i := 0; i < 2; i += 1 {\n n := *r\n x = n\n }\n}";
        let error = checked(body).unwrap_err();
        assert_eq!(
            diagnostic_label_source(body, &error, "later loop iteration").0,
            "r"
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "borrow begins here").0,
            "&x"
        );
    }
    #[test]
    fn borrow_diagnostic_labels_competing_implicit_argument_reborrows() {
        let body = "fn both(a: &mut u8, b: &mut u8) -> void {}\nfn f() -> void {\n u8 x = 1\n r := &mut x\n both(r, r)\n}";
        let error = checked(body).unwrap_err();
        let source = format!("package test\n{body}\n");
        let origin = error
            .labels
            .iter()
            .find(|label| label.message.contains("begins here"))
            .unwrap();
        assert_eq!(
            origin.span.start,
            source.find("both(r, r)").unwrap() + "both(".len()
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "cannot mutably borrow").0,
            "r"
        );
    }
    #[test]
    fn borrow_diagnostic_labels_escape_from_mutable_foreach() {
        let body = "fn f() -> void {\n values := [2]u8{1, 2}\n &mut u8 escaped\n for item in &mut values { escaped = item }\n *escaped = 3\n}";
        let error = checked(body).unwrap_err();
        assert!(error.message.contains("mutable foreach element borrow"));
        assert_eq!(
            diagnostic_label_source(body, &error, "mutable element borrow begins").0,
            "&mut values"
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "borrow is used here").0,
            "escaped"
        );
    }
    #[test]
    fn borrow_diagnostic_labels_scope_and_value_block_escapes() {
        let body = "fn f() -> u8 {\n &u8 view\n {\n u8 x = 1\n view = &x\n }\n return *view\n}";
        let error = checked(body).unwrap_err();
        assert_eq!(
            diagnostic_label_source(body, &error, "borrow begins here").0,
            "&x"
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "borrow is used here").0,
            "view"
        );
        assert!(
            !error
                .labels
                .iter()
                .any(|label| label.message.contains("leaves its scope here"))
        );

        let body = "fn f() -> u8 {\n view := {\n u8 x = 1\n &x\n }\n return *view\n}";
        let error = checked(body).unwrap_err();
        assert!(error.message.contains("value block cannot yield"));
        assert_eq!(
            diagnostic_label_source(body, &error, "local borrow begins here").0,
            "&x"
        );
        assert!(
            diagnostic_label_source(body, &error, "local value `x`")
                .0
                .starts_with("u8 x")
        );
    }
    #[test]
    fn borrow_diagnostic_explains_implicit_destruction_use() {
        let body = "struct View {\n &u8 r\n fn drop(self: &mut Self) -> void { n := *self.r }\n}\nfn f() -> void {\n u8 x = 1\n v := View{r: &x}\n x = 2\n}";
        let error = checked(body).unwrap_err();
        assert!(
            diagnostic_label_source(body, &error, "destroyed at scope exit")
                .0
                .starts_with("v := View")
        );
        assert!(
            !error
                .labels
                .iter()
                .any(|label| label.message == "borrow is used here")
        );
    }
    #[test]
    fn shadowed_binding_is_not_labeled_as_a_borrow_use() {
        // Existing liveness is conservative across shadowing. Its explanation
        // must not claim that the new scalar binding uses the outer borrow.
        let body = "fn f() -> void {\n u8 x = 1\n r := &x\n x = 2\n {\n r := 3\n n := r\n }\n}";
        let error = checked(body).unwrap_err();
        assert!(error.message.contains("live shared borrow"));
        assert!(
            !error
                .labels
                .iter()
                .any(|label| label.message == "borrow is used here")
        );
        assert!(
            diagnostic_label_source(body, &error, "borrow is held")
                .0
                .starts_with("r := &x")
        );
    }
    #[test]
    fn move_diagnostic_keeps_branch_origin() {
        for branch in ["if b { take(s) }", "if b {} else { take(s) }"] {
            let body = format!(
                "struct S {{ u8 n }}\nfn take(s: S) -> void {{}}\nfn f(b: bool) -> u8 {{\n s := S{{n: 1}}\n {branch}\n return s.n\n}}"
            );
            let error = checked(&body).unwrap_err();
            let source = format!("package test\n{body}\n");
            let moved = error
                .labels
                .iter()
                .find(|label| label.message.contains("is moved here"))
                .unwrap();
            assert_eq!(
                moved.span.start,
                source.find("take(s)").unwrap() + "take(".len()
            );
            assert_eq!(diagnostic_label_source(&body, &error, "cannot use").0, "s");
            assert!(
                diagnostic_label_source(&body, &error, "binding `s` is declared")
                    .0
                    .starts_with("s := S")
            );
        }
    }
    #[test]
    fn move_diagnostic_uses_latest_move_after_reinitialization() {
        let body = "struct S { u8 n }\nfn take(s: S) -> void {}\nfn f() -> u8 {\n s := S{n: 1}\n take(s)\n s = S{n: 2}\n take(s)\n return s.n\n}";
        let error = checked(body).unwrap_err();
        let source = format!("package test\n{body}\n");
        let moved = error
            .labels
            .iter()
            .find(|label| label.message.contains("is moved here"))
            .unwrap();
        assert_eq!(
            moved.span.start,
            source.rfind("take(s)").unwrap() + "take(".len()
        );
    }
    #[test]
    fn uninitialized_binding_diagnostic_does_not_invent_a_move() {
        let body = "fn f(b: bool) -> u8 {\n u8 x\n if b { x = 1 }\n return x\n}";
        let error = checked(body).unwrap_err();
        assert_eq!(
            diagnostic_label_source(body, &error, "not initialized on every path").0,
            "x"
        );
        assert!(
            !error
                .labels
                .iter()
                .any(|label| label.message.contains("moved here"))
        );
    }
    #[test]
    fn return_contract_diagnostic_labels_contract_parameter_and_return() {
        let body = "fn choose(a: &u8, b: &u8) -> &u8 from(a) { return b }";
        let error = checked(body).unwrap_err();
        assert_eq!(
            diagnostic_label_source(body, &error, "return contract allows").0,
            "from(a)"
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "borrowed source `b`").0,
            "b: &u8"
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "returned borrow comes").0,
            "b"
        );
    }
    #[test]
    fn local_return_diagnostic_labels_static_contract_and_local_borrow() {
        let body = "fn f() -> &u8 from(static) {\n u8 x = 1\n r := &x\n return r\n}";
        let error = checked(body).unwrap_err();
        assert_eq!(
            diagnostic_label_source(body, &error, "return contract allows").0,
            "from(static)"
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "local borrow begins").0,
            "&x"
        );
        assert_eq!(
            diagnostic_label_source(body, &error, "does not live long enough").0,
            "r"
        );
    }
    #[test]
    fn inferred_return_contract_is_labeled_and_available_on_error() {
        let body =
            "struct S {\n u8 n\n fn choose(self: &Self, other: &u8) -> &u8 { return other }\n}";
        let source = format!("package test\n{body}\n");
        let mut program = crate::parser::parse(&source).unwrap();
        let error = check(&mut program).unwrap_err();
        assert!(
            diagnostic_label_source(body, &error, "inferred return contract")
                .0
                .contains("&u8")
        );
        assert_eq!(
            program
                .functions
                .iter()
                .find(|function| function.name == "S.choose")
                .unwrap()
                .from,
            ["self"]
        );
    }
    #[test]
    fn shared_loan_ends_at_last_use() {
        accepts("fn f() -> u8 {\n u8 x = 1\n r := &x\n n := *r\n x = 2\n return n + x\n}");
    }
    #[test]
    fn live_shared_loan_blocks_write() {
        rejects(
            "fn f() -> u8 {\n u8 x = 1\n r := &x\n x = 2\n return *r\n}",
            "live shared borrow",
        );
    }
    #[test]
    fn live_mutable_loan_blocks_read() {
        rejects(
            "fn f() -> u8 {\n u8 x = 1\n r := &mut x\n n := x\n *r = 2\n return n\n}",
            "live mutable borrow",
        );
    }
    #[test]
    fn shared_reference_cannot_mutate() {
        rejects("fn f(x: &u8) -> void { *x = 1 }", "immutable");
    }
    #[test]
    fn mutable_reference_can_reborrow() {
        accepts(
            "fn set(x: &mut u8) -> void { *x = 2 }\nfn f() -> u8 {\n u8 x = 1\n r := &mut x\n set(r)\n set(r)\n return *r\n}",
        );
    }
    #[test]
    fn overlapping_arguments_rejected() {
        rejects(
            "fn both(a: &mut u8, b: &mut u8) -> void {}\nfn f() -> void {\n u8 x = 1\n both(&mut x, &mut x)\n}",
            "overlapping",
        );
    }
    #[test]
    fn disjoint_struct_fields_can_borrow() {
        accepts(
            "struct Pair {\n u8 a\n u8 b\n}\nfn f() -> u8 {\n p := Pair{a: 1, b: 2}\n a := &mut p.a\n b := &mut p.b\n *a = 3\n *b = 4\n return p.a + p.b\n}",
        );
    }
    #[test]
    fn indices_conservatively_overlap() {
        rejects(
            "fn f() -> u8 {\n a := [2]u8{1, 2}\n x := &mut a[0]\n y := &mut a[1]\n return *x + *y\n}",
            "live mutable borrow",
        );
    }
    #[test]
    fn returned_local_reference_rejected() {
        rejects(
            "fn f() -> &u8 from(static) {\n u8 x = 1\n return &x\n}",
            "local value",
        );
    }
    #[test]
    fn inferred_borrowed_return() {
        accepts(
            "fn identity(x: &u8) -> &u8 { return x }\nfn f() -> u8 {\n u8 x = 1\n r := identity(&x)\n return *r\n}",
        );
    }
    #[test]
    fn ambiguous_borrowed_return_requires_contract() {
        rejects(
            "fn choose(a: &u8, b: &u8) -> &u8 { return a }",
            "explicit `from",
        );
    }
    #[test]
    fn return_contract_checked() {
        rejects(
            "fn choose(a: &u8, b: &u8) -> &u8 from(a) { return b }",
            "outside the return contract",
        );
    }
    #[test]
    fn returned_aggregate_keeps_borrows() {
        rejects(
            "struct View { &u8 item }\nfn f() -> u8 {\n u8 x = 1\n v := View{item: &x}\n x = 2\n return *v.item\n}",
            "live shared borrow",
        );
    }
    #[test]
    fn inner_source_cannot_escape() {
        rejects(
            "fn f() -> u8 {\n &u8 r\n {\n u8 x = 1\n r = &x\n }\n return *r\n}",
            "outlives its source",
        );
    }
    #[test]
    fn result_must_be_handled() {
        rejects(
            "enum E { Bad }\nfn fail() -> u8!E { return err(E.Bad) }\nfn f() -> void { fail() }",
            "Result must",
        );
        rejects(
            "enum E { Bad }\nfn fail() -> u8!E { return err(E.Bad) }\nfn f() -> void { r := fail() }",
            "never handled",
        );
        rejects(
            "enum E { Bad }\nfn fail() -> u8!E { return err(E.Bad) }\nfn f() -> void { _ = fail() }",
            "cannot be discarded",
        );
    }
    #[test]
    fn result_match_and_propagation() {
        accepts(
            "enum E { Bad }\nfn fail() -> u8!E { return ok(3) }\nfn wrap() -> u8!E {\n x := fail()?\n return ok(x + 1)\n}\nfn f() -> u8 {\n match wrap() {\n ok(x) => { return x }\n err(_) => { return 0 }\n }\n}",
        );
    }
    #[test]
    fn error_propagation_requires_same_error() {
        rejects(
            "enum A { Bad }\nenum B { Bad }\nfn a() -> u8!A { return ok(1) }\nfn b() -> u8!B {\n x := a()?\n return ok(x)\n}",
            "error type differs",
        );
    }
    #[test]
    fn exhaustive_enum_match() {
        accepts(
            "enum E { A, B }\nfn f(e: E) -> u8 { match e {\n E.A => { return 1 }\n E.B => { return 2 }\n} }",
        );
        rejects(
            "enum E { A, B }\nfn f(e: E) -> u8 { match e { E.A => { return 1 } } }",
            "non-exhaustive",
        );
    }
    #[test]
    fn payload_enum_match() {
        accepts(
            "enum E { Item(u8 n), Empty }\nfn f() -> u8 {\n e := E.Item(4)\n match e {\n E.Item(n) => { return n }\n E.Empty => { return 0 }\n }\n}",
        );
    }
    #[test]
    fn unsafe_function_requires_unsafe_block_in_body() {
        rejects(
            "unsafe fn f(p: *mut u8) -> void { *p = 1 }",
            "explicit unsafe block",
        );
    }
    #[test]
    fn unsafe_calls_checked() {
        rejects(
            "unsafe fn f() -> void {}\nfn g() -> void { f() }",
            "explicit unsafe block",
        );
    }
    #[test]
    fn mmio_checked() {
        accepts(
            "import \"core/mmio\"\nfn f(address: usize) -> u32 { unsafe { return mmio.read32(address) } }",
        );
        rejects(
            "import \"core/mmio\"\nfn f(address: usize) -> u32 { return mmio.read32(address) }",
            "explicit unsafe block",
        );
    }
    #[test]
    fn drop_signature_and_direct_calls() {
        rejects(
            "struct S { fn drop(self: &Self) -> void {} }",
            "drop must have signature",
        );
        rejects(
            "struct S { fn drop(self: &mut Self) -> void {} }\nfn f() -> void {\n s := S{}\n s.drop()\n}",
            "cannot be called directly",
        );
    }
    #[test]
    fn public_api_cannot_expose_private_type() {
        rejects(
            "struct Private {}\npub fn f() -> Private { return Private{} }",
            "exposes private type",
        );
    }
    #[test]
    fn recursive_owned_layout_rejected() {
        rejects("struct Recursive { Recursive next }", "infinitely sized");
    }
    #[test]
    fn generic_function_specialized() {
        let program = checked(
            "fn identity<T>(x: T) -> T { return x }\nfn f() -> u8 { return identity<u8>(7) }",
        )
        .unwrap();
        assert!(
            program
                .functions
                .iter()
                .any(|f| f.name.starts_with("identity$"))
        );
    }
    #[test]
    fn generic_struct_specialized() {
        accepts(
            "struct Box<T> { T value }\nfn f() -> u8 {\n b := Box<u8>{value: 4}\n return b.value\n}",
        );
    }
    #[test]
    fn generic_borrows_propagate() {
        rejects(
            "struct View<T> { T value }\nfn f() -> u8 {\n u8 x = 1\n v := View<&u8>{value: &x}\n x = 2\n return *v.value\n}",
            "live shared borrow",
        );
    }
    #[test]
    fn array_borrow_coerces_to_slice() {
        accepts(
            "fn first(a: &[u8]) -> u8 { return a[0] }\nfn f() -> u8 {\n a := [2]u8{3, 4}\n return first(&a)\n}",
        );
    }
    #[test]
    fn foreach_reference_bindings() {
        accepts(
            "fn f() -> u8 {\n a := [2]u8{1, 2}\n for value in &mut a { *value += 1 }\n total := 0u8\n for i, value in a { total += *value + i as u8 }\n return total\n}",
        );
    }
    #[test]
    fn loans_are_live_across_loop_back_edges() {
        rejects(
            "fn f() -> void {\n u8 x = 1\n r := &x\n for i := 0; i < 2; i += 1 {\n n := *r\n x = n\n }\n}",
            "live shared borrow",
        );
    }
    #[test]
    fn move_in_loop_rejected() {
        rejects(
            "struct S {}\nfn take(s: S) -> void {}\nfn f() -> void {\n s := S{}\n for i := 0; i < 2; i += 1 { take(s) }\n}",
            "moved in a loop",
        );
    }
    #[test]
    fn samples_spec_example() {
        accepts(
            "pub struct Samples {\n [4]u16 values\n pub fn offset(self: &mut Self, amount: u16) -> void {\n for value in &mut self.values { *value += amount }\n }\n pub fn weighted_sum(self: &Self) -> u32 {\n total := 0u32\n for i, value in self.values {\n weight := i as u32 + 1\n total += weight * (*value as u32)\n }\n return total\n }\n pub fn view(self: &Self) -> &[u16] { return &self.values }\n}\npub fn demo() -> u32 {\n samples := Samples{values: [4]u16{1, 2, 3, 4}}\n view := samples.view()\n first := view[0]\n samples.offset(10)\n return samples.weighted_sum() + first as u32\n}",
        );
    }
    #[test]
    fn repeated_mutable_reborrow_arguments_conflict() {
        rejects(
            "fn both(a: &mut u8, b: &mut u8) -> void {}\nfn f() -> void {\n u8 x = 1\n r := &mut x\n both(r, r)\n}",
            "overlapping",
        );
    }
    #[test]
    fn nested_references_retain_transitive_loans() {
        rejects(
            "fn f() -> u8 {\n u8 x = 1\n r := &x\n rr := &r\n x = 2\n return **rr\n}",
            "live shared borrow",
        );
    }
    #[test]
    fn destructor_keeps_borrow_live() {
        rejects(
            "struct View {\n &u8 r\n fn drop(self: &mut Self) -> void { n := *self.r }\n}\nfn f() -> void {\n u8 x = 1\n v := View{r: &x}\n x = 2\n}",
            "live shared borrow",
        );
    }
    #[test]
    fn destructor_order_preserves_borrowed_sources() {
        rejects(
            "struct View {\n &u8 r\n fn drop(self: &mut Self) -> void { n := *self.r }\n}\nfn f() -> void {\n View v\n u8 x = 1\n v = View{r: &x}\n}",
            "would run after",
        );
    }
    #[test]
    fn constant_arithmetic_checked_before_codegen() {
        rejects(
            "const u8 BAD = 255 + 1\nfn main() -> void {}",
            "cannot wrap",
        );
        rejects("fn main() -> void { const u8 BAD = 1 / 0 }", "zero");
    }
    #[test]
    fn borrowed_result_match_handles_both_variants() {
        accepts(
            "enum E { Bad }\nfn value() -> u8!E { return ok(2) }\nfn f() -> u8 {\n r := value()\n match &r {\n ok(v) => { return *v }\n err(_) => { return 0 }\n }\n}",
        );
    }
    #[test]
    fn static_slice_rvalues_are_collections() {
        accepts(
            "fn bytes() -> &[u8] from(static) { return b\"hi\" }\nfn f() -> usize {\n total := bytes().len\n for c in b\"hi\" { total += *c as usize }\n return total + bytes()[0] as usize\n}",
        );
    }
    #[test]
    fn loop_initializer_infers_counter_type() {
        accepts("fn f(count: usize) -> void { for i := 0; i < count; i += 1 {} }");
    }
    #[test]
    fn raw_pointer_borrow_cannot_invent_a_checked_lifetime() {
        rejects(
            "fn f(p: *mut u8) -> &u8 from(static) { unsafe { return &*p } }",
            "lifetime primitive",
        );
        rejects(
            "fn f(p: *const &u8) -> &u8 from(static) { unsafe { return *p } }",
            "checked-borrow values",
        );
    }
    #[test]
    fn unused_generic_declarations_still_reject_duplicates() {
        rejects(
            "fn f<T>(x: T) -> T { return x }\nfn f<U>(x: U) -> U { return x }",
            "duplicate declaration",
        );
        rejects("struct S<T, T> { T value }", "duplicate generic");
    }
    #[test]
    fn empty_forever_loop_diverges() {
        accepts("fn f() -> u8 { for {} }");
    }
    #[test]
    fn immutable_runtime_bindings_preserve_reference_permissions() {
        accepts(
            "fn value() -> u32 { 3 }\nfn f() -> u32 { let limit: u32 = value(); count := 0u32; count += limit; count }",
        );
        rejects("fn f() -> void { let n = 2u32; n = 3 }", "immutable");
        rejects(
            "struct S { u8 n }\nfn f() -> void { let s = S{n: 1}; s.n = 2 }",
            "immutable",
        );
        rejects(
            "fn f() -> void { let a = [2]u8{1, 2}; a[0] = 2 }",
            "immutable",
        );
        accepts("struct S { u8 n }\nfn f() -> u8 { s := S{n: 1}; let r = &mut s; r.n = 2; r.n }");
        accepts("fn f() -> u8 { a := [2]u8{1, 2}; let s = &mut a[..]; s[0] = 3; s[0] }");
        accepts(
            "fn set(r: &mut u8) -> void { *r = 3 }\nfn f() -> u8 { n := 1u8; let r = &mut n; set(r); *r }",
        );
        rejects(
            "fn f() -> void { n := 1u8; m := 2u8; let r = &mut n; r = &mut m }",
            "immutable",
        );
    }
    #[test]
    fn concise_match_arms_must_handle_their_own_results() {
        accepts(
            "enum E { Bad }\nfn make() -> u8!E { ok(1) }\nfn use(n: u8) -> void {}\nfn f() -> void { match make() { ok(n) => use(n), err(_) => use(0) } }",
        );
        rejects(
            "enum E { Bad }\nfn make() -> u8!E { ok(1) }\nfn f() -> void { match true { true => make(), false => make() } }",
            "Result must be handled",
        );
        rejects(
            "enum E { Bad }\nfn make() -> u8!E { ok(1) }\nfn f() -> void { match true { true => { make() }, false => {} } }",
            "Result must be handled",
        );
    }
    #[test]
    fn recursive_patterns_check_nested_coverage() {
        accepts(
            "fn f(n: Option<Option<u8>>) -> u8 { match n { some(some(v)) => v, some(none) | none => 0 } }",
        );
        rejects(
            "fn f(n: Option<Option<u8>>) -> u8 { match n { some(some(v)) => v, none => 0 } }",
            "non-exhaustive",
        );
        accepts(
            "struct Pair { bool a; bool b }\nfn f(p: Pair) -> u8 { match p { Pair{a: true, b: _} => 1, Pair{a: false, b: true} => 2, Pair{a: false, b: false} => 3 } }",
        );
        rejects(
            "struct Pair { bool a; bool b }\nfn f(p: Pair) -> u8 { match p { Pair{a: true, b: true} => 1, Pair{a: false, b: false} => 2 } }",
            "non-exhaustive",
        );
    }
    #[test]
    fn pattern_ranges_and_alternatives_are_typed_and_exhaustive() {
        accepts("fn f(n: u8) -> u8 { match n { 0..=127 => 1, 128..=255 => 2 } }");
        accepts("fn f(n: i8) -> u8 { match n { -128..0 => 1, 0..=127 => 2 } }");
        rejects(
            "fn f(n: u8) -> u8 { match n { 0..255 => 1 } }",
            "non-exhaustive",
        );
        rejects(
            "fn f(n: u8) -> u8 { match n { 8..8 => 1, _ => 2 } }",
            "empty or reversed",
        );
        rejects(
            "fn f(n: u8) -> u8 { match n { 0..=256 => 1, _ => 2 } }",
            "out of range",
        );
        rejects(
            "enum E { A(u8 x), B(u32 y) }\nfn f(e: E) -> void { match e { E.A(x) | E.B(x) => {} } }",
            "same names with the same types",
        );
        rejects(
            "enum E { A(u8 x), B(u8 y) }\nfn f(e: E) -> void { match e { E.A(x) | E.B(y) => {} } }",
            "same names with the same types",
        );
    }
    #[test]
    fn guards_are_boolean_and_do_not_contribute_coverage() {
        accepts(
            "fn f(n: Option<u8>) -> u8 { match n { some(v) if v > 3 => v, some(v) => v + 1, none => 0 } }",
        );
        rejects(
            "fn f(n: Option<u8>) -> u8 { match n { some(v) if v > 3 => v, none => 0 } }",
            "non-exhaustive",
        );
        rejects(
            "fn f(n: u8) -> u8 { match n { v if v => 1, _ => 2 } }",
            "expected `bool`",
        );
        rejects(
            "struct S {}\nfn test(s: S) -> bool { true }\nfn f(s: Option<S>) -> void { match s { some(v) if test(v) => {}, _ => {} } }",
            "match guards cannot move",
        );
        rejects(
            "fn test(r: &mut u8) -> bool { *r = 2; true }\nfn f(n: &mut Option<u8>) -> void { match n { some(v) if test(v) => {}, _ => {} } }",
            "match guards cannot move",
        );
    }
    #[test]
    fn conditional_bindings_share_recursive_patterns_and_scope() {
        accepts("fn f(n: Option<Option<u8>>) -> u8 { if let some(some(v)) = n { v } else { 0 } }");
        accepts("fn f(n: Option<Option<u8>>) -> u8 { let some(some(v)) = n else { return 0 }; v }");
        accepts("struct S { u8 n }\nfn f(s: S) -> u8 { let S{n} = s; n }");
        rejects(
            "fn f(n: Option<u8>) -> void { let some(v) = n }",
            "requires an `else`",
        );
        rejects(
            "fn f(n: Option<u8>) -> void { let some(v) = n else {} }",
            "must diverge",
        );
        rejects(
            "fn f(n: Option<u8>) -> u8 { if let some(v) = n {}; v }",
            "unknown binding `v`",
        );
        rejects(
            "fn f(n: Option<u8>) -> void { if let some(v) = n { v = 1 } }",
            "immutable",
        );
    }
    #[test]
    fn conditional_and_recursive_patterns_cannot_discard_results() {
        rejects(
            "enum E { Bad }\nfn f(n: u8!E) -> void { if let ok(v) = n {} }",
            "conditional patterns cannot discard a Result",
        );
        rejects(
            "enum E { Bad }\nfn f(n: u8!E) -> void { let ok(v) = n else { return } }",
            "conditional patterns cannot discard a Result",
        );
        rejects(
            "enum E { Bad }\nfn f(n: Option<u8!E>) -> void { if let some(ok(v)) = n {} }",
            "conditional patterns cannot discard a Result",
        );
        rejects(
            "enum E { Bad }\nfn f(n: Option<u8!E>) -> void { match n { some(_) => {}, none => {} } }",
            "nested Result cannot be discarded",
        );
        rejects(
            "enum E { Bad }\nfn f(n: Option<u8!E>) -> void { match n { some(r) => {}, none => {} } }",
            "never handled",
        );
        accepts(
            "enum E { Bad }\nfn f(n: Option<u8!E>) -> void { match n { some(ok(v)) => {}, some(err(e)) => {}, none => {} } }",
        );
        accepts(
            "enum E { Bad }\nfn f(n: &Option<u8!E>) -> void { match n { some(r) => { match r { ok(v) => {}, err(e) => {} } }, none => {} } }",
        );
    }
    #[test]
    fn destructuring_preserves_borrows_moves_and_custom_drop() {
        rejects(
            "struct S { u8 n }\nfn f(s: S) -> u8 { let S{n} = s; s.n }",
            "moved",
        );
        rejects(
            "struct S { u8 n; fn drop(self: &mut Self) -> void {} }\nfn f(s: S) -> u8 { let S{n} = s; n }",
            "custom drop",
        );
        accepts(
            "struct S { u8 n; fn drop(self: &mut Self) -> void {} }\nfn f(s: S) -> u8 { let S{n} = &s; *n }",
        );
        accepts("struct S { u8 n }\nfn f(s: &mut S) -> u8 { let S{n} = s; *n = 4; *n }");
        rejects(
            "fn f() -> &u8 from(static) { n := some(2u8); let some(v) = &n else { for {} }; v }",
            "cannot return a borrow",
        );
    }
    #[test]
    fn generic_patterns_supply_payload_types_to_instantiation() {
        accepts(
            "struct Box<T> { T value }\nfn identity<T>(value: T) -> T { value }\nfn read<T>(box: Box<T>) -> T { let Box{value} = box; identity(value) }\nfn f() -> u8 { read(Box<u8>{value: 3}) }",
        );
        accepts(
            "enum Choice<T> { Value(T value), Empty }\nfn read<T>(choice: Choice<T>, default: T) -> T { match choice { Choice.Value(value) => value, Choice.Empty => default } }\nfn f(c: Choice<u8>) -> u8 { read(c, 0u8) }",
        );
    }
    #[test]
    fn borrowed_struct_patterns_preserve_disjoint_field_loans() {
        accepts(
            "struct Pair { i32 left; i32 right }\nfn f() -> i32 { pair := Pair{left: 0, right: 0}; if let Pair{left, right} = &mut pair { *left = 1; *right = 2 }; pair.left + pair.right }",
        );
        accepts(
            "struct Pair { i32 left; i32 right }\nfn f() -> i32 { pair := Pair{left: 0, right: 0}; let Pair{left, right} = &mut pair; *left = 1; *right = 2; *left + *right }",
        );
        accepts(
            "struct Pair { i32 left; i32 right }\nfn f() -> i32 { pair := Pair{left: 0, right: 0}; match &mut pair { Pair{left, right} => { *left = 1; *right = 2; *left + *right } } }",
        );
        rejects(
            "struct Pair { i32 left; i32 right }\nfn f() -> void { pair := Pair{left: 0, right: 0}; let Pair{left, right} = &mut pair; pair.left = 3; *left = 1; *right = 2 }",
            "borrow",
        );
    }
    #[test]
    fn irrefutable_borrowed_conditionals_handle_result_obligations() {
        accepts(
            "fn consume(n: i32) {}\nfn f(result: i32!i32) { if let ok(value) | err(value) = &result { consume(*value) } }",
        );
        accepts("fn f(result: i32!i32) -> i32 { let ok(value) | err(value) = &result; *value }");
        accepts("enum E { A, B }\nfn f(e: E) -> u8 { match e { A => 1, B => 2 } }");
        accepts(
            "struct Box<T> { T value }\nfn f<T>(input: Box<T>) -> T { let Box<T>{value} = input; value }\nfn main() -> u8 { f(Box<u8>{value: 3}) }",
        );
        accepts(
            "enum Choice<T> { Value(T value), Empty }\nfn f(c: Choice<u8>) -> u8 { match c { Choice.Value::<u8>(n) => n, Choice<u8>.Empty => 0 } }\nfn main() -> u8 { f(Choice.Value::<u8>(3)) }",
        );
    }
    #[test]
    fn excessive_pattern_expansion_reports_a_diagnostic() {
        let fields = (0..13)
            .map(|n| format!("bool f{n}"))
            .collect::<Vec<_>>()
            .join("; ");
        let patterns = (0..13)
            .map(|n| format!("f{n}: true | false"))
            .collect::<Vec<_>>()
            .join(", ");
        rejects(
            &format!(
                "struct Many {{ {fields} }}\nfn f(value: Many) {{ match value {{ Many{{ {patterns} }} => {{}} }} }}"
            ),
            "more than 4096 alternatives",
        );
    }

    #[test]
    fn conditional_results_allow_only_result_free_unmatched_payloads() {
        accepts(
            "fn use(r: i32!i32) { match r { ok(_) => {}, err(_) => {} } }\nfn f(input: Option<i32!i32>) { if let some(r) = input { use(r) } }",
        );
        accepts(
            "fn f(input: Option<i32!i32>) -> i32 { let some(r) = input else { return 0 }; match r { ok(v) | err(v) => v } }",
        );
        accepts(
            "struct Box { Option<i32!i32> value }\nfn f(input: Box) -> i32 { if let Box{value: some(r)} = input { match r { ok(v) | err(v) => v } } else { 0 } }",
        );
        accepts(
            "enum E { Item(i32!i32 value), Empty }\nfn f(input: E) -> i32 { let E.Item(r) = input else { return 0 }; match r { ok(v) | err(v) => v } }",
        );
        accepts(
            "fn f(input: Option<i32!i32>) -> i32 { if let some(r) = &input { match r { ok(v) | err(v) => *v } } else { 0 } }",
        );
        rejects(
            "fn f(input: Option<i32!i32>) { if let some(r) = input {} }",
            "never handled",
        );
        rejects(
            "enum E { Item(i32!i32 value), Empty }\nfn f(input: E) { if let E.Empty = input {} }",
            "conditional patterns cannot discard a Result",
        );
        rejects(
            "struct Box { Option<i32!i32> value }\nfn f(input: Box) { if let Box{value: some(ok(v))} = input {} }",
            "conditional patterns cannot discard a Result",
        );
    }
}
