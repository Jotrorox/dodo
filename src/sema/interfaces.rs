//! Interfaces: declared method sets, generic bounds, and `dyn` values.
//!
//! Conformance is structural. A type implements an interface when it has a
//! method of the same name and signature for every method the interface
//! declares or inherits, with `Self` replaced by that type. Bounds are checked
//! where a generic declaration is specialized, so a wrong type argument is
//! reported at the use instead of somewhere inside the specialized body.
use super::*;

/// The `Self` placeholder in interface method signatures.
pub(crate) const SELF: &str = "Self";

#[derive(Clone)]
pub(super) struct InterfaceInfo {
    /// Own and inherited methods, parents first, in vtable order.
    pub(super) methods: Vec<Function>,
    /// Why the interface cannot be used as `dyn`, with the offending method.
    pub(super) not_object_safe: Option<(Span, String)>,
}

/// Why a type does not implement an interface: labelled spans and notes.
type Mismatch = (Vec<(Span, String)>, Vec<String>);

/// One bound to check after specialization: `ty` must implement `interface`.
#[derive(Clone, Debug)]
pub(super) struct Obligation {
    pub(super) ty: Type,
    pub(super) interface: String,
    /// Where the generic declaration was used with `ty`.
    pub(super) span: Span,
    /// The bound in the generic declaration.
    pub(super) bound: Span,
    pub(super) param: String,
    /// The generic declaration; its package decides which methods it can see.
    pub(super) owner: String,
    pub(super) owner_is_type: bool,
}

/// Own and inherited methods of every interface, parents first. Codegen uses
/// the same order for vtable slots.
pub(crate) fn flattened_interfaces(program: &Program) -> Check<HashMap<String, Vec<Function>>> {
    let declarations: HashMap<_, _> = program
        .interfaces
        .iter()
        .map(|interface| (interface.name.as_str(), interface))
        .collect();
    let mut result = HashMap::new();
    for interface in &program.interfaces {
        flatten(interface, &declarations, &mut result, &mut Vec::new())?;
    }
    Ok(result)
}

fn flatten(
    interface: &Interface,
    declarations: &HashMap<&str, &Interface>,
    result: &mut HashMap<String, Vec<Function>>,
    active: &mut Vec<String>,
) -> Check<Vec<Function>> {
    if let Some(methods) = result.get(&interface.name) {
        return Ok(methods.clone());
    }
    if active.contains(&interface.name) {
        return Err(Diagnostic::new(
            interface.span,
            format!(
                "interface `{}` includes itself through its parents",
                interface.name
            ),
        ));
    }
    active.push(interface.name.clone());
    let mut methods: Vec<Function> = Vec::new();
    let add = |method: Function, methods: &mut Vec<Function>| -> Check<()> {
        if let Some(existing) = methods.iter().find(|m| m.name == method.name) {
            if !same_signature(existing, &method) {
                return Err(Diagnostic::new(
                    interface.span,
                    format!(
                        "interface `{}` inherits two different `{}` methods",
                        interface.name, method.name
                    ),
                )
                .label(existing.span, signature_text(existing))
                .label(method.span, signature_text(&method)));
            }
            return Ok(());
        }
        methods.push(method);
        Ok(())
    };
    for parent in &interface.parents {
        let Some(declaration) = declarations.get(parent.as_str()) else {
            return Err(Diagnostic::new(
                interface.span,
                format!("unknown interface `{parent}`"),
            ));
        };
        for method in flatten(declaration, declarations, result, active)? {
            add(method, &mut methods)?;
        }
    }
    for method in &interface.methods {
        add(method.clone(), &mut methods)?;
    }
    active.pop();
    result.insert(interface.name.clone(), methods.clone());
    Ok(methods)
}

fn same_signature(left: &Function, right: &Function) -> bool {
    left.params.len() == right.params.len()
        && left
            .params
            .iter()
            .zip(&right.params)
            .all(|(a, b)| a.ty == b.ty && (a.name == "self") == (b.name == "self"))
        && left.ret == right.ret
        && left.unsafe_ == right.unsafe_
}

/// `fn name(&mut self, value: u8) -> R`, for diagnostics.
pub(crate) fn signature_text(function: &Function) -> String {
    let name = function.name.rsplit('.').next().unwrap_or(&function.name);
    let params: Vec<String> = function
        .params
        .iter()
        .map(|param| match (&param.ty, param.name.as_str()) {
            (Type::Ref(mutable, _), "self") => {
                format!("&{}self", if *mutable { "mut " } else { "" })
            }
            _ => format!("{}: {}", param.name, param.ty),
        })
        .collect();
    let mut text = format!(
        "{}fn {name}({})",
        if function.unsafe_ { "unsafe " } else { "" },
        params.join(", ")
    );
    if function.ret != Type::Void {
        text.push_str(&format!(" -> {}", function.ret));
    }
    if !function.from.is_empty() {
        text.push_str(&format!(" from({})", function.from.join(", ")));
    }
    text
}

