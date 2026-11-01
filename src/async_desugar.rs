//! Expand `async fn` declarations into ordinary declarations.
//!
//! `async fn blink(led: &mut L, period_ms: u32) -> u32 { body }` becomes
//!
//! - a struct `BlinkFuture<L>` whose fields are the parameters,
//! - the constructor `blink(led, period_ms) -> BlinkFuture<L>`, which only
//!   stores its arguments, so calling an async fn runs none of its body,
//! - the methods `poll(&mut self, cx: &mut executor.Context) -> executor.Poll`,
//!   `output(&mut self) -> u32` (unless the result is void), and `drop`,
//!   whose bodies the backend generates, and
//! - `BlinkFuture.body(led, period_ms) -> u32`, the original body. The checker
//!   and the ownership analysis see it as a plain function in which `await`
//!   is allowed; the backend compiles it into `poll` as a state machine whose
//!   locals live in the future.
//!
//! A future is therefore an executor task: `poll` has the `executor.Task`
//! signature, so async and hand-written tasks run side by side.
//!
//! `await` must begin its statement's evaluation (see `validate_block`), so no
//! value computed earlier in the same statement has to survive a suspension.
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use std::collections::HashSet;

const EXECUTOR: &str = "std/embedded/executor";

pub(crate) fn expand(program: &mut Program, identifiers: &HashSet<&str>) -> Result<(), Diagnostic> {
    for interface in &program.interfaces {
        if let Some(method) = interface.methods.iter().find(|m| m.async_part.is_some()) {
            return Err(Diagnostic::new(
                method.span,
                "interface methods cannot be `async`; declare `poll`-style methods, or return a future type",
            ));
        }
    }
    // Await outside an async fn is reported by the checker, which knows the
    // enclosing function; here only the async bodies' structure is checked.
    if !program
        .functions
        .iter()
        .any(|f| f.async_part == Some(AsyncPart::Body))
    {
        return Ok(());
    }
    let alias = if program.imports.iter().any(|import| import == EXECUTOR) {
        program
            .import_aliases
            .iter()
            .find(|(import, _)| import == EXECUTOR)
            .map_or("executor", |(_, alias)| alias.as_str())
            .to_owned()
    } else {
        let mut alias = "__dodo_async_executor".to_owned();
        while identifiers.contains(alias.as_str()) {
            alias.push('_');
        }
        program.imports.push(EXECUTOR.to_owned());
        program
            .import_aliases
            .push((EXECUTOR.to_owned(), alias.clone()));
        program
            .implicit_import_aliases
            .push((EXECUTOR.to_owned(), alias.clone()));
        alias
    };
    let declared: HashSet<String> = program
        .structs
        .iter()
        .map(|s| s.name.clone())
        .chain(program.enums.iter().map(|e| e.name.clone()))
        .chain(program.interfaces.iter().map(|i| i.name.clone()))
        .chain(program.aliases.iter().map(|a| a.name.clone()))
        .collect();
    let mut generated = Vec::new();
    let mut structs = Vec::new();
    for function in &mut program.functions {
        if function.async_part != Some(AsyncPart::Body) {
            continue;
        }
        validate_signature(function)?;
        if let Some(body) = &function.body {
            validate_block(body)?;
        }
        let future = future_name(&function.name);
        if declared.contains(&future) || structs.iter().any(|s: &Struct| s.name == future) {
            return Err(Diagnostic::new(
                function.span,
                format!(
                    "async fn `{}` names its future type `{future}`, which is already declared",
                    function.name
                ),
            )
            .note("rename the async fn or the other declaration"));
        }
        let (structure, parts) = expand_function(function, &future, &alias);
        structs.push(structure);
        generated.extend(parts);
    }
    program.structs.extend(structs);
    // The original declarations become the constructors, in place, so source
    // order and editor positions stay as written.
    for function in &mut program.functions {
        if function.async_part == Some(AsyncPart::Body) {
            let index = generated
                .iter()
                .position(|f: &Function| {
                    f.async_part == Some(AsyncPart::Constructor) && f.name == function.name
                })
                .unwrap();
            *function = generated.remove(index);
        }
    }
    program.functions.extend(generated);
    Ok(())
}

/// `blink` → `BlinkFuture`, and the method `Led.blink` → `LedBlinkFuture`.
pub(crate) fn future_name(function: &str) -> String {
    let mut name = String::new();
    for part in function.split('.') {
        for word in part.split('_').filter(|w| !w.is_empty()) {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                name.extend(first.to_uppercase());
                name.push_str(chars.as_str());
            }
        }
    }
    name.push_str("Future");
    name
}

fn validate_signature(function: &Function) -> Result<(), Diagnostic> {
    let reason = if function.extern_ {
        Some("an async fn cannot be extern")
    } else if function.unsafe_ {
        Some("an async fn cannot be unsafe; use unsafe blocks in its body")
    } else if function.test {
        Some("a test cannot be async; run the future with an executor in the test")
    } else if function.interrupt.is_some() {
        Some("an interrupt handler cannot be async; raise a signal for a task instead")
    } else if function.printing.is_some() || function.pio || function.wgsl {
        Some("@compiler declarations cannot be async")
    } else if !function.placement.is_empty() {
        Some("@section and @align do not apply to async fns")
    } else if function.inlining != crate::ast::Inlining::Auto {
        Some("@inline and @cold do not apply to async fns")
    } else if !function.stores.is_empty() || !function.requires_plain.is_empty() {
        Some("an async fn cannot declare stores(...) or requires_plain(...)")
    } else if function.body.is_none() {
        Some("an async fn needs a body")
    } else {
        None
    };
    match reason {
        Some(reason) => Err(Diagnostic::new(function.span, reason)),
        None => Ok(()),
    }
}

fn expand_function(function: &Function, future: &str, alias: &str) -> (Struct, Vec<Function>) {
    let span = function.span;
    let zero = Span {
        start: span.start,
        end: span.start,
    };
    let future_ty = if function.generics.is_empty() {
        Type::Named(future.to_owned())
    } else {
        Type::Generic(
            future.to_owned(),
            function.generics.iter().cloned().map(Type::Named).collect(),
        )
    };
    let field_name = |param: &Param| {
        if param.name == "self" {
            "self_".to_owned()
        } else {
            param.name.clone()
        }
    };
    let structure = Struct {
        name: future.to_owned(),
        public: function.public,
        generics: function.generics.clone(),
        bounds: function.bounds.clone(),
        implements: vec![],
        fields: function
            .params
            .iter()
            .map(|param| Field {
                name: field_name(param),
                ty: param.ty.clone(),
                public: false,
                span: param.span,
            })
            .collect(),
        span,
        repr_c: false,
        unsafe_send: false,
        unsafe_sync: false,
        interior_mutable: false,
        async_future: true,
    };
    let receiver = Param {
        name: "self".to_owned(),
        ty: Type::Ref(true, Box::new(future_ty.clone())),
        span: zero,
    };
    let method = |name: &str, params: Vec<Param>, ret: Type, part: AsyncPart| Function {
        name: format!("{future}.{name}"),
        imported: false,
        test: false,
        ignore: None,
        public: part != AsyncPart::Drop,
        unsafe_: false,
        extern_: false,
        printing: None,
        pio: false,
        wgsl: false,
        generics: function.generics.clone(),
        bounds: function.bounds.clone(),
        generic_instance: false,
        params,
        ret,
        ret_span: zero,
        from: vec![],
        stores: vec![],
        requires_plain: vec![],
        from_span: None,
        body: None,
        span: zero,
        environment: None,
        interrupt: None,
        link_name: None,
        placement: Placement::default(),
        inlining: crate::ast::Inlining::Auto,
        async_part: Some(part),
    };
    let mut parts = vec![method(
        "poll",
        vec![
            receiver.clone(),
            Param {
                name: "cx".to_owned(),
                ty: Type::Ref(true, Box::new(Type::Named(format!("{alias}.Context")))),
                span: zero,
            },
        ],
        Type::Named(format!("{alias}.Poll")),
        AsyncPart::Poll,
    )];
    if function.ret != Type::Void {
        parts.push(method(
            "output",
            vec![receiver.clone()],
            function.ret.clone(),
            AsyncPart::Output,
        ));
    }
    parts.push(method("drop", vec![receiver], Type::Void, AsyncPart::Drop));
    let mut body = method(
        "body",
        function.params.clone(),
        function.ret.clone(),
        AsyncPart::Body,
    );
    body.public = false;
    body.span = function.span;
    body.ret_span = function.ret_span;
    body.from = function.from.clone();
    body.from_span = function.from_span;
    body.body = function.body.clone();
    parts.push(body);
    let literal_name = if function.generics.is_empty() {
        future.to_owned()
    } else {
        future_ty.to_string()
    };
    let literal = Expr::new(
        ExprKind::Struct(
            literal_name,
            function
                .params
                .iter()
                .map(|param| {
                    (
                        field_name(param),
                        Expr::new(ExprKind::Name(param.name.clone()), zero),
                    )
                })
                .collect(),
        ),
        zero,
    );
    let mut constructor = function.clone();
    constructor.ret = future_ty;
    constructor.from = vec![];
    constructor.from_span = None;
    constructor.body = Some(vec![Stmt {
        kind: StmtKind::Return(Some(literal)),
        span: zero,
    }]);
    constructor.async_part = Some(AsyncPart::Constructor);
    parts.push(constructor);
    (structure, parts)
}