pub(super) fn substitute_self(ty: &Type, replacement: &Type) -> Type {
    match ty {
        Type::Named(name) if name == SELF => replacement.clone(),
        Type::Array(n, t) => Type::Array(*n, Box::new(substitute_self(t, replacement))),
        Type::Ref(m, t) => Type::Ref(*m, Box::new(substitute_self(t, replacement))),
        Type::Slice(m, t) => Type::Slice(*m, Box::new(substitute_self(t, replacement))),
        Type::Raw(m, t) => Type::Raw(*m, Box::new(substitute_self(t, replacement))),
        Type::Option(t) => Type::Option(Box::new(substitute_self(t, replacement))),
        Type::MaybeUninit(t) => Type::MaybeUninit(Box::new(substitute_self(t, replacement))),
        Type::Result(t, e) => Type::Result(
            Box::new(substitute_self(t, replacement)),
            Box::new(substitute_self(e, replacement)),
        ),
        _ => ty.clone(),
    }
}

fn mentions_self(ty: &Type) -> bool {
    match ty {
        Type::Named(name) => name == SELF,
        Type::Array(_, t)
        | Type::Ref(_, t)
        | Type::Slice(_, t)
        | Type::Raw(_, t)
        | Type::Option(t)
        | Type::MaybeUninit(t) => mentions_self(t),
        Type::Result(t, e) => mentions_self(t) || mentions_self(e),
        _ => false,
    }
}

/// A method is callable through `dyn` when its receiver is `&self` or
/// `&mut self` and `Self` appears nowhere else, so every implementation has
/// the same machine signature apart from the receiver's address.
fn object_safety(method: &Function) -> Option<String> {
    let name = &method.name;
    let receiver = method.params.first().filter(|p| p.name == "self");
    match receiver.map(|p| &p.ty) {
        Some(Type::Ref(_, inner)) if **inner == Type::Named(SELF.into()) => {}
        Some(_) => {
            return Some(format!(
                "method `{name}` takes `self` by value; `dyn` methods need `&self` or `&mut self`"
            ));
        }
        None => {
            return Some(format!(
                "method `{name}` has no `self` receiver, so it cannot be called on a value"
            ));
        }
    }
    if method.params[1..].iter().any(|p| mentions_self(&p.ty)) || mentions_self(&method.ret) {
        return Some(format!(
            "method `{name}` uses `Self` outside its receiver, which differs between implementations"
        ));
    }
    None
}

impl Context {
    /// Validate interface declarations and add a callable signature
    /// `Interface.method` for every method of an object-safe interface.
    pub(super) fn interface_table(
        program: &Program,
    ) -> Check<(HashMap<String, InterfaceInfo>, Vec<Function>)> {
        let flattened = flattened_interfaces(program)?;
        let mut table = HashMap::new();
        let mut dispatch = Vec::new();
        for interface in &program.interfaces {
            for method in &interface.methods {
                if method.name == "drop" {
                    return Err(Diagnostic::new(
                        method.span,
                        "interfaces cannot declare `drop`; destruction belongs to the concrete type",
                    ));
                }
            }
            let methods = flattened[&interface.name].clone();
            let not_object_safe = methods
                .iter()
                .find_map(|method| object_safety(method).map(|reason| (method.span, reason)));
            if not_object_safe.is_none() {
                let object = Type::Dyn(interface.name.clone());
                for method in &methods {
                    let mut function = method.clone();
                    function.name = format!("{}.{}", interface.name, method.name);
                    function.public = interface.public;
                    for param in &mut function.params {
                        param.ty = substitute_self(&param.ty, &object);
                    }
                    function.ret = substitute_self(&function.ret, &object);
                    dispatch.push(function);
                }
            }
            table.insert(
                interface.name.clone(),
                InterfaceInfo {
                    methods,
                    not_object_safe,
                },
            );
        }
        Ok((table, dispatch))
    }

    /// `&dyn from` converts to `&dyn to` when `to`'s methods begin `from`'s
    /// method table, as for an interface's first parent and its ancestors.
    pub(super) fn dyn_upcast(&self, from: &str, to: &str) -> bool {
        let (Some(from), Some(to)) = (self.interfaces.get(from), self.interfaces.get(to)) else {
            return false;
        };
        to.methods.len() <= from.methods.len()
            && to
                .methods
                .iter()
                .zip(&from.methods)
                .all(|(a, b)| a.name == b.name && same_signature(a, b))
    }