const PLACEMENT: &str = "`await` must begin its statement: write it as a statement, an initializer, an assigned or returned value, or an `if`, `for`, or `match` condition, optionally followed by `?`, `!`, or `as`";

fn misplaced(span: Span) -> Diagnostic {
    Diagnostic::new(span, PLACEMENT)
        .note("bind the awaited value to a local first: `value := await future`")
}

/// Every `await` in an async body must be the first thing its statement
/// evaluates, so nothing computed before it in the statement must survive
/// a suspension.
fn validate_block(block: &Block) -> Result<(), Diagnostic> {
    block.iter().try_for_each(validate_stmt)
}

fn validate_stmt(statement: &Stmt) -> Result<(), Diagnostic> {
    match &statement.kind {
        StmtKind::Let { value, .. } => value.as_ref().map_or(Ok(()), leading),
        StmtKind::Assign { target, value, .. } => {
            no_await(target)?;
            if contains_await(value) && !pure_place(target) {
                return Err(Diagnostic::new(
                    target.span,
                    "a value assigned with `await` must go to a local or one of its fields",
                )
                .note("await into a local first, then assign it"));
            }
            leading(value)
        }
        StmtKind::Expr(value) | StmtKind::Yield(value) => leading(value),
        StmtKind::Return(value) => value.as_ref().map_or(Ok(()), leading),
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            leading(condition)?;
            validate_block(then_block)?;
            validate_block(else_block)
        }
        StmtKind::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            leading(value)?;
            validate_block(then_block)?;
            validate_block(else_block)
        }
        StmtKind::LetPattern {
            value, else_block, ..
        } => {
            leading(value)?;
            else_block.as_ref().map_or(Ok(()), validate_block)
        }
        StmtKind::For {
            init,
            condition,
            step,
            body,
        } => {
            if let Some(init) = init {
                validate_stmt(init)?;
            }
            if let Some(condition) = condition {
                leading(condition)?;
            }
            if let Some(step) = step {
                validate_stmt(step)?;
            }
            validate_block(body)
        }
        StmtKind::ForEach { iterable, body, .. } => {
            no_await(iterable)?;
            validate_block(body)
        }
        StmtKind::Match { value, arms } => {
            leading(value)?;
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    no_await(guard)?;
                }
                validate_block(&arm.body)?;
            }
            Ok(())
        }
        StmtKind::Break | StmtKind::Continue => Ok(()),
        StmtKind::Block(block) | StmtKind::Unsafe(block) => validate_block(block),
    }
}

/// An expression whose evaluation may begin with `await`.
fn leading(expression: &Expr) -> Result<(), Diagnostic> {
    match &expression.kind {
        ExprKind::Await { value, .. } => no_await(value),
        ExprKind::Try(inner) | ExprKind::Unwrap(inner) | ExprKind::Cast(inner, _) => leading(inner),
        // `if` and `match` expressions are value blocks of statements.
        ExprKind::ValueBlock(block) => validate_block(block),
        _ => no_await(expression),
    }
}

/// A place whose address needs no evaluation that could be lost across a
/// suspension: a local, a dereferenced local, or their fields.
fn pure_place(expression: &Expr) -> bool {
    match &expression.kind {
        ExprKind::Name(_) => true,
        ExprKind::Field(base, _) => pure_place(base),
        ExprKind::Unary(UnaryOp::Deref, base) => matches!(base.kind, ExprKind::Name(_)),
        _ => false,
    }
}