    pub(super) fn validate_dyn(&self, name: &str, span: Span) -> Check<()> {
        let Some(interface) = self.interfaces.get(name) else {
            let mut diagnostic = Diagnostic::new(span, format!("unknown interface `{name}`"));
            if self.structs.contains_key(name) || self.enums.contains_key(name) {
                diagnostic = diagnostic.note(format!(
                    "`{name}` is a concrete type; `dyn` requires an interface"
                ));
            }
            return Err(diagnostic);
        };
        if let Some((method, reason)) = &interface.not_object_safe {
            return Err(Diagnostic::new(
                span,
                format!("interface `{name}` cannot be used as `dyn {name}`"),
            )
            .label(*method, reason.clone())
            .note("such interfaces can still constrain generic parameters"));
        }
        Ok(())
    }

    /// Check that `ty` implements `interface`, as seen from package `viewer`.
    /// The error carries labels and notes; callers supply the message's span.
    pub(super) fn implementation(
        &self,
        ty: &Type,
        interface: &str,
        viewer: &str,
    ) -> Result<(), Mismatch> {
        let Some(info) = self.interfaces.get(interface) else {
            return Err((vec![], vec![format!("unknown interface `{interface}`")]));
        };
        if let Type::Dyn(name) = ty
            && name == interface
        {
            return Ok(());
        }
        let owner = match ty {
            Type::Named(name) | Type::Dyn(name) => name.clone(),
            _ if info.methods.is_empty() => return Ok(()),
            _ => {
                return Err((
                    vec![],
                    vec![format!(
                        "`{ty}` has no methods; implement interfaces with a struct or enum"
                    )],
                ));
            }
        };
        let mut labels = Vec::new();
        let mut notes = Vec::new();
        for required in &info.methods {
            let mut expected = required.clone();
            for param in &mut expected.params {
                param.ty = substitute_self(&param.ty, ty);
            }
            expected.ret = substitute_self(&expected.ret, ty);
            let qualified = format!("{owner}.{}", required.name);
            let Some(found) = self.functions.get(&qualified) else {
                notes.push(format!(
                    "missing method `{}`",
                    signature_text(&Function {
                        from: vec![],
                        ..expected.clone()
                    })
                ));
                labels.push((
                    signature_span(required),
                    "required by this interface method".into(),
                ));
                continue;
            };
            let found_function = Function {
                name: qualified.clone(),
                params: found.params.clone(),
                ret: found.ret.clone(),
                unsafe_: found.unsafe_,
                from: found.from.clone(),
                ..expected.clone()
            };
            let receiver_matches = (expected.params.first().map(|p| p.name.as_str())
                == Some("self"))
                == (found.params.first().map(|p| p.name.as_str()) == Some("self"));
            let types_match = receiver_matches
                && expected.params.len() == found.params.len()
                && expected
                    .params
                    .iter()
                    .zip(&found.params)
                    .all(|(a, b)| a.ty == b.ty)
                && expected.ret == found.ret;
            let span = found.span;
            if !types_match {
                notes.push(format!(
                    "method `{}` has signature `{}`, but `{interface}` requires `{}`",
                    required.name,
                    signature_text(&Function {
                        from: vec![],
                        ..found_function
                    }),
                    signature_text(&Function {
                        from: vec![],
                        ..expected
                    }),
                ));
                labels.push((span, "this method has a different signature".into()));
                labels.push((
                    signature_span(required),
                    "required by this interface method".into(),
                ));
                continue;
            }
            if found.unsafe_ && !expected.unsafe_ || found.extern_ {
                notes.push(format!(
                    "method `{}` is unsafe, but `{interface}` declares it safe",
                    required.name
                ));
                labels.push((span, "this method is unsafe".into()));
                continue;
            }
            if found.unavailable || !found.stores.is_empty() {
                notes.push(format!(
                    "method `{}` declares element or storage contracts, which interface methods cannot express",
                    required.name
                ));
                labels.push((span, "this method has storage contracts".into()));
                continue;
            }
            if !found.public && self.function_namespace(&qualified) != viewer {
                notes.push(format!(
                    "method `{}` is private to its package; interface methods must be `pub`",
                    required.name
                ));
                labels.push((span, "this method is private".into()));
                continue;
            }
            // A borrowed result may come from fewer sources than the interface
            // allows, never from more: callers only know the interface's contract.
            let allowed = match self.borrow_sources(&expected) {
                Ok(sources) => positions(&sources, &expected.params),
                Err(_) => HashSet::new(),
            };
            let actual = positions(&found.from, &found.params);
            if !actual
                .iter()
                .all(|source| source.0 == usize::MAX && !source.1 || allowed.contains(source))
            {
                notes.push(format!(
                    "method `{}` returns a borrow `from({})`, but `{interface}` only allows `from({})`",
                    required.name,
                    found.from.join(", "),
                    self.borrow_sources(&expected)
                        .unwrap_or_default()
                        .join(", ")
                ));
                labels.push((
                    span,
                    "this method's borrowed result has other sources".into(),
                ));
            }
        }
        if notes.is_empty() {
            Ok(())
        } else {
            Err((labels, notes))
        }
    }