fn no_await(expression: &Expr) -> Result<(), Diagnostic> {
    match find_await(expression) {
        Some(span) => Err(misplaced(span)),
        None => Ok(()),
    }
}

pub(crate) fn contains_await(expression: &Expr) -> bool {
    find_await(expression).is_some()
}

fn find_await(expression: &Expr) -> Option<Span> {
    match &expression.kind {
        ExprKind::Await { .. } => Some(expression.span),
        ExprKind::Int(..)
        | ExprKind::Float(..)
        | ExprKind::Bool(_)
        | ExprKind::String(..)
        | ExprKind::Name(_) => None,
        ExprKind::Array(_, items) => items.iter().find_map(find_await),
        ExprKind::Repeat(inner, _)
        | ExprKind::Constant(inner, _)
        | ExprKind::Unary(_, inner)
        | ExprKind::Field(inner, _)
        | ExprKind::Cast(inner, _)
        | ExprKind::Try(inner)
        | ExprKind::Unwrap(inner) => find_await(inner),
        ExprKind::ValueBlock(block) => find_await_block(block),
        ExprKind::Range(a, b) | ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => {
            find_await(a).or_else(|| find_await(b))
        }
        ExprKind::Slice {
            base, start, end, ..
        } => find_await(base)
            .or_else(|| start.as_deref().and_then(find_await))
            .or_else(|| end.as_deref().and_then(find_await)),
        ExprKind::Struct(_, fields) => fields.iter().find_map(|(_, value)| find_await(value)),
        ExprKind::Call { args, .. } => args.iter().find_map(find_await),
        ExprKind::MethodCall { receiver, args, .. } => {
            find_await(receiver).or_else(|| args.iter().find_map(find_await))
        }
        ExprKind::Invoke { callee, args } => {
            find_await(callee).or_else(|| args.iter().find_map(find_await))
        }
        ExprKind::Closure(closure) => find_await_block(&closure.body),
        ExprKind::Asm(asm) => asm.inputs().find_map(find_await),
    }
}

fn find_await_block(block: &Block) -> Option<Span> {
    block.iter().find_map(find_await_stmt)
}

fn find_await_stmt(statement: &Stmt) -> Option<Span> {
    let expressions: Vec<&Expr> = match &statement.kind {
        StmtKind::Let { value, .. } => value.iter().collect(),
        StmtKind::Assign { target, value, .. } => vec![target, value],
        StmtKind::Expr(value) | StmtKind::Yield(value) => vec![value],
        StmtKind::Return(value) => value.iter().collect(),
        StmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            return find_await(condition)
                .or_else(|| find_await_block(then_block))
                .or_else(|| find_await_block(else_block));
        }
        StmtKind::IfLet {
            value,
            then_block,
            else_block,
            ..
        } => {
            return find_await(value)
                .or_else(|| find_await_block(then_block))
                .or_else(|| find_await_block(else_block));
        }
        StmtKind::LetPattern {
            value, else_block, ..
        } => {
            return find_await(value).or_else(|| else_block.as_ref().and_then(find_await_block));
        }
        StmtKind::For {
            init,
            condition,
            step,
            body,
        } => {
            return init
                .as_deref()
                .and_then(find_await_stmt)
                .or_else(|| condition.as_ref().and_then(find_await))
                .or_else(|| step.as_deref().and_then(find_await_stmt))
                .or_else(|| find_await_block(body));
        }
        StmtKind::ForEach { iterable, body, .. } => {
            return find_await(iterable).or_else(|| find_await_block(body));
        }
        StmtKind::Match { value, arms } => {
            return find_await(value).or_else(|| {
                arms.iter().find_map(|arm| {
                    arm.guard
                        .as_ref()
                        .and_then(find_await)
                        .or_else(|| find_await_block(&arm.body))
                })
            });
        }
        StmtKind::Break | StmtKind::Continue => vec![],
        StmtKind::Block(block) | StmtKind::Unsafe(block) => return find_await_block(block),
    };
    expressions.into_iter().find_map(find_await)
}

#[cfg(test)]
mod tests {
    use super::future_name;

    #[test]
    fn future_names_are_camel_case() {
        assert_eq!(future_name("blink"), "BlinkFuture");
        assert_eq!(future_name("read_line"), "ReadLineFuture");
        assert_eq!(future_name("Led.blink_twice"), "LedBlinkTwiceFuture");
    }
}