    pub(super) fn implementation_diagnostic(
        &self,
        ty: &Type,
        interface: &str,
        viewer: &str,
        span: Span,
    ) -> Option<Diagnostic> {
        let (labels, notes) = self.implementation(ty, interface, viewer).err()?;
        let mut diagnostic = Diagnostic::new(
            span,
            format!("`{ty}` does not implement interface `{interface}`"),
        );
        for (span, label) in labels {
            diagnostic = diagnostic.label(span, label);
        }
        for note in notes {
            diagnostic = diagnostic.note(note);
        }
        Some(diagnostic)
    }

    pub(super) fn check_obligation(&self, obligation: &Obligation) -> Check<()> {
        let viewer = if obligation.owner_is_type {
            type_namespace(&obligation.owner).to_owned()
        } else {
            self.function_namespace(&obligation.owner)
        };
        let owner = obligation
            .owner
            .split_once('$')
            .map_or(obligation.owner.as_str(), |(template, _)| template);
        match self.implementation_diagnostic(
            &obligation.ty,
            &obligation.interface,
            &viewer,
            obligation.span,
        ) {
            None => Ok(()),
            Some(diagnostic) => Err(diagnostic
                .primary_label(format!(
                    "`{}` is used as `{}` here",
                    obligation.ty, obligation.param
                ))
                .label(
                    obligation.bound,
                    format!(
                        "`{owner}` requires `{}: {}`",
                        obligation.param, obligation.interface
                    ),
                )),
        }
    }

    /// Declared `struct Name: Interface` conformance, checked where it is declared.
    pub(super) fn check_declared_implementations(&self, program: &Program) -> Check<()> {
        for structure in &program.structs {
            for (interface, span) in &structure.implements {
                if !self.interfaces.contains_key(interface) {
                    return Err(Diagnostic::new(
                        *span,
                        format!("unknown interface `{interface}`"),
                    ));
                }
                if let Some(diagnostic) = self.implementation_diagnostic(
                    &Type::Named(structure.name.clone()),
                    interface,
                    type_namespace(&structure.name),
                    *span,
                ) {
                    return Err(diagnostic.primary_label(format!(
                        "`{}` declares that it implements `{interface}`",
                        structure.name
                    )));
                }
            }
        }
        Ok(())
    }
}

// Borrow sources by parameter position; `static` is `usize::MAX`. The flag
// marks `.stored` sources.
fn positions(sources: &[String], params: &[Param]) -> HashSet<(usize, bool)> {
    sources
        .iter()
        .filter_map(|source| {
            if source == "static" {
                return Some((usize::MAX, false));
            }
            let (name, stored) = match source.strip_suffix(".stored") {
                Some(name) => (name, true),
                None => (source.as_str(), false),
            };
            params
                .iter()
                .position(|p| p.name == name)
                .map(|index| (index, stored))
        })
        .collect()
}

/// Bounds must name the declaration's own type parameters and real interfaces.
pub(super) fn validate_bounds(program: &Program) -> Check<()> {
    let interfaces: HashSet<_> = program.interfaces.iter().map(|i| i.name.as_str()).collect();
    let concrete: HashSet<_> = program
        .structs
        .iter()
        .map(|s| s.name.as_str())
        .chain(program.enums.iter().map(|e| e.name.as_str()))
        .collect();
    let check = |generics: &[String], bounds: &[Bound]| -> Check<()> {
        for bound in bounds {
            if !generics.contains(&bound.param) {
                return Err(Diagnostic::new(
                    bound.span,
                    format!(
                        "`{}` is not a type parameter of this declaration",
                        bound.param
                    ),
                ));
            }
            for interface in &bound.interfaces {
                if !interfaces.contains(interface.as_str()) {
                    let mut diagnostic =
                        Diagnostic::new(bound.span, format!("unknown interface `{interface}`"));
                    if concrete.contains(interface.as_str()) {
                        diagnostic = diagnostic.note(format!(
                            "`{interface}` is a concrete type; bounds name interfaces"
                        ));
                    }
                    return Err(diagnostic);
                }
            }
        }
        Ok(())
    };
    for structure in &program.structs {
        check(&structure.generics, &structure.bounds)?;
    }
    for enumeration in &program.enums {
        check(&enumeration.generics, &enumeration.bounds)?;
    }
    for function in &program.functions {
        check(&function.generics, &function.bounds)?;
    }
    let mut names = HashSet::new();
    for interface in &program.interfaces {
        if !names.insert(interface.name.as_str()) || concrete.contains(interface.name.as_str()) {
            return Err(Diagnostic::new(
                interface.span,
                format!("duplicate declaration `{}`", interface.name),
            ));
        }
    }
    Ok(())
}
